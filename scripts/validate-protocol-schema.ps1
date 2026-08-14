[CmdletBinding()]
param(
    [string]$SchemaFile,
    [string]$BaselineFile,
    [switch]$RequireT007InProgress
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($SchemaFile)) {
    $SchemaFile = Join-Path $PSScriptRoot '..\protocol\schema\domain-v1.json'
}
if ([string]::IsNullOrWhiteSpace($BaselineFile)) {
    $BaselineFile = Join-Path $PSScriptRoot '..\protocol\compatibility\domain-v1-baseline.json'
}

function From-CodePoints {
    param([int[]]$CodePoints)
    return -join ($CodePoints | ForEach-Object { [char]$_ })
}

function Add-Error {
    param([System.Collections.Generic.List[string]]$Errors, [string]$Message)
    $Errors.Add($Message)
}

function Get-StringSet {
    param([object[]]$Values)
    $set = @{}
    foreach ($value in @($Values)) {
        $set[[string]$value] = $true
    }
    return $set
}

function Test-RequiredStrings {
    param(
        [string]$Context,
        [object[]]$Actual,
        [object[]]$Required,
        [System.Collections.Generic.List[string]]$Errors
    )
    $actualSet = Get-StringSet -Values $Actual
    foreach ($requiredValue in @($Required)) {
        $key = [string]$requiredValue
        if (-not $actualSet.ContainsKey($key)) {
            Add-Error -Errors $Errors -Message "$Context is missing required value: $key"
        }
    }
}

function Get-MessageByType {
    param([object[]]$Messages)
    $result = @{}
    foreach ($message in @($Messages)) {
        $type = [string]$message.type
        if (-not [string]::IsNullOrWhiteSpace($type)) {
            $result[$type] = $message
        }
    }
    return $result
}

$errors = [System.Collections.Generic.List[string]]::new()
$schema = $null
$baseline = $null
try {
    $schema = Get-Content -Raw -Encoding UTF8 -LiteralPath (Resolve-Path -LiteralPath $SchemaFile).Path | ConvertFrom-Json
} catch {
    Add-Error -Errors $errors -Message 'Schema file is not valid JSON.'
}
try {
    $baseline = Get-Content -Raw -Encoding UTF8 -LiteralPath (Resolve-Path -LiteralPath $BaselineFile).Path | ConvertFrom-Json
} catch {
    Add-Error -Errors $errors -Message 'Compatibility baseline is not valid JSON.'
}

