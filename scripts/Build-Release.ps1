<#
.SYNOPSIS
    Builds the KLIF release: klif.exe (desktop app, UI embedded) and klif-cli.exe, into one folder.

.DESCRIPTION
    1. Checks that every version in the tree agrees (workspace Cargo.toml, app/src-tauri Cargo.toml and
       tauri.conf.json, app/ui/package.json, VERSION) and that no `private` folder would ship.
    2. app/ui:        npm run build -- --emptyOutDir   -> app/ui/dist  (the Svelte UI, minified)
       app/ui/dist is emptied first, so nothing left by an older build (a 0.2 dist\private, old hashed assets)
       can be embedded; the private-folder and woff2-only font checks then run on the fresh output. The build
       has two pages: index.html (the window) and webui.html (klif-webui, which the engine in klif.exe serves).
    3. app/src-tauri: cargo build --release --features custom-protocol   -> klif.exe
       The custom-protocol feature makes Tauri serve the UI embedded from app/ui/dist at compile time
       (http://tauri.localhost) instead of tauri.conf.json's devUrl. Without it a release build still loads
       the dev server.
    4. repo root:     cargo build --release -p klif-cli                  -> klif-cli.exe
    5. Copies both to OutDir, optionally signs them (-Sign), prints their SHA256. The Agent Skill that teaches a
       coding agent to drive KLIF with klif-cli (skills\klif\SKILL.md) is copied to OutDir\skills\klif\SKILL.md.

    Paths stay out of the binaries: both builds run with --remap-path-prefix for the repo root, CARGO_HOME
    (default %USERPROFILE%\.cargo) and the user profile (RUSTFLAGS), debug info is stripped and /PDBALTPATH keeps
    a PDB path out of the PE header. Both exes are scanned for 'ExecutionPolicy' and 'powershell.exe' (ASCII and
    UTF-16; KLIF never starts PowerShell). Check the result with `strings` before publishing an exe.

    The exes are self-contained (WebView2 itself is the system runtime). At startup klif.exe finds its machine
    config by walking up from the exe's directory (and the working directory) to <repo>/.local/klif.toml,
    then %APPDATA%\KLIF\klif.toml; KLIF_CONFIG overrides both.

    Nothing is started, stopped or committed by this script. If an exe in OutDir is running the copy is
    refused (close KLIF first; the script never kills it).

.PARAMETER OutDir
    Where klif.exe and klif-cli.exe are placed. Default: <repo>/dist/KLIF.
.PARAMETER SkipUi
    Reuse the existing app/ui/dist instead of running the UI build. It is checked as it is: a stale private folder
    in it stops the script (run without -SkipUi to rebuild it from an empty folder).
.PARAMETER TargetDir
    Cargo target directory for both release builds (the shell in <TargetDir>\shell, the CLI in <TargetDir>\cli).
    Default: <repo>/.local/target-release (.local is gitignored).
.PARAMETER Sign
    Sign both exes with signtool. Configured by environment variables, none of them stored anywhere:
      KLIF_SIGN_CERT_THUMBPRINT  SHA-1 thumbprint of a code signing certificate in the Windows certificate store, or
      KLIF_SIGN_AZURE_DLIB + KLIF_SIGN_AZURE_METADATA  Azure Trusted Signing: the Azure.CodeSigning.Dlib.dll path and
                                 the metadata .json path (sign in with `az login` / environment credentials first).
      KLIF_SIGN_TIMESTAMP_URL    RFC 3161 timestamp server (defaults: DigiCert for a certificate, Microsoft for Azure).
    When none is configured the signing step is skipped with a warning; the build is not an error.
.PARAMETER CheckOnly
    Run the checks (versions, tools, no private folder, files not in use) and stop before building anything.
#>
[CmdletBinding()]
param(
    [string]$OutDir,
    [switch]$SkipUi,
    [string]$TargetDir,
    [switch]$Sign,
    [switch]$CheckOnly
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$uiDir = Join-Path $repo 'app\ui'
$shellDir = Join-Path $repo 'app\src-tauri'
if (-not $OutDir) { $OutDir = Join-Path $repo 'dist\KLIF' }
if (-not $TargetDir) { $TargetDir = Join-Path $repo '.local\target-release' }
$OutDir = [System.IO.Path]::GetFullPath($OutDir)
$TargetDir = [System.IO.Path]::GetFullPath($TargetDir)
$shellTarget = Join-Path $TargetDir 'shell'
$cliTarget = Join-Path $TargetDir 'cli'
$outShell = Join-Path $OutDir 'klif.exe'
$outCli = Join-Path $OutDir 'klif-cli.exe'
$skillSrc = Join-Path $repo 'skills\klif\SKILL.md'
$outSkill = Join-Path $OutDir 'skills\klif\SKILL.md'

# Native tools (npm, cargo) write progress to stderr. Under ErrorActionPreference=Stop, PowerShell 5.1
# turns redirected stderr lines into terminating NativeCommandErrors, so judge them by exit code only.
function Invoke-Native([scriptblock]$Block) {
    $prev = $ErrorActionPreference
    $script:ErrorActionPreference = 'Continue'
    try { & $Block | Out-Host; return $LASTEXITCODE } finally { $script:ErrorActionPreference = $prev }
}
function Step([string]$text) { Write-Host ("== " + $text) -ForegroundColor Cyan }
function Need([string]$name) {
    if (-not (Get-Command $name -ErrorAction SilentlyContinue)) { throw "$name was not found on PATH." }
}

# Run a script block with some environment variables set, then put the old values back.
function Invoke-WithEnv([hashtable]$Vars, [scriptblock]$Block) {
    $saved = @{}
    foreach ($k in $Vars.Keys) {
        $saved[$k] = [Environment]::GetEnvironmentVariable($k, 'Process')
        [Environment]::SetEnvironmentVariable($k, [string]$Vars[$k], 'Process')
    }
    try { & $Block } finally {
        foreach ($k in $Vars.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k], 'Process') }
    }
}

