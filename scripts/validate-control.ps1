[CmdletBinding()]
param(
    [string]$ControlFile,
    [switch]$RequireAllCompleted
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($ControlFile)) {
    $ControlFile = Join-Path $PSScriptRoot '..\docs\PROJECT_CONTROL.md'
}

function ConvertFrom-CodePoints {
    param([int[]]$CodePoints)
    return -join ($CodePoints | ForEach-Object { [char]$_ })
}

# Keep this source file ASCII-only so Windows PowerShell 5.1 can parse it
# without relying on a UTF-8 BOM. Chinese field names are built at runtime.
$statusLabel = ConvertFrom-CodePoints @(0x72B6, 0x6001)
$acceptanceLabel = ConvertFrom-CodePoints @(0x9A8C, 0x6536)
$testLabel = ConvertFrom-CodePoints @(0x6D4B, 0x8BD5)
$evidenceLabel = ConvertFrom-CodePoints @(0x8BC1, 0x636E)
$notStartedStatus = ConvertFrom-CodePoints @(0x672A, 0x5F00, 0x59CB)
$inProgressStatus = ConvertFrom-CodePoints @(0x8FDB, 0x884C, 0x4E2D)
$completedStatus = ConvertFrom-CodePoints @(0x5DF2, 0x5B8C, 0x6210)
$blockedStatus = ConvertFrom-CodePoints @(0x963B, 0x585E)
$emptyEvidence = [char]0x2014
$labelSeparator = '(?:' + [regex]::Escape([string][char]0xFF1A) + '|:)'

$resolvedControlFile = (Resolve-Path -LiteralPath $ControlFile).Path
$lines = @(Get-Content -LiteralPath $resolvedControlFile -Encoding UTF8)
$allowedStatuses = @($notStartedStatus, $inProgressStatus, $completedStatus, $blockedStatus) |
    ForEach-Object { [regex]::Escape($_) }
$taskPattern = '^- \[(?<mark>[ x])\] T(?<id>\d{3}) (?<title>.+?) \| ' +
    [regex]::Escape($statusLabel) + $labelSeparator + '(?<status>' + ($allowedStatuses -join '|') + ') \| ' +
    [regex]::Escape($acceptanceLabel) + $labelSeparator + '(?<acceptance>.+?) \| ' +
    [regex]::Escape($testLabel) + $labelSeparator + '(?<test>.+?) \| ' +
    [regex]::Escape($evidenceLabel) + $labelSeparator + '(?<evidence>.+)$'
$taskPrefixPattern = '^- \[[ x]\] T\d{3} '
$tasks = @()
$errors = [System.Collections.Generic.List[string]]::new()

for ($index = 0; $index -lt $lines.Count; $index++) {
    $line = $lines[$index]
    if ($line -match $taskPrefixPattern) {
        if ($line -notmatch $taskPattern) {
            $errors.Add("Line $($index + 1) has an invalid task format.")
            continue
        }

        $task = [pscustomobject]@{
            Line       = $index + 1
            Id         = [int]$Matches.id
            Mark       = $Matches.mark
            Status     = $Matches.status
            Acceptance = $Matches.acceptance.Trim()
            Test       = $Matches.test.Trim()
            Evidence   = $Matches.evidence.Trim()
        }
        $tasks += $task

        if ($task.Status -eq $completedStatus) {
            if ($task.Mark -ne 'x') {
                $errors.Add("T$($task.Id.ToString('000')) is completed but is not checked.")
            }
            if ($task.Evidence -eq $emptyEvidence -or [string]::IsNullOrWhiteSpace($task.Evidence)) {
                $errors.Add("T$($task.Id.ToString('000')) is completed but has no evidence.")
            }
        }
        elseif ($task.Mark -eq 'x') {
            $errors.Add("T$($task.Id.ToString('000')) is checked but its status is not completed.")
        }

        if ([string]::IsNullOrWhiteSpace($task.Acceptance) -or [string]::IsNullOrWhiteSpace($task.Test)) {
            $errors.Add("T$($task.Id.ToString('000')) is missing acceptance or test content.")
        }
    }
}

if ($tasks.Count -eq 0) {
    $errors.Add('No task lines were found.')
}
else {
    $duplicateIds = $tasks | Group-Object Id | Where-Object Count -gt 1
    foreach ($duplicate in $duplicateIds) {
        $errors.Add("Duplicate task T$(([int]$duplicate.Name).ToString('000')).")
    }

    for ($expected = 1; $expected -le $tasks.Count; $expected++) {
        if ($tasks[$expected - 1].Id -ne $expected) {
            $errors.Add("Task sequence error: expected T$($expected.ToString('000')), found T$($tasks[$expected - 1].Id.ToString('000')).")
        }
    }

    $inProgress = @($tasks | Where-Object Status -eq $inProgressStatus)
    if ($inProgress.Count -gt 1) {
        $errors.Add("There are $($inProgress.Count) tasks in progress; at most one is allowed.")
    }

    $firstNonCompleted = $tasks | Where-Object Status -ne $completedStatus | Select-Object -First 1
    $completedAfterGap = $tasks | Where-Object {
        $firstNonCompleted -and $_.Id -gt $firstNonCompleted.Id -and $_.Status -eq $completedStatus
    }
    if ($completedAfterGap) {
        $errors.Add("T$($firstNonCompleted.Id.ToString('000')) is unfinished, but a later task is completed.")
    }

    $startedAfterCurrent = $tasks | Where-Object {
        $firstNonCompleted -and $_.Id -gt $firstNonCompleted.Id -and $_.Status -ne $notStartedStatus
    }
    if ($startedAfterCurrent) {
        $errors.Add("T$($firstNonCompleted.Id.ToString('000')) is the current task, but a later task has already started.")
    }

    if ($RequireAllCompleted) {
        $unfinished = @($tasks | Where-Object Status -ne $completedStatus)
        if ($unfinished.Count -gt 0) {
            $errors.Add("All tasks are required, but $($unfinished.Count) remain; first: T$($unfinished[0].Id.ToString('000')).")
        }
    }
}

if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Error $_ }
    exit 1
}

$completedCount = @($tasks | Where-Object Status -eq $completedStatus).Count
$inProgressCount = @($tasks | Where-Object Status -eq $inProgressStatus).Count
$blockedCount = @($tasks | Where-Object Status -eq $blockedStatus).Count
$notStartedCount = $tasks.Count - $completedCount - $inProgressCount - $blockedCount
Write-Host "Control document valid: total=$($tasks.Count), completed=$completedCount, in_progress=$inProgressCount, blocked=$blockedCount, not_started=$notStartedCount."
