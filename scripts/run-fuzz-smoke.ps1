[CmdletBinding()]
param(
    [int]$TimeoutSeconds = 300,
    [string]$RepositoryRoot
)
$ErrorActionPreference = 'Stop'
if (-not $RepositoryRoot) { $RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path }
$outDir = Join-Path $RepositoryRoot 'artifacts\fuzz'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$corpusPath = Join-Path $RepositoryRoot 'fuzz\smoke-corpus.json'
$corpus = Get-Content -Raw -Encoding UTF8 -LiteralPath $corpusPath | ConvertFrom-Json
$targetCount = @($corpus.targets).Count

$previousPreference = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$testOutput = & cargo test -p fuzz-targets --locked 2>&1 | Out-String
$exitCode = $LASTEXITCODE
$stopwatch.Stop()
$ErrorActionPreference = $previousPreference
$elapsed = [math]::Round($stopwatch.Elapsed.TotalSeconds, 3)

$passed = ($exitCode -eq 0) -and ($testOutput -match "test result: ok\. $targetCount passed; 0 failed")
$result = [ordered]@{
    schema_version      = 1
    task                = 'T036'
    status              = if ($passed) { 'pass' } else { 'fail' }
    generated_at_utc    = (Get-Date).ToUniversalTime().ToString('o')
    elapsed_seconds     = $elapsed
    timeout_seconds     = $TimeoutSeconds
    target_count        = $targetCount
    targets_passed      = if ($passed) { $targetCount } else { 0 }
    exit_code           = $exitCode
    target_names        = @($corpus.targets | ForEach-Object { $_.name })
}
$result | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $outDir 'smoke-results.json')
if (-not $passed) {
    Write-Error "fuzz smoke failed: exit=$exitCode elapsed=${elapsed}s"
    Write-Output $testOutput
    exit 1
}
Write-Host "fuzz smoke pass: $targetCount targets in ${elapsed}s (timeout=${TimeoutSeconds}s)"