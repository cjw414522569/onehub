[CmdletBinding()]
param(
    [string]$ReportFile,
    [string]$MatrixFile,
    [string]$FixtureFile,
    [string]$MarkdownFile,
    [switch]$SkipBuilds,
    [switch]$SkipNegativeTests
)

$ErrorActionPreference = 'Stop'
$root = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ([string]::IsNullOrWhiteSpace($ReportFile)) { $ReportFile = Join-Path $root 'spikes\terminal-engines\report.json' }
if ([string]::IsNullOrWhiteSpace($MatrixFile)) { $MatrixFile = Join-Path $root 'spikes\terminal-engines\matrix.json' }
if ([string]::IsNullOrWhiteSpace($FixtureFile)) { $FixtureFile = Join-Path $root 'spikes\terminal-engines\fixtures\vt-unicode-replay.txt' }
if ([string]::IsNullOrWhiteSpace($MarkdownFile)) { $MarkdownFile = Join-Path $root 'docs\reports\TERMINAL_ENGINE_SPIKE.md' }
$reportPath = [System.IO.Path]::GetFullPath($ReportFile)
$matrixPath = [System.IO.Path]::GetFullPath($MatrixFile)
$fixturePath = [System.IO.Path]::GetFullPath($FixtureFile)
$markdownPath = [System.IO.Path]::GetFullPath($MarkdownFile)
$manifestPath = Join-Path $root 'spikes\terminal-engines\Cargo.toml'
$validatorPath = Join-Path $root 'scripts\validate-terminal-engine-spike.ps1'

function Invoke-NativeStep {
    param(
        [string]$FilePath,
        [string[]]$Arguments,
        [string]$Label
    )
    $oldErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $merged = & $FilePath @Arguments 2>&1
        $exitCode = $LASTEXITCODE
        if ($null -eq $exitCode) { $exitCode = 0 }
    }
    finally {
        $ErrorActionPreference = $oldErrorActionPreference
    }
    $text = ($merged | ForEach-Object { [string]$_ }) -join [Environment]::NewLine
    if ($exitCode -ne 0) {
        throw "$Label failed with exit code $exitCode.`n$text"
    }
    Write-Host "$Label passed (exit=$exitCode)."
    return $text
}

if (-not $SkipBuilds) {
    Invoke-NativeStep -FilePath 'cargo' -Arguments @('check', '--manifest-path', $manifestPath) -Label 'cargo check' | Out-Null
    Invoke-NativeStep -FilePath 'cargo' -Arguments @('test', '--manifest-path', $manifestPath) -Label 'cargo test' | Out-Null
}

Invoke-NativeStep -FilePath 'cargo' -Arguments @(
    'run', '--release', '--manifest-path', $manifestPath, '--',
    '--fixture', $fixturePath,
    '--matrix', $matrixPath,
    '--output', $reportPath
) -Label 'terminal engine release spike' | Out-Null

Invoke-NativeStep -FilePath 'powershell' -Arguments @(
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $validatorPath,
    '-ReportFile', $reportPath,
    '-MatrixFile', $matrixPath
) -Label 'terminal engine report validation' | Out-Null

$archivedJsonPath = Join-Path $root 'docs\reports\TERMINAL_ENGINE_SPIKE.json'
Copy-Item -LiteralPath $reportPath -Destination $archivedJsonPath -Force
Write-Host "JSON report archived: $archivedJsonPath"

$report = Get-Content -Raw -Encoding UTF8 -LiteralPath $reportPath | ConvertFrom-Json
$lines = [System.Collections.Generic.List[string]]::new()
$lines.Add('# Terminal Engine Spike')
$lines.Add('')
$lines.Add("Generated: $($report.generated_at_utc)")
$lines.Add("Corpus: $($report.corpus_bytes) bytes, SHA-256 $($report.corpus_sha256)")
$lines.Add("Repetitions: $($report.repetitions); terminal size: $($report.terminal_size.columns)x$($report.terminal_size.rows)")
$lines.Add("vttest: $($report.vttest_status) $($report.vttest_blocked_reason)")
$lines.Add('')
$lines.Add('## Candidate results')
$lines.Add('')
$lines.Add('| Candidate | Version | License | p50 ms | p95 ms | MiB/s | Errors | Output SHA-256 |')
$lines.Add('| --- | --- | --- | ---: | ---: | ---: | ---: | --- |')
foreach ($candidate in @($report.candidates)) {
    $lines.Add("| $($candidate.id) | $($candidate.version) | $($candidate.license) | $($candidate.metrics.p50_ms) | $($candidate.metrics.p95_ms) | $($candidate.metrics.throughput_mb_s) | $($candidate.metrics.errors) | $($candidate.output_sha256) |")
}
$lines.Add('')
$lines.Add('## Interpretation')
$lines.Add('')
$lines.Add('- `alacritty_terminal` is exercised through its real `vte::ansi::Processor` and `Term` state model.')
$lines.Add('- `wezterm-fork-proxy` is the explicitly named `tattoy-wezterm-term` fork; it must not be represented as the official standalone `wezterm-term` crate.')
$lines.Add('- `adapter-baseline` is an independent auditable CSI/OSC/UTF-8 baseline, not a production parser.')
$lines.Add('- `vttest` is reported as blocked when the executable is unavailable; the shared corpus is not presented as full vttest coverage.')
[System.IO.File]::WriteAllText($markdownPath, ($lines -join [Environment]::NewLine), (New-Object System.Text.UTF8Encoding($false)))
Write-Host "Markdown report written: $markdownPath"

if (-not $SkipNegativeTests) {
    $negativeCases = @(
        @{ Name = 'missing candidate'; File = Join-Path $root 'scripts\testdata\invalid-terminal-engine-missing-candidate.json'; Expected = 'Missing required candidate' },
        @{ Name = 'corpus hash mismatch'; File = Join-Path $root 'scripts\testdata\invalid-terminal-engine-corpus-hash.json'; Expected = 'corpus_sha256 differs' },
        @{ Name = 'missing license'; File = Join-Path $root 'scripts\testdata\invalid-terminal-engine-license.json'; Expected = 'license must be non-empty' },
        @{ Name = 'blocked vttest marked passed'; File = Join-Path $root 'scripts\testdata\invalid-terminal-engine-vttest-passed.json'; Expected = 'passed vttest_status requires vttest_command' }
    )
    foreach ($case in $negativeCases) {
        $oldErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            $output = & powershell -NoProfile -ExecutionPolicy Bypass -File $validatorPath -ReportFile $case.File -MatrixFile $matrixPath 2>&1
            $exitCode = $LASTEXITCODE
        }
        finally {
            $ErrorActionPreference = $oldErrorActionPreference
        }
        $text = ($output | ForEach-Object { [string]$_ }) -join [Environment]::NewLine
        if ($exitCode -eq 0) { throw "Negative case '$($case.Name)' unexpectedly passed." }
        if ($text -notlike "*$($case.Expected)*") { throw "Negative case '$($case.Name)' did not report expected text '$($case.Expected)'.`n$text" }
        Write-Host "negative case '$($case.Name)' rejected as expected (exit=$exitCode)."
    }
}

Write-Host "Terminal engine spike completed: $reportPath"