# ---- versions ---------------------------------------------------------------------------------------------------

function Get-TomlVersion([string]$Path, [string]$Section) {
    $text = Get-Content -LiteralPath $Path -Raw
    $m = [regex]::Match($text, '(?ms)^\[' + [regex]::Escape($Section) + '\][ \t]*\r?\n(.*?)(?=^\[|\z)')
    if (-not $m.Success) { throw "[$Section] was not found in $Path." }
    $v = [regex]::Match($m.Groups[1].Value, '(?m)^\s*version\s*=\s*"([^"]+)"')
    if (-not $v.Success) { throw "No version in [$Section] of $Path." }
    return $v.Groups[1].Value
}

function Get-JsonVersion([string]$Path) {
    $v = (Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json).version
    if (-not $v) { throw "No version in $Path." }
    return [string]$v
}

# Every place the version lives must say the same thing, or the exe, its VERSIONINFO and the docs disagree.
function Assert-Versions {
    $versions = [ordered]@{
        'Cargo.toml [workspace.package] (klif-cli and the crates)' = Get-TomlVersion (Join-Path $repo 'Cargo.toml') 'workspace.package'
        'app/src-tauri/Cargo.toml [package] (klif.exe)'           = Get-TomlVersion (Join-Path $shellDir 'Cargo.toml') 'package'
        'app/src-tauri/tauri.conf.json (VERSIONINFO)'             = Get-JsonVersion (Join-Path $shellDir 'tauri.conf.json')
        'app/ui/package.json'                                     = Get-JsonVersion (Join-Path $uiDir 'package.json')
        'VERSION'                                                 = (Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()
    }
    $distinct = @($versions.Values | Select-Object -Unique)
    if ($distinct.Count -ne 1) {
        $lines = $versions.GetEnumerator() | ForEach-Object { '  {0,-60} {1}' -f $_.Key, $_.Value }
        throw ("The versions do not agree:`n" + ($lines -join "`n") + "`nMake them the same and run this script again.")
    }
    return $distinct[0]
}

# The UI's private skins and public/private assets are local-only: nothing named `private` may be embedded in an
# exe or sit in the output folder.
function Assert-NoPrivate([string]$Root, [string]$What) {
    if (-not (Test-Path -LiteralPath $Root)) { return }
    $hit = Get-ChildItem -LiteralPath $Root -Recurse -Force -ErrorAction SilentlyContinue | Where-Object { $_.Name -ieq 'private' } | Select-Object -First 1
    if ($hit) {
        throw "$What contains a private folder ($($hit.FullName)). Private assets never ship: remove it (and fix the UI build so it is not copied) and run this script again."
    }
}

# Fonts ship as woff2 only (the Vite build refuses the others too): no .otf/.ttf/.woff/.eot may be embedded.
function Assert-Woff2Only([string]$Root, [string]$What) {
    if (-not (Test-Path -LiteralPath $Root)) { return }
    $hit = Get-ChildItem -LiteralPath $Root -Recurse -File -Force -ErrorAction SilentlyContinue |
        Where-Object { @('.otf', '.ttf', '.woff', '.eot') -contains $_.Extension.ToLowerInvariant() } | Select-Object -First 1
    if ($hit) {
        throw "$What contains a font that is not woff2 ($($hit.FullName)). Fonts ship as woff2 only: remove it and run this script again."
    }
}

# KLIF never starts PowerShell, so its exes must not name it (ASCII or UTF-16, any case). 'Bypass' is not searched:
# tao, muda and keyboard-types legitimately contain 'RfBypass'.
function Assert-NoPowerShellText([string]$Exe) {
    $text = [System.Text.Encoding]::GetEncoding(28591).GetString([System.IO.File]::ReadAllBytes($Exe))
    foreach ($word in @('ExecutionPolicy', 'powershell.exe')) {
        $wide = ($word.ToCharArray() | ForEach-Object { [string]$_ + [char]0 }) -join ''
        foreach ($form in @($word, $wide)) {
            if ($text.IndexOf($form, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
                throw "$Exe contains the text '$word'. KLIF never starts PowerShell: find the code or dependency that names it and remove it."
            }
        }
    }
}

# ---- signing ----------------------------------------------------------------------------------------------------

function Find-SignTool {
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cmd) { return $cmd.Source }
    foreach ($root in @(${env:ProgramFiles(x86)}, $env:ProgramFiles)) {
        if (-not $root) { continue }
        $kits = Join-Path $root 'Windows Kits\10\bin'
        if (-not (Test-Path -LiteralPath $kits)) { continue }
        $found = Get-ChildItem -LiteralPath $kits -Directory -ErrorAction SilentlyContinue | Sort-Object Name -Descending | ForEach-Object {
            $p = Join-Path $_.FullName 'x64\signtool.exe'
            if (Test-Path -LiteralPath $p) { $p }
        } | Select-Object -First 1
        if ($found) { return $found }
    }
    return $null
}

# Returns $true when the files were signed, $false when signing is not configured (skipped).
function Invoke-Signing([string[]]$Files) {
    $thumb = $env:KLIF_SIGN_CERT_THUMBPRINT
    $dlib = $env:KLIF_SIGN_AZURE_DLIB
    $meta = $env:KLIF_SIGN_AZURE_METADATA
    $azure = $dlib -and $meta
    if (-not $thumb -and -not $azure) {
        Write-Warning 'Signing skipped: set KLIF_SIGN_CERT_THUMBPRINT, or KLIF_SIGN_AZURE_DLIB and KLIF_SIGN_AZURE_METADATA (see -Sign in the script help).'
        return $false
    }
    $tool = Find-SignTool
    if (-not $tool) { throw 'signtool.exe was not found (install the Windows SDK); the signing step cannot run.' }
    $timestamp = $env:KLIF_SIGN_TIMESTAMP_URL
    if ($thumb) {
        if (-not $timestamp) { $timestamp = 'http://timestamp.digicert.com' }
        $args1 = @('sign', '/fd', 'SHA256', '/sha1', $thumb, '/tr', $timestamp, '/td', 'SHA256', '/d', 'KLIF')
    } else {
        if (-not $timestamp) { $timestamp = 'http://timestamp.acs.microsoft.com' }
        $args1 = @('sign', '/fd', 'SHA256', '/tr', $timestamp, '/td', 'SHA256', '/dlib', $dlib, '/dmdf', $meta, '/d', 'KLIF')
    }
    foreach ($f in $Files) {
        $code = Invoke-Native { & $tool @args1 $f 2>&1 }
        if ($code -ne 0) { throw "signtool sign failed for $f ($code)." }
        $code = Invoke-Native { & $tool verify /pa /q $f 2>&1 }
        if ($code -ne 0) { throw "The signature of $f does not verify ($code)." }
    }
    return $true
}

# ---- checks -----------------------------------------------------------------------------------------------------

Need cargo
$npm = $null
if (-not $SkipUi) {
    # npm.cmd explicitly: the npm.ps1 shim mangles its arguments on some Windows PowerShell setups.
    $npm = (Get-Command npm.cmd -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $npm) { throw 'npm.cmd was not found on PATH (install Node.js).' }
}

Step 'Checks'
$version = Assert-Versions
if (-not (Test-Path -LiteralPath $skillSrc)) { throw "The Agent Skill is missing: $skillSrc." }
Write-Host "version: $version (all five places agree)"
Assert-NoPrivate $OutDir 'The output folder'
$uiDist = Join-Path $uiDir 'dist'
if ($SkipUi) {
    Assert-NoPrivate $uiDist 'app/ui/dist (-SkipUi reuses it; run without -SkipUi to rebuild it from an empty folder)'
    Assert-Woff2Only $uiDist 'app/ui/dist (-SkipUi reuses it; run without -SkipUi to rebuild it from an empty folder)'
} elseif (Test-Path -LiteralPath (Join-Path $uiDist 'private')) {
    Write-Host 'app/ui/dist has a private folder left by an older build; the UI build empties app/ui/dist first.'
}

# Refuse early if a destination exe is in use (copying over a running exe fails after the whole build).
foreach ($pair in @(@($outShell, 'klif'), @($outCli, 'klif-cli'))) {
    if (-not (Test-Path -LiteralPath $pair[0])) { continue }
    $full = (Resolve-Path -LiteralPath $pair[0]).Path
    $running = Get-Process -Name $pair[1] -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -eq $full } catch { $false }
    }
    if ($running) {
        throw ("$($pair[1]) is running from $full (pid " + (($running | ForEach-Object Id) -join ', ') + '). Close it and run this script again.')
    }
}
if ($CheckOnly) {
    Write-Host 'Checks passed (-CheckOnly: nothing was built).' -ForegroundColor Green
    return
}

