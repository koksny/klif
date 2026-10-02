<#
.SYNOPSIS
    Exports the KLIF launcher catalog (cards, profiles, presets, availability, launch rules) as JSON
    without running the launcher.

.DESCRIPTION
    Parses the launcher script with the PowerShell language parser and evaluates ONLY the
    catalog-building part of it:

      * every top-level function definition (defined, never called unless a kept statement calls it)
      * every top-level statement that passes a purity check (see KxStatementReason)

    A statement is skipped (and listed under meta.skippedStatements) when it
      * references an invocation-mode variable ($Snapshot, $Gui, $ValidateManifest, $ResetState,
        $NoLaunch, $NoPersist, $PSBoundParameters, $StatePath, $PSScriptRoot, $PSCommandPath),
      * contains exit / throw,
      * dot-sources or uses the call operator, writes the environment, or touches static members of
        types that are not on a small read-only allow-list,
      * calls (transitively through launcher functions) any command that is not on a small read-only
        allow-list (Where-Object, Select-Object, Test-Path, Join-Path, ...).

    As a second line of defence, the cmdlets that could write, start, kill or touch the network are
    shadowed by functions that throw before any launcher code runs.

    The saved launcher state is never read unless -StateFile is given, and then only a whitelist of
    non-secret fields is exported (the API key is reduced to a presence flag).

    Output is one JSON document, ASCII only (non-ASCII is \u-escaped), on stdout (or -OutFile, UTF-8
    without BOM). NOTE: when launched from a PowerShell host, ">" (observed on 5.1.26100) prepends a
    UTF-8 BOM and turns LF into CRLF; strict JSON parsers (serde_json) reject the BOM. Capture the
    process stdout through a pipe (std::process::Command) or use -OutFile.

    This script contains no machine paths: every path in the output comes from the launcher.

.PARAMETER LauncherPath
    Path of Launch-LLM.ps1 (no default).
.PARAMETER GuiShellPath
    Path of Launch-LLM.Gui.ps1. Default: beside the launcher.
.PARAMETER StateFile
    Optional launcher state JSON; exports sanitised selections (never the API key).
.PARAMETER OutFile
    Write the JSON here instead of stdout.
.PARAMETER SkipManifest
    Omit the manifest section (the launcher's own Test-Manifest still runs because it is a pure top-level statement).
.PARAMETER LoadOnly
    Load the catalog into the caller's scope and return (dot-source this script to use it as a library).
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [string] $LauncherPath,
    [string] $GuiShellPath,
    [string] $StateFile,
    [string] $OutFile,
    [switch] $SkipManifest,
    [switch] $LoadOnly
)

$ErrorActionPreference = 'Stop'
$kxWatch = [System.Diagnostics.Stopwatch]::StartNew()

# --------------------------------------------------------------------------------------------
# AST helpers
# --------------------------------------------------------------------------------------------
function KxParse([string] $Path) {
    $tokens = $null
    $errs = $null
    $tree = [System.Management.Automation.Language.Parser]::ParseFile($Path, [ref] $tokens, [ref] $errs)
    if ($errs -and $errs.Count -gt 0) { throw "Parse error in ${Path}: $($errs[0].Message)" }
    return $tree
}

function KxFlatten([string] $Text, [int] $Max = 90) {
    $s = ($Text -replace '\s+', ' ').Trim()
    if ($s.Length -gt $Max) { $s = $s.Substring(0, $Max - 3) + '...' }
    return $s
}

function KxSha256File([string] $Path) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $fs = [System.IO.File]::OpenRead($Path)
        try { return ([BitConverter]::ToString($sha.ComputeHash($fs))).Replace('-', '').ToLowerInvariant() } finally { $fs.Dispose() }
    } finally { $sha.Dispose() }
}

function KxSha256Text([string] $Text) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes(($Text -replace "`r`n", "`n"))
        return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    } finally { $sha.Dispose() }
}

function KxProp($Object, [string] $Name) {
    $p = $Object.PSObject.Properties[$Name]
    if ($p) { return $p.Value }
    return $null
}

# Allow-lists for the purity analysis. Everything else counts as "may act".
$kxAllowedCmdlets = @('Where-Object', 'ForEach-Object', 'Select-Object', 'Group-Object', 'Sort-Object', 'Measure-Object', 'Join-Path', 'Split-Path', 'Test-Path', 'Out-Null')
$kxAllowedStatic = '^(math|System\.Math|array|System\.Array|string|System\.String|IO\.Path|System\.IO\.Path|System\.Management\.Automation\.Language\.Parser)$'
$kxAllowedInstance = @('Split', 'Substring', 'Contains', 'ContainsKey', 'Add', 'GetEnumerator', 'Trim', 'TrimEnd', 'TrimStart', 'Replace', 'PadRight', 'PadLeft', 'StartsWith', 'EndsWith', 'IndexOf', 'LastIndexOf', 'ToString', 'ToLowerInvariant', 'ToUpperInvariant', 'Remove')
$kxModeVars = @('Snapshot', 'Gui', 'ValidateManifest', 'ResetState', 'NoLaunch', 'NoPersist', 'PSBoundParameters', 'StatePath', 'PSCommandPath', 'PSScriptRoot')
$kxGuarded = @(
    'Move-Item', 'Copy-Item', 'New-Item', 'Remove-Item', 'Rename-Item', 'Set-Content', 'Add-Content', 'Out-File',
    'Clear-Content', 'Get-Content', 'Set-Clipboard', 'Start-Process', 'Stop-Process', 'Start-Job', 'Invoke-Expression',
    'Invoke-Command', 'Invoke-WebRequest', 'Invoke-RestMethod', 'Get-NetTCPConnection', 'Get-CimInstance',
    'Get-WmiObject', 'Set-ItemProperty', 'New-ItemProperty', 'Remove-ItemProperty'
)

$kxFunctions = @{}
$kxFunctionOrder = New-Object System.Collections.Generic.List[string]
$kxFnReason = @{}

