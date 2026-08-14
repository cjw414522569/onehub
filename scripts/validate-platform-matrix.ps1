[CmdletBinding()]
param(
    [string]$ControlFile,
    [switch]$RequireTaskInProgress
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($ControlFile)) {
    $ControlFile = Join-Path $PSScriptRoot '..\docs\PROJECT_CONTROL.md'
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

# Keep this script ASCII-only for Windows PowerShell 5.1 parsing.
$resolved = (Resolve-Path -LiteralPath $ControlFile).Path
$text = Get-Content -Raw -Encoding UTF8 -LiteralPath $resolved
$errors = [System.Collections.Generic.List[string]]::new()

$desktopWord = From-CodePoints @(0x684C,0x9762)
$phoneWord = From-CodePoints @(0x624B,0x673A)
$tabletWord = From-CodePoints @(0x5E73,0x677F)
$originalWord = From-CodePoints @(0x539F,0x751F)
$packageWord = From-CodePoints @(0x5305)
$tvWord = From-CodePoints @(0x7535,0x89C6)
$watchWord = From-CodePoints @(0x624B,0x8868)
$matrixHeading = '## 1.1 ' + (From-CodePoints @(0x5E73,0x53F0,0x652F,0x6301,0x77E9,0x9635,0xFF08,0x0054,0x0030,0x0030,0x0031,0xFF09))
$protocolHeading = '## 2. ' + (From-CodePoints @(0x5F3A,0x5236,0x6267,0x884C,0x534F,0x8BAE))
$inProgressText = From-CodePoints @(0x72B6,0x6001,0xFF1A,0x8FDB,0x884C,0x4E2D)
$nativeTcpText = From-CodePoints @(0x539F,0x751F,0x0020,0x0054,0x0043,0x0050,0x0020,0x0053,0x0053,0x0048)
$gatewayText = From-CodePoints @(0x4EC5,0x901A,0x8FC7,0x81EA,0x6258,0x7BA1,0x0020,0x0057,0x0065,0x0062,0x0053,0x006F,0x0063,0x006B,0x0065,0x0074,0x002F,0x0051,0x0055,0x0049,0x0043,0x0020,0x0053,0x0053,0x0048,0x0020,0x7F51,0x5173)
$noDirectTcpText = From-CodePoints @(0x4E0D,0x76F4,0x8FDE,0x0020,0x0054,0x0043,0x0050,0x0020,0x0032,0x0032)
$noCommitText = From-CodePoints @(0x4E0D,0x627F,0x8BFA)
$unsupportedText = From-CodePoints @(0x4E0D,0x652F,0x6301)
$recentStableText = From-CodePoints @(0x6700,0x8FD1,0x4E24,0x4E2A,0x7A33,0x5B9A,0x5927,0x7248,0x672C)
$rustSystemsText = From-CodePoints @(0x0052,0x0075,0x0073,0x0074,0x0020,0x6838,0x5FC3,0x652F,0x6301,0x7684,0x7CFB,0x7EDF)
$spikeResultText = From-CodePoints @(0x53EF,0x884C,0x6027,0x0020,0x0073,0x0070,0x0069,0x006B,0x0065,0x0020,0x7ED3,0x679C,0x4E3A,0x51C6)
$browserRuntimeText = From-CodePoints @(0x6D4F,0x89C8,0x5668,0x8FD0,0x884C,0x65F6)
$requiredRows = @(
    ('Windows ' + $desktopWord),
    ('macOS ' + $desktopWord),
    ('Linux ' + $desktopWord),
    'iPhone/iPad',
    ('Android ' + $phoneWord + '/' + $tabletWord),
    'Web/PWA',
    'CLI',
    'HarmonyOS NEXT',
    ('ChromeOS ' + $originalWord + $packageWord),
    ('BSD/' + $tvWord + '/' + $watchWord)
)
$requiredPhrases = @($nativeTcpText, $gatewayText, $noDirectTcpText, 'Tier 1', 'Tier 2', $noCommitText, $unsupportedText)
$forbiddenVague = @(
    (From-CodePoints @(0x5168,0x90E8,0x652F,0x6301)),
    (From-CodePoints @(0x6240,0x6709,0x5E73,0x53F0,0x90FD,0x652F,0x6301)),
    (From-CodePoints @(0x4EFB,0x610F,0x7248,0x672C)),
    (From-CodePoints @(0x5168,0x5E73,0x53F0,0x652F,0x6301)),
    (From-CodePoints @(0x65E0,0x9650,0x627F,0x8BFA))
)
$versionPolicies = @('Windows 10 22H2', 'macOS 13 Ventura', 'glibc 2.35', 'iOS/iPadOS 16.4', 'Android 10 / API 29', $recentStableText, $rustSystemsText, $spikeResultText, [char]0x2014)
$runtimePolicies = @('x64', 'ARM64', 'x86_64', 'arm64', 'aarch64', 'arm64-v8a', $browserRuntimeText, $spikeResultText, [char]0x2014)

$matrixStart = $text.IndexOf($matrixHeading)
$matrixEnd = $text.IndexOf($protocolHeading)
if ($matrixStart -lt 0 -or $matrixEnd -le $matrixStart) {
    $errors.Add('T001 matrix section is missing or out of order.')
} else {
    $matrix = $text.Substring($matrixStart, $matrixEnd - $matrixStart)
    foreach ($row in $requiredRows) {
        if ($matrix -notmatch ('(?m)^\| ' + [regex]::Escape($row) + ' \|')) {
            $errors.Add("Missing platform row: $row")
        }
    }

    $tableRows = @($matrix -split "`r?`n" | Where-Object { $_ -match '^\| .+ \|$' })
    if ($tableRows.Count -lt 11) {
        $errors.Add("Platform table has $($tableRows.Count) rows; expected header, separator, and ten platform rows.")
    }

    foreach ($phrase in $requiredPhrases) {
        if (-not $matrix.Contains($phrase)) {
            $errors.Add("Missing required matrix constraint: $phrase")
        }
    }

    foreach ($phrase in $forbiddenVague) {
        if (Test-ContainsAny -Text ($tableRows -join "`n") -Candidates @($phrase)) {
            $errors.Add("Forbidden vague promise appears in platform table: $phrase")
        }
    }

    foreach ($line in $tableRows | Select-Object -Skip 2) {
        if (-not (Test-ContainsAny -Text $line -Candidates $versionPolicies)) {
            $errors.Add("Platform row lacks an explicit minimum-version policy: $line")
        }
        if (-not (Test-ContainsAny -Text $line -Candidates $runtimePolicies)) {
            $errors.Add("Platform row lacks an explicit CPU/runtime policy: $line")
        }
    }

    $webRow = ($tableRows | Where-Object { $_ -match '^\| Web/PWA \|' }) | Select-Object -First 1
    $webSelfHosted = From-CodePoints @(0x81EA,0x6258,0x7BA1)
    if (-not $webRow -or -not $webRow.Contains($webSelfHosted) -or -not $webRow.Contains($noDirectTcpText) -or -not $webRow.Contains('Tier 2')) {
        $errors.Add('Web/PWA row does not state the gateway-only limitation and Tier 2 status.')
    }
}

$taskLine = ($text -split "`r?`n" | Where-Object { $_ -match '^\- \[[ x]\] T001 ' }) | Select-Object -First 1
if (-not $taskLine) {
    $errors.Add('T001 task line is missing.')
} elseif ($RequireTaskInProgress -and -not $taskLine.Contains($inProgressText)) {
    $errors.Add('T001 must be in progress before its test is run.')
}

if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Host "Platform matrix valid: $($requiredRows.Count) required rows, explicit version/runtime policies, Web gateway limitation, and bounded support claims verified."
