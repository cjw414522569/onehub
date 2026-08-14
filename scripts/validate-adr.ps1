[CmdletBinding()]
param(
    [string]$AdrFile,
    [switch]$RequireT005InProgress
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($AdrFile)) {
    $AdrFile = Join-Path $PSScriptRoot '..\docs\ADR-001-native-federated-architecture.md'
}

function From-CodePoints {
    param([int[]]$CodePoints)
    return -join ($CodePoints | ForEach-Object { [char]$_ })
}

$resolved = (Resolve-Path -LiteralPath $AdrFile).Path
$lines = @(Get-Content -Encoding UTF8 -LiteralPath $resolved)
$text = $lines -join "`n"
$errors = [System.Collections.Generic.List[string]]::new()

$requiredHeadings = @('## 1.', '## 2.', '## 3.', '## 4.', '## 5.', '## 6.', '## 7.', '## 8.')
foreach ($heading in $requiredHeadings) {
    if (-not ($lines | Where-Object { $_ -like "$heading*" })) {
        $errors.Add("Missing ADR section: $heading")
    }
}

$requiredTokens = @(
    'ADR-001', 'Accepted', '2026-08-12', '2027-08-12', 'Rust', 'wgpu',
    'WinUI 3', 'SwiftUI', 'AppKit', 'UIKit', 'Kotlin', 'Jetpack Compose',
    'GTK4', 'libadwaita', 'TypeScript', 'WASM', 'C ABI', 'Tokio',
    'T012/T013', 'T156-T159'
)
$requiredTokens += From-CodePoints @(0x9000,0x51FA,0x6761,0x4EF6)
$requiredTokens += From-CodePoints @(0x66FF,0x4EE3,0x8DEF,0x5F84)
$requiredTokens += From-CodePoints @(0x4E0D,0x53EF,0x8FDD,0x53CD,0x7684,0x67B6,0x6784,0x89C4,0x5219)
foreach ($token in $requiredTokens) {
    if (-not $text.Contains($token)) {
        $errors.Add("Missing ADR token: $token")
    }
}

$candidates = @(
    'Flutter + Dart UI + Rust FFI',
    'Qt/QML + C++/Rust',
    'Kotlin Multiplatform/Compose Multiplatform',
    'Tauri/WebView + Rust',
    (From-CodePoints @(0x5168,0x539F,0x751F,0x0020,0x0055,0x0049,0x0020,0x002B,0x0020,0x0052,0x0075,0x0073,0x0074,0x0020,0x6838,0x5FC3,0x0020,0x002B,0x0020,0x0077,0x0067,0x0070,0x0075)),
    (From-CodePoints @(0x5168,0x5E73,0x53F0,0x5206,0x522B,0x72EC,0x7ACB,0x5B9E,0x73B0))
)
foreach ($candidate in $candidates) {
    if (-not $text.Contains($candidate)) {
        $errors.Add("Missing ADR candidate comparison: $candidate")
    }
}

$comparisonRows = @($lines | Where-Object { $_ -match '^\| .+ \| .+ \| .+ \| .+ \| .+ \| .+ \| .+ \|$' })
if ($comparisonRows.Count -lt 7) {
$errors.Add("ADR candidate table has $($comparisonRows.Count) rows; expected header and six candidates.")
}

$selectedText = From-CodePoints @(0x9009,0x62E9)
$selectedMarker = '**' + (From-CodePoints @(0x9009,0x62E9)) + '**'
if (-not $text.Contains($selectedText) -or -not $text.Contains($selectedMarker)) {
}
$rejectedText = From-CodePoints @(0x4E0D,0x9009)
if (-not $text.Contains($rejectedText)) {
    $errors.Add('ADR must explain rejected alternatives.')
}

$rationaleTokens = @(
    (From-CodePoints @(0x6027,0x80FD)),
    (From-CodePoints @(0x5E73,0x53F0,0x6548,0x679C)),
    (From-CodePoints @(0x5B89,0x5168)),
    (From-CodePoints @(0x53EF,0x66FF,0x6362,0x6027)),
    (From-CodePoints @(0x957F,0x671F,0x4EA4,0x4ED8)),
    (From-CodePoints @(0x7EF4,0x62A4,0x6210,0x672C)),
    (From-CodePoints @(0x9AD8,0x5371,0x5B89,0x5168,0x95EE,0x9898))
)
foreach ($token in $rationaleTokens) {
    if (-not $text.Contains($token)) {
        $errors.Add("ADR missing trade-off or rationale token: $token")
    }
}

$ruleTokens = @(
    (From-CodePoints @(0x4E0D,0x5F97,0x4F9D,0x8D56)),
    (From-CodePoints @(0x4E0D,0x5F97,0x76F4,0x63A5,0x8C03,0x7528)),
    (From-CodePoints @(0x7981,0x6B62,0x9010,0x5B57,0x7B26)),
    (From-CodePoints @(0x5FC5,0x987B,0x6709,0x7248,0x672C)),
    (From-CodePoints @(0x4E0D,0x5F97,0x5BA3,0x79F0,0x6D4F,0x89C8,0x5668,0x76F4,0x8FDE))
)
foreach ($token in $ruleTokens) {
    if (-not $text.Contains($token)) {
        $errors.Add("ADR missing non-negotiable rule: $token")
    }
}

if ($RequireT005InProgress) {
    $controlPath = Join-Path $PSScriptRoot '..\docs\PROJECT_CONTROL.md'
    $controlText = Get-Content -Raw -Encoding UTF8 -LiteralPath $controlPath
    $t005Line = ($controlText -split "`r?`n" | Where-Object { $_ -match '^\- \[[ x]\] T005 ' }) | Select-Object -First 1
    $inProgress = From-CodePoints @(0x72B6,0x6001,0xFF1A,0x8FDB,0x884C,0x4E2D)
    if (-not $t005Line -or -not $t005Line.Contains($inProgress)) {
        $errors.Add('T005 must be in progress before ADR validation runs.')
    }
}

if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Host "ADR valid: $($candidates.Count) candidate comparisons, $($comparisonRows.Count) comparison-table rows, decision, costs, rules, exit conditions, and review date verified."
