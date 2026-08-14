[CmdletBinding()]
param(
    [string]$ReportFile,
    [string]$ArtifactsDirectory,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($ReportFile)) {
    $ReportFile = Join-Path $root 'docs\reports\WGPU_FEASIBILITY.json'
}
if ([string]::IsNullOrWhiteSpace($ArtifactsDirectory)) {
    $ArtifactsDirectory = Join-Path $root 'artifacts\perf\wgpu-feasibility'
}
$reportPath = [System.IO.Path]::GetFullPath($ReportFile)
$artifactPath = [System.IO.Path]::GetFullPath($ArtifactsDirectory)
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $reportPath) | Out-Null
New-Item -ItemType Directory -Force -Path $artifactPath | Out-Null

$fixturePath = Join-Path $root 'spikes\wgpu-terminal\fixtures\replay-10mb.bin'
$manifestPath = Join-Path $root 'spikes\wgpu-terminal\fixtures\manifest.json'
$releaseBinary = Join-Path $root 'spikes\wgpu-terminal\target\release\wgpu-terminal.exe'
$debugBinary = Join-Path $root 'spikes\wgpu-terminal\target\debug\wgpu-terminal.exe'
if (-not (Test-Path -LiteralPath $fixturePath) -or -not (Test-Path -LiteralPath $manifestPath)) {
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root 'scripts\generate-wgpu-fixtures.ps1')
    if ($LASTEXITCODE -ne 0) { throw 'Fixture generation failed.' }
}
if (-not $SkipBuild) {
    cargo build --manifest-path (Join-Path $root 'spikes\wgpu-terminal\Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'wgpu debug/profile build failed.' }
    cargo build --release --manifest-path (Join-Path $root 'spikes\wgpu-terminal\Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'wgpu release build failed.' }
}

function Invoke-Benchmark {
    param(
        [string]$Backend,
        [string]$DeviceClass,
        [string]$Profile
    )
    $id = "$DeviceClass-$Backend-$Profile"
    $jsonPath = Join-Path $artifactPath "$id.json"
    $tracePath = Join-Path $artifactPath "$id-trace"
    New-Item -ItemType Directory -Force -Path $tracePath | Out-Null
    $command = "wgpu-terminal.exe --backend $Backend --scenario all --replay $fixturePath --frames 30 --scroll-lines 1000000 --device-class $DeviceClass --trace-dir $tracePath --output $jsonPath"

    $canRun = ($env:OS -eq 'Windows_NT' -and $DeviceClass -eq 'windows' -and @('dx12', 'vulkan') -contains $Backend)
    if (-not $canRun) {
        $blocked = [ordered]@{
            schema_version = 1
            status = 'blocked_environment'
            device_class = $DeviceClass
            backend_request = $Backend
            profile = $Profile
            reason = if ($env:OS -ne 'Windows_NT') { 'runner host is not Windows' } elseif ($DeviceClass -ne 'windows') { 'target device is not attached to this runner host' } else { 'backend is not available in the native Windows runner policy' }
            command = $command
        }
        $blocked | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 -LiteralPath $jsonPath
        return [pscustomobject]@{
            id = $id
            device_class = $DeviceClass
            backend = $Backend
            profile = $Profile
            command = $command
            exit_code = 2
            status = 'blocked_environment'
            report_file = $jsonPath
            trace_dir = $tracePath
            trace_files = @()
            blocked_reason = $blocked.reason
            stdout = ''
        }
    }

    $binary = if ($Profile -eq 'release') { $releaseBinary } else { $debugBinary }
    $old = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $output = if ($Profile -eq 'release') {
            & $binary --backend $Backend --scenario all --replay $fixturePath --frames 30 --scroll-lines 1000000 --device-class $DeviceClass --trace-dir $tracePath --output $jsonPath 2>&1
        } else {
            & (Join-Path $root 'spikes\wgpu-terminal\target\debug\wgpu-terminal.exe') --backend $Backend --scenario all --replay $fixturePath --frames 30 --scroll-lines 1000000 --device-class $DeviceClass --trace-dir $tracePath --output $jsonPath 2>&1
        }
        $exitCode = $LASTEXITCODE
        if ($null -eq $exitCode) { $exitCode = 0 }
    }
    finally { $ErrorActionPreference = $old }
    $report = $null
    if (Test-Path -LiteralPath $jsonPath) {
        try { $report = Get-Content -Raw -Encoding UTF8 -LiteralPath $jsonPath | ConvertFrom-Json } catch { $report = $null }
    }
    $traceFiles = @(Get-ChildItem -LiteralPath $tracePath -Recurse -File -ErrorAction SilentlyContinue)
    [pscustomobject]@{
        id = $id
        device_class = $DeviceClass
        backend = $Backend
        profile = $Profile
        command = $command
        exit_code = [int]$exitCode
        status = if ($null -ne $report) { [string]$report.status } elseif ($exitCode -eq 2) { 'blocked_environment' } else { 'failed' }
        report_file = $jsonPath
        trace_dir = $tracePath
        trace_files = @($traceFiles | ForEach-Object { $_.FullName })
        blocked_reason = if ($null -ne $report -and $report.PSObject.Properties['error']) { [string]$report.error } else { '' }
        stdout = (($output | ForEach-Object { [string]$_ }) -join [Environment]::NewLine).Trim()
    }
}

$matrix = @(
    @{ device = 'windows'; backends = @('dx12','vulkan','metal','webgpu') },
    @{ device = 'macos-intel'; backends = @('dx12','vulkan','metal','webgpu') },
    @{ device = 'macos-apple-silicon'; backends = @('dx12','vulkan','metal','webgpu') },
    @{ device = 'linux'; backends = @('dx12','vulkan','metal','webgpu') },
    @{ device = 'ios-ipados'; backends = @('dx12','vulkan','metal','webgpu') },
    @{ device = 'android'; backends = @('dx12','vulkan','metal','webgpu') },
    @{ device = 'web-pwa'; backends = @('dx12','vulkan','metal','webgpu') }
)
$results = [System.Collections.Generic.List[object]]::new()
foreach ($row in $matrix) {
    foreach ($backend in $row.backends) {
        foreach ($profile in @('debug', 'release')) {
            $results.Add((Invoke-Benchmark -Backend $backend -DeviceClass $row.device -Profile $profile))
        }
    }
}

$manifest = Get-Content -Raw -Encoding UTF8 -LiteralPath $manifestPath | ConvertFrom-Json
$report = [ordered]@{
    schema_version = 1
    generated_at_utc = [DateTime]::UtcNow.ToString('o')
    fixture = $manifest
    toolchain = [ordered]@{
        rustc = (& rustc -V 2>&1 | Out-String).Trim()
        cargo = (& cargo -V 2>&1 | Out-String).Trim()
        wgpu = '30.0.0'
    }
    backend_matrix = @('dx12','metal','vulkan','webgpu')
    device_matrix = @('windows','macos-intel','macos-apple-silicon','linux','ios-ipados','android','web-pwa')
    scenario_matrix = @('replay-10mb','4k-full-refresh','ligature-fi-ffi','ime-composition-proxy','scrollback-million-lines')
    delivery_phase = 'windows-first'
    current_release_platforms = @('windows')
    reserved_device_classes = @('macos-intel','macos-apple-silicon','linux','ios-ipados','android','web-pwa')
    interface_reservation = [ordered]@{
        protocol_schema = 'protocol/schema/domain-v1.json'
        dependency_rules = 'architecture/dependency-rules.json'
        renderer_contract = 'docs/ADR-004-terminal-abstraction.md'
        abi_boundary = 'docs/ADR-002-module-boundaries-and-dependencies.md'
    }
    windows_first = [ordered]@{
        accepted_native_cells = 4
        required_native_backends = @('dx12','vulkan')
        web_reference = 'browser_webgpu'
    }
    host_policy = 'Only windows/dx12 and windows/vulkan execute on this Windows runner; every other cell is explicit blocked_environment.'
    results = @($results)
    status = if (@($results | Where-Object { $_.status -eq 'failed' }).Count -eq 0) { 'complete_with_blocked_environment' } else { 'failed' }
    limitations = @(
        'The current runner has real DX12 and Vulkan evidence only on the Windows host GPU.',
        'Metal and browser WebGPU require their native host/browser runners and are recorded as blocked when unavailable.',
        'Ligature, IME, and scrollback are explicit headless proxies; native text-shaper, OS IME, and RSS/private-bytes gates remain follow-up platform work.',
        'No device farm or iOS/Android hardware is attached in this environment.'
    )
}
$report | ConvertTo-Json -Depth 12 | Set-Content -Encoding UTF8 -LiteralPath $reportPath
Write-Host "wgpu feasibility matrix completed: report=$reportPath results=$($results.Count)"
