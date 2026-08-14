[CmdletBinding()]
param(
    [ValidateSet('bootstrap', 'doctor', 'format', 'lint', 'test', 'benchmark')]
    [string]$Command = 'doctor',
    [switch]$Ci,
    [switch]$SkipPlatform
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

function Invoke-Native {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $false)][string[]]$CommandArgs = @()
    )
    Write-Host "==> $Label"
    & $Executable @CommandArgs
    if ($LASTEXITCODE -ne 0) {
        throw "$Label failed with exit code $LASTEXITCODE"
    }
    Write-Host "PASS: $Label"
}

function Invoke-PowerShellFile {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $false)][string[]]$Arguments = @()
    )
    Invoke-Native -Label $Label -Executable 'powershell' -CommandArgs (@('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $Path) + $Arguments)
}

function Assert-Command {
    param([Parameter(Mandatory = $true)][string]$Name)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required command is unavailable: $Name"
    }
}

function Get-ToolchainValidatorArgs {
    if ($SkipPlatform) { return @('.', '--no-probe') }
    return @('.')
}

function Invoke-Doctor {
    Assert-Command 'node'
    Assert-Command 'cargo'
    Invoke-Native -Label 'toolchain lock doctor' -Executable 'node' -CommandArgs (@('.\scripts\validate-toolchain-lock.mjs') + (Get-ToolchainValidatorArgs))
    Invoke-Native -Label 'workspace contract doctor' -Executable 'node' -CommandArgs @('.\scripts\validate-workspace.mjs', '.')
    Invoke-Native -Label 'git governance doctor' -Executable 'node' -CommandArgs @('.\scripts\validate-git-governance.mjs', '.')
    Invoke-Native -Label 'supply chain gate doctor' -Executable 'node' -CommandArgs @('.\scripts\validate-supply-chain.mjs', '.')
    Invoke-PowerShellFile -Label 'architecture rules doctor' -Path '.\scripts\validate-architecture-rules.ps1'
    Invoke-Native -Label 'cargo metadata doctor' -Executable 'cargo' -CommandArgs @('metadata', '--no-deps', '--format-version', '1', '--quiet')
}

function Invoke-Bootstrap {
    Assert-Command 'node'
    Assert-Command 'cargo'
    Write-Host 'Bootstrap is non-interactive and idempotent; package installation is delegated to the locked host toolchain policy.'
    Invoke-Native -Label 'validate locked toolchain projections' -Executable 'node' -CommandArgs (@('.\scripts\validate-toolchain-lock.mjs') + (Get-ToolchainValidatorArgs))
    Invoke-Native -Label 'fetch locked Rust dependencies' -Executable 'cargo' -CommandArgs @('fetch', '--locked')
    Invoke-Native -Label 'generate workspace metadata' -Executable 'cargo' -CommandArgs @('metadata', '--no-deps', '--format-version', '1', '--quiet')
    Invoke-Doctor
}

function Invoke-Format {
    Assert-Command 'cargo'
    if ($Ci) {
        Invoke-Native -Label 'check Rust formatting' -Executable 'cargo' -CommandArgs @('fmt', '--all', '--', '--check')
    } else {
        Invoke-Native -Label 'format Rust workspace' -Executable 'cargo' -CommandArgs @('fmt', '--all')
        Invoke-Native -Label 'verify Rust formatting' -Executable 'cargo' -CommandArgs @('fmt', '--all', '--', '--check')
    }
}

function Invoke-Lint {
    Assert-Command 'cargo'
    Assert-Command 'node'
    Invoke-Native -Label 'Rust clippy' -Executable 'cargo' -CommandArgs @('clippy', '--workspace', '--all-targets', '--all-features', '--', '-D', 'warnings')
    Invoke-Native -Label 'workspace lint contract' -Executable 'node' -CommandArgs @('.\scripts\validate-workspace.mjs', '.')
    Invoke-Native -Label 'toolchain lint contract' -Executable 'node' -CommandArgs (@('.\scripts\validate-toolchain-lock.mjs') + (Get-ToolchainValidatorArgs))
}

function Invoke-Test {
    Assert-Command 'cargo'
    Assert-Command 'node'
    Invoke-Native -Label 'Rust workspace tests' -Executable 'cargo' -CommandArgs @('test', '--workspace')
    Invoke-Native -Label 'workspace contract tests' -Executable 'node' -CommandArgs @('.\scripts\test-workspace-contract.mjs', '.')
    Invoke-Native -Label 'toolchain lock tests' -Executable 'node' -CommandArgs @('.\scripts\test-toolchain-lock.mjs', '.')
    Invoke-Native -Label 'supply chain contract tests' -Executable 'node' -CommandArgs @('.\scripts\test-supply-chain.mjs', '.')
    foreach ($manifest in @('spikes/wgpu-terminal/Cargo.toml', 'spikes/terminal-engines/Cargo.toml', 'spikes/ssh-engines/Cargo.toml', 'spikes/terminal-contract/Cargo.toml')) {
        Invoke-Native -Label "spike tests $manifest" -Executable 'cargo' -CommandArgs @('test', '--manifest-path', $manifest, '--locked')
    }
}

function Invoke-Benchmark {
    Assert-Command 'cargo'
    Assert-Command 'node'
    Invoke-Native -Label 'compile workspace benchmarks' -Executable 'cargo' -CommandArgs @('bench', '--workspace', '--no-run')
    Invoke-PowerShellFile -Label 'validate performance budget contract' -Path '.\scripts\validate-budget.ps1'
    Invoke-Native -Label 'benchmark metadata snapshot' -Executable 'cargo' -CommandArgs @('metadata', '--no-deps', '--format-version', '1', '--quiet')
}

Push-Location $Root
try {
    switch ($Command) {
        'bootstrap' { Invoke-Bootstrap }
        'doctor' { Invoke-Doctor }
        'format' { Invoke-Format }
        'lint' { Invoke-Lint }
        'test' { Invoke-Test }
        'benchmark' { Invoke-Benchmark }
        default { throw "Unsupported developer command: $Command" }
    }
    Write-Host "Developer command '$Command' completed successfully."
} finally {
    Pop-Location
}