function KxSingleReason($Node) {
    if ($Node -is [System.Management.Automation.Language.ExitStatementAst]) { return 'exit statement' }
    if ($Node -is [System.Management.Automation.Language.ThrowStatementAst]) { return 'throw statement' }
    if ($Node -is [System.Management.Automation.Language.VariableExpressionAst]) {
        $vp = $Node.VariablePath
        if ($vp.IsDriveQualified -and $vp.DriveName -eq 'env') { return 'environment variable access' }
        return $null
    }
    if ($Node -is [System.Management.Automation.Language.CommandAst]) {
        $op = [string] $Node.InvocationOperator
        if ($op -eq 'Dot') { return 'dot-source' }
        if ($op -eq 'Ampersand') { return 'call operator' }
        $name = $Node.GetCommandName()
        if (-not $name) { return 'dynamic command' }
        if ($kxAllowedCmdlets -contains $name) { return $null }
        if ($kxFunctions.ContainsKey($name)) {
            $inner = KxFnReason $name
            if ($inner) { return "calls ${name}: $inner" }
            return $null
        }
        return "command $name"
    }
    if ($Node -is [System.Management.Automation.Language.InvokeMemberExpressionAst]) {
        $member = if ($Node.Member -is [System.Management.Automation.Language.StringConstantExpressionAst]) { $Node.Member.Value } else { '<dynamic>' }
        if ($Node.Static -and $Node.Expression -is [System.Management.Automation.Language.TypeExpressionAst]) {
            $tn = $Node.Expression.TypeName.FullName
            if ($tn -match $kxAllowedStatic) { return $null }
            return "static call [$tn]::$member"
        }
        if ($kxAllowedInstance -contains $member) { return $null }
        return "method call .$member()"
    }
    if ($Node -is [System.Management.Automation.Language.MemberExpressionAst]) {
        if ($Node.Static -and $Node.Expression -is [System.Management.Automation.Language.TypeExpressionAst]) {
            $tn = $Node.Expression.TypeName.FullName
            if ($tn -match $kxAllowedStatic -or $tn -match '^(System\.)?Environment$') { return $null }
            return "static member [$tn]::$($Node.Member.Extent.Text)"
        }
        return $null
    }
    if ($Node -is [System.Management.Automation.Language.AssignmentStatementAst]) {
        if ($Node.Left -is [System.Management.Automation.Language.MemberExpressionAst] -and $Node.Left.Static) { return 'assignment to static member' }
        return $null
    }
    return $null
}

function KxNodeReason($Node) {
    foreach ($n in $Node.FindAll({ $true }, $true)) {
        $r = KxSingleReason $n
        if ($r) { return $r }
    }
    return $null
}

function KxFnReason([string] $Name) {
    if ($kxFnReason.ContainsKey($Name)) { return $kxFnReason[$Name] }
    $kxFnReason[$Name] = $null   # assume pure while analysing (call cycles)
    $reason = KxNodeReason $kxFunctions[$Name].Body
    $kxFnReason[$Name] = $reason
    return $reason
}

function KxStatementReason($Statement) {
    foreach ($v in $Statement.FindAll({ $args[0] -is [System.Management.Automation.Language.VariableExpressionAst] }, $true)) {
        if ($kxModeVars -contains $v.VariablePath.UserPath) { return "references invocation-mode variable `$$($v.VariablePath.UserPath)" }
    }
    return (KxNodeReason $Statement)
}

# --------------------------------------------------------------------------------------------
# Load the catalog part of the launcher into THIS scope
# --------------------------------------------------------------------------------------------
$kxLauncherFull = (Resolve-Path -LiteralPath $LauncherPath).ProviderPath
$kxWorkshop = Split-Path -Parent $kxLauncherFull
if (-not $GuiShellPath) { $GuiShellPath = Join-Path $kxWorkshop 'Launch-LLM.Gui.ps1' }
$kxAst = KxParse $kxLauncherFull
if ($kxAst.BeginBlock -or $kxAst.ProcessBlock -or $kxAst.DynamicParamBlock) { throw 'Launcher uses named blocks; unsupported.' }

foreach ($kxS in $kxAst.EndBlock.Statements) {
    if ($kxS -is [System.Management.Automation.Language.FunctionDefinitionAst]) {
        $kxFunctions[$kxS.Name] = $kxS
        $kxFunctionOrder.Add($kxS.Name)
    }
}

$kxSkipped = New-Object System.Collections.Generic.List[object]
$kxKeep = New-Object System.Collections.Generic.List[object]
foreach ($kxS in $kxAst.EndBlock.Statements) {
    if ($kxS -is [System.Management.Automation.Language.FunctionDefinitionAst]) { continue }
    $kxWhy = KxStatementReason $kxS
    if ($kxWhy) {
        $kxSkipped.Add([ordered]@{ line = $kxS.Extent.StartLineNumber; endLine = $kxS.Extent.EndLineNumber; reason = $kxWhy; text = (KxFlatten $kxS.Extent.Text 80) })
    } else {
        $kxKeep.Add($kxS)
    }
}

# stand-ins for the launcher parameters and for the one runtime probe we neutralise
$kxStatePathDefault = ''
if ($kxAst.ParamBlock) {
    foreach ($kxP in $kxAst.ParamBlock.Parameters) {
        if ($kxP.Name.VariablePath.UserPath -eq 'StatePath' -and $kxP.DefaultValue -is [System.Management.Automation.Language.StringConstantExpressionAst]) {
            $kxStatePathDefault = $kxP.DefaultValue.Value
        }
    }
}
$StatePath = $kxStatePathDefault
$Backend = 'HIP'
$Hardware = '9070'
$StartModel = $null
$NoLaunch = $false
$NoPersist = $true
$Snapshot = $false
$ValidateManifest = $false
$ResetState = $false
$Gui = $false
$busyInfo = [pscustomobject]@{ Ports = @{}; Owners = @{}; Processes = @(); Summary = '' }   # replaces Get-BusyInfo (network probe)

# second line of defence: shadow anything that could write, start/kill a process or use the network
foreach ($kxG in $kxGuarded) {
    . ([scriptblock]::Create("function $kxG { throw 'KLIF exporter guard: $kxG is blocked while the launcher catalog is evaluated.' }"))
}

foreach ($kxName in $kxFunctionOrder) {
    . ([scriptblock]::Create($kxFunctions[$kxName].Extent.Text))
}
foreach ($kxS in $kxKeep) {
    try {
        . ([scriptblock]::Create($kxS.Extent.Text))
    } catch {
        throw "Launcher statement at line $($kxS.Extent.StartLineNumber) failed: $($_.Exception.Message)"
    }
}
foreach ($kxNeed in 'cards', 'profiles', 'launcherPresets', 'cacheValues', 'portValues', 'hardwareValues', 'contextIndexByModel', 'bindAddress', 'languageContextTags') {
    if (-not (Test-Path -LiteralPath "variable:$kxNeed")) { throw "Catalog variable `$$kxNeed was not produced; the launcher layout changed." }
}
$kxLoadMs = $kxWatch.ElapsedMilliseconds
$kxLaps = [ordered]@{ load = $kxLoadMs }
$kxLapLast = $kxLoadMs
function KxLap([string] $Name) { $now = $kxWatch.ElapsedMilliseconds; $script:kxLaps[$Name] = $now - $script:kxLapLast; $script:kxLapLast = $now }

if ($LoadOnly) { return }

# --------------------------------------------------------------------------------------------
# JSON writer (ASCII only, ordered, no dependency on ConvertTo-Json quirks)
# --------------------------------------------------------------------------------------------
function KxJsonString([System.Text.StringBuilder] $Sb, [string] $S) {
    [void] $Sb.Append('"')
    if ([regex]::IsMatch($S, '^[\x20\x21\x23-\x5b\x5d-\x7e]*$')) {
        [void] $Sb.Append($S)
    } else {
        foreach ($ch in $S.ToCharArray()) {
            $c = [int] $ch
            if ($c -eq 0x22) { [void] $Sb.Append('\"') }
            elseif ($c -eq 0x5c) { [void] $Sb.Append('\\') }
            elseif ($c -eq 0x0a) { [void] $Sb.Append('\n') }
            elseif ($c -eq 0x0d) { [void] $Sb.Append('\r') }
            elseif ($c -eq 0x09) { [void] $Sb.Append('\t') }
            elseif ($c -lt 0x20 -or $c -gt 0x7e) { [void] $Sb.Append(('\u{0:x4}' -f $c)) }
            else { [void] $Sb.Append($ch) }
        }
    }
    [void] $Sb.Append('"')
}

function KxJson([System.Text.StringBuilder] $Sb, $Value, [int] $Depth, [int] $Expand) {
    if ($null -eq $Value) { [void] $Sb.Append('null'); return }
    if ($Value -is [string]) { KxJsonString $Sb $Value; return }
    if ($Value -is [char]) { KxJsonString $Sb ([string] $Value); return }
    if ($Value -is [bool]) { [void] $Sb.Append($(if ($Value) { 'true' } else { 'false' })); return }
    if ($Value -is [datetime]) { KxJsonString $Sb ($Value.ToUniversalTime().ToString('o')); return }
    if ($Value -is [int] -or $Value -is [long] -or $Value -is [int16] -or $Value -is [byte] -or $Value -is [uint32] -or $Value -is [uint64]) {
        [void] $Sb.Append([Convert]::ToString($Value, [System.Globalization.CultureInfo]::InvariantCulture)); return
    }
    if ($Value -is [double] -or $Value -is [single] -or $Value -is [decimal]) {
        [void] $Sb.Append(([double] $Value).ToString('R', [System.Globalization.CultureInfo]::InvariantCulture)); return
    }
    if ($Value -is [System.Collections.IDictionary]) {
        $first = $true
        [void] $Sb.Append('{')
        foreach ($k in @($Value.Keys)) {
            if (-not $first) { [void] $Sb.Append(',') }
            if ($Depth -lt $Expand) { [void] $Sb.Append("`n"); [void] $Sb.Append(' ' * (2 * ($Depth + 1))) }
            KxJsonString $Sb ([string] $k)
            [void] $Sb.Append(':')
            if ($Depth -lt $Expand) { [void] $Sb.Append(' ') }
            KxJson $Sb $Value[$k] ($Depth + 1) $Expand
            $first = $false
        }
        if (-not $first -and $Depth -lt $Expand) { [void] $Sb.Append("`n"); [void] $Sb.Append(' ' * (2 * $Depth)) }
        [void] $Sb.Append('}')
        return
    }
    if ($Value -is [System.Collections.IEnumerable]) {
        $first = $true
        [void] $Sb.Append('[')
        foreach ($item in $Value) {
            if (-not $first) { [void] $Sb.Append(',') }
            if ($Depth -lt $Expand) { [void] $Sb.Append("`n"); [void] $Sb.Append(' ' * (2 * ($Depth + 1))) }
            KxJson $Sb $item ($Depth + 1) $Expand
            $first = $false
        }
        if (-not $first -and $Depth -lt $Expand) { [void] $Sb.Append("`n"); [void] $Sb.Append(' ' * (2 * $Depth)) }
        [void] $Sb.Append(']')
        return
    }
    KxJsonString $Sb ([string] $Value)
}

# --------------------------------------------------------------------------------------------
# Facts read from the launcher through its own functions
# --------------------------------------------------------------------------------------------
$kxExists = @{}
function KxExists([string] $Path) {
    if (-not $kxExists.ContainsKey($Path)) { $kxExists[$Path] = [bool] (Test-Path -LiteralPath $Path -PathType Leaf) }
    return $kxExists[$Path]
}

