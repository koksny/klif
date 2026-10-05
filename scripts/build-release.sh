#!/bin/bash
# Builds the KLIF release for macOS (Apple silicon): KLIF.app (desktop app, UI embedded) and klif-cli, into one
# folder, plus a zip of that folder and its SHA-256. The macOS twin of scripts/Build-Release.ps1.
#
#  1. Checks that every version in the tree agrees (workspace Cargo.toml, app/src-tauri Cargo.toml and
#     tauri.conf.json, app/ui/package.json, VERSION) and that no `private` folder would ship.
#  2. app/ui:        npm run build -- --emptyOutDir                      -> app/ui/dist (the Svelte UI, minified)
#  3. app/src-tauri: cargo build --release --locked --features custom-protocol   -> klif (the app's executable)
#     custom-protocol makes Tauri serve the UI embedded from app/ui/dist instead of tauri.conf.json's devUrl.
#  4. repo root:     cargo build --release --locked -p klif-cli                  -> klif-cli
#  5. Assembles KLIF.app (Info.plist, icon.icns), copies klif-cli and skills/klif/SKILL.md next to it, signs both
#     programs, zips the folder (ditto, so the bundle's signature survives) and prints the SHA-256 of everything.
#
# Paths stay out of the binaries: both builds run with --remap-path-prefix for the repo, CARGO_HOME and HOME, debug
# info is stripped, and both programs are searched for the home folder and the repo path afterwards.
#
# Signing (codesign):
#   default     ad-hoc ("-"): runs on the Mac that built it; another Mac's Gatekeeper refuses it until the user
#               allows it (docs/platforms.md, "Gatekeeper").
#   --sign      a Developer ID Application identity from the keychain, hardened runtime, secure timestamp.
#               KLIF_SIGN_IDENTITY picks one (its name or SHA-1); without it the only Developer ID Application
#               identity in the keychain is used.
#   --notarize  (implies --sign) submits the zip with `xcrun notarytool submit --keychain-profile`, waits, staples
#               the ticket to KLIF.app and zips again. The profile is KLIF_NOTARY_PROFILE (default "klif-notary"),
#               created once with `xcrun notarytool store-credentials`; this script never sees a password or key.
#
# Options: --out DIR (default <repo>/dist/KLIF), --target-dir DIR (default <repo>/.local/target-release),
#          --skip-ui (reuse app/ui/dist as it is), --check-only (checks only, nothing is built).
#
# Nothing is started, stopped or committed by this script. If KLIF or klif-cli runs from the output folder the
# copy is refused (quit it first; the script never stops it).

set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
ui_dir="$repo/app/ui"
shell_dir="$repo/app/src-tauri"
out_dir="$repo/dist/KLIF"
target_dir="$repo/.local/target-release"
skip_ui=0
check_only=0
sign=0
notarize=0

while [ $# -gt 0 ]; do
    case "$1" in
        --out) out_dir="$2"; shift 2 ;;
        --target-dir) target_dir="$2"; shift 2 ;;
        --skip-ui) skip_ui=1; shift ;;
        --check-only) check_only=1; shift ;;
        --sign) sign=1; shift ;;
        --notarize) sign=1; notarize=1; shift ;;
        -h|--help) sed -n '2,32p' "$0"; exit 0 ;;
        *) echo "Unknown option: $1 (see --help)." >&2; exit 2 ;;
    esac
done

step() { printf '\033[36m== %s\033[0m\n' "$1"; }
fail() { printf '\033[31mError: %s\033[0m\n' "$1" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "$1 was not found on PATH."; }

case "$out_dir" in /*) ;; *) out_dir="$PWD/$out_dir" ;; esac
case "$target_dir" in /*) ;; *) target_dir="$PWD/$target_dir" ;; esac
[ "$(uname -s)" = "Darwin" ] || fail "This script builds the macOS release; on Windows use scripts/Build-Release.ps1."
[ "$(uname -m)" = "arm64" ] || fail "This script builds for Apple silicon (arm64); this Mac reports $(uname -m)."

app_name="KLIF.app"
out_app="$out_dir/$app_name"
out_cli="$out_dir/klif-cli"
skill_src="$repo/skills/klif/SKILL.md"
out_skill="$out_dir/skills/klif/SKILL.md"

# ---- versions ----------------------------------------------------------------------------------------------------

# The version in a toml [section].
toml_version() {
    awk -v sec="[$2]" '
        $0 == sec { inside = 1; next }
        /^\[/ { inside = 0 }
        inside && /^[[:space:]]*version[[:space:]]*=/ { gsub(/.*=[[:space:]]*"|".*/, ""); print; exit }
    ' "$1"
}

# The top-level "version" of a JSON file (the first one, as the files here are written).
json_version() {
    sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$1" | head -n 1
}

check_versions() {
    local a b c d e
    a="$(toml_version "$repo/Cargo.toml" workspace.package)"
    b="$(toml_version "$shell_dir/Cargo.toml" package)"
    c="$(json_version "$shell_dir/tauri.conf.json")"
    d="$(json_version "$ui_dir/package.json")"
    e="$(tr -d ' \t\r\n' < "$repo/VERSION")"
    if [ -z "$a" ] || [ "$a" != "$b" ] || [ "$a" != "$c" ] || [ "$a" != "$d" ] || [ "$a" != "$e" ]; then
        printf 'The versions do not agree:\n' >&2
        printf '  %-60s %s\n' 'Cargo.toml [workspace.package] (klif-cli and the crates)' "$a" \
            'app/src-tauri/Cargo.toml [package] (KLIF.app)' "$b" \
            'app/src-tauri/tauri.conf.json' "$c" \
            'app/ui/package.json' "$d" \
            'VERSION' "$e" >&2
        fail "Make them the same and run this script again."
    fi
    version="$a"
}

# The UI's private skins and public/private assets are local-only: nothing named `private` may ship.
assert_no_private() {
    [ -e "$1" ] || return 0
    local hit
    hit="$(find "$1" -iname private -print -quit 2>/dev/null)"
    [ -z "$hit" ] || fail "$2 contains a private folder ($hit). Private assets never ship: remove it and run this script again."
}

# Fonts ship as woff2 only.
assert_woff2_only() {
    [ -e "$1" ] || return 0
    local hit
    hit="$(find "$1" -type f \( -iname '*.otf' -o -iname '*.ttf' -o -iname '*.woff' -o -iname '*.eot' \) -print -quit 2>/dev/null)"
    [ -z "$hit" ] || fail "$2 contains a font that is not woff2 ($hit). Fonts ship as woff2 only."
}

# No build path may be left in a program: the home folder, the repo, CARGO_HOME.
assert_no_paths() {
    local f="$1" p
    for p in "$HOME" "$repo" "$cargo_home"; do
        if LC_ALL=C grep -aqF "$p" "$f"; then
            fail "$f contains the path $p. Check the --remap-path-prefix flags of this script."
        fi
    done
}

running_from() {
    pgrep -f "^$1( |\$)" >/dev/null 2>&1
}

# ---- signing -----------------------------------------------------------------------------------------------------

# The Developer ID Application identity to sign with (KLIF_SIGN_IDENTITY, or the only one in the keychain).
find_identity() {
    if [ -n "${KLIF_SIGN_IDENTITY:-}" ]; then
        identity="$KLIF_SIGN_IDENTITY"
        return
    fi
    local ids n
    ids="$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/^ *[0-9]*) \([0-9A-F]\{40\}\) "\(Developer ID Application: .*\)"$/\1/p')"
    n="$(printf '%s' "$ids" | grep -c . || true)"
    [ "$n" -ge 1 ] || fail "No Developer ID Application identity is in the keychain. Create one in your Apple Developer account (docs/platforms.md, \"Signing\"), or build without --sign."
    [ "$n" -eq 1 ] || fail "Several Developer ID Application identities are in the keychain: set KLIF_SIGN_IDENTITY to the one to use."
    identity="$ids"
}

sign_all() {
    if [ "$sign" -eq 1 ]; then
        find_identity
        step "Sign with a Developer ID Application identity (hardened runtime)"
        codesign --force --options runtime --timestamp --sign "$identity" "$out_cli"
        codesign --force --options runtime --timestamp --sign "$identity" "$out_app"
    else
        step "Sign ad-hoc (no identity; see --sign)"
        codesign --force --sign - "$out_cli"
        codesign --force --sign - "$out_app"
    fi
    codesign --verify --strict --verbose=1 "$out_app" 2>&1 | sed 's/^/  /'
    codesign --verify --strict "$out_cli"
}

make_zip() {
    rm -f "$zip"
    # ditto keeps the bundle's extended attributes and signature; --keepParent puts the KLIF folder in the zip.
    ditto -c -k --keepParent "$out_dir" "$zip"
}

# ---- checks ------------------------------------------------------------------------------------------------------

need cargo
need codesign
need ditto
[ "$skip_ui" -eq 1 ] || need npm
[ "$notarize" -eq 0 ] || xcrun --find notarytool >/dev/null 2>&1 || fail "xcrun notarytool was not found (install the Xcode Command Line Tools)."

step "Checks"
check_versions
[ -f "$skill_src" ] || fail "The Agent Skill is missing: $skill_src."
echo "version: $version (all five places agree)"
assert_no_private "$out_dir" "The output folder"
ui_dist="$ui_dir/dist"
if [ "$skip_ui" -eq 1 ]; then
    assert_no_private "$ui_dist" "app/ui/dist (--skip-ui reuses it; run without --skip-ui to rebuild it)"
    assert_woff2_only "$ui_dist" "app/ui/dist (--skip-ui reuses it; run without --skip-ui to rebuild it)"
fi
# Refuse early if a destination program runs (replacing it mid-run would break it).
for p in "$out_app/Contents/MacOS/klif" "$out_cli"; do
    if [ -e "$p" ] && running_from "$p"; then
        fail "$p is running. Quit it and run this script again."
    fi
done
if [ "$check_only" -eq 1 ]; then
    printf '\033[32mChecks passed (--check-only: nothing was built).\033[0m\n'
    exit 0
fi

# ---- build -------------------------------------------------------------------------------------------------------

started=$(date +%s)

# 1. The UI -> app/ui/dist
if [ "$skip_ui" -eq 1 ]; then
    step "UI build skipped (--skip-ui)"
else
    step "UI: npm run build -- --emptyOutDir"
    if [ ! -d "$ui_dir/node_modules" ]; then
        step "UI: npm ci (node_modules missing)"
        (cd "$ui_dir" && npm ci)
    fi
    marker="$(mktemp)"
    (cd "$ui_dir" && npm run build -- --emptyOutDir)
    [ "$ui_dist/index.html" -nt "$marker" ] || fail "app/ui/dist/index.html is older than this UI build: the build did not write app/ui/dist."
    rm -f "$marker"
fi
[ -f "$ui_dist/index.html" ] || fail "app/ui/dist/index.html is missing: nothing to embed."
# klif-webui's page is the second entry of the same build; the engine in KLIF.app serves it on the LAN.
[ -f "$ui_dist/webui.html" ] || fail "app/ui/dist/webui.html is missing: klif-webui would have no page to serve."
assert_no_private "$ui_dist" "app/ui/dist (what KLIF.app embeds)"
assert_woff2_only "$ui_dist" "app/ui/dist (what KLIF.app embeds)"
ui_done=$(date +%s)

# Shared by both builds. CARGO_ENCODED_RUSTFLAGS separates flags with 0x1F, so a path with a space survives; the
# later --remap-path-prefix rules win, so the broad one (HOME) comes first.
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
sep=$'\x1f'
flags="--remap-path-prefix=$HOME=user${sep}--remap-path-prefix=$cargo_home=cargo${sep}--remap-path-prefix=$repo=klif"
# Flags a caller set in RUSTFLAGS come first, in their order. (CARGO_ENCODED_RUSTFLAGS also replaces any
# [build] rustflags from a .cargo/config.toml: put such flags into RUSTFLAGS for this script.)
if [ -n "${RUSTFLAGS:-}" ]; then
    user_flags=""
    for t in $RUSTFLAGS; do user_flags="$user_flags$t$sep"; done
    flags="$user_flags$flags"
fi
export CARGO_ENCODED_RUSTFLAGS="$flags"
export CARGO_PROFILE_RELEASE_STRIP=debuginfo
export CARGO_PROFILE_RELEASE_DEBUG=false
unset RUSTFLAGS
# The oldest macOS the programs load on (tauri.macos.conf.json, Info.plist below).
min_macos="$(sed -n 's/^[[:space:]]*"minimumSystemVersion"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$shell_dir/tauri.macos.conf.json" | head -n 1)"
export MACOSX_DEPLOYMENT_TARGET="${min_macos:-13.0}"

