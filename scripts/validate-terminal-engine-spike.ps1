[CmdletBinding()]
param(
    [string]$ReportFile,
    [string]$MatrixFile
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($ReportFile)) {
    $ReportFile = Join-Path $PSScriptRoot '..\spikes\terminal-engines\report.json'
}
if ([string]::IsNullOrWhiteSpace($MatrixFile)) {
    $MatrixFile = Join-Path $PSScriptRoot '..\spikes\terminal-engines\matrix.json'
}

function Add-Error {
    param([System.Collections.Generic.List[string]]$Errors, [string]$Message)
    $Errors.Add($Message)
}

function Test-NonEmpty {
    param([string]$Context, [object]$Value, [System.Collections.Generic.List[string]]$Errors)
    if ([string]::IsNullOrWhiteSpace([string]$Value)) {
        Add-Error -Errors $Errors -Message "$Context must be non-empty."
    }
}

function Test-Result {
    param([string]$Context, [object]$Result, [System.Collections.Generic.List[string]]$Errors)
    if ($null -eq $Result) {
        Add-Error -Errors $Errors -Message "$Context is missing."
        return
    }
    if (@('passed', 'blocked_environment', 'failed') -notcontains [string]$Result.status) {
        Add-Error -Errors $Errors -Message "$Context.status is invalid."
    }
    if ($null -eq $Result.exit_code) {
        Add-Error -Errors $Errors -Message "$Context.exit_code is missing."
    }
    Test-NonEmpty -Context "$Context.command" -Value $Result.command -Errors $Errors
}

$errors = [System.Collections.Generic.List[string]]::new()
$report = $null
$matrix = $null

if (-not (Test-Path -LiteralPath $ReportFile)) {
    Add-Error -Errors $errors -Message "Report file does not exist: $ReportFile"
}
else {
    try { $report = Get-Content -Raw -Encoding UTF8 -LiteralPath (Resolve-Path -LiteralPath $ReportFile).Path | ConvertFrom-Json }
    catch { Add-Error -Errors $errors -Message 'Report file is not valid JSON.' }
}

if (-not (Test-Path -LiteralPath $MatrixFile)) {
    Add-Error -Errors $errors -Message "Matrix file does not exist: $MatrixFile"
}
else {
    try { $matrix = Get-Content -Raw -Encoding UTF8 -LiteralPath (Resolve-Path -LiteralPath $MatrixFile).Path | ConvertFrom-Json }
    catch { Add-Error -Errors $errors -Message 'Matrix file is not valid JSON.' }
}

if ($null -ne $matrix) {
    if ([int]$matrix.schema_version -ne 1) { Add-Error -Errors $errors -Message 'Matrix schema_version must be 1.' }
    $requiredTests = @('vt-compatibility', 'unicode', 'replay', 'throughput', 'license')
    $matrixIds = @($matrix.tests | ForEach-Object { [string]$_.id })
    foreach ($requiredTest in $requiredTests) {
        if ($matrixIds -notcontains $requiredTest) { Add-Error -Errors $errors -Message "Matrix is missing test: $requiredTest" }
    }
}

if ($null -ne $report) {
    if ([int]$report.schema_version -ne 1) { Add-Error -Errors $errors -Message 'Report schema_version must be 1.' }
    Test-NonEmpty -Context 'report.generated_at_utc' -Value $report.generated_at_utc -Errors $errors
    Test-NonEmpty -Context 'report.corpus_sha256' -Value $report.corpus_sha256 -Errors $errors
    Test-NonEmpty -Context 'report.vttest_status' -Value $report.vttest_status -Errors $errors
    if (@('passed', 'blocked_environment', 'failed') -notcontains [string]$report.vttest_status) {
        Add-Error -Errors $errors -Message 'report.vttest_status is invalid.'
    }
    $requiredCandidates = @('alacritty_terminal', 'wezterm-fork-proxy', 'adapter-baseline')
    $candidateMap = @{}
    foreach ($candidate in @($report.candidates)) {
        $candidateMap[[string]$candidate.id] = $candidate
    }
    foreach ($candidateId in $requiredCandidates) {
        if (-not $candidateMap.ContainsKey($candidateId)) {
            Add-Error -Errors $errors -Message "Missing required candidate: $candidateId"
            continue
        }
        $candidate = $candidateMap[$candidateId]
        Test-NonEmpty -Context "candidate[$candidateId].version" -Value $candidate.version -Errors $errors
        Test-NonEmpty -Context "candidate[$candidateId].license" -Value $candidate.license -Errors $errors
        Test-NonEmpty -Context "candidate[$candidateId].output_sha256" -Value $candidate.output_sha256 -Errors $errors
        Test-NonEmpty -Context "candidate[$candidateId].corpus_sha256" -Value $candidate.corpus_sha256 -Errors $errors
        if ([string]$candidate.corpus_sha256 -ne [string]$report.corpus_sha256) {
            Add-Error -Errors $errors -Message "candidate[$candidateId] corpus_sha256 differs from report corpus_sha256."
        }
        Test-Result -Context "candidate[$candidateId].vt" -Result $candidate.vt -Errors $errors
        Test-Result -Context "candidate[$candidateId].unicode" -Result $candidate.unicode -Errors $errors
        Test-Result -Context "candidate[$candidateId].replay" -Result $candidate.replay -Errors $errors
        Test-Result -Context "candidate[$candidateId].throughput" -Result $candidate.throughput -Errors $errors
        Test-Result -Context "candidate[$candidateId].license_check" -Result $candidate.license_check -Errors $errors
        if ($null -eq $candidate.metrics -or $null -eq $candidate.metrics.p50_ms -or $null -eq $candidate.metrics.p95_ms -or $null -eq $candidate.metrics.throughput_mb_s) {
            Add-Error -Errors $errors -Message "candidate[$candidateId].metrics must include p50_ms, p95_ms and throughput_mb_s."
        }
        if ([string]$candidate.status -eq 'blocked_environment') {
            Test-NonEmpty -Context "candidate[$candidateId].blocked_reason" -Value $candidate.blocked_reason -Errors $errors
        }
    }
    if ([string]$report.vttest_status -eq 'blocked_environment') {
        Test-NonEmpty -Context 'report.vttest_blocked_reason' -Value $report.vttest_blocked_reason -Errors $errors
    }
    if ([string]$report.vttest_status -eq 'passed' -and [string]::IsNullOrWhiteSpace([string]$report.vttest_command)) {
        Add-Error -Errors $errors -Message 'A passed vttest_status requires vttest_command.'
    }
}

if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Host "Terminal engine spike report valid: candidates=$(@($report.candidates).Count), vttest=$($report.vttest_status), corpus=$($report.corpus_sha256)."
