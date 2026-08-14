[CmdletBinding()]
param(
    [string]$ContractFile,
    [string]$AdrFile
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($ContractFile)) {
    $ContractFile = Join-Path $PSScriptRoot '..\protocol\terminal\terminal-contract-v1.json'
}
if ([string]::IsNullOrWhiteSpace($AdrFile)) {
    $AdrFile = Join-Path $PSScriptRoot '..\docs\ADR-004-terminal-abstraction.md'
}

function Add-Error {
    param([System.Collections.Generic.List[string]]$Errors, [string]$Message)
    $Errors.Add($Message)
}

function Has-Property {
    param([object]$Object, [string]$Name)
    return $null -ne $Object -and $null -ne $Object.PSObject.Properties[$Name]
}

function Require-Property {
    param([object]$Object, [string]$Context, [string]$Name, [System.Collections.Generic.List[string]]$Errors)
    if (-not (Has-Property -Object $Object -Name $Name)) {
        Add-Error -Errors $Errors -Message "$Context is missing property: $Name"
        return $false
    }
    return $true
}

function Require-NonEmpty {
    param([object]$Object, [string]$Context, [string]$Name, [System.Collections.Generic.List[string]]$Errors)
    if (-not (Require-Property -Object $Object -Context $Context -Name $Name -Errors $Errors)) { return }
    if ([string]::IsNullOrWhiteSpace([string]$Object.$Name)) {
        Add-Error -Errors $Errors -Message "$Context.$Name must be non-empty."
    }
}

function Require-BooleanTrue {
    param([object]$Object, [string]$Context, [string]$Name, [System.Collections.Generic.List[string]]$Errors)
    if (-not (Require-Property -Object $Object -Context $Context -Name $Name -Errors $Errors)) { return }
    if ($Object.$Name -ne $true) {
        Add-Error -Errors $Errors -Message "$Context.$Name must be true."
    }
}

function Require-ArrayContains {
    param([object]$Object, [string]$Context, [string]$Name, [string[]]$Values, [System.Collections.Generic.List[string]]$Errors)
    if (-not (Require-Property -Object $Object -Context $Context -Name $Name -Errors $Errors)) { return }
    $actual = @($Object.$Name | ForEach-Object { [string]$_ })
    foreach ($value in $Values) {
        if ($actual -notcontains $value) {
            Add-Error -Errors $Errors -Message "$Context.$Name is missing required value: $value"
        }
    }
}

$errors = [System.Collections.Generic.List[string]]::new()
$contract = $null
if (-not (Test-Path -LiteralPath $ContractFile)) {
    Add-Error -Errors $errors -Message "Contract file does not exist: $ContractFile"
}
else {
    try {
        $contract = Get-Content -Raw -Encoding UTF8 -LiteralPath (Resolve-Path -LiteralPath $ContractFile).Path | ConvertFrom-Json
    }
    catch {
        Add-Error -Errors $errors -Message 'Contract file is not valid JSON.'
    }
}

$adrText = $null
if (-not (Test-Path -LiteralPath $AdrFile)) {
    Add-Error -Errors $errors -Message "ADR file does not exist: $AdrFile"
}
else {
    try {
        $adrText = Get-Content -Raw -Encoding UTF8 -LiteralPath (Resolve-Path -LiteralPath $AdrFile).Path
    }
    catch {
        Add-Error -Errors $errors -Message 'ADR file could not be read as UTF-8 text.'
    }
}

if ($null -ne $adrText) {
    if ($adrText -notmatch '(?m)^# ADR-004:') {
        Add-Error -Errors $errors -Message 'ADR must declare the ADR-004 title.'
    }
    if ($adrText -notmatch '(?m)^- Status: Accepted\s*$') {
        Add-Error -Errors $errors -Message 'ADR status must be Accepted.'
    }
    foreach ($number in 1..10) {
        if ($adrText -notmatch "(?m)^## $number\.") {
            Add-Error -Errors $errors -Message "ADR is missing required section heading: ## $number."
        }
    }
    foreach ($token in @(
        'alacritty_terminal',
        'tattoy-wezterm-term',
        'parser',
        'screen model',
        'render diff',
        'input encoder',
        'batch',
        'sequence',
        'backpressure',
        'cancellation',
        'structured error',
        'replaceable',
        'no per-character ABI'
    )) {
        if ($adrText -notmatch [regex]::Escape($token)) {
            Add-Error -Errors $errors -Message "ADR is missing required token: $token"
        }
    }
}

