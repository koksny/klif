#!/bin/bash
# Builds the KLIF release for Linux (x86_64): klif (desktop app, UI embedded) and klif-cli, into one folder, plus
# a tar.gz of that folder and its SHA-256. The Linux twin of scripts/Build-Release.ps1 and scripts/build-release.sh.
#
#  1. Checks that every version in the tree agrees (workspace Cargo.toml, app/src-tauri Cargo.toml and
#     tauri.conf.json, app/ui/package.json, VERSION) and that no `private` folder would ship.
#  2. app/ui:        npm run build -- --emptyOutDir                      -> app/ui/dist (the Svelte UI, minified)
#  3. app/src-tauri: cargo build --release --locked --features custom-protocol   -> klif (the desktop app)
#     custom-protocol makes Tauri serve the UI embedded from app/ui/dist instead of tauri.conf.json's devUrl.
#  4. repo root:     cargo build --release --locked -p klif-cli                  -> klif-cli
#  5. Copies both, skills/klif/SKILL.md, klif.desktop and the icon into the folder, packs it as
#     KLIF-<version>-linux-x86_64.tar.gz and prints the SHA-256 of everything.
#
# Build needs (Debian / Ubuntu): build-essential pkg-config libssl-dev libwebkit2gtk-4.1-dev
# libayatana-appindicator3-dev librsvg2-dev libxdo-dev, Rust 1.90+, Node.js 20.19+ or 22.12+. The programs then need
# libwebkit2gtk-4.1-0, libayatana-appindicator3-1 and libssl3 on the machine that runs them (klif-cli: libssl3 only).
# The binaries link the build machine's glibc: build on the oldest distribution you want them to run on.
#
# Paths stay out of the binaries: both builds run with --remap-path-prefix for the repo, CARGO_HOME and HOME, debug
# info is stripped, and both programs are searched for the home folder and the repo path afterwards.
#
# Options: --out DIR (default <repo>/dist/KLIF), --target-dir DIR (default <repo>/.local/target-release-linux),
#          --skip-ui (reuse app/ui/dist as it is), --check-only (checks only, nothing is built).
#
# Nothing is started, stopped or committed by this script. If klif or klif-cli runs from the output folder the copy
# is refused (quit it first; the script never stops it).

set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
ui_dir="$repo/app/ui"
shell_dir="$repo/app/src-tauri"
out_dir="$repo/dist/KLIF"
target_dir="$repo/.local/target-release-linux"
skip_ui=0
check_only=0

while [ $# -gt 0 ]; do
    case "$1" in
        --out) out_dir="$2"; shift 2 ;;
        --target-dir) target_dir="$2"; shift 2 ;;
        --skip-ui) skip_ui=1; shift ;;
        --check-only) check_only=1; shift ;;
        -h|--help) sed -n '2,27p' "$0"; exit 0 ;;
        *) echo "Unknown option: $1 (see --help)." >&2; exit 2 ;;
    esac
done

step() { printf '\033[36m== %s\033[0m\n' "$1"; }
fail() { printf '\033[31mError: %s\033[0m\n' "$1" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "$1 was not found on PATH."; }

