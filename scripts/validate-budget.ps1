[CmdletBinding()]
param(
    [string]$BudgetFile,
    [switch]$RequireT003InProgress
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($BudgetFile)) {
    $BudgetFile = Join-Path $PSScriptRoot '..\docs\PERFORMANCE_BUDGET.md'
}

function From-CodePoints {
    param([int[]]$CodePoints)
    return -join ($CodePoints | ForEach-Object { [char]$_ })
}

function Test-ContainsAny {
    param(
        [string]$Text,
        [string[]]$Candidates
    )
    foreach ($candidate in $Candidates) {
        if ($Text.Contains($candidate)) {
            return $true
        }
    }
    return $false
}

$resolved = (Resolve-Path -LiteralPath $BudgetFile).Path
$lines = @(Get-Content -Encoding UTF8 -LiteralPath $resolved)
$text = $lines -join "`n"
$errors = [System.Collections.Generic.List[string]]::new()

if (-not ($lines | Where-Object { $_ -like '## 1.*' })) { $errors.Add('Missing benchmark device section.') }
if (-not ($lines | Where-Object { $_ -like '## 2.*' })) { $errors.Add('Missing numeric budget section.') }
if (-not ($lines | Where-Object { $_ -like '## 3.*' })) { $errors.Add('Missing statistics and regression section.') }

$platforms = @('Windows', 'macOS Intel', 'macOS Apple Silicon', 'Linux', 'iOS/iPadOS', 'Android', 'Web/PWA', 'CLI')
foreach ($platform in $platforms) {
    if (-not $text.Contains($platform)) {
        $errors.Add("Missing benchmark platform: $platform")
    }
}

$requiredTokens = @('Release', 'P50/P95/P99', '95%', '30', '4K', 'MB/s', 'RSS', 'GPU', 'RTT', 'artifacts/perf')
$submissionToken = From-CodePoints @(0x63D0,0x4EA4,0x0020,0x0053,0x0048,0x0041)
if (-not $text.Contains($submissionToken)) { $errors.Add('Missing measurement token: submission SHA.') }
foreach ($token in $requiredTokens) {
    if (-not $text.Contains($token)) {
        $errors.Add("Missing measurement token: $token")
    }
}

$budgetStart = $text.IndexOf('## 2.')
$budgetEnd = $text.IndexOf('## 3.')
if ($budgetStart -lt 0 -or $budgetEnd -le $budgetStart) {
    $errors.Add('Budget section is missing or out of order.')
} else {
    $budget = $text.Substring($budgetStart, $budgetEnd - $budgetStart)
    $tableRows = @($budget -split "`r?`n" | Where-Object { $_ -match '^\| .+ \|$' })
    if ($tableRows.Count -lt 17) {
        $errors.Add("Numeric budget table has $($tableRows.Count) rows; expected header, separator, and at least fifteen metric rows.")
    }
    $dataRows = @($tableRows | Select-Object -Skip 2)
    $measurementMethodLabel = From-CodePoints @(0x6D4B,0x91CF,0x65B9,0x6CD5)
    $zeroToleranceText = From-CodePoints @(0x0030,0x0020,0x6B21)
    $notApplicableText = From-CodePoints @(0x4E0D,0x9002,0x7528)
    foreach ($line in $dataRows) {
        $columns = @($line -split "\|")
        if ($columns.Count -lt 10) {
            $errors.Add("Budget row has fewer than nine fields: $line")
            continue
        }
        $isZeroTolerance = $line.Contains($zeroToleranceText)
        $isNotApplicable = $line.Contains($notApplicableText)
        if (-not $isZeroTolerance -and -not $isNotApplicable -and $line -notmatch "(<=|>=)\s*[0-9]") {
            $errors.Add("Budget row lacks a numeric comparator or explicit non-applicable target: $line")
        }
        $method = $columns[$columns.Count - 2].Trim()
        if ([string]::IsNullOrWhiteSpace($method) -or $method -eq $measurementMethodLabel) {
            $errors.Add("Budget row lacks a measurement method: $line")
        }
    }

$metricElectric = From-CodePoints @(0x7535,0x91CF)
$metricPackage = From-CodePoints @(0x5305,0x4F53)
$metricCrash = From-CodePoints @(0x5D29,0x6E83,0x7387)
$metricTokens = @('P95', 'P50', 'P99', 'FPS', 'MB/s', 'RSS', $metricElectric, $metricPackage, $metricCrash)
    foreach ($metric in $metricTokens) {
        if (-not $budget.Contains($metric)) {
            $errors.Add("Budget section lacks metric token: $metric")
        }
    }
}

$deviceStart = $text.IndexOf('## 1.')
$deviceEnd = $text.IndexOf('## 2.')
if ($deviceStart -ge 0 -and $deviceEnd -gt $deviceStart) {
    $devices = $text.Substring($deviceStart, $deviceEnd - $deviceStart)
    $deviceRows = @($devices -split "`r?`n" | Where-Object { $_ -match '^\| .+ \|$' })
    if ($deviceRows.Count -lt 9) {
        $errors.Add("Benchmark device table has $($deviceRows.Count) rows; expected header and eight platform rows.")
    }
    foreach ($platform in $platforms) {
        $platformRow = $deviceRows | Where-Object { $_.Contains($platform) } | Select-Object -First 1
        if (-not $platformRow) {
            $errors.Add("Benchmark device row missing for $platform")
        } elseif ($platformRow -notmatch '\| .+ \| .+ \| .+ \| .+ \| .+ \|$') {
            $errors.Add("Benchmark device row is incomplete for $platform")
        }
    }
}

$securityText = From-CodePoints @(0x6D4B,0x91CF,0x539F,0x5219)
if (-not $text.Contains($securityText)) {
    $errors.Add('Budget must state measurement principles.')
}

if ($RequireT003InProgress) {
    $controlPath = Join-Path $PSScriptRoot '..\docs\PROJECT_CONTROL.md'
    $controlText = Get-Content -Raw -Encoding UTF8 -LiteralPath $controlPath
    $t003Line = ($controlText -split "`r?`n" | Where-Object { $_ -match '^\- \[[ x]\] T003 ' }) | Select-Object -First 1
    $inProgress = From-CodePoints @(0x72B6,0x6001,0xFF1A,0x8FDB,0x884C,0x4E2D)
    if (-not $t003Line -or -not $t003Line.Contains($inProgress)) {
        $errors.Add('T003 must be in progress before its budget test is run.')
    }
}

if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Host "Performance budget valid: $($platforms.Count) platforms, numeric metric rows, measurement devices, and statistical gates verified."