# Cap probe: ask the launcher's own Limit-PromptCacheForSelection what it does for (card, context).
function KxProbePromptCap($Card, [int] $Context) {
    $saved = @{
        f = $script:selectedFamily; s = $script:selectedSize; q = $script:selectedQuant
        c = $script:promptCacheMiB; m = $script:message; i = $contextIndexByModel[$Card.Id]
    }
    try {
        $script:selectedFamily = $Card.FamilyId
        $script:selectedSize = $Card.SizeLabel
        $script:selectedQuant = $Card.Quant
        $contextIndexByModel[$Card.Id] = [array]::IndexOf(@($Card.Contexts), $Context)
        $script:promptCacheMiB = 1048576
        $changed = Limit-PromptCacheForSelection
        if ($changed) { return [int] $script:promptCacheMiB }
        return $null
    } finally {
        $script:selectedFamily = $saved.f; $script:selectedSize = $saved.s; $script:selectedQuant = $saved.q
        $script:promptCacheMiB = $saved.c; $script:message = $saved.m; $contextIndexByModel[$Card.Id] = $saved.i
    }
}

$kxCardById = @{}
foreach ($kxC in $cards) { $kxCardById[$kxC.Id] = $kxC }

$kxWarnings = New-Object System.Collections.Generic.List[string]

# ---- cards ----
$kxCardsOut = New-Object System.Collections.Generic.List[object]
foreach ($kxC in $cards) {
    $kxKind = KxProp $kxC 'Kind'
    $kxSd = [bool] (Test-SdServerCard $kxC)
    $kxCtx = New-Object System.Collections.Generic.List[object]
    foreach ($kxV in $kxC.Contexts) {
        $kxE = [ordered]@{ value = [int] $kxV; label = (Format-Context ([int] $kxV)) }
        if ([int] $kxV -ge 1000000) {
            $kxE['width'] = [int] [math]::Floor([int] $kxV / 10000)
            $kxE['height'] = [int] ([int] $kxV % 10000)
        }
        $kxCtx.Add($kxE)
    }
    $kxStates = [ordered]@{}
    foreach ($kxB in 'HIP', 'Vulkan') {
        $kxRow = [ordered]@{}
        foreach ($kxH in $hardwareValues) {
            $kxSt = Get-CombinationState $kxC $kxB $kxH -IgnorePort
            $kxRow[[string] $kxH] = [ordered]@{ state = $kxSt; short = (Get-ShortState $kxSt) }
        }
        $kxStates[$kxB] = $kxRow
    }
    $kxCaps = [ordered]@{}
    if (-not $kxSd) {
        foreach ($kxV in $kxC.Contexts) {
            $kxCap = KxProbePromptCap $kxC ([int] $kxV)
            if ($null -ne $kxCap) { $kxCaps[[string] $kxV] = $kxCap }
        }
    }
    $kxBonsaiKv = $null
    if (Test-BonsaiCard $kxC) {
        $kxBonsaiKv = [ordered]@{}
        foreach ($kxV in $kxC.Contexts) { $kxBonsaiKv[[string] $kxV] = Get-BonsaiCacheTypeForContext ([int] $kxV) }
    }
    $kxDefaultIdx = [int] $contextIndexByModel[$kxC.Id]
    $kxCardsOut.Add([ordered]@{
        id = $kxC.Id
        familyId = $kxC.FamilyId
        family = $kxC.Family
        sizeLabel = $kxC.SizeLabel
        name = $kxC.Name
        quant = $kxC.Quant
        fileSize = $kxC.Size
        memoryNote = $kxC.Memory
        kind = $(if ($kxKind) { [string] $kxKind } else { 'Llm' })
        isSdServer = $kxSd
        isDenseQwen27 = [bool] (Test-DenseQwen27Card $kxC)
        isBonsai = [bool] (Test-BonsaiCard $kxC)
        maxContext = [int] $kxC.MaxContext
        contexts = $kxCtx
        defaultContextIndex = $kxDefaultIdx
        defaultContext = [int] $kxC.Contexts[$kxDefaultIdx]
        promptCacheCaps = $kxCaps
        bonsaiKvByContext = $kxBonsaiKv
        states = $kxStates
    })
}

KxLap 'cards'
# ---- profiles ----
$kxProfilesOut = New-Object System.Collections.Generic.List[object]
$kxOrphans = New-Object System.Collections.Generic.List[string]
$kxOrphanIds = New-Object System.Collections.Generic.List[string]
$kxStateCount = [ordered]@{}
foreach ($kxP in $profiles) {
    $kxCard = $kxCardById[$kxP.ModelId]
    $kxSd = [bool] (Test-SdServerCard $kxCard)
    if ($null -eq $kxCard) {
        $kxOrphans.Add("$($kxP.ModelId)|$($kxP.Backend)|$($kxP.Hardware)|$($kxP.Context)")
        if (-not $kxOrphanIds.Contains([string] $kxP.ModelId)) { $kxOrphanIds.Add([string] $kxP.ModelId) }
    }
    $kxPairs = New-Object System.Collections.Generic.List[object]
    foreach ($kxE in $kxP.Arguments.GetEnumerator()) { $kxPairs.Add(@([string] $kxE.Key, $kxE.Value)) }
    $kxState = Get-ProfileState $kxP -IgnorePort
    if (-not $kxStateCount.Contains($kxState)) { $kxStateCount[$kxState] = 0 }
    $kxStateCount[$kxState] = [int] $kxStateCount[$kxState] + 1
    $kxProfilesOut.Add([ordered]@{
        key = "$($kxP.ModelId)|$($kxP.Backend)|$($kxP.Hardware)|$($kxP.Context)"
        cardId = $kxP.ModelId
        backend = $kxP.Backend
        hardware = $kxP.Hardware
        context = [int] $kxP.Context
        contextLabel = (Format-Context ([int] $kxP.Context))
        script = $kxP.Script
        args = $kxPairs
        binary = $kxP.Binary
        model = $kxP.Model
        buildCommand = $kxP.BuildCommand
        label = $kxP.Label
        endpoint = $kxP.Endpoint
        serverPort = [int] $kxP.ServerPort
        injects = [ordered]@{
            promptCacheMiB = (-not $kxSd)
            port = (-not $kxSd)
            cacheType = (-not $kxSd)
            vision = [bool] (Test-DenseQwen27Card $kxCard)
        }
        orphan = ($null -eq $kxCard)
        state = $kxState
        stateShort = (Get-ShortState $kxState)
        scriptExists = (KxExists $kxP.Script)
        modelExists = (KxExists $kxP.Model)
        binaryExists = (KxExists $kxP.Binary)
    })
}

KxLap 'profiles'
if ($kxOrphans.Count -gt 0) {
    $kxWarnings.Add("$($kxOrphans.Count) profile(s) reference card id(s) with no card in `$cards ($($kxOrphanIds -join ', ')); the TUI and GUI can never select them (profiles[].orphan = true)")
}
# ---- presets (same resolution as the GUI: card by family/size/quant, profile by card/backend/hardware/context) ----
$kxPresetsOut = New-Object System.Collections.Generic.List[object]
foreach ($kxR in $launcherPresets) {
    $kxCard = $cards | Where-Object { $_.FamilyId -eq $kxR.FamilyId -and $_.SizeLabel -eq $kxR.Size -and $_.Quant -eq $kxR.Quant } | Select-Object -First 1
    $kxProf = $null
    $kxResolved = [ordered]@{ cardId = $null; profileKey = $null; state = 'UNSUPPORTED'; contextIndex = -1 }
    if ($kxCard) {
        $kxProf = $profiles | Where-Object { $_.ModelId -eq $kxCard.Id -and $_.Backend -eq $kxR.Backend -and $_.Hardware -eq $kxR.Hardware -and $_.Context -eq $kxR.Context } | Select-Object -First 1
        $kxResolved['cardId'] = $kxCard.Id
        $kxResolved['contextIndex'] = [array]::IndexOf(@($kxCard.Contexts), [int] $kxR.Context)
        if ($kxProf) {
            $kxResolved['profileKey'] = "$($kxProf.ModelId)|$($kxProf.Backend)|$($kxProf.Hardware)|$($kxProf.Context)"
            $kxResolved['state'] = Get-ProfileState $kxProf -IgnorePort
        }
    }
    $kxSd = if ($kxCard) { [bool] (Test-SdServerCard $kxCard) } else { $false }
    $kxPresetCache = KxProp $kxR 'CacheType'
    $kxApplyCache = $null
    if (-not $kxSd -and [int] $kxR.CacheMiB -in $cacheValues) { $kxApplyCache = [int] $kxR.CacheMiB }
    $kxApplyPort = $null
    if (-not $kxSd -and [int] $kxR.Port -in $portValues) { $kxApplyPort = [int] $kxR.Port }
    $kxApplyKv = $null
    if ($kxPresetCache -in @('q4_0', 'q8_0')) { $kxApplyKv = [string] $kxPresetCache }
    elseif ($kxCard -and (Test-BonsaiCard $kxCard)) { $kxApplyKv = Get-BonsaiCacheTypeForContext ([int] $kxR.Context) }
    $kxCappedCache = $kxApplyCache
    if ($null -ne $kxApplyCache -and $kxCard) {
        $kxProbe = KxProbePromptCap $kxCard ([int] $kxR.Context)
        if ($null -ne $kxProbe -and $kxApplyCache -gt $kxProbe) { $kxCappedCache = $kxProbe }
    }
    $kxPresetsOut.Add([ordered]@{
        id = $kxR.Id
        label = $kxR.Label
        familyId = $kxR.FamilyId
        sizeLabel = $kxR.Size
        quant = $kxR.Quant
        backend = $kxR.Backend
        hardware = $kxR.Hardware
        context = [int] $kxR.Context
        contextLabel = (Format-Context ([int] $kxR.Context))
        cacheMiB = [int] $kxR.CacheMiB
        cacheType = $(if ($kxPresetCache) { [string] $kxPresetCache } else { $null })
        port = [int] $kxR.Port
        resolved = $kxResolved
        applies = [ordered]@{
            promptCacheMiB = $kxCappedCache
            serverPort = $kxApplyPort
            cacheType = $kxApplyKv
            note = 'null = leave the current setting untouched (sd-server cards ignore cache/port; only Bonsai or a preset with CacheType sets KV)'
        }
    })
}

KxLap 'presets'
# ---- wrappers (parameter lists the launcher itself collected in Test-Manifest) ----
$kxManifest = $null
if (-not $SkipManifest) {
    if (Test-Path -LiteralPath variable:validation) { $kxManifest = $validation } else { $kxManifest = Test-Manifest }
}
$kxWrappers = [ordered]@{}
if ($script:paramCache) {
    foreach ($kxK in @($script:paramCache.Keys | Sort-Object)) {
        $kxWrappers[[string] $kxK] = [ordered]@{ exists = (KxExists ([string] $kxK)); params = @($script:paramCache[$kxK]) }
    }
}