# ---- build ------------------------------------------------------------------------------------------------------

$sw = [System.Diagnostics.Stopwatch]::StartNew()

# 1. The UI -> app/ui/dist
if ($SkipUi) {
    Step 'UI build skipped (-SkipUi)'
} else {
    # --emptyOutDir: always start from an empty app/ui/dist (also if the Vite config ever turns emptying off), so
    # what klif.exe embeds is exactly this build.
    Step 'UI: npm run build -- --emptyOutDir'
    $uiStart = Get-Date
    Push-Location $uiDir
    try {
        if (-not (Test-Path -LiteralPath 'node_modules')) {
            Step 'UI: npm ci (node_modules missing)'
            $code = Invoke-Native { & $npm ci 2>&1 }
            if ($code -ne 0) { throw "npm ci failed ($code)." }
        }
        $code = Invoke-Native { & $npm run build -- --emptyOutDir 2>&1 }
        if ($code -ne 0) { throw "npm run build failed ($code)." }
    } finally { Pop-Location }
}
$uiIndex = Join-Path $uiDist 'index.html'
if (-not (Test-Path -LiteralPath $uiIndex)) {
    throw "app/ui/dist/index.html is missing: nothing to embed."
}
# klif-webui's page is the second entry of the same build; the engine in klif.exe serves it on the LAN.
if (-not (Test-Path -LiteralPath (Join-Path $uiDist 'webui.html'))) {
    throw "app/ui/dist/webui.html is missing: klif-webui would have no page to serve."
}
if (-not $SkipUi -and (Get-Item -LiteralPath $uiIndex).LastWriteTime -lt $uiStart.AddSeconds(-2)) {
    throw "app/ui/dist/index.html is older than this UI build: the build did not write app/ui/dist."
}
Assert-NoPrivate $uiDist 'app/ui/dist (what klif.exe embeds)'
Assert-Woff2Only $uiDist 'app/ui/dist (what klif.exe embeds)'
$uiSeconds = [math]::Round($sw.Elapsed.TotalSeconds, 1)