if ($null -ne $contract) {
    if ([int]$contract.schema_version -ne 1) {
        Add-Error -Errors $errors -Message 'schema_version must be 1.'
    }
    Require-NonEmpty -Object $contract -Context 'contract' -Name 'contract_id' -Errors $errors
    Require-NonEmpty -Object $contract -Context 'contract' -Name 'status' -Errors $errors
    if ([string]$contract.status -ne 'accepted') {
        Add-Error -Errors $errors -Message 'contract.status must be accepted.'
    }
    if (Require-Property -Object $contract -Context 'contract' -Name 'source_evidence' -Errors $errors) {
        Require-NonEmpty -Object $contract.source_evidence -Context 'source_evidence' -Name 'spike_report' -Errors $errors
        Require-NonEmpty -Object $contract.source_evidence -Context 'source_evidence' -Name 'spike_report_sha256' -Errors $errors
    }

    if (Require-Property -Object $contract -Context 'contract' -Name 'invariants' -Errors $errors) {
        $invariants = $contract.invariants
        foreach ($name in @('batch_only', 'parser_chunking_equivalent', 'snapshot_deterministic', 'no_per_character_abi', 'no_ui_dependency', 'bounded_memory', 'sequence_monotonic')) {
            Require-BooleanTrue -Object $invariants -Context 'invariants' -Name $name -Errors $errors
        }
    }

    if (Require-Property -Object $contract -Context 'contract' -Name 'stable_wire' -Errors $errors) {
        $wire = $contract.stable_wire
        Require-NonEmpty -Object $wire -Context 'stable_wire' -Name 'encoding' -Errors $errors
        Require-NonEmpty -Object $wire -Context 'stable_wire' -Name 'sequence_field' -Errors $errors
        Require-NonEmpty -Object $wire -Context 'stable_wire' -Name 'cancellation' -Errors $errors
        Require-NonEmpty -Object $wire -Context 'stable_wire' -Name 'backpressure' -Errors $errors
        Require-NonEmpty -Object $wire -Context 'stable_wire' -Name 'error_model' -Errors $errors
        Require-ArrayContains -Object $wire -Context 'stable_wire' -Name 'forbidden_values' -Values @('raw-borrowed-reference', 'unbounded-buffer', 'secret-debug-string') -Errors $errors
        Require-ArrayContains -Object $wire -Context 'stable_wire' -Name 'forbidden_values' -Values @('per-character-widget', 'language-exception') -Errors $errors
    }

    $requiredLayers = @('parser', 'screen_model', 'render_diff', 'input_encoder')
    $layerMap = @{}
    if (-not (Require-Property -Object $contract -Context 'contract' -Name 'layers' -Errors $errors)) {
        $layerMap = @{}
    }
    else {
        foreach ($layer in @($contract.layers)) {
            $id = [string]$layer.id
            if ([string]::IsNullOrWhiteSpace($id)) {
                Add-Error -Errors $errors -Message 'A layer is missing id.'
                continue
            }
            if ($layerMap.ContainsKey($id)) {
                Add-Error -Errors $errors -Message "Duplicate layer: $id"
                continue
            }
            $layerMap[$id] = $layer
            Require-BooleanTrue -Object $layer -Context "layer[$id]" -Name 'replaceable' -Errors $errors
            Require-NonEmpty -Object $layer -Context "layer[$id]" -Name 'interface' -Errors $errors
            Require-NonEmpty -Object $layer -Context "layer[$id]" -Name 'input_batch' -Errors $errors
            Require-NonEmpty -Object $layer -Context "layer[$id]" -Name 'output_batch' -Errors $errors
            Require-ArrayContains -Object $layer -Context "layer[$id]" -Name 'forbidden_dependencies' -Values @('ssh', 'sqlite', 'platform-ui') -Errors $errors
            if (Require-Property -Object $layer -Context "layer[$id]" -Name 'public_types' -Errors $errors) {
                if (@($layer.public_types).Count -eq 0) {
                    Add-Error -Errors $errors -Message "layer[$id].public_types must not be empty."
                }
            }
        }
    }
    foreach ($requiredLayer in $requiredLayers) {
        if (-not $layerMap.ContainsKey($requiredLayer)) {
            Add-Error -Errors $errors -Message "Missing required layer: $requiredLayer"
        }
    }

    if (Require-Property -Object $contract -Context 'contract' -Name 'render_op_kinds' -Errors $errors) {
        Require-ArrayContains -Object $contract -Context 'contract' -Name 'render_op_kinds' -Values @('fill', 'copy', 'clear', 'cursor', 'title') -Errors $errors
    }
    if (Require-Property -Object $contract -Context 'contract' -Name 'input_event_kinds' -Errors $errors) {
        Require-ArrayContains -Object $contract -Context 'contract' -Name 'input_event_kinds' -Values @('text', 'key', 'paste', 'mouse', 'resize') -Errors $errors
    }
}

if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Host "Terminal contract valid: id=$($contract.contract_id), layers=$(@($contract.layers).Count), batch-only=$($contract.invariants.batch_only), replaceable=true."