# ---- GUI side facts (extracted from the GUI AST, not evaluated) ----
$kxGuiFns = @{}
$kxGuiOk = Test-Path -LiteralPath $GuiShellPath -PathType Leaf
$kxGuiAst = $null
$kxGuiFull = $null
if ($kxGuiOk) {
    $kxGuiFull = (Resolve-Path -LiteralPath $GuiShellPath).ProviderPath
    $kxGuiAst = KxParse $kxGuiFull
    foreach ($kxF in $kxGuiAst.FindAll({ $args[0] -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $true)) { $kxGuiFns[$kxF.Name] = $kxF }
} else {
    $kxWarnings.Add("GUI shell not found beside the launcher: $GuiShellPath")
}

function KxStrings($Node) {
    $out = New-Object System.Collections.Generic.List[string]
    foreach ($n in $Node.FindAll({ $args[0] -is [System.Management.Automation.Language.StringConstantExpressionAst] }, $true)) { $out.Add([string] $n.Value) }
    return $out.ToArray()
}

$kxRuntimeLogs = $null
$kxFixedArgs = @()
$kxSkipNames = @()
$kxInjectFlags = @()
$kxSessionFmt = $null
$kxDateFmt = $null
$kxSanitize = $null
$kxSdOnlyKinds = @('Image', 'Video')
$kxGenModeCards = @()
$kxStartProc = [ordered]@{}
if ($kxGuiOk) {
    # runtime log directory: first constant assignment to $logDir
    foreach ($kxA in $kxGuiAst.FindAll({ $args[0] -is [System.Management.Automation.Language.AssignmentStatementAst] }, $true)) {
        if ($kxA.Left -is [System.Management.Automation.Language.VariableExpressionAst] -and $kxA.Left.VariablePath.UserPath -eq 'logDir') {
            $kxLits = @(KxStrings $kxA.Right)
            if ($kxLits.Count -eq 1) { $kxRuntimeLogs = $kxLits[0]; break }
        }
    }
    if (-not $kxRuntimeLogs) { $kxWarnings.Add('runtime-logs directory could not be extracted from the GUI shell') }

    if ($kxGuiFns.ContainsKey('Start-GuiLaunch')) {
        $kxSg = $kxGuiFns['Start-GuiLaunch']
        $kxArgAssign = $kxSg.FindAll({ $args[0] -is [System.Management.Automation.Language.AssignmentStatementAst] -and $args[0].Left -is [System.Management.Automation.Language.VariableExpressionAst] -and $args[0].Left.VariablePath.UserPath -eq 'argList' }, $true) | Select-Object -First 1
        if ($kxArgAssign) {
            $kxLit = $kxArgAssign.Right.FindAll({ $args[0] -is [System.Management.Automation.Language.ArrayLiteralAst] }, $true) | Select-Object -First 1
            if ($kxLit) { $kxFixedArgs = @($kxLit.Elements | Where-Object { $_ -is [System.Management.Automation.Language.StringConstantExpressionAst] } | ForEach-Object { [string] $_.Value }) }
        }
        $kxIn = $kxSg.FindAll({ $args[0] -is [System.Management.Automation.Language.BinaryExpressionAst] -and @('Iin', 'Cin', 'In') -contains [string] $args[0].Operator }, $true) | Select-Object -First 1
        if ($kxIn) { $kxSkipNames = @(KxStrings $kxIn.Right) }
        $kxAllStr = @(KxStrings $kxSg)
        $kxInjectFlags = @($kxAllStr | Where-Object { $_ -match '^-[A-Za-z]+$' -and ($kxFixedArgs -notcontains $_) })
        $kxFmtOp = $kxSg.FindAll({ $args[0] -is [System.Management.Automation.Language.BinaryExpressionAst] -and [string] $args[0].Operator -eq 'Format' }, $true) | Select-Object -First 1
        if ($kxFmtOp) { $kxSessionFmt = @(KxStrings $kxFmtOp.Left)[0] }
        $kxRepl = $kxSg.FindAll({ $args[0] -is [System.Management.Automation.Language.BinaryExpressionAst] -and ([string] $args[0].Operator -eq 'Ireplace' -or [string] $args[0].Operator -eq 'Replace') }, $true) | Select-Object -First 1
        if ($kxRepl) { $kxSanitize = @(KxStrings $kxRepl.Right) }
        foreach ($kxCmd in $kxSg.FindAll({ $args[0] -is [System.Management.Automation.Language.CommandAst] }, $true)) {
            $kxCn = $kxCmd.GetCommandName()
            if ($kxCn -eq 'Get-Date') {
                $kxEls = @($kxCmd.CommandElements)
                for ($kxI = 0; $kxI -lt $kxEls.Count - 1; $kxI++) {
                    if ($kxEls[$kxI] -is [System.Management.Automation.Language.CommandParameterAst] -and $kxEls[$kxI].ParameterName -eq 'Format') { $kxDateFmt = @(KxStrings $kxEls[$kxI + 1])[0] }
                }
            }
            if ($kxCn -eq 'Start-Process' -and $kxStartProc.Count -eq 0) {
                $kxEls = @($kxCmd.CommandElements)
                for ($kxI = 1; $kxI -lt $kxEls.Count; $kxI++) {
                    if ($kxEls[$kxI] -is [System.Management.Automation.Language.CommandParameterAst]) {
                        $kxName = $kxEls[$kxI].ParameterName
                        if ($kxI + 1 -lt $kxEls.Count -and $kxEls[$kxI + 1] -isnot [System.Management.Automation.Language.CommandParameterAst]) {
                            $kxStartProc[$kxName] = (KxFlatten $kxEls[$kxI + 1].Extent.Text 60); $kxI++
                        } else { $kxStartProc[$kxName] = $true }
                    }
                }
            }
        }
    } else { $kxWarnings.Add('Start-GuiLaunch not found in the GUI shell') }

    if ($kxGuiFns.ContainsKey('Get-GuiProfile')) {
        $kxNotIn = $kxGuiFns['Get-GuiProfile'].FindAll({ $args[0] -is [System.Management.Automation.Language.BinaryExpressionAst] -and @('Inotin', 'Cnotin', 'Notin') -contains [string] $args[0].Operator }, $true) | Select-Object -First 1
        if ($kxNotIn) { $kxGenModeCards = @(KxStrings $kxNotIn.Right) }
    } else { $kxWarnings.Add('Get-GuiProfile not found in the GUI shell') }
}

KxLap 'guiAst'
# ---- fingerprints: any change to a rule-bearing function is visible to the consumer ----
$kxFp = [ordered]@{}
foreach ($kxN in 'New-Profile', 'Convert-ImageSizeToContext', 'Test-SdServerCard', 'Test-DenseQwen27Card', 'Test-BonsaiCard', 'Get-BonsaiCacheTypeForContext', 'Limit-PromptCacheForSelection', 'Get-Profile', 'Get-ProfileState', 'Get-EffectivePort', 'Get-CombinationState', 'Get-DeferredReason', 'Format-ProfileCommand', 'Format-Context', 'Set-LauncherLlamaApiKeyEnv', 'Get-BusyInfo', 'Test-Manifest') {
    if ($kxFunctions.ContainsKey($kxN)) { $kxFp["launcher:$kxN"] = KxSha256Text $kxFunctions[$kxN].Extent.Text } else { $kxWarnings.Add("launcher function missing: $kxN") }
}
foreach ($kxN in 'Start-GuiLaunch', 'Get-GuiProfile', 'Apply-GuiPreset', 'Get-MatchingLauncherPreset') {
    if ($kxGuiFns.ContainsKey($kxN)) { $kxFp["gui:$kxN"] = KxSha256Text $kxGuiFns[$kxN].Extent.Text }
}

# ---- state (optional, sanitised) ----
$kxStateOut = $null
if ($StateFile) {
    $kxRaw = [System.IO.File]::ReadAllText((Resolve-Path -LiteralPath $StateFile).ProviderPath)
    $kxObj = $kxRaw | ConvertFrom-Json
    $kxStateOut = [ordered]@{}
    foreach ($kxF in 'Version', 'Family', 'Size', 'Quant', 'Backend', 'Hardware', 'Context', 'PromptCacheMiB', 'ServerPort', 'GenerationMode', 'Vision', 'CacheType', 'UpdatedAt') {
        $kxV = KxProp $kxObj $kxF
        $kxStateOut[$kxF] = $kxV
    }
    $kxKeyProp = KxProp $kxObj 'ApiKey'
    $kxStateOut['apiKeyPresent'] = (-not [string]::IsNullOrWhiteSpace([string] $kxKeyProp))
    $kxKeyProp = $null
    $kxObj = $null
    $kxRaw = $null
}

# ---- families, endpoints, defaults ----
$kxFamilies = New-Object System.Collections.Generic.List[object]
foreach ($kxFo in $familyOptions) { $kxFamilies.Add([ordered]@{ id = $kxFo.FamilyId; label = $kxFo.Family }) }

$kxEndpoints = New-Object System.Collections.Generic.List[object]
$kxSeen = @{}
foreach ($kxP in $profiles) {
    $kxEk = "$($kxP.Endpoint)|$($kxP.ServerPort)"
    if (-not $kxSeen.ContainsKey($kxEk)) {
        $kxSeen[$kxEk] = $true
        $kxEndpoints.Add([ordered]@{ endpoint = $kxP.Endpoint; port = [int] $kxP.ServerPort; followsPortRow = ([int] $kxP.ServerPort -eq 7030) })
    }
}

$kxBind = [string] $bindAddress
$kxBusy = New-Object System.Collections.Generic.List[object]
foreach ($kxPt in 7030..7035) { $kxBusy.Add([ordered]@{ port = $kxPt; host = $kxBind }) }
foreach ($kxPt in 1234, 1235, 8188) { $kxBusy.Add([ordered]@{ port = $kxPt; host = '127.0.0.1' }) }

$kxRoots = [ordered]@{
    workshop = $kxWorkshop
    launcher = $kxLauncherFull
    guiShell = $kxGuiFull
    qwenLaunchers = $qwenRoot
    gemmaServer = $gemmaRoot
    sdServer = $sdRoot
    flashNext = $flashNextRoot
    runtimeLogs = $kxRuntimeLogs
    statePathDefault = $kxStatePathDefault
}

$kxDefaults = [ordered]@{
    note = 'what the launcher uses when no saved state exists (evaluated, not read from the state file)'
    family = $selectedFamily
    size = $selectedSize
    quant = $selectedQuant
    backend = $Backend
    hardware = $Hardware
    promptCacheMiB = [int] $promptCacheMiB
    serverPort = [int] $serverPort
    generationMode = $generationMode
    vision = $vision
    cacheType = $cacheType
}

$kxRules = [ordered]@{
    note = 'Rules transcribed from the GUI launch path; the *_extracted fields come from the GUI AST, the rest is guarded by meta.fingerprints and was cross-checked by Verify-KlifCatalog.ps1'
    sdServerKinds = $kxSdOnlyKinds
    denseQwen27 = [ordered]@{ familyId = 'qwen'; sizeLabel = '27B'; injectsVision = $true }
    generationModeOverrideCardIds_extracted = $kxGenModeCards
    generationModeValues = @('Thinking', 'Instruct')
    generationModeArgument = 'GenerationMode'
    promptCacheCap = 'per card/context, see cards[].promptCacheCaps (value = MiB ceiling); the GUI applies it after every selection change and the result is persisted (sticky)'
    bonsaiKv = 'cards[].bonsaiKvByContext is the launcher default; the GUI only applies it at startup without saved CacheType and on a preset without CacheType, never on a context change'
    injectedArgsOrder_extracted = $kxInjectFlags
    skipProfileArgsWhenNotSd_extracted = $kxSkipNames
    visionValues = @('on', 'off')
    kvValues = @('q4_0', 'q8_0')
}

$kxEnvelope = [ordered]@{
    note = 'GUI Start-GuiLaunch (not the TUI Enter path). Fields marked _extracted come from the GUI AST.'
    host = $(if ($kxStartProc.Contains('FilePath')) { ([string] $kxStartProc['FilePath']).Trim("'") } else { $null })
    fixedArgsBeforeScript_extracted = @($kxFixedArgs)
    windowStyle = $(if ($kxStartProc.Contains('WindowStyle')) { [string] $kxStartProc['WindowStyle'] } else { $null })
    workingDirectory = 'dirname(profile.script)'
    startProcessParameters_extracted = $kxStartProc
    argvOrder = @(
        'host fixed args', 'profile.script',
        'profile.args in order (when the card is NOT sd-server, drop Port and PromptCacheMiB)',
        'when NOT sd-server: -PromptCacheMiB <effective cache> -Port <serverPort>',
        'when card is dense Qwen 27B: -Vision <on|off>',
        'when NOT sd-server: -CacheType <q4_0|q8_0>'
    )
    argumentSeparation = 'each name and each value is its own argv token; values are not quoted by the launcher'
    environment = [ordered]@{
        variable = 'LLAMA_API_KEY'
        rule = 'always removed from the child environment first; set to the trimmed API key only when the card is NOT sd-server and the key is non-empty'
    }
    sessionName = [ordered]@{
        format_extracted = $kxSessionFmt
        dateFormat_extracted = $kxDateFmt
        args = @('<date>', 'card.id', '<effective port>')
        sanitizePattern_extracted = $(if ($kxSanitize) { $kxSanitize[0] } else { $null })
        sanitizeReplacement_extracted = $(if ($kxSanitize -and $kxSanitize.Count -gt 1) { $kxSanitize[1] } else { $null })
        effectivePort = 'profile.serverPort when it is not 7030 (pinned sd-server), else the PORT row'
    }
    logs = [ordered]@{
        directory = $kxRuntimeLogs
        stdout = '<directory>/<sessionName>.out.log'
        stderr = '<directory>/<sessionName>.err.log'
    }
}

$kxAvail = [ordered]@{
    precedence = @(
        [ordered]@{ order = 1; state = 'UNSUPPORTED'; short = 'N/A'; when = 'no profile for card+backend+hardware+context' },
        [ordered]@{ order = 2; state = 'SCRIPT MISSING'; short = 'NO SCRIPT'; when = 'profile.script is not a file' },
        [ordered]@{ order = 3; state = 'MODEL MISSING'; short = 'NO MODEL'; when = 'profile.model is not a file' },
        [ordered]@{ order = 4; state = 'BUILD REQUIRED'; short = 'BUILD'; when = 'profile.binary is not a file' },
        [ordered]@{ order = 5; state = 'BUSY'; short = 'BUSY'; when = 'runtime only: effective port is listening (skipped when IgnorePort; this exporter never probes the network)' },
        [ordered]@{ order = 6; state = 'READY'; short = 'READY'; when = 'none of the above' }
    )
    deferred = 'Get-DeferredReason always returns null in this launcher version, so DEFERRED is never produced'
    cardStatesUse = 'cards[].states is Get-CombinationState -IgnorePort at the card default context; profiles[].state is per profile'
}

$kxMeta = [ordered]@{
    exporter = 'Export-KlifCatalog.ps1'
    schema = 'klif-catalog/1'
    generatedAtUtc = [datetime]::UtcNow
    psVersion = $PSVersionTable.PSVersion.ToString()
    timingsMs = $kxLaps
    launcher = [ordered]@{ sha256 = (KxSha256File $kxLauncherFull); lastWriteTimeUtc = ([System.IO.File]::GetLastWriteTimeUtc($kxLauncherFull)); bytes = ([System.IO.FileInfo] $kxLauncherFull).Length }
    guiShell = $(if ($kxGuiOk) { [ordered]@{ sha256 = (KxSha256File $kxGuiFull); lastWriteTimeUtc = ([System.IO.File]::GetLastWriteTimeUtc($kxGuiFull)); bytes = ([System.IO.FileInfo] $kxGuiFull).Length } } else { $null })
    counts = [ordered]@{ cards = $cards.Count; profiles = $profiles.Count; orphanProfiles = $kxOrphans.Count; reachableProfiles = ($profiles.Count - $kxOrphans.Count); presets = @($launcherPresets).Count; byState = $kxStateCount }
    evaluatedStatements = $kxKeep.Count
    skippedStatements = $kxSkipped
    warnings = $kxWarnings
    fingerprints = $kxFp
}

$kxDoc = [ordered]@{
    schema = 'klif-catalog/1'
    meta = $kxMeta
    roots = $kxRoots
    network = [ordered]@{
        bindAddress = $kxBind
        defaultPort = [int] $port
        portValues = @($portValues)
        cacheValuesMiB = @($cacheValues)
        pinnedEndpoints = $kxEndpoints
        busyProbeEndpoints = $kxBusy
    }
    hardwareValues = @($hardwareValues)
    backendValues = @('HIP', 'Vulkan')
    contextPresets = [ordered]@{ languageContexts = @($languageContexts); tags = @($languageContextTags.GetEnumerator() | ForEach-Object { [ordered]@{ value = [int] $_.Key; tag = [string] $_.Value } }) }
    availability = $kxAvail
    families = $kxFamilies
    cards = $kxCardsOut
    presets = $kxPresetsOut
    profiles = $kxProfilesOut
    wrappers = $kxWrappers
    defaults = $kxDefaults
    rules = $kxRules
    envelope = $kxEnvelope
    manifest = $(if ($kxManifest) { [ordered]@{
        cards = [int] $kxManifest.Cards; profiles = [int] $kxManifest.Profiles; readyProfiles = [int] $kxManifest.ReadyProfiles
        buildRequiredProfiles = [int] $kxManifest.BuildRequiredProfiles; missingModelProfiles = [int] $kxManifest.MissingModelProfiles
        errors = @($kxManifest.Errors); warnings = @($kxManifest.Warnings)
    } } else { $null })
}
if ($null -ne $kxStateOut) { $kxDoc['state'] = $kxStateOut }

KxLap 'assemble'
$kxSb = New-Object System.Text.StringBuilder 600000
KxJson $kxSb $kxDoc 0 2
[void] $kxSb.Append("`n")
$kxJsonText = $kxSb.ToString()

if ($OutFile) {
    [System.IO.File]::WriteAllText($OutFile, $kxJsonText, (New-Object System.Text.UTF8Encoding($false)))
} else {
    [Console]::Out.Write($kxJsonText)
    [Console]::Out.Flush()
}
