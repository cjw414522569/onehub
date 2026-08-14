[CmdletBinding()]
param(
    [int]$TimeoutSeconds = 900,
    [int]$IterationsMultiplier = 1,
    [string]$RepositoryRoot
)
$ErrorActionPreference = 'Stop'
if (-not $RepositoryRoot) { $RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path }
if ($IterationsMultiplier -lt 1) { throw 'IterationsMultiplier must be >= 1' }

$outDir = Join-Path $RepositoryRoot 'artifacts\fuzz'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$corpusPath = Join-Path $RepositoryRoot 'fuzz\smoke-corpus.json'
$corpus = Get-Content -Raw -Encoding UTF8 -LiteralPath $corpusPath | ConvertFrom-Json
$coreTargetCount = @($corpus.targets).Count
$sshTargetCount = @($corpus.ssh_targets).Count

function Invoke-FuzzSuite {
    param([string[]]$Arguments, [int]$Expected)
    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $output = & cargo test @Arguments 2>&1 | Out-String
    $exit = $LASTEXITCODE
    $ErrorActionPreference = $previousPreference
    $passed = ($exit -eq 0) -and ($output -match "test result: ok\. $Expected passed; 0 failed")
    if (-not $passed) { Write-Output $output }
    return @{ exit = $exit; passed = $passed }
}

$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$coreResult = $null
$sshResult = $null
for ($iteration = 1; $iteration -le $IterationsMultiplier; $iteration++) {
    $coreResult = Invoke-FuzzSuite -Arguments @('-p', 'fuzz-targets', '--locked') -Expected $coreTargetCount
    $sshResult = Invoke-FuzzSuite -Arguments @('-p', 'ssh-backend', '--locked', 'fuzz_') -Expected $sshTargetCount
    if (-not $coreResult.passed -or -not $sshResult.passed) { break }
}
$stopwatch.Stop()
$elapsed = [math]::Round($stopwatch.Elapsed.TotalSeconds, 3)

# Archive the corpus plus any crash artifacts under a timestamped directory.
$archiveDir = Join-Path $outDir ("corpus-archive\" + (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ'))
New-Item -ItemType Directory -Force -Path $archiveDir | Out-Null
Copy-Item -LiteralPath $corpusPath -Destination (Join-Path $archiveDir 'smoke-corpus.json') -Force
$crashesDir = Join-Path $outDir 'crashes'
if (Test-Path -LiteralPath $crashesDir) {
    Get-ChildItem -LiteralPath $crashesDir -File | Copy-Item -Destination $archiveDir -Force
}

$passed = $coreResult.passed -and $sshResult.passed
$result = [ordered]@{
    schema_version            = 1
    task                      = 'T036+T061'
    status                    = if ($passed) { 'pass' } else { 'fail' }
    generated_at_utc          = (Get-Date).ToUniversalTime().ToString('o')
    elapsed_seconds           = $elapsed
    timeout_seconds           = $TimeoutSeconds
    iterations_multiplier     = $IterationsMultiplier
    core_target_count         = $coreTargetCount
    core_targets_passed       = if ($coreResult.passed) { $coreTargetCount } else { 0 }
    ssh_target_count          = $sshTargetCount
    ssh_targets_passed        = if ($sshResult.passed) { $sshTargetCount } else { 0 }
    archive_dir               = $archiveDir
    target_names              = @($corpus.targets | ForEach-Object { $_.name })
    ssh_target_names          = @($corpus.ssh_targets | ForEach-Object { $_.name })
}
$result | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $outDir 'smoke-results.json')
if (-not $passed) {
    Write-Error "fuzz smoke failed: core=$($coreResult.exit) ssh=$($sshResult.exit) elapsed=${elapsed}s"
    exit 1
}
Write-Host "fuzz smoke pass: $coreTargetCount core + $sshTargetCount ssh targets in ${elapsed}s (timeout=${TimeoutSeconds}s, multiplier=${IterationsMultiplier})"