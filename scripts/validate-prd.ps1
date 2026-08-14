[CmdletBinding()]
param(
    [string]$PrdFile,
    [switch]$RequireT002InProgress
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($PrdFile)) {
    $PrdFile = Join-Path $PSScriptRoot '..\docs\PRD.md'
}

function From-CodePoints {
    param([int[]]$CodePoints)
    return -join ($CodePoints | ForEach-Object { [char]$_ })
}

$resolved = (Resolve-Path -LiteralPath $PrdFile).Path
$lines = @(Get-Content -Encoding UTF8 -LiteralPath $resolved)
$text = $lines -join "`n"
$errors = [System.Collections.Generic.List[string]]::new()
$rowRegex = '^\| (?<id>(?:FR|NR)-\d{3}) \| (?<priority>P[0-3]) \| (?<story>.+?) \| (?<acceptance>.+?) \| (?<platform>.+?) \| (?<dependencies>.+?) \|$'
$candidateLines = @($lines | Where-Object { $_ -match '^\| (?:FR|NR)-' })
$rows = @()

if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
    $errors.Add('PRD file does not exist.')
}

foreach ($line in $candidateLines) {
    $match = [regex]::Match($line, $rowRegex)
    if (-not $match.Success) {
        $errors.Add("Malformed requirement row: $line")
        continue
    }
    $row = [pscustomobject]@{
        Id = $match.Groups['id'].Value
        Priority = $match.Groups['priority'].Value
        Story = $match.Groups['story'].Value.Trim()
        Acceptance = $match.Groups['acceptance'].Value.Trim()
        Platform = $match.Groups['platform'].Value.Trim()
        Dependencies = $match.Groups['dependencies'].Value.Trim()
    }
    $rows += $row
    foreach ($field in @('Story','Acceptance','Platform','Dependencies')) {
        if ([string]::IsNullOrWhiteSpace($row.$field)) {
            $errors.Add("$($row.Id) has an empty $field field.")
        }
    }
    if ($row.Story.Length -lt 8) {
        $errors.Add("$($row.Id) user story is too short to be testable.")
    }
    if ($row.Acceptance.Length -lt 12) {
        $errors.Add("$($row.Id) acceptance criteria are too short.")
    }
}

if ($rows.Count -lt 1) {
    $errors.Add('No requirement rows were found.')
}

$duplicateIds = @($rows | Group-Object Id | Where-Object Count -gt 1)
foreach ($duplicate in $duplicateIds) {
    $errors.Add("Duplicate requirement ID: $($duplicate.Name)")
}

$frRows = @($rows | Where-Object { $_.Id -match '^FR-' })
$nrRows = @($rows | Where-Object { $_.Id -match '^NR-' })
if ($frRows.Count -lt 20) {
    $errors.Add("Expected at least 20 functional requirements, found $($frRows.Count).")
}
if ($nrRows.Count -lt 5) {
    $errors.Add("Expected at least 5 non-commitment requirements, found $($nrRows.Count).")
}

foreach ($row in $frRows) {
    $number = [int]$row.Id.Substring(3)
    if ($number -ge 1 -and $number -le 99 -and $row.Priority -notin @('P0','P1')) {
        $errors.Add("Functional requirement $($row.Id) must be P0 or P1, found $($row.Priority).")
    }
    if ($number -ge 100 -and $row.Priority -ne 'P2') {
        $errors.Add("Post-launch functional requirement $($row.Id) must be P2, found $($row.Priority).")
    }
}
foreach ($row in $nrRows) {
    if ($row.Priority -ne 'P3') {
        $errors.Add("Non-commitment requirement $($row.Id) must be P3, found $($row.Priority).")
    }
}

$requiredHeadings = @('## 1.', '## 2.', '## 3.', '## 4.', '## 5.', '## 6.')
foreach ($heading in $requiredHeadings) {
    if (-not ($lines | Where-Object { $_ -like "$heading*" })) {
        $errors.Add("Missing PRD section heading: $heading")
    }
}

$gatewayToken = From-CodePoints @(0x7F51,0x5173)
$requiredTokens = @('FR-001','FR-015','FR-101','NR-001','P0','P1','P2','P3','Web/PWA','Tier 2','WebSocket/QUIC',$gatewayToken,'SSH','CLI')
foreach ($token in $requiredTokens) {
    if (-not $text.Contains($token)) {
        $errors.Add("Missing required PRD token: $token")
    }
}

$noCommitmentText = From-CodePoints @(0x4E0D,0x627F,0x8BFA)
if (-not $text.Contains($noCommitmentText)) {
    $errors.Add('PRD must explicitly state non-commitment boundaries.')
}
$securityStorageText = From-CodePoints @(0x7CFB,0x7EDF,0x5B89,0x5168,0x5B58,0x50A8)
if (-not $text.Contains($securityStorageText)) {
    $errors.Add('PRD must require system secure storage for secrets.')
}

if ($RequireT002InProgress) {
    $controlPath = Join-Path $PSScriptRoot '..\docs\PROJECT_CONTROL.md'
    $controlText = Get-Content -Raw -Encoding UTF8 -LiteralPath $controlPath
    $t002Line = ($controlText -split "`r?`n" | Where-Object { $_ -match '^\- \[[ x]\] T002 ' }) | Select-Object -First 1
    $inProgress = From-CodePoints @(0x72B6,0x6001,0xFF1A,0x8FDB,0x884C,0x4E2D)
    if (-not $t002Line -or -not $t002Line.Contains($inProgress)) {
        $errors.Add('T002 must be in progress before its schema test is run.')
    }
}

if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Host "PRD valid: $($rows.Count) requirement rows, $($frRows.Count) functional, $($nrRows.Count) non-commitment, required sections and boundary tokens verified."