if ($null -ne $schema -and $null -ne $baseline) {
    if ([string]$schema.protocol_id -ne [string]$baseline.protocol_id) {
        Add-Error -Errors $errors -Message 'Schema protocol_id does not match baseline.'
    }
    if ([int]$schema.schema_version -ne 1 -or [int]$baseline.schema_version -ne 1) {
        Add-Error -Errors $errors -Message 'schema_version must remain 1 for this compatibility baseline.'
    }
    if ([int]$schema.version.major -ne [int]$baseline.major) {
        Add-Error -Errors $errors -Message 'Schema major version changed incompatibly.'
    }
    if ([int]$schema.version.minimum_compatible_major -gt [int]$baseline.major) {
        Add-Error -Errors $errors -Message 'minimum_compatible_major excludes the baseline major.'
    }
    if ([int]$schema.version.minimum_compatible_minor -gt 0) {
        Add-Error -Errors $errors -Message 'minimum_compatible_minor excludes the v1.0 baseline.'
    }

    $envelopeRequired = @($schema.envelope.required_fields)
    Test-RequiredStrings -Context 'envelope.required_fields' -Actual $envelopeRequired -Required $baseline.required_envelope_fields -Errors $errors
    if ([int]$schema.envelope.max_body_bytes -lt 65536) { Add-Error -Errors $errors -Message 'max_body_bytes is below the minimum safe protocol budget.' }
    if ([int]$schema.envelope.max_message_bytes -lt [int]$schema.envelope.max_body_bytes) { Add-Error -Errors $errors -Message 'max_message_bytes must be >= max_body_bytes.' }

    $messageMap = Get-MessageByType -Messages @($schema.messages)
    $messageTypes = @($messageMap.Keys)
    Test-RequiredStrings -Context 'messages' -Actual $messageTypes -Required $baseline.required_message_types -Errors $errors
    if ($messageTypes.Count -ne @($schema.messages).Count) { Add-Error -Errors $errors -Message 'Message types must be unique.' }
    foreach ($message in @($schema.messages)) {
        $type = [string]$message.type
        if ([string]::IsNullOrWhiteSpace($type)) { Add-Error -Errors $errors -Message 'A message is missing type.'; continue }
        if (@($message.required).Count -eq 0) { Add-Error -Errors $errors -Message "Message ${type} has no required fields." }
        if ([string]$message.stability -ne 'stable') { Add-Error -Errors $errors -Message "Message ${type} is not marked stable." }
    }

    Test-RequiredStrings -Context 'capability_negotiation.feature_ids' -Actual @($schema.capability_negotiation.feature_ids) -Required $baseline.required_features -Errors $errors
    foreach ($capabilityField in @('supported_schema_majors','supported_message_types','features','max_message_bytes','max_in_flight')) {
        if (@($schema.capability_negotiation.required_fields) -notcontains $capabilityField) { Add-Error -Errors $errors -Message "Capability negotiation lacks required field: $capabilityField" }
    }

    $cancelMessage = $messageMap['request.cancel']
    if ($null -eq $cancelMessage) { Add-Error -Errors $errors -Message 'request.cancel message is missing.' }
    else { Test-RequiredStrings -Context 'request.cancel.required' -Actual @($cancelMessage.required) -Required @('request_id','reason','state') -Errors $errors }
    $cancelStates = @($schema.cancellation.states)
    Test-RequiredStrings -Context 'cancellation.states' -Actual $cancelStates -Required @('requested','acknowledged','completed','rejected') -Errors $errors
    if ($schema.cancellation.idempotent -ne $true) { Add-Error -Errors $errors -Message 'Cancellation must be idempotent.' }

    $flowMessage = $messageMap['flow.window_update']
    if ($null -eq $flowMessage) { Add-Error -Errors $errors -Message 'flow.window_update message is missing.' }
    else { Test-RequiredStrings -Context 'flow.window_update.required' -Actual @($flowMessage.required) -Required @('stream_id','credit_bytes','credit_messages','sequence') -Errors $errors }
    if ($schema.flow_control.sender_must_not_exceed_credit -ne $true) { Add-Error -Errors $errors -Message 'Flow control must prohibit sender credit overrun.' }

    $errorMessage = $messageMap['error.structured']
    if ($null -eq $errorMessage) { Add-Error -Errors $errors -Message 'error.structured message is missing.' }
    else { Test-RequiredStrings -Context 'error.structured.required' -Actual @($errorMessage.required) -Required @('code','class','retryable','message_key','details','request_id') -Errors $errors }
    Test-RequiredStrings -Context 'structured_errors.classes' -Actual @($schema.structured_errors.classes) -Required @('cancelled','timeout','protocol','resource_exhausted') -Errors $errors

    Test-RequiredStrings -Context 'ffi.required_fields' -Actual @($baseline.required_abi_fields) -Required @('schema_version','message_type','byte_len','request_id','cancel','backpressure','error_code') -Errors $errors
    if ([int]$schema.ffi.abi_version -ne 1) { Add-Error -Errors $errors -Message 'FFI abi_version must be 1.' }
    if ([string]$schema.ffi.handle_type -ne 'opaque-u64') { Add-Error -Errors $errors -Message 'FFI handle_type must remain opaque-u64.' }
    if (@($schema.ffi.functions).Count -lt 5) { Add-Error -Errors $errors -Message 'FFI function surface is incomplete.' }
    foreach ($forbidden in @('raw-borrowed-reference','language-exception','secret-debug-string','unbounded-buffer')) {
        if (@($schema.ffi.forbidden_values) -notcontains $forbidden) { Add-Error -Errors $errors -Message "FFI forbidden value is missing: $forbidden" }
    }
    if ([string]$schema.wasm.exported_namespace -ne 'ssh_client_domain_v1') { Add-Error -Errors $errors -Message 'WASM namespace must be ssh_client_domain_v1.' }
}

if ($RequireT007InProgress) {
    $controlPath = Join-Path $PSScriptRoot '..\docs\PROJECT_CONTROL.md'
    $controlText = Get-Content -Raw -Encoding UTF8 -LiteralPath $controlPath
    $t007Line = ($controlText -split "`r?`n" | Where-Object { $_ -match '^\- \[[ x]\] T007 ' }) | Select-Object -First 1
    $inProgress = From-CodePoints @(0x72B6,0x6001,0xFF1A,0x8FDB,0x884C,0x4E2D)
    if (-not $t007Line -or -not $t007Line.Contains($inProgress)) { Add-Error -Errors $errors -Message 'T007 must be in progress before protocol validation runs.' }
}

if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Host "Protocol schema valid: v1 compatibility baseline, $(@($schema.messages).Count) messages, capabilities, cancellation, flow control, structured errors, C ABI and WASM contracts verified."