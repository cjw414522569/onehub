[CmdletBinding()]
param(
    [string]$ReportFile,
    [string]$MarkdownFile
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($ReportFile)) {
    $ReportFile = Join-Path $root 'docs\reports\WGPU_FEASIBILITY.json'
}
if ([string]::IsNullOrWhiteSpace($MarkdownFile)) {
    $MarkdownFile = Join-Path $root 'docs\reports\WGPU_FEASIBILITY.md'
}
$report = Get-Content -Raw -Encoding UTF8 -LiteralPath $ReportFile | ConvertFrom-Json
$outputPath = [System.IO.Path]::GetFullPath($MarkdownFile)
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $outputPath) | Out-Null

$lines = [System.Collections.Generic.List[string]]::new()
$lines.Add('# wgpu Shared Terminal Feasibility Report')
$lines.Add('')
$lines.Add("Generated: $($report.generated_at_utc)")
$lines.Add("Status: **$($report.status)**")
$lines.Add('')
$lines.Add('## Delivery phase')
$lines.Add('')
$lines.Add('- Delivery phase: **Windows-first**. The current release gate covers the Windows native client only; this is an execution order decision, not an architecture reduction.')
$lines.Add("- Current release platforms: $(@($report.current_release_platforms) -join ', '); accepted native cells: $($report.windows_first.accepted_native_cells); required native backends: $(@($report.windows_first.required_native_backends) -join ', ').")
$lines.Add("- Web/PWA remains a reference benchmark via $($report.windows_first.web_reference) and is not a current native release platform.")
$lines.Add("- Reserved future device classes: $(@($report.reserved_device_classes) -join ', '). They retain the shared protocol, dependency, renderer, and ABI boundaries but are not converted into current passes.")
$lines.Add("- Interface reservation: protocol=$($report.interface_reservation.protocol_schema); dependencies=$($report.interface_reservation.dependency_rules); renderer=$($report.interface_reservation.renderer_contract); ABI=$($report.interface_reservation.abi_boundary).")
$lines.Add('')
$lines.Add('## Scope')
$lines.Add('')
$lines.Add('- Backends: DX12, Metal, Vulkan, WebGPU.')
$lines.Add('- Device classes: Windows, macOS Intel, macOS Apple Silicon, Linux, iOS/iPadOS, Android, Web/PWA.')
$lines.Add('- Profiles: debug and release.')
$lines.Add('- Scenarios: 10 MiB replay, 4K refresh, ligature fixture, IME composition proxy, million-line scrollback proxy.')
$lines.Add("- Fixture: ``$($report.fixture.file)``; bytes=$($report.fixture.bytes); SHA-256=$($report.fixture.sha256).")
$lines.Add('')
$lines.Add('## Real host evidence')
$lines.Add('')
$lines.Add('| Cell | Adapter | Replay P50/P95 | Replay MB/s | 4K P50/P95 | 4K FPS | Trace SHA-256 |')
$lines.Add('| --- | --- | ---: | ---: | ---: | ---: | --- |')
foreach ($result in @($report.results | Where-Object status -eq 'passed' | Sort-Object id)) {
    $native = Get-Content -Raw -Encoding UTF8 -LiteralPath $result.report_file | ConvertFrom-Json
    $replay = $native.scenarios | Where-Object id -eq 'replay-10mb'
    $frame = $native.scenarios | Where-Object id -eq '4k-full-refresh'
    $traceFile = @(Get-ChildItem -LiteralPath $result.trace_dir -Recurse -File -ErrorAction SilentlyContinue | Select-Object -First 1)
    $traceHash = if ($traceFile.Count -eq 1) { (Get-FileHash -Algorithm SHA256 -LiteralPath $traceFile[0].FullName).Hash.ToLowerInvariant() } else { 'missing' }
    $lines.Add("| ``$($result.id)`` | $($native.adapter.name) / $($native.adapter.backend) | $($replay.p50_ms) / $($replay.p95_ms) ms | $([math]::Round([double]$replay.throughput_mb_s, 2)) | $($frame.p50_ms) / $($frame.p95_ms) ms | $([math]::Round([double]$frame.fps, 2)) | ``$traceHash`` |")
}
$lines.Add('')
$lines.Add('The four real cells are Windows + NVIDIA GeForce RTX 5060: DX12/Vulkan in debug and release. They pass the Windows T003 replay and 4K thresholds. The trace files are wgpu `trace.ron` captures produced with `--trace-dir`.')
$lines.Add('')
$browserBenchmarkPath = Join-Path $root 'artifacts\perf\wgpu-feasibility\browser-webgpu-benchmark.json'
$lines.Add('## Browser WebGPU evidence')
$lines.Add('')
if (Test-Path -LiteralPath $browserBenchmarkPath) {
    $browser = Get-Content -Raw -Encoding UTF8 -LiteralPath $browserBenchmarkPath | ConvertFrom-Json
    $browserReplay = @($browser.scenarios | Where-Object id -eq 'replay-10mb')[0]
    $browserFrame = @($browser.scenarios | Where-Object id -eq '4k-full-refresh')[0]
    $lines.Add('- Runner: Chrome via CDP; backend: WebGPU; adapter/device creation: passed.')
    $lines.Add("- Fixture: ``$($browser.fixture.file)``; bytes=$($browser.fixture.bytes); SHA-256=$($browser.fixture.sha256).")
    $lines.Add("- 10 MiB replay: samples=$($browserReplay.samples); P50/P95/P99=$($browserReplay.p50_ms)/$($browserReplay.p95_ms)/$($browserReplay.p99_ms) ms; throughput=$([math]::Round([double]$browserReplay.throughput_mb_s, 2)) MB/s; passed=$($browserReplay.passed).")
    $lines.Add("- 4K render pass: samples=$($browserFrame.samples); P50/P95/P99=$($browserFrame.p50_ms)/$($browserFrame.p95_ms)/$($browserFrame.p99_ms) ms; FPS=$([math]::Round([double]$browserFrame.fps, 2)); passed=$($browserFrame.passed).")
    $lines.Add('- Ligature, IME, and scrollback remain explicitly `proxy_passed`; this browser artifact is not native shaping, OS IME, RSS/private-bytes, or soak evidence.')
    $lines.Add('- Artifact: ``artifacts/perf/wgpu-feasibility/browser-webgpu-benchmark.json``; validator: ``scripts/validate-browser-webgpu-benchmark.mjs``.')
}
else {
    $lines.Add('- Browser benchmark artifact is not present on this runner; WebGPU browser evidence remains blocked.')
}
$lines.Add('')
$preflightPath = Join-Path $root 'artifacts\perf\wgpu-feasibility\runner-preflight.json'
$lines.Add('## Runner preflight')
$lines.Add('')
if (Test-Path -LiteralPath $preflightPath) {
    $preflight = Get-Content -Raw -Encoding UTF8 -LiteralPath $preflightPath | ConvertFrom-Json
    $available = @($preflight.matrix.available_runners) -join ', '
    $missing = @($preflight.matrix.missing_runners) -join ', '
    $lines.Add("- Artifact: ``artifacts/perf/wgpu-feasibility/runner-preflight.json``; status=$($preflight.status); platform=$($preflight.platform); matrix_complete=$($preflight.matrix.complete).")
    $lines.Add("- Available runners: ``$available``.")
    $lines.Add("- Missing runners: ``$missing``.")
    $containerCaps = @($preflight.host_capabilities.containers.PSObject.Properties | Sort-Object Name | ForEach-Object { "$($_.Name)=$($_.Value.ToString().ToLowerInvariant())" }) -join ', '
    $linuxCaps = @($preflight.host_capabilities.linux_compat.PSObject.Properties | Sort-Object Name | ForEach-Object { "$($_.Name)=$($_.Value.ToString().ToLowerInvariant())" }) -join ', '
    $mobileCaps = @($preflight.host_capabilities.mobile_sdk_tools.PSObject.Properties | Sort-Object Name | ForEach-Object { "$($_.Name)=$($_.Value.ToString().ToLowerInvariant())" }) -join ', '
    $lines.Add("- Host capabilities: containers [$containerCaps]; linux_compat [$linuxCaps]; mobile_sdk_tools [$mobileCaps].")
    $lines.Add('- Validator: ``scripts/validate-runner-preflight.mjs``; preflight is environment evidence only and does not convert blocked matrix cells or proxy scenarios into passes.')
}
else {
    $lines.Add('- Runner preflight artifact is not present on this runner; device-farm readiness remains unverified.')
}
$lines.Add('')
$bundlePath = Join-Path $root 'artifacts\perf\wgpu-feasibility\device-farm-bundle.json'
$lines.Add('## Device-farm bundle')
$lines.Add('')
if (Test-Path -LiteralPath $bundlePath) {
    $bundle = Get-Content -Raw -Encoding UTF8 -LiteralPath $bundlePath | ConvertFrom-Json
    $lines.Add("- Artifact: ``artifacts/perf/wgpu-feasibility/device-farm-bundle.json``; status=$($bundle.status); files=$(@($bundle.files).Count); matrix_cells=$(@($bundle.matrix.required_results).Count); preflight_status=$($bundle.preflight.status).")
    $lines.Add('- Validator: ``scripts/validate-wgpu-device-farm-bundle.mjs``; the bundle is a submission manifest and does not convert blocked cells into passes.')
    $lines.Add('- Required remote-runner inputs: fixed fixture, base report, preflight snapshot, matrix IDs, runner commands, and SHA-256 entries for every file.')
}
else {
    $lines.Add('- Device-farm bundle artifact is not present on this runner; remote submission remains unprepared.')
}
$lines.Add('')
$lines.Add('## Matrix status')
$lines.Add('')
$lines.Add('| Result status | Count | Meaning |')
$lines.Add('| --- | ---: | --- |')
$lines.Add("| passed | $(@($report.results | Where-Object status -eq 'passed').Count) | Real host execution with report and trace |")
$lines.Add("| blocked_environment | $(@($report.results | Where-Object status -eq 'blocked_environment').Count) | Target device/backend is not attached or cannot run in this host policy |")
$lines.Add('')
$lines.Add('Blocked cells are intentionally retained in the JSON matrix; they are not treated as capability passes.')
$lines.Add('')
$lines.Add('## Acceptance boundary')
$lines.Add('')
$lines.Add('- **Replay and 4K:** real offscreen wgpu evidence exists for DX12 and Vulkan on the current Windows GPU, and a separate Chrome CDP/WebGPU fixture-backed benchmark is recorded; Metal and other device classes still require native runners.')
$lines.Add('- **Ligature:** `fi`, `ffi`, and U+FB03 are present in the fixture, but the headless result is `proxy_passed`; native shaping/rasterization is not claimed.')
$lines.Add('- **IME:** `n -> ni -> CJK1 -> CJK2` is represented as a contract payload, but no native OS IME bridge is claimed by this headless prototype.')
$lines.Add('- **Scrollback:** the million-line index traversal proxy runs, but T003 RSS/private-bytes and 72-hour soak evidence require platform harnesses.')
$lines.Add('')
$lines.Add('## Blocking condition')
$lines.Add('')
$lines.Add('The current Windows-first phase is complete for its four accepted native cells: Windows DX12/Vulkan in debug/release, plus the separate Chrome WebGPU reference benchmark. The reserved macOS, Linux, iOS/iPadOS, Android, and future native Web/PWA runners remain explicit blocked_environment matrix cells until their native runners are attached. Native shaping, OS IME, RSS/private-bytes, and 72-hour soak evidence remain follow-up platform gates and are not claimed by this phase.')

[System.IO.File]::WriteAllLines($outputPath, $lines, [System.Text.UTF8Encoding]::new($false))
Write-Host "Rendered wgpu feasibility markdown: $outputPath"
