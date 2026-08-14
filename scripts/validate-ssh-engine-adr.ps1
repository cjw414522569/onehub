[CmdletBinding()]
param(
    [string]$AdrFile,
    [string]$ReportFile,
    [switch]$RequireT009InProgress
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($AdrFile)) {
    $AdrFile = Join-Path $PSScriptRoot '..\docs\ADR-003-ssh-backend-selection.md'
}
if ([string]::IsNullOrWhiteSpace($ReportFile)) {
    $ReportFile = Join-Path $PSScriptRoot '..\docs\reports\SSH_ENGINE_SPIKE.json'
}

function From-CodePoints {
    param([int[]]$CodePoints)
    return -join ($CodePoints | ForEach-Object { [char]$_ })
}

function Add-Error {
    param([System.Collections.Generic.List[string]]$Errors, [string]$Message)
    $Errors.Add($Message)
}

function Test-ContainsAll {
    param(
        [string]$Text,
        [string[]]$Tokens,
        [System.Collections.Generic.List[string]]$Errors,
        [string]$Context
    )
    foreach ($token in @($Tokens)) {
        if (-not $Text.Contains($token)) {
            Add-Error -Errors $Errors -Message "$Context is missing required token: $token"
        }
    }
}

function Test-NonEmpty {
    param(
        [string]$Context,
        [object]$Value,
        [System.Collections.Generic.List[string]]$Errors
    )
    if ([string]::IsNullOrWhiteSpace([string]$Value)) {
        Add-Error -Errors $Errors -Message "$Context must be non-empty."
    }
}

$errors = [System.Collections.Generic.List[string]]::new()
$adrText = $null
$report = $null

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

if (-not (Test-Path -LiteralPath $ReportFile)) {
    Add-Error -Errors $errors -Message "Spike report does not exist: $ReportFile"
}
else {
    try {
        $report = Get-Content -Raw -Encoding UTF8 -LiteralPath (Resolve-Path -LiteralPath $ReportFile).Path | ConvertFrom-Json
    }
    catch {
        Add-Error -Errors $errors -Message 'Spike report is not valid JSON.'
    }
}

if ($null -ne $adrText) {
    $requiredHeadings = @(
        '## 1.', '## 2.', '## 3.', '## 4.', '## 5.',
        '## 6.', '## 7.', '## 8.', '## 9.', '## 10.'
    )
    foreach ($heading in $requiredHeadings) {
        if (-not (($adrText -split "`r?`n") | Where-Object { $_ -like "$heading*" })) {
            Add-Error -Errors $errors -Message "ADR-003 is missing section: $heading"
        }
    }

    $statusAccepted = From-CodePoints @(0x41,0x63,0x63,0x65,0x70,0x74,0x65,0x64)
    $decisionToken = From-CodePoints @(0x4E3B,0x540E,0x7AEF)
    $fallbackToken = From-CodePoints @(0x56DE,0x9000)
    $adapterToken = From-CodePoints @(0x9002,0x914D,0x5668)
    $unsupportedToken = From-CodePoints @(0x4E0D,0x652F,0x6301)
    $algorithmToken = From-CodePoints @(0x7B97,0x6CD5)
    $securityToken = From-CodePoints @(0x5B89,0x5168)
    $evidenceToken = From-CodePoints @(0x8BC1,0x636E)
    $reviewToken = From-CodePoints @(0x590D,0x5BA1)
    $primaryLabel = From-CodePoints @(0x4E3B,0x540E,0x7AEF)
    $asPrimaryLabel = From-CodePoints @(0x4F5C,0x4E3A,0x4E3B)
    $fallbackLabel = From-CodePoints @(0x517C,0x5BB9,0x56DE,0x9000)
    $colonPattern = '(?:' + [regex]::Escape((From-CodePoints @(0xFF1A))) + '|:)'
    $fallbackPattern = [regex]::Escape($fallbackLabel)
    $primaryPattern = [regex]::Escape($primaryLabel)
    $asPrimaryPattern = [regex]::Escape($asPrimaryLabel)
    $requiredTokens = @(
        'ADR-003', $statusAccepted, '2026-08-12', '2027-08-12',
        'russh', 'libssh2', 'libssh', 'system OpenSSH',
        'SSH_ENGINE_SPIKE.json', 'SSH_ENGINE_SPIKE.md',
        $decisionToken, $fallbackToken, $adapterToken, $unsupportedToken,
        $algorithmToken, $securityToken, $evidenceToken, $reviewToken,
        'Tokio', 'Rust', 'C ABI', 'known_hosts', 'SFTP', 'ProxyJump',
        'ssh-dss', 'ssh-rsa', 'diffie-hellman-group1-sha1', '3des-cbc',
        'aes128-cbc', 'hmac-sha1', 'ssh-ed25519', 'curve25519-sha256',
        'ssh2-rs', 'system OpenSSH client'
    )
    Test-ContainsAll -Text $adrText -Tokens $requiredTokens -Errors $errors -Context 'ADR-003'

    $tableRows = @($adrText -split "`r?`n" | Where-Object { $_ -match '^\| .+ \| .+ \| .+ \| .+ \| .+ \|$' })
    if ($tableRows.Count -lt 5) {
        Add-Error -Errors $errors -Message "ADR-003 comparison/decision table has $($tableRows.Count) rows; expected header plus at least four candidate rows."
    }

    if ($adrText -notmatch ('(?im)' + $primaryPattern + '\s*' + $colonPattern + '\s*russh') -and $adrText -notmatch ('(?im)russh\s*' + $asPrimaryPattern)) {
        Add-Error -Errors $errors -Message 'ADR-003 must explicitly select russh as the primary backend.'
    }
    if ($adrText -notmatch '(?im)libssh2\s*(?:/|\(|\\)\s*(?:ssh2-rs|ssh2)' -and $adrText -notmatch ('(?im)' + $fallbackPattern + '.*libssh2')) {
        Add-Error -Errors $errors -Message 'ADR-003 must explicitly select libssh2/ssh2-rs as the desktop compatibility fallback.'
    }

    $boundaryMarkers = @('ssh-backend trait', 'SshBackend', 'HostKeyPolicy', 'SessionHandle', 'CommandResult', 'TransferHandle')
    foreach ($marker in $boundaryMarkers) {
        if (-not $adrText.Contains($marker)) {
            Add-Error -Errors $errors -Message "ADR-003 adapter boundary is missing marker: $marker"
        }
    }

    $disabledMarker = From-CodePoints @(0x7981,0x6B62,0x542F,0x7528)
    $explicitDisabledMarker = From-CodePoints @(0x660E,0x786E,0x7981,0x7528)
    $unsupportedMarker = From-CodePoints @(0x4E0D,0x652F,0x6301)
    $unverifiedMarker = From-CodePoints @(0x672A,0x9A8C,0x8BC1)
    $negativePolicyMarkers = @($disabledMarker, $explicitDisabledMarker, $unsupportedMarker, $unverifiedMarker)
    $negativePolicyCount = 0
    foreach ($marker in $negativePolicyMarkers) {
        if ($adrText.Contains($marker)) { $negativePolicyCount++ }
    }
    if ($negativePolicyCount -lt 2) {
        Add-Error -Errors $errors -Message 'ADR-003 must distinguish unsupported/disabled algorithms from unverified capabilities.'
    }
}

if ($null -ne $report) {
    if ([int]$report.schema_version -ne 1) {
        Add-Error -Errors $errors -Message 'Spike report schema_version must be 1.'
    }
    if ([string]$report.spike_id -ne 'ssh-engine-compat-v1') {
        Add-Error -Errors $errors -Message 'Spike report must be ssh-engine-compat-v1.'
    }
    $candidateMap = @{}
    foreach ($candidate in @($report.candidates)) {
        $candidateMap[[string]$candidate.id] = $candidate
    }
    foreach ($candidateId in @('russh', 'libssh2', 'libssh', 'system-openssh')) {
        if (-not $candidateMap.ContainsKey($candidateId)) {
            Add-Error -Errors $errors -Message "Spike report is missing candidate: $candidateId"
        }
    }
    if ($candidateMap.ContainsKey('russh') -and [string]$candidateMap['russh'].status -ne 'passed') {
        Add-Error -Errors $errors -Message 'Spike report russh status must be passed for this ADR decision.'
    }
    if ($candidateMap.ContainsKey('libssh2') -and [string]$candidateMap['libssh2'].status -ne 'passed') {
        Add-Error -Errors $errors -Message 'Spike report libssh2 status must be passed for this ADR decision.'
    }
    if ($candidateMap.ContainsKey('libssh') -and [string]$candidateMap['libssh'].status -ne 'blocked_environment') {
        Add-Error -Errors $errors -Message 'Spike report libssh must retain blocked_environment status until native prerequisites are available.'
    }
    if ($candidateMap.ContainsKey('system-openssh')) {
        Test-NonEmpty -Context 'Spike report system-openssh blocked_reason' -Value $candidateMap['system-openssh'].blocked_reason -Errors $errors
    }
}

if ($RequireT009InProgress) {
    $controlPath = Join-Path $PSScriptRoot '..\docs\PROJECT_CONTROL.md'
    $controlText = Get-Content -Raw -Encoding UTF8 -LiteralPath $controlPath
    $t009Line = ($controlText -split "`r?`n" | Where-Object { $_ -match '^\- \[[ x]\] T009 ' }) | Select-Object -First 1
    $inProgress = From-CodePoints @(0x72B6,0x6001,0xFF1A,0x8FDB,0x884C,0x4E2D)
    if (-not $t009Line -or -not $t009Line.Contains($inProgress)) {
        Add-Error -Errors $errors -Message 'T009 must be in progress before ADR-003 validation runs.'
    }
}

if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Host 'ADR-003 valid: T008 evidence cross-check, primary/fallback decision, adapter boundary, algorithm policy, and review gates verified.'
