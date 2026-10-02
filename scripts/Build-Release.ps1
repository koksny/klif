<#
.SYNOPSIS
    Builds the KLIF desktop app as a release exe with the UI embedded (no dev server, no console window).

.DESCRIPTION
    1. app/ui:        npm run build            -> app/ui/dist  (the Svelte UI, minified)
    2. app/src-tauri: cargo build --release --features custom-protocol
                                               -> <repo>/.local/target-release/release/klif.exe
       The custom-protocol feature makes Tauri serve the UI embedded from app/ui/dist at compile time
       (http://tauri.localhost) instead of tauri.conf.json's devUrl. Without it a release build still loads
       the dev server.
    3. Copies klif.exe to <repo>/dist/KLIF/klif.exe (OutDir to change).

    The exe is self-contained (WebView2 itself is the system runtime). At startup it finds its machine
    config by walking up from the exe's directory (and the working directory) to <repo>/.local/klif.toml,
    then %APPDATA%\KLIF\klif.toml; KLIF_CONFIG overrides both. Logs, the WebView2 profile and the window
    geometry live next to that klif.toml (klif-shell.log, webview-data, window.json).

    Nothing is started, stopped or committed by this script. If klif.exe in OutDir is running the copy is
    refused (close KLIF first; the script never kills it).

.PARAMETER OutDir
    Where klif.exe is placed. Default: <repo>/dist/KLIF.
.PARAMETER SkipUi
    Reuse the existing app/ui/dist instead of running the UI build.
.PARAMETER TargetDir
    Cargo target directory for the release build. Default: <repo>/.local/target-release (.local is gitignored).
#>
[CmdletBinding()]
param(
    [string]$OutDir,
    [switch]$SkipUi,
    [string]$TargetDir
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$uiDir = Join-Path $repo 'app\ui'
$shellDir = Join-Path $repo 'app\src-tauri'
if (-not $OutDir) { $OutDir = Join-Path $repo 'dist\KLIF' }
if (-not $TargetDir) { $TargetDir = Join-Path $repo '.local\target-release' }
$outExe = Join-Path $OutDir 'klif.exe'

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

Need cargo
$npm = $null
if (-not $SkipUi) {
    # npm.cmd explicitly: the npm.ps1 shim mangles its arguments on some Windows PowerShell setups.
    $npm = (Get-Command npm.cmd -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $npm) { throw 'npm.cmd was not found on PATH (install Node.js).' }
}

# Refuse early if the destination exe is in use (copying over a running exe fails after the whole build).
if (Test-Path -LiteralPath $outExe) {
    $full = (Resolve-Path -LiteralPath $outExe).Path
    $running = Get-Process -Name 'klif' -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -eq $full } catch { $false }
    }
    if ($running) {
        throw ("KLIF is running from $full (pid " + (($running | ForEach-Object Id) -join ', ') + '). Close it and run this script again.')
    }
}

$sw = [System.Diagnostics.Stopwatch]::StartNew()

# 1. The UI -> app/ui/dist
if ($SkipUi) {
    Step 'UI build skipped (-SkipUi)'
} else {
    Step 'UI: npm run build'
    Push-Location $uiDir
    try {
        if (-not (Test-Path -LiteralPath 'node_modules')) {
            Step 'UI: npm ci (node_modules missing)'
            $code = Invoke-Native { & $npm ci 2>&1 }
            if ($code -ne 0) { throw "npm ci failed ($code)." }
        }
        $code = Invoke-Native { & $npm run build 2>&1 }
        if ($code -ne 0) { throw "npm run build failed ($code)." }
    } finally { Pop-Location }
}
if (-not (Test-Path -LiteralPath (Join-Path $uiDir 'dist\index.html'))) {
    throw "app/ui/dist/index.html is missing: nothing to embed."
}
$uiSeconds = [math]::Round($sw.Elapsed.TotalSeconds, 1)

# 2. The shell (embeds app/ui/dist at compile time)
Step "Shell: cargo build --release --features custom-protocol (target $TargetDir)"
$prevTarget = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = $TargetDir
Push-Location $shellDir
try {
    $code = Invoke-Native { & cargo build --release --features custom-protocol 2>&1 }
    if ($code -ne 0) { throw "cargo build failed ($code)." }
} finally {
    Pop-Location
    if ($null -eq $prevTarget) { Remove-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue } else { $env:CARGO_TARGET_DIR = $prevTarget }
}
$built = Join-Path $TargetDir 'release\klif.exe'
if (-not (Test-Path -LiteralPath $built)) { throw "cargo did not produce $built." }

# 3. Publish
Step "Copy to $OutDir"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Copy-Item -LiteralPath $built -Destination $outExe -Force
$sw.Stop()

$item = Get-Item -LiteralPath $outExe
$cfg = Join-Path $repo '.local\klif.toml'
Write-Host ''
Write-Host ('exe:    ' + $item.FullName)
Write-Host ('size:   {0:N1} MiB' -f ($item.Length / 1MB))
Write-Host ('built:  {0:N0} s total ({1} s UI, {2:N0} s shell + copy)' -f $sw.Elapsed.TotalSeconds, $uiSeconds, ($sw.Elapsed.TotalSeconds - $uiSeconds))
if (Test-Path -LiteralPath $cfg) {
    Write-Host ('config: ' + $cfg + ' (found from the exe by walking up to the repo root)')
} else {
    Write-Host ('config: ' + $cfg + ' not found; the exe falls back to %APPDATA%\KLIF\klif.toml (or KLIF_CONFIG)') -ForegroundColor Yellow
}
Write-Host ('log:    klif-shell.log next to klif.toml')
