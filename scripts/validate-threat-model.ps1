[CmdletBinding()]
param(
    [string]$ThreatFile,
    [switch]$RequireT004InProgress
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($ThreatFile)) {
    $ThreatFile = Join-Path $PSScriptRoot '..\docs\THREAT_MODEL.md'
}

function From-CodePoints {
    param([int[]]$CodePoints)
    return -join ($CodePoints | ForEach-Object { [char]$_ })
}

function Add-RequiredToken {
    param(
        [string]$Token,
        [string]$Text,
        [System.Collections.Generic.List[string]]$Errors
    )
    if (-not $Text.Contains($Token)) {
        $Errors.Add("Missing threat-model token: $Token")
    }
}

$resolved = (Resolve-Path -LiteralPath $ThreatFile).Path
$lines = @(Get-Content -Encoding UTF8 -LiteralPath $resolved)
$text = $lines -join "`n"
$errors = [System.Collections.Generic.List[string]]::new()

$requiredHeadings = @('## 1.', '## 2.', '## 3.', '## 4.', '## 5.', '## 6.', '## 7.')
foreach ($heading in $requiredHeadings) {
    if (-not ($lines | Where-Object { $_ -like "$heading*" })) {
        $errors.Add("Missing threat-model section: $heading")
    }
}

$requiredTokens = @(
    'A-001', 'A-010', 'B-001', 'B-011', 'TH-001', 'TH-016',
    'B-003', 'B-004', 'B-008', 'B-009', 'B-010', 'B-011',
    'C ABI', 'SQLite', 'Web', 'SSRF', 'E2EE'
)
$requiredTokens += From-CodePoints @(0x526A,0x8D34,0x677F)
$requiredTokens += From-CodePoints @(0x4EE3,0x7406)
$requiredTokens += From-CodePoints @(0x66F4,0x65B0)
$requiredTokens += From-CodePoints @(0x9884,0x9632,0x63A7,0x5236)
$requiredTokens += From-CodePoints @(0x68C0,0x6D4B,0x002F,0x54CD,0x5E94)
$requiredTokens += From-CodePoints @(0x6B8B,0x4F59,0x98CE,0x9669)
foreach ($token in $requiredTokens) {
    Add-RequiredToken -Token $token -Text $text -Errors $errors
}

$assetRows = @($lines | Where-Object { $_ -match '^\| A-\d{3} \|' })
$boundaryRows = @($lines | Where-Object { $_ -match '^\| B-\d{3} ' })
$threatRows = @($lines | Where-Object { $_ -match '^\| TH-\d{3} \|' })
$reviewRows = @($lines | Where-Object { $_ -match '^- \[ \] R-\d{2} ' })
if ($assetRows.Count -lt 10) { $errors.Add("Expected at least 10 assets, found $($assetRows.Count).") }
if ($boundaryRows.Count -lt 11) { $errors.Add("Expected at least 11 trust-boundary subjects, found $($boundaryRows.Count).") }
if ($threatRows.Count -lt 16) { $errors.Add("Expected at least 16 threats, found $($threatRows.Count).") }
if ($reviewRows.Count -lt 15) { $errors.Add("Expected at least 15 manual review checks, found $($reviewRows.Count).") }

$seenIds = @{}
foreach ($row in @($assetRows + $boundaryRows + $threatRows)) {
    $match = [regex]::Match($row, '^\| (?<id>(?:A|B|TH)-\d{3})')
    if ($match.Success) {
        $id = $match.Groups['id'].Value
        if ($seenIds.ContainsKey($id)) {
            $errors.Add("Duplicate threat-model ID: $id")
        } else {
            $seenIds[$id] = $true
        }
    }
}

for ($number = 1; $number -le 15; $number++) {
    $reviewId = 'R-' + $number.ToString('00')
    $reviewPattern = '^- \[ \] ' + [regex]::Escape($reviewId) + ' '
    if (-not ($reviewRows | Where-Object { $_ -match $reviewPattern })) {
        $errors.Add("Missing manual review item: $reviewId")
    }

if (-not ($text.Contains('FFI') -or $text.Contains('C ABI'))) {
    $errors.Add('Missing required control domain: FFI/C ABI')
}

$reviewPath = Join-Path (Split-Path -Parent $resolved) 'THREAT_MODEL_REVIEW.md'
if (-not (Test-Path -LiteralPath $reviewPath -PathType Leaf)) {
    $errors.Add('Missing manual threat-model review record.')
} else {
    $reviewText = Get-Content -Raw -Encoding UTF8 -LiteralPath $reviewPath
    foreach ($reviewToken in @('MODEL-PASS', 'implementation evidence pending', 'R-01', 'R-15')) {
        if (-not $reviewText.Contains($reviewToken)) {
            $errors.Add("Missing review-record token: $reviewToken")
        }
    }
    $reviewRecordRows = @($reviewText -split "`r?`n" | Where-Object { $_ -match '^\| R-\d{2} \|' })
    if ($reviewRecordRows.Count -lt 15) {
        $errors.Add("Manual review record has $($reviewRecordRows.Count) rows; expected at least 15.")
    }
}
}

$noDefaultHighRisk = From-CodePoints @(0x5F53,0x524D,0x6CA1,0x6709,0x9ED8,0x8BA4,0x63A5,0x53D7,0x7684,0x9AD8,0x5371,0x98CE,0x9669)
if (-not $text.Contains($noDefaultHighRisk)) {
    $errors.Add('Threat model must state that no high-risk exception is accepted by default.')
}

$controlDomains = @(
    (From-CodePoints @(0x5BA2,0x6237,0x7AEF)),
    (From-CodePoints @(0x6570,0x636E,0x5E93)),
    (From-CodePoints @(0x540C,0x6B65)),
    (From-CodePoints @(0x7F51,0x5173)),
    (From-CodePoints @(0x526A,0x8D34,0x677F)),
    (From-CodePoints @(0x4EE3,0x7406)),
    (From-CodePoints @(0x66F4,0x65B0))
)
foreach ($domain in $controlDomains) {
    if (-not $text.Contains($domain)) {
        $errors.Add("Missing required control domain: $domain")
    }
}

if ($RequireT004InProgress) {
    $controlPath = Join-Path $PSScriptRoot '..\docs\PROJECT_CONTROL.md'
    $controlText = Get-Content -Raw -Encoding UTF8 -LiteralPath $controlPath
    $t004Line = ($controlText -split "`r?`n" | Where-Object { $_ -match '^\- \[[ x]\] T004 ' }) | Select-Object -First 1
    $inProgress = From-CodePoints @(0x72B6,0x6001,0xFF1A,0x8FDB,0x884C,0x4E2D)
    if (-not $t004Line -or -not $t004Line.Contains($inProgress)) {
        $errors.Add('T004 must be in progress before threat-model lint runs.')
    }
}

if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Host "Threat model valid: $($assetRows.Count) assets, $($boundaryRows.Count) trust-boundary subjects, $($threatRows.Count) threats, $($reviewRows.Count) manual checks, and required control domains verified."