case "$out_dir" in /*) ;; *) out_dir="$PWD/$out_dir" ;; esac
case "$target_dir" in /*) ;; *) target_dir="$PWD/$target_dir" ;; esac
[ "$(uname -s)" = "Linux" ] || fail "This script builds the Linux release; on Windows use scripts/Build-Release.ps1, on a Mac scripts/build-release.sh."
[ "$(uname -m)" = "x86_64" ] || fail "This script builds for x86_64; this machine reports $(uname -m)."

out_app="$out_dir/klif"
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
    a="$(toml_version "$repo/Cargo.toml" workspace.package | tr -d '\r')"
    b="$(toml_version "$shell_dir/Cargo.toml" package | tr -d '\r')"
    c="$(json_version "$shell_dir/tauri.conf.json" | tr -d '\r')"
    d="$(json_version "$ui_dir/package.json" | tr -d '\r')"
    e="$(tr -d ' \t\r\n' < "$repo/VERSION")"
    if [ -z "$a" ] || [ "$a" != "$b" ] || [ "$a" != "$c" ] || [ "$a" != "$d" ] || [ "$a" != "$e" ]; then
        printf 'The versions do not agree:\n' >&2
        printf '  %-60s %s\n' 'Cargo.toml [workspace.package] (klif-cli and the crates)' "$a" \
            'app/src-tauri/Cargo.toml [package] (klif)' "$b" \
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

# ---- checks ------------------------------------------------------------------------------------------------------

need cargo
need pkg-config
need tar
need sha256sum
[ "$skip_ui" -eq 1 ] || need npm
pkg-config --exists webkit2gtk-4.1 || fail "webkit2gtk-4.1 was not found (Debian / Ubuntu: apt install libwebkit2gtk-4.1-dev)."
pkg-config --exists openssl || fail "OpenSSL was not found (Debian / Ubuntu: apt install libssl-dev pkg-config)."

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
for p in "$out_app" "$out_cli"; do
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
# klif-webui's page is the second entry of the same build; the engine in klif serves it on the LAN.
[ -f "$ui_dist/webui.html" ] || fail "app/ui/dist/webui.html is missing: klif-webui would have no page to serve."
assert_no_private "$ui_dist" "app/ui/dist (what klif embeds)"
assert_woff2_only "$ui_dist" "app/ui/dist (what klif embeds)"
ui_done=$(date +%s)

# Shared by both builds. CARGO_ENCODED_RUSTFLAGS separates flags with 0x1F, so a path with a space survives; the
# later --remap-path-prefix rules win, so the broad one (HOME) comes first.
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
sep=$'\x1f'
flags="--remap-path-prefix=$HOME=user${sep}--remap-path-prefix=$cargo_home=cargo${sep}--remap-path-prefix=$repo=klif"
if [ -n "${RUSTFLAGS:-}" ]; then
    user_flags=""
    for t in $RUSTFLAGS; do user_flags="$user_flags$t$sep"; done
    flags="$user_flags$flags"
fi
export CARGO_ENCODED_RUSTFLAGS="$flags"
export CARGO_PROFILE_RELEASE_STRIP=debuginfo
export CARGO_PROFILE_RELEASE_DEBUG=false
unset RUSTFLAGS

# 2. The desktop app (embeds app/ui/dist at compile time)
step "klif: cargo build --release --locked --features custom-protocol (target $target_dir/shell)"
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
mkdir -p "$out_dir" "$(dirname "$out_skill")"
cp "$built_shell" "$out_app"
cp "$built_cli" "$out_cli"
chmod 755 "$out_app" "$out_cli"
cp "$skill_src" "$out_skill"
cp "$shell_dir/icons/32x32.png" "$out_dir/klif.png"
# A launcher entry for the folder as it is; `Exec` and `Icon` hold relative names: install it with absolute paths
# (docs/platforms.md, "Linux").
cat > "$out_dir/klif.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=KLIF
Comment=Local inference stack manager
Exec=klif
Icon=klif
Terminal=false
Categories=Development;
DESKTOP
assert_no_private "$out_dir" "The output folder"
for f in "$out_app" "$out_cli"; do assert_no_paths "$f"; done

# 5. Pack
tarball="$(dirname "$out_dir")/KLIF-$version-linux-x86_64.tar.gz"
step "Pack $tarball"
rm -f "$tarball"
tar -czf "$tarball" -C "$(dirname "$out_dir")" "$(basename "$out_dir")"
(cd "$(dirname "$tarball")" && sha256sum "$(basename "$tarball")" > "$(basename "$tarball").sha256")
finished=$(date +%s)

echo
echo "version: $version"
for f in "$out_app" "$out_cli" "$tarball"; do
    printf '%-44s %6.1f MiB  sha256 %s\n' "${f#"$(dirname "$out_dir")"/}" "$(awk "BEGIN { printf \"%.1f\", $(stat -c %s "$f") / 1048576 }")" "$(sha256sum "$f" | cut -d' ' -f1)"
done
echo "built:   $((finished - started)) s total ($((ui_done - started)) s UI)"
echo "out:     $out_dir"
echo "tarball: $tarball (+ .sha256)"
echo "skill:   $out_skill (the Agent Skill for klif-cli)"
if [ -f "$repo/.local/klif.toml" ]; then
    echo "config:  $repo/.local/klif.toml (found from the programs by walking up to the repo root)"
else
    echo "config:  .local/klif.toml not found; KLIF uses \${XDG_CONFIG_HOME:-~/.config}/klif/klif.toml (or KLIF_CONFIG)"
fi
echo "log:     klif-shell.log in the logs folder ([paths] logs_dir, default \${XDG_DATA_HOME:-~/.local/share}/klif/logs)"