# Build environment shared by both exes. Later --remap-path-prefix rules win over earlier ones, so the broad one
# (the user profile) comes first. CARGO_ENCODED_RUSTFLAGS separates flags with 0x1F, so a profile path with a
# space survives; it replaces RUSTFLAGS for these builds (flags a caller already set in RUSTFLAGS are kept).
$cargoHome = $env:CARGO_HOME
if (-not $cargoHome) { $cargoHome = Join-Path $env:USERPROFILE '.cargo' }
$flags = New-Object System.Collections.Generic.List[string]
if ($env:RUSTFLAGS) { foreach ($t in ($env:RUSTFLAGS -split '\s+')) { if ($t) { $flags.Add($t) } } }
$flags.Add('--remap-path-prefix=' + $env:USERPROFILE + '=user')
$flags.Add('--remap-path-prefix=' + $cargoHome + '=cargo')
$flags.Add('--remap-path-prefix=' + $repo + '=klif')
# Keep the PDB path (a build-folder path) out of the PE header if a PDB is produced at all.
$flags.Add('-Clink-arg=/PDBALTPATH:%_PDB%')
$buildEnv = @{
    CARGO_ENCODED_RUSTFLAGS      = ($flags -join [char]0x1F)
    CARGO_PROFILE_RELEASE_STRIP  = 'debuginfo'
    CARGO_PROFILE_RELEASE_DEBUG  = 'false'
}