# 2. The desktop app (embeds app/ui/dist at compile time)
step "KLIF.app: cargo build --release --locked --features custom-protocol (target $target_dir/shell)"
(cd "$shell_dir" && CARGO_TARGET_DIR="$target_dir/shell" cargo build --release --locked --features custom-protocol)
built_shell="$target_dir/shell/release/klif"
[ -f "$built_shell" ] || fail "cargo did not produce $built_shell."

# 3. The command-line tool (root workspace)
step "klif-cli: cargo build --release --locked -p klif-cli (target $target_dir/cli)"
(cd "$repo" && CARGO_TARGET_DIR="$target_dir/cli" cargo build --release --locked -p klif-cli)
built_cli="$target_dir/cli/release/klif-cli"
[ -f "$built_cli" ] || fail "cargo did not produce $built_cli."

# 4. Assemble
step "Assemble $out_dir"
rm -rf "$out_app"
mkdir -p "$out_app/Contents/MacOS" "$out_app/Contents/Resources" "$(dirname "$out_skill")"
cp "$built_shell" "$out_app/Contents/MacOS/klif"
cp "$shell_dir/icons/icon.icns" "$out_app/Contents/Resources/icon.icns"
identifier="$(sed -n 's/^[[:space:]]*"identifier"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$shell_dir/tauri.conf.json" | head -n 1)"
copyright="$(sed -n 's/^[[:space:]]*"copyright"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$shell_dir/tauri.conf.json" | head -n 1)"
cat > "$out_app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>KLIF</string>
  <key>CFBundleExecutable</key><string>klif</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundleIdentifier</key><string>${identifier}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>KLIF</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>LSMinimumSystemVersion</key><string>${MACOSX_DEPLOYMENT_TARGET}</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>${copyright}</string>
  <key>NSLocalNetworkUsageDescription</key><string>KLIF connects to the other KLIF machines and model servers you add on your local network, and serves klif-webui to your phone when you turn it on.</string>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST
plutil -lint -s "$out_app/Contents/Info.plist" || fail "Info.plist does not parse."
cp "$built_cli" "$out_cli"
cp "$skill_src" "$out_skill"
assert_no_private "$out_dir" "The output folder"
for f in "$out_app/Contents/MacOS/klif" "$out_cli"; do assert_no_paths "$f"; done

# 5. Sign, zip, notarize
sign_all
zip="$(dirname "$out_dir")/KLIF-$version-macos-arm64.zip"
step "Zip $zip"
make_zip
if [ "$notarize" -eq 1 ]; then
    profile="${KLIF_NOTARY_PROFILE:-klif-notary}"
    step "Notarize (xcrun notarytool, keychain profile \"$profile\"); this waits for Apple"
    xcrun notarytool submit "$zip" --keychain-profile "$profile" --wait
    xcrun stapler staple "$out_app"
    xcrun stapler validate "$out_app"
    step "Zip again with the stapled ticket"
    make_zip
fi
shasum -a 256 "$zip" | sed "s|  .*/|  |" > "$zip.sha256"
finished=$(date +%s)

echo
if [ "$notarize" -eq 1 ]; then how="signed, notarized"; elif [ "$sign" -eq 1 ]; then how="signed"; else how="ad-hoc signed"; fi
echo "version: $version ($how)"
for f in "$out_app/Contents/MacOS/klif" "$out_cli" "$zip"; do
    printf '%-44s %6.1f MiB  sha256 %s\n' "${f#"$(dirname "$out_dir")"/}" "$(echo "scale=1; $(stat -f %z "$f") / 1048576" | bc)" "$(shasum -a 256 "$f" | cut -d' ' -f1)"
done
echo "built:   $((finished - started)) s total ($((ui_done - started)) s UI)"
echo "out:     $out_dir"
echo "zip:     $zip (+ .sha256)"
echo "skill:   $out_skill (the Agent Skill for klif-cli)"
if [ -f "$repo/.local/klif.toml" ]; then
    echo "config:  $repo/.local/klif.toml (found from the programs by walking up to the repo root)"
else
    echo "config:  .local/klif.toml not found; KLIF uses ~/Library/Application Support/KLIF/klif.toml (or KLIF_CONFIG)"
fi
echo "log:     klif-shell.log in the logs folder ([paths] logs_dir, default ~/Library/Logs/KLIF)"
