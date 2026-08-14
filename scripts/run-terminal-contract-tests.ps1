[CmdletBinding()]
param(
    [string]$ReportFile
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($ReportFile)) {
    $ReportFile = Join-Path $root 'docs\reports\TERMINAL_CONTRACT_TEST.md'
}
$reportPath = [System.IO.Path]::GetFullPath($ReportFile)
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $reportPath) | Out-Null

function Invoke-Check {
    param([string]$Command, [scriptblock]$Action)
    $old = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $output = & $Action 2>&1
        $exitCode = $LASTEXITCODE
        if ($null -eq $exitCode) { $exitCode = 0 }
    }
    finally {
        $ErrorActionPreference = $old
    }
    [pscustomobject]@{
        command = $Command
        exit_code = [int]$exitCode
        output = (($output | ForEach-Object { [string]$_ }) -join [Environment]::NewLine).Trim()
    }
}

$checks = @()
$checks += Invoke-Check 'cargo fmt --manifest-path .\spikes\terminal-contract\Cargo.toml -- --check' {
    cargo fmt --manifest-path .\spikes\terminal-contract\Cargo.toml -- --check
}
$checks += Invoke-Check 'cargo check --manifest-path .\spikes\terminal-contract\Cargo.toml' {
    cargo check --manifest-path .\spikes\terminal-contract\Cargo.toml
}
$checks += Invoke-Check 'cargo test --manifest-path .\spikes\terminal-contract\Cargo.toml' {
    cargo test --manifest-path .\spikes\terminal-contract\Cargo.toml
}
$checks += Invoke-Check 'powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\validate-terminal-contract.ps1' {
    powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\validate-terminal-contract.ps1
}

$negativeFiles = @(
    'scripts\testdata\invalid-terminal-contract-missing-layer.json',
    'scripts\testdata\invalid-terminal-contract-per-character-abi.json'
)
foreach ($relative in $negativeFiles) {
    $path = Join-Path $root $relative
    $check = Invoke-Check "powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\validate-terminal-contract.ps1 -ContractFile $relative" {
        powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root 'scripts\validate-terminal-contract.ps1') -ContractFile $path
    }
    if ($check.exit_code -ne 1) {
        throw "Negative fixture did not fail with exit code 1: $relative (exit=$($check.exit_code))"
    }
    $checks += $check
}

$failed = @($checks | Where-Object exit_code -ne 0 | Where-Object {
    $_.command -notlike '*invalid-terminal-contract*'
})
if ($failed.Count -gt 0) {
    throw "Terminal contract checks failed: $($failed.command -join ', ')"
}

$lines = [System.Collections.Generic.List[string]]::new()
$lines.Add('# Terminal Contract Test Report')
$lines.Add('')
$lines.Add("Generated: $([DateTime]::UtcNow.ToString('o'))")
$lines.Add('Contract: `protocol/terminal/terminal-contract-v1.json`')
$lines.Add('ADR: `docs/ADR-004-terminal-abstraction.md`')
$lines.Add('')
$lines.Add('| Check | Exit code | Result |')
$lines.Add('| --- | ---: | --- |')
foreach ($check in $checks) {
    $result = if ($check.exit_code -eq 0) { 'passed' } elseif ($check.command -like '*invalid-terminal-contract*' -and $check.exit_code -eq 1) { 'rejected as expected' } else { 'failed' }
    $lines.Add("| ``$($check.command)`` | $($check.exit_code) | $result |")
}
$lines.Add('')
$lines.Add('## Evidence')
$lines.Add('')
$lines.Add('- Rust contract tests cover parser chunking equivalence and structured diagnostics.')
$lines.Add('- Two independent screen-model wrappers produce identical deterministic snapshots.')
$lines.Add('- Render diff tests cover no-op and bounded single-region changes with stable operation order.')
$lines.Add('- Input encoder tests cover text, key, paste, mouse, and resize in one batch-only trait.')
$lines.Add('- Negative fixtures prove missing layers and `no_per_character_abi=false` exit with code 1.')
[System.IO.File]::WriteAllLines($reportPath, $lines, [System.Text.UTF8Encoding]::new($false))

Write-Host "Terminal contract tests passed: report=$reportPath"