# 2. The desktop app (embeds app/ui/dist at compile time)
Step "klif.exe: cargo build --release --locked --features custom-protocol (target $shellTarget)"
$buildEnv['CARGO_TARGET_DIR'] = $shellTarget
Push-Location $shellDir
try {
    Invoke-WithEnv $buildEnv {
        $code = Invoke-Native { & cargo build --release --locked --features custom-protocol 2>&1 }
        if ($code -ne 0) { throw "cargo build (klif.exe) failed ($code)." }
    }
} finally { Pop-Location }
$builtShell = Join-Path $shellTarget 'release\klif.exe'
if (-not (Test-Path -LiteralPath $builtShell)) { throw "cargo did not produce $builtShell." }

# 3. The command-line tool (root workspace)
Step "klif-cli.exe: cargo build --release --locked -p klif-cli (target $cliTarget)"
$buildEnv['CARGO_TARGET_DIR'] = $cliTarget
Push-Location $repo
try {
    Invoke-WithEnv $buildEnv {
        $code = Invoke-Native { & cargo build --release --locked -p klif-cli 2>&1 }
        if ($code -ne 0) { throw "cargo build (klif-cli.exe) failed ($code)." }
    }
} finally { Pop-Location }
$builtCli = Join-Path $cliTarget 'release\klif-cli.exe'
if (-not (Test-Path -LiteralPath $builtCli)) { throw "cargo did not produce $builtCli." }

# 4. Publish
Step "Copy to $OutDir"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Copy-Item -LiteralPath $builtShell -Destination $outShell -Force
Copy-Item -LiteralPath $builtCli -Destination $outCli -Force
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $outSkill) | Out-Null
Copy-Item -LiteralPath $skillSrc -Destination $outSkill -Force
Assert-NoPrivate $OutDir 'The output folder'
foreach ($exe in @($outShell, $outCli)) { Assert-NoPowerShellText $exe }

$signed = $false
if ($Sign) {
    Step 'Sign'
    $signed = Invoke-Signing @($outShell, $outCli)
}
$sw.Stop()

Write-Host ''
Write-Host ('version: ' + $version + $(if ($signed) { ' (signed)' } else { ' (unsigned)' }))
foreach ($f in @($outShell, $outCli)) {
    $item = Get-Item -LiteralPath $f
    $hash = (Get-FileHash -LiteralPath $f -Algorithm SHA256).Hash.ToLowerInvariant()
    Write-Host ('{0,-14} {1,6:N1} MiB  sha256 {2}' -f $item.Name, ($item.Length / 1MB), $hash)
}
Write-Host ('built:  {0:N0} s total ({1} s UI, {2:N0} s Rust + copy)' -f $sw.Elapsed.TotalSeconds, $uiSeconds, ($sw.Elapsed.TotalSeconds - $uiSeconds))
Write-Host ('out:    ' + $OutDir)
Write-Host ('skill:  ' + $outSkill + ' (the Agent Skill for klif-cli)')
$cfg = Join-Path $repo '.local\klif.toml'
if (Test-Path -LiteralPath $cfg) {
    Write-Host ('config: ' + $cfg + ' (found from the exe by walking up to the repo root)')
} else {
    Write-Host 'config: .local\klif.toml not found; the exes fall back to %APPDATA%\KLIF\klif.toml (or KLIF_CONFIG)' -ForegroundColor Yellow
}
Write-Host 'log:    klif-shell.log in the logs folder ([paths] logs_dir, default <data dir>\logs)'
