[CmdletBinding()]
param(
    [string]$ReportFile,
    [string]$MatrixFile
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($ReportFile)) {
    $ReportFile = Join-Path $PSScriptRoot '..\spikes\ssh-engines\report.json'
}
if ([string]::IsNullOrWhiteSpace($MatrixFile)) {
    $MatrixFile = Join-Path $PSScriptRoot '..\spikes\ssh-engines\test-matrix.json'
}

function Add-Error {
    param([System.Collections.Generic.List[string]]$Errors, [string]$Message)
    $Errors.Add($Message)
}

function Get-StringSet {
    param([object[]]$Values)
    $set = @{}
    foreach ($value in @($Values)) {
        $key = [string]$value
        if (-not [string]::IsNullOrWhiteSpace($key)) {
            $set[$key] = $true
        }
    }
    return $set
}

function Test-NonEmptyString {
    param(
        [string]$Context,
        [object]$Value,
        [System.Collections.Generic.List[string]]$Errors
    )
    if ([string]::IsNullOrWhiteSpace([string]$Value)) {
        Add-Error -Errors $Errors -Message "$Context must be a non-empty string."
    }
}

function Test-CommandResult {
    param(
        [string]$Context,
        [object]$Result,
        [System.Collections.Generic.List[string]]$Errors
    )
    if ($null -eq $Result) {
        Add-Error -Errors $Errors -Message "$Context is missing a command result."
        return
    }
    Test-NonEmptyString -Context "$Context.command" -Value $Result.command -Errors $Errors
    if ($null -eq $Result.exit_code) {
        Add-Error -Errors $Errors -Message "$Context.exit_code is missing."
    }
    if ($null -eq $Result.status -or @('passed', 'failed', 'blocked_environment') -notcontains [string]$Result.status) {
        Add-Error -Errors $Errors -Message "$Context.status must be passed, failed, or blocked_environment."
    }
}

$errors = [System.Collections.Generic.List[string]]::new()
$report = $null
$matrix = $null

if (-not (Test-Path -LiteralPath $ReportFile)) {
    Add-Error -Errors $errors -Message "Report file does not exist: $ReportFile"
}
else {
    try {
        $report = Get-Content -Raw -Encoding UTF8 -LiteralPath (Resolve-Path -LiteralPath $ReportFile).Path | ConvertFrom-Json
    }
    catch {
        Add-Error -Errors $errors -Message 'Report file is not valid JSON.'
    }
}

if (-not (Test-Path -LiteralPath $MatrixFile)) {
    Add-Error -Errors $errors -Message "Matrix file does not exist: $MatrixFile"
}
else {
    try {
        $matrix = Get-Content -Raw -Encoding UTF8 -LiteralPath (Resolve-Path -LiteralPath $MatrixFile).Path | ConvertFrom-Json
    }
    catch {
        Add-Error -Errors $errors -Message 'Matrix file is not valid JSON.'
    }
}

