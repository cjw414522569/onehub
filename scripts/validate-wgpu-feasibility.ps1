[CmdletBinding()]
param(
    [string]$ReportFile,
    [switch]$RequireT012InProgress
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($ReportFile)) {
    $ReportFile = Join-Path $root 'docs\reports\WGPU_FEASIBILITY.json'
}

function Add-Error {
    param([System.Collections.Generic.List[string]]$Errors, [string]$Message)
    $Errors.Add($Message)
}

function Has-Property {
    param([object]$Object, [string]$Name)
    return $null -ne $Object -and $null -ne $Object.PSObject.Properties[$Name]
}

function Require-Property {
    param([object]$Object, [string]$Context, [string]$Name, [System.Collections.Generic.List[string]]$Errors)
    if (-not (Has-Property $Object $Name)) {
        Add-Error $Errors "$Context is missing property: $Name"
        return $false
    }
    return $true
}

function Require-NonEmpty {
    param([object]$Object, [string]$Context, [string]$Name, [System.Collections.Generic.List[string]]$Errors)
    if (-not (Require-Property $Object $Context $Name $Errors)) { return }
    if ([string]::IsNullOrWhiteSpace([string]$Object.$Name)) {
        Add-Error $Errors "$Context.$Name must be non-empty."
    }
}

function Require-ArrayExact {
    param([object]$Object, [string]$Context, [string]$Name, [string[]]$Expected, [System.Collections.Generic.List[string]]$Errors)
    if (-not (Require-Property $Object $Context $Name $Errors)) { return }
    $actual = @($Object.$Name | ForEach-Object { [string]$_ })
    if ($actual.Count -ne $Expected.Count -or (@($actual | Where-Object { $Expected -notcontains $_ }).Count -gt 0) -or (@($Expected | Where-Object { $actual -notcontains $_ }).Count -gt 0)) {
        Add-Error $Errors "$Context.$Name must equal: $($Expected -join ', ')"
    }
}

function Resolve-ReportPath {
    param([string]$Path)
    if ([System.IO.Path]::IsPathRooted($Path)) {
        return [System.IO.Path]::GetFullPath($Path)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $root $Path))
}

$errors = [System.Collections.Generic.List[string]]::new()
$report = $null
$reportPath = $null
if (-not (Test-Path -LiteralPath $ReportFile)) {
    Add-Error $errors "Report file does not exist: $ReportFile"
}
else {
    try {
        $reportPath = (Resolve-Path -LiteralPath $ReportFile).Path
        $report = Get-Content -Raw -Encoding UTF8 -LiteralPath $reportPath | ConvertFrom-Json
    }
    catch {
        Add-Error $errors 'Report file is not valid UTF-8 JSON.'
    }
}

if ($null -ne $report) {
    if ([int]$report.schema_version -ne 1) { Add-Error $errors 'schema_version must be 1.' }
    if (@('complete_with_blocked_environment', 'passed') -notcontains [string]$report.status) {
        Add-Error $errors 'status must be complete_with_blocked_environment or passed.'
    }
    Require-Property $report 'report' 'fixture' $errors | Out-Null
    Require-Property $report 'report' 'results' $errors | Out-Null
    Require-ArrayExact $report 'report' 'backend_matrix' @('dx12','metal','vulkan','webgpu') $errors
    Require-ArrayExact $report 'report' 'device_matrix' @('windows','macos-intel','macos-apple-silicon','linux','ios-ipados','android','web-pwa') $errors
    Require-ArrayExact $report 'report' 'scenario_matrix' @('replay-10mb','4k-full-refresh','ligature-fi-ffi','ime-composition-proxy','scrollback-million-lines') $errors
    if ([string]$report.delivery_phase -ne 'windows-first') { Add-Error $errors 'delivery_phase must be windows-first.' }
    Require-ArrayExact $report 'report' 'current_release_platforms' @('windows') $errors
    Require-ArrayExact $report 'report' 'reserved_device_classes' @('macos-intel','macos-apple-silicon','linux','ios-ipados','android','web-pwa') $errors
    if ($null -eq $report.interface_reservation) {
        Add-Error $errors 'interface_reservation is required.'
    }
    else {
        $interfaceReservationExpected = [ordered]@{
            protocol_schema = 'protocol/schema/domain-v1.json'
            dependency_rules = 'architecture/dependency-rules.json'
            renderer_contract = 'docs/ADR-004-terminal-abstraction.md'
            abi_boundary = 'docs/ADR-002-module-boundaries-and-dependencies.md'
        }
        foreach ($entry in $interfaceReservationExpected.GetEnumerator()) {
            if ([string]$report.interface_reservation.($entry.Key) -ne $entry.Value) { Add-Error $errors "interface_reservation.$($entry.Key) must equal $($entry.Value)." }
        }
    }
    if ($null -eq $report.windows_first) {
        Add-Error $errors 'windows_first is required.'
    }
    else {
        if ([int]$report.windows_first.accepted_native_cells -ne 4) { Add-Error $errors 'windows_first.accepted_native_cells must be 4.' }
        Require-ArrayExact $report.windows_first 'windows_first' 'required_native_backends' @('dx12','vulkan') $errors
        if ([string]$report.windows_first.web_reference -ne 'browser_webgpu') { Add-Error $errors 'windows_first.web_reference must be browser_webgpu.' }
    }

    if ($null -ne $report.fixture) {
        if ([int]$report.fixture.bytes -ne 10485760) { Add-Error $errors 'fixture.bytes must be 10485760.' }
        Require-NonEmpty $report.fixture 'fixture' 'file' $errors
        Require-NonEmpty $report.fixture 'fixture' 'sha256' $errors
        $fixturePath = Resolve-ReportPath ([string]$report.fixture.file)
        if (-not (Test-Path -LiteralPath $fixturePath)) {
            Add-Error $errors "Fixture file does not exist: $fixturePath"
        }
        else {
            $fixtureInfo = Get-Item -LiteralPath $fixturePath
            if ([int64]$fixtureInfo.Length -ne 10485760) { Add-Error $errors 'Fixture file length is not 10485760 bytes.' }
            $actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $fixturePath).Hash.ToLowerInvariant()
            if ($actualHash -ne ([string]$report.fixture.sha256).ToLowerInvariant()) { Add-Error $errors 'Fixture SHA-256 does not match report.' }
        }
    }

    $results = @($report.results)
    if ($results.Count -ne 56) { Add-Error $errors "results must contain 56 device/backend/profile cells; found $($results.Count)." }
    $duplicateIds = $results | Group-Object id | Where-Object Count -gt 1
    if ($duplicateIds) { Add-Error $errors 'results contains duplicate ids.' }
    foreach ($result in $results) {
        foreach ($field in @('id','device_class','backend','profile','command','status','report_file','trace_dir')) {
            Require-NonEmpty $result 'result' $field $errors
        }
        if (@('debug','release') -notcontains [string]$result.profile) { Add-Error $errors "Invalid profile in $($result.id)." }
        if (@('dx12','metal','vulkan','webgpu') -notcontains [string]$result.backend) { Add-Error $errors "Invalid backend in $($result.id)." }
        if (@('windows','macos-intel','macos-apple-silicon','linux','ios-ipados','android','web-pwa') -notcontains [string]$result.device_class) { Add-Error $errors "Invalid device class in $($result.id)." }
        if (@('passed','blocked_environment') -notcontains [string]$result.status) { Add-Error $errors "Unexpected result status in $($result.id): $($result.status)." }
        if ([int]$result.exit_code -notin @(0,2)) { Add-Error $errors "Unexpected exit code in $($result.id): $($result.exit_code)." }
        $resultPath = Resolve-ReportPath ([string]$result.report_file)
        if (-not (Test-Path -LiteralPath $resultPath)) { Add-Error $errors "Missing result report for $($result.id)." }
        if ($result.status -eq 'blocked_environment') {
            if ([string]::IsNullOrWhiteSpace([string]$result.blocked_reason)) { Add-Error $errors "Blocked result lacks blocked_reason: $($result.id)." }
            continue
        }
        if ([int]$result.exit_code -ne 0) { Add-Error $errors "Passed result must have exit_code 0: $($result.id)." }
        $traceFiles = @(Get-ChildItem -LiteralPath ([string]$result.trace_dir) -Recurse -File -ErrorAction SilentlyContinue)
        if ($traceFiles.Count -eq 0) { Add-Error $errors "Passed result lacks GPU trace files: $($result.id)." }
        if (-not (Test-Path -LiteralPath $resultPath)) { continue }
        try { $native = Get-Content -Raw -Encoding UTF8 -LiteralPath $resultPath | ConvertFrom-Json } catch { Add-Error $errors "Native result is invalid JSON: $($result.id)."; continue }
        if ([string]$native.status -ne 'passed') { Add-Error $errors "Native result status is not passed: $($result.id)." }
        if ([int]$native.replay_bytes -ne 10485760) { Add-Error $errors "Native replay size mismatch: $($result.id)." }
        $scenarios = @($native.scenarios)
        if ($scenarios.Count -ne 5) { Add-Error $errors "Native result must contain five scenarios: $($result.id)."; continue }
        foreach ($scenario in $scenarios) {
            if (@('replay-10mb','4k-full-refresh','ligature-fi-ffi','ime-composition-proxy','scrollback-million-lines') -notcontains [string]$scenario.id) { Add-Error $errors "Unknown scenario in $($result.id): $($scenario.id)." }
            if ($scenario.id -eq 'replay-10mb' -and ([double]$scenario.throughput_mb_s -lt 150 -or -not $scenario.passed)) { Add-Error $errors "10MB replay threshold failed in $($result.id)." }
            if ($scenario.id -eq '4k-full-refresh' -and ([double]$scenario.fps -lt [double]$scenario.target -or -not $scenario.passed)) { Add-Error $errors "4K FPS threshold failed in $($result.id)." }
            if ($scenario.id -in @('ligature-fi-ffi','ime-composition-proxy','scrollback-million-lines') -and [string]$scenario.status -ne 'proxy_passed') { Add-Error $errors "Proxy scenario must be marked proxy_passed in $($result.id): $($scenario.id)." }
        }
    }

    $hasDeviceFarm = $false
    if (Has-Property $report 'device_farm') {
        $hasDeviceFarm = $true
        $farm = $report.device_farm
        if ($null -eq $farm -or [int]$farm.schema_version -ne 1) { Add-Error $errors 'device_farm.schema_version must be 1.' }
        if ($null -eq $farm -or -not (Has-Property $farm 'source')) { Add-Error $errors 'device_farm.source is required.' }
        if ($null -eq $farm -or -not (Has-Property $farm 'preflight')) {
            Add-Error $errors 'device_farm.preflight is required.'
        }
        else {
            $preflight = $farm.preflight
            if ($null -eq $preflight -or [int]$preflight.schema_version -ne 1) { Add-Error $errors 'device_farm.preflight.schema_version must be 1.' }
            if ($null -eq $preflight -or @('blocked_environment','partial','ready') -notcontains [string]$preflight.status) { Add-Error $errors 'device_farm.preflight.status is invalid.' }
            if ($null -eq $preflight -or -not (Has-Property $preflight 'runners')) {
                Add-Error $errors 'device_farm.preflight.runners is required.'
            }
            else {
                foreach ($runnerName in @('native_windows','native_linux','native_metal','ios_native','android_native','browser_webgpu')) {
                    if (-not (Has-Property $preflight.runners $runnerName)) {
                        Add-Error $errors "device_farm.preflight.runners.$runnerName is required."
                    }
                    elseif ($null -eq $preflight.runners.$runnerName -or $preflight.runners.$runnerName.available -isnot [bool]) {
                        Add-Error $errors "device_farm.preflight.runners.$runnerName.available must be boolean."
                    }
                }
            }
        }
        if ($null -eq $farm -or -not (Has-Property $farm 'results_replaced')) {
            Add-Error $errors 'device_farm.results_replaced is required.'
        }
        else {
            $replaced = @($farm.results_replaced | ForEach-Object { [string]$_ })
            $duplicateReplaced = $replaced | Group-Object | Where-Object Count -gt 1
            if ($duplicateReplaced) { Add-Error $errors 'device_farm.results_replaced contains duplicates.' }
            foreach ($id in $replaced) {
                if (@($results | Where-Object id -eq $id).Count -ne 1) { Add-Error $errors "device_farm references unknown or duplicate result: $id" }
                elseif (@($results | Where-Object { $_.id -eq $id -and $_.status -eq 'passed' }).Count -ne 1) { Add-Error $errors "device_farm replacement is not passed: $id" }
                elseif ($null -ne $farm.preflight -and (Has-Property $farm.preflight 'runners')) {
                    $farmResult = @($results | Where-Object id -eq $id)[0]
                    $runnerName = switch ([string]$farmResult.device_class) {
                        'windows' { 'native_windows'; break }
                        'linux' { 'native_linux'; break }
                        'macos-intel' { 'native_metal'; break }
                        'macos-apple-silicon' { 'native_metal'; break }
                        'ios-ipados' { 'ios_native'; break }
                        'android' { 'android_native'; break }
                        'web-pwa' { 'browser_webgpu'; break }
                        default { $null }
                    }
                    if ([string]::IsNullOrWhiteSpace($runnerName)) { Add-Error $errors "device_farm has no runner mapping for: $id" }
                    elseif (-not (Has-Property $farm.preflight.runners $runnerName) -or $farm.preflight.runners.$runnerName.available -ne $true) { Add-Error $errors "device_farm preflight runner is unavailable for passed result: $id ($runnerName)" }
                }
            }
        }
    }
    $passedResults = @($results | Where-Object status -eq 'passed')
    if ($hasDeviceFarm) {
        if ($passedResults.Count -lt 4) { Add-Error $errors "Device-farm report must retain at least four real host results; found $($passedResults.Count)." }
    }
    elseif ($passedResults.Count -ne 4) {
        Add-Error $errors "Exactly four real host results are expected; found $($passedResults.Count)."
    }
    foreach ($expected in @('windows-dx12-debug','windows-dx12-release','windows-vulkan-debug','windows-vulkan-release')) {
        if (@($passedResults | Where-Object id -eq $expected).Count -ne 1) { Add-Error $errors "Missing expected real result: $expected" }
    }
    if ([string]$report.status -eq 'passed' -and @($results | Where-Object status -eq 'blocked_environment').Count -gt 0) { Add-Error $errors 'Report cannot be passed while blocked_environment results remain.' }
}

if ($RequireT012InProgress) {
    $controlPath = Join-Path $root 'docs\PROJECT_CONTROL.md'
    $controlText = Get-Content -Raw -Encoding UTF8 -LiteralPath $controlPath
    $inProgress = -join ([int[]](0x72B6, 0x6001, 0xFF1A, 0x8FDB, 0x884C, 0x4E2D) | ForEach-Object { [char]$_ })
    if ($controlText -notmatch '(?m)^- \[ \] T012 ' -or -not $controlText.Contains($inProgress)) {
        Add-Error $errors 'T012 must be in progress for this validation mode.'
    }
}

if ($null -ne $reportPath) {
    $rawReport = Get-Content -Raw -Encoding UTF8 -LiteralPath $reportPath
    foreach ($secretToken in @('PRIVATE KEY', 'BEGIN OPENSSH', 'password=', 'passphrase=', 'secret=')) {
        if ($rawReport -match [regex]::Escape($secretToken)) { Add-Error $errors "Report contains forbidden secret-like token: $secretToken" }
    }
}

if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Error $_ }
    exit 1
}

$blockedCount = @($report.results | Where-Object status -eq 'blocked_environment').Count
$passedCount = @($report.results | Where-Object status -eq 'passed').Count
Write-Host "wgpu feasibility report valid: results=$(@($report.results).Count), passed=$passedCount, blocked_environment=$blockedCount, fixture_bytes=$($report.fixture.bytes)."