if ($null -ne $report) {
    if ([int]$report.schema_version -ne 1) {
        Add-Error -Errors $errors -Message 'Report schema_version must be 1.'
    }
    Test-NonEmptyString -Context 'report.generated_at_utc' -Value $report.generated_at_utc -Errors $errors

    $candidateIds = @('russh', 'libssh2', 'libssh', 'system-openssh')
    $candidateMap = @{}
    foreach ($candidate in @($report.candidates)) {
        $id = [string]$candidate.id
        if ([string]::IsNullOrWhiteSpace($id)) {
            Add-Error -Errors $errors -Message 'A candidate is missing id.'
            continue
        }
        if ($candidateMap.ContainsKey($id)) {
            Add-Error -Errors $errors -Message "Duplicate candidate id: $id"
        }
        $candidateMap[$id] = $candidate
    }
    $actualCandidates = Get-StringSet -Values $candidateMap.Keys
    foreach ($requiredId in $candidateIds) {
        if (-not $actualCandidates.ContainsKey($requiredId)) {
            Add-Error -Errors $errors -Message "Missing required candidate: $requiredId"
        }
    }

    $passedCount = 0
    $blockedCount = 0
    $requiredCheckIds = @()
    if ($null -ne $matrix) {
        $requiredCheckIds = @($matrix.tests | ForEach-Object { [string]$_.id })
    }
    foreach ($candidateId in $candidateIds) {
        if (-not $candidateMap.ContainsKey($candidateId)) { continue }
        $candidate = $candidateMap[$candidateId]
        Test-NonEmptyString -Context "candidate[$candidateId].version" -Value $candidate.version -Errors $errors
        Test-NonEmptyString -Context "candidate[$candidateId].license" -Value $candidate.license -Errors $errors
        Test-NonEmptyString -Context "candidate[$candidateId].repository" -Value $candidate.repository -Errors $errors
        if (@('passed', 'failed', 'blocked_environment') -notcontains [string]$candidate.status) {
            Add-Error -Errors $errors -Message "candidate[$candidateId].status is invalid."
        }
        if ([string]$candidate.status -eq 'passed') { $passedCount++ }
        if ([string]$candidate.status -eq 'blocked_environment') { $blockedCount++ }
        if ([string]$candidate.status -eq 'blocked_environment') {
            Test-NonEmptyString -Context "candidate[$candidateId].blocked_reason" -Value $candidate.blocked_reason -Errors $errors
            Test-NonEmptyString -Context "candidate[$candidateId].reproduction" -Value $candidate.reproduction -Errors $errors
        }
        if (@($candidate.checks).Count -eq 0) {
            Add-Error -Errors $errors -Message "candidate[$candidateId] has no checks."
        }
        $candidateCheckIds = Get-StringSet -Values @($candidate.checks | ForEach-Object { [string]$_.id })
        foreach ($requiredCheckId in $requiredCheckIds) {
            if (-not $candidateCheckIds.ContainsKey($requiredCheckId)) {
                Add-Error -Errors $errors -Message "candidate[$candidateId] is missing matrix check: $requiredCheckId"
            }
        }
        foreach ($check in @($candidate.checks)) {
            Test-NonEmptyString -Context "candidate[$candidateId].check.id" -Value $check.id -Errors $errors
            Test-CommandResult -Context "candidate[$candidateId].check[$($check.id)]" -Result $check -Errors $errors
            if ([string]$check.id -eq 'performance-smoke') {
                Test-NonEmptyString -Context "candidate[$candidateId].check[performance-smoke].measurement_scope" -Value $check.measurement_scope -Errors $errors
            }
        }
    }
    if ($passedCount -eq 0) {
        Add-Error -Errors $errors -Message 'Report must contain at least one passed candidate.'
    }
    if ($blockedCount -eq 0) {
        Add-Error -Errors $errors -Message 'Report must contain at least one controlled blocked_environment candidate.'
    }
    $opensshCandidate = $candidateMap['system-openssh']
    if ($null -ne $opensshCandidate -and [string]$opensshCandidate.status -eq 'passed' -and [string]::IsNullOrWhiteSpace([string]$opensshCandidate.blocked_reason)) {
        Add-Error -Errors $errors -Message 'system-openssh must retain an explicit sshd/daemon integration limitation when sshd is unavailable.'
    }
}

if ($null -ne $matrix) {
    $requiredMatrixIds = @('metadata', 'build', 'host-key-callback', 'authentication-api', 'exec', 'interactive-channel', 'sftp-forwarding-api', 'cancel-timeout', 'performance-smoke', 'secret-log-scan')
    $matrixIds = @($matrix.tests | ForEach-Object { [string]$_.id })
    $matrixSet = Get-StringSet -Values $matrixIds
    foreach ($requiredTestId in $requiredMatrixIds) {
        if (-not $matrixSet.ContainsKey($requiredTestId)) {
            Add-Error -Errors $errors -Message "Matrix is missing required test: $requiredTestId"
        }
    }
    if ($matrixIds.Count -ne ($matrixSet.Keys | Measure-Object).Count) {
        Add-Error -Errors $errors -Message 'Matrix test ids must be unique.'
    }
}

if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Host "SSH engine spike report valid: candidates=$(@($report.candidates).Count), tests=$(@($matrix.tests).Count), passed=$(@($report.candidates | Where-Object status -eq 'passed').Count), blocked_environment=$(@($report.candidates | Where-Object status -eq 'blocked_environment').Count)."
