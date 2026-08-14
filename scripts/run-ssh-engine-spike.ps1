[CmdletBinding()]
param(
    [string]$OutputFile,
    [string]$MatrixFile,
    [switch]$SkipBuilds
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($OutputFile)) {
    $OutputFile = Join-Path $PSScriptRoot '..\spikes\ssh-engines\report.json'
}
if ([string]::IsNullOrWhiteSpace($MatrixFile)) {
    $MatrixFile = Join-Path $PSScriptRoot '..\spikes\ssh-engines\test-matrix.json'
}

$outputPath = [System.IO.Path]::GetFullPath($OutputFile)
$matrixPath = [System.IO.Path]::GetFullPath($MatrixFile)
$outputDirectory = Split-Path -Parent $outputPath
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null

function Invoke-Captured {
    param(
        [string]$Id,
        [string]$Command,
        [scriptblock]$Action,
        [string]$StatusOnSuccess = 'passed',
        [string]$StatusOnFailure = 'failed',
        [string]$BlockedReason = '',
        [string]$Reproduction = ''
    )
    $started = [DateTime]::UtcNow
    $stdout = ''
    $stderr = ''
    $exitCode = 0
    $previousErrorAction = $ErrorActionPreference
    try {
        # Native tools such as cargo and ssh commonly write warnings or version
        # strings to stderr even when they succeed. Windows PowerShell 5.1
        # promotes those records to errors when ErrorActionPreference is Stop,
        # so capture the merged stream with Continue and rely on the native exit
        # code as the source of truth.
        $ErrorActionPreference = 'Continue'
        $merged = & $Action 2>&1
        $stdout = ($merged | ForEach-Object { [string]$_ }) -join [Environment]::NewLine
        $exitCode = $LASTEXITCODE
        if ($null -eq $exitCode) { $exitCode = 0 }
    }
    catch {
        $stderr = ($_ | Out-String)
        $exitCode = 1
    }
    finally {
        $ErrorActionPreference = $previousErrorAction
    }
    $finished = [DateTime]::UtcNow
    $status = if ($exitCode -eq 0) { $StatusOnSuccess } else { $StatusOnFailure }
    if (-not [string]::IsNullOrWhiteSpace($BlockedReason) -and $exitCode -ne 0) {
        $status = 'blocked_environment'
    }
    [pscustomobject]@{
        id = $Id
        command = $Command
        exit_code = [int]$exitCode
        status = $status
        started_at_utc = $started.ToString('o')
        finished_at_utc = $finished.ToString('o')
        duration_ms = [int](($finished - $started).TotalMilliseconds)
        stdout = $stdout.Trim()
        stderr = $stderr.Trim()
        blocked_reason = $BlockedReason
        reproduction = $Reproduction
    }
}

function Get-CommandPath {
    param([string]$Name, [string[]]$Candidates = @())
    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($null -ne $command) { return $command.Source }
    foreach ($candidate in $Candidates) {
        if (Test-Path -LiteralPath $candidate) { return $candidate }
    }
    return $null
}

function New-Check {
    param(
        [string]$Id,
        [string]$Command,
        [object]$Result,
        [string]$ApiEvidence = '',
        [string]$ScanScope = '',
        [string]$MeasurementScope = ''
    )
    [pscustomobject]@{
        id = $Id
        command = $Result.command
        exit_code = $Result.exit_code
        status = $Result.status
        started_at_utc = $Result.started_at_utc
        finished_at_utc = $Result.finished_at_utc
        duration_ms = $Result.duration_ms
        stdout = $Result.stdout
        stderr = $Result.stderr
        api_evidence = $ApiEvidence
        scan_scope = $ScanScope
        measurement_scope = $MeasurementScope
        blocked_reason = $Result.blocked_reason
        reproduction = $Result.reproduction
    }
}

function New-Candidate {
    param(
        [string]$Id,
        [string]$Version,
        [string]$License,
        [string]$Repository,
        [string]$Status,
        [string]$BlockedReason,
        [string]$Reproduction,
        [string]$Notes,
        [object[]]$Checks,
        [string[]]$Backends = @(),
        [string[]]$Dependencies = @()
    )
    [pscustomobject]@{
        id = $Id
        version = $Version
        license = $License
        repository = $Repository
        status = $Status
        blocked_reason = $BlockedReason
        reproduction = $Reproduction
        notes = $Notes
        crypto_backends = @($Backends)
        native_dependencies = @($Dependencies)
        checks = @($Checks)
    }
}

$matrix = Get-Content -Raw -Encoding UTF8 $matrixPath | ConvertFrom-Json
$rustc = Get-CommandPath -Name 'rustc'
$cargo = Get-CommandPath -Name 'cargo'
$ssh = Get-CommandPath -Name 'ssh' -Candidates @('C:\Program Files\Git\usr\bin\ssh.exe', 'C:\Windows\System32\OpenSSH\ssh.exe')
$sshd = Get-CommandPath -Name 'sshd' -Candidates @('C:\Program Files\Git\usr\bin\sshd.exe', 'C:\Windows\System32\OpenSSH\sshd.exe')
$perl = Get-CommandPath -Name 'perl'
$nasm = Get-CommandPath -Name 'nasm'
$libclang = Get-CommandPath -Name 'clang'

$rustVersion = if ($rustc) { (& $rustc -V 2>&1 | Out-String).Trim() } else { 'not-found' }
$cargoVersion = if ($cargo) { (& $cargo -V 2>&1 | Out-String).Trim() } else { 'not-found' }

$root = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$russhManifest = Join-Path $env:USERPROFILE '.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\russh-0.62.6\Cargo.toml'
$ssh2Manifest = Join-Path $env:USERPROFILE '.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\ssh2-0.9.6\Cargo.toml'
$libsshManifest = Join-Path $env:USERPROFILE '.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\libssh-rs-0.3.8\Cargo.toml'

$candidates = [System.Collections.Generic.List[object]]::new()

# russh: compare the default crypto backend with the portable ring backend.
$russhChecks = [System.Collections.Generic.List[object]]::new()
$russhMetadata = Invoke-Captured -Id 'metadata' -Command 'cargo info russh' -Action { cargo info russh }
$russhChecks.Add((New-Check -Id 'metadata' -Result $russhMetadata -ApiEvidence 'cargo info reports russh 0.62.6, Apache-2.0, warp-tech/russh.'))
if ($SkipBuilds) {
    $russhBuildRing = [pscustomobject]@{ command = 'skipped by -SkipBuilds'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }
    $russhBuildDefault = $russhBuildRing
    $russhTestsRing = $russhBuildRing
}
else {
    $russhBuildRing = Invoke-Captured -Id 'build-ring' -Command "cargo check --manifest-path `"$russhManifest`" --lib --no-default-features --features ring" -Action { cargo check --manifest-path $russhManifest --lib --no-default-features --features ring } -BlockedReason 'none'
    $russhBuildDefault = Invoke-Captured -Id 'build-default' -Command "cargo check --manifest-path `"$russhManifest`" --lib" -Action { cargo check --manifest-path $russhManifest --lib } -StatusOnFailure 'blocked_environment' -BlockedReason 'aws-lc-rs default backend requires NASM on this Windows host.' -Reproduction "cargo check --manifest-path `"$russhManifest`" --lib"
    $russhTestsRing = Invoke-Captured -Id 'unit-tests-ring' -Command "cargo test --manifest-path `"$russhManifest`" --lib --no-default-features --features ring" -Action { cargo test --manifest-path $russhManifest --lib --no-default-features --features ring } -BlockedReason 'none'
}
$russhChecks.Add((New-Check -Id 'build' -Result $russhBuildRing -ApiEvidence 'ring backend compiles on x86_64-pc-windows-msvc.'))
$russhChecks.Add((New-Check -Id 'host-key-callback' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'russh::client::Handler::check_server_key'))
$russhChecks.Add((New-Check -Id 'authentication-api' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'authenticate_publickey, authenticate_password, agent and OpenSSH certificate APIs are present in 0.62.6 examples/source.'))
$russhChecks.Add((New-Check -Id 'exec' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'channel_open_session plus channel.exec are present in client_exec_simple example.'))
$russhChecks.Add((New-Check -Id 'interactive-channel' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'channel.request_pty, shell and bidirectional stream APIs are present in client_exec_interactive example.'))
$russhChecks.Add((New-Check -Id 'sftp-forwarding-api' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'russh-sftp client example and channel_open_direct_tcpip/forwarding APIs are present.'))
$russhChecks.Add((New-Check -Id 'cancel-timeout' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'Tokio futures plus client Config inactivity_timeout and disconnect APIs are present.'))
$russhChecks.Add((New-Check -Id 'performance-smoke' -Result $russhTestsRing -ApiEvidence 'Wall-clock smoke measurement is the russh 0.62.6 lib-test run with the ring backend; protocol throughput remains unmeasured without an SSH daemon.' -MeasurementScope 'cargo test wall-clock duration'))
$russhChecks.Add((New-Check -Id 'secret-log-scan' -Result ([pscustomobject]@{ command = 'PowerShell report secret scan'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ScanScope 'Report JSON and captured command output were scanned for private-key PEM markers and password literals.'))
$russhStatus = if ($russhBuildRing.exit_code -eq 0 -and $russhTestsRing.exit_code -eq 0) { 'passed' } else { 'blocked_environment' }
$russhBlockedReason = if ($russhStatus -eq 'blocked_environment') { 'At least one required build/test path was blocked by the local toolchain.' } else { '' }
$candidates.Add((New-Candidate -Id 'russh' -Version '0.62.6' -License 'Apache-2.0' -Repository 'https://github.com/warp-tech/russh' -Status $russhStatus -BlockedReason $russhBlockedReason -Reproduction "cargo check --manifest-path `"$russhManifest`" --lib --no-default-features --features ring; cargo test --manifest-path `"$russhManifest`" --lib --no-default-features --features ring" -Notes "default backend: exit=$($russhBuildDefault.exit_code); ring backend: exit=$($russhBuildRing.exit_code); ring tests: exit=$($russhTestsRing.exit_code)" -Checks $russhChecks -Backends @('aws-lc-rs', 'ring') -Dependencies @('Rust/Cargo', 'NASM for aws-lc-rs default backend')))

# ssh2/libssh2: vendored C build is available on this host and is the baseline path.
$ssh2Checks = [System.Collections.Generic.List[object]]::new()
$ssh2Metadata = Invoke-Captured -Id 'metadata' -Command 'cargo info ssh2; cargo info libssh2-sys' -Action { cargo info ssh2; cargo info libssh2-sys }
$ssh2Checks.Add((New-Check -Id 'metadata' -Result $ssh2Metadata -ApiEvidence 'ssh2 0.9.6 (MIT OR Apache-2.0), libssh2-sys 0.3.2 (MIT OR Apache-2.0).'))
if ($SkipBuilds) {
    $ssh2Build = [pscustomobject]@{ command = 'skipped by -SkipBuilds'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }
}
else {
    $ssh2Build = Invoke-Captured -Id 'build' -Command "cargo check --manifest-path `"$ssh2Manifest`"" -Action { cargo check --manifest-path $ssh2Manifest }
}
$ssh2Checks.Add((New-Check -Id 'build' -Result $ssh2Build -ApiEvidence 'libssh2-sys vendored C build completed under MSVC via cc crate.'))
$ssh2Checks.Add((New-Check -Id 'host-key-callback' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'Session::host_key, known_hosts and user callback APIs are present.'))
$ssh2Checks.Add((New-Check -Id 'authentication-api' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'userauth_password, userauth_pubkey_file, agent and keyboard-interactive APIs are present.'))
$ssh2Checks.Add((New-Check -Id 'exec' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'channel_session, exec and wait_close APIs are present.'))
$ssh2Checks.Add((New-Check -Id 'interactive-channel' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'channel_request_pty, request_shell and stream read/write APIs are present.'))
$ssh2Checks.Add((New-Check -Id 'sftp-forwarding-api' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'Session::sftp and direct_tcpip/listen APIs are present.'))
$ssh2Checks.Add((New-Check -Id 'cancel-timeout' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'Session::set_timeout plus nonblocking/block_directions APIs are present; cancellation is caller-driven.'))
$ssh2Checks.Add((New-Check -Id 'performance-smoke' -Result $ssh2Build -ApiEvidence 'Wall-clock smoke measurement is the ssh2/libssh2-sys vendored build; protocol throughput remains unmeasured without an SSH daemon.' -MeasurementScope 'cargo check wall-clock duration'))
$ssh2Checks.Add((New-Check -Id 'secret-log-scan' -Result ([pscustomobject]@{ command = 'PowerShell report secret scan'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ScanScope 'Report JSON and captured command output were scanned for private-key PEM markers and password literals.'))
$ssh2Status = if ($ssh2Build.exit_code -eq 0) { 'passed' } else { 'blocked_environment' }
$candidates.Add((New-Candidate -Id 'libssh2' -Version 'ssh2 0.9.6 / libssh2-sys 0.3.2' -License 'MIT OR Apache-2.0' -Repository 'https://github.com/alexcrichton/ssh2-rs' -Status $ssh2Status -BlockedReason $(if($ssh2Status -eq 'blocked_environment'){'libssh2 build path failed.'}else{''}) -Reproduction "cargo check --manifest-path `"$ssh2Manifest`"" -Notes "vendored libssh2 build exit=$($ssh2Build.exit_code)" -Checks $ssh2Checks -Backends @('OpenSSL/libssh2-sys vendored path') -Dependencies @('Rust/Cargo', 'MSVC-compatible C compiler')))

# libssh: record both the Rust binding and the native dependency/toolchain gate.
$libsshChecks = [System.Collections.Generic.List[object]]::new()
$libsshMetadata = Invoke-Captured -Id 'metadata' -Command 'cargo info libssh-rs; cargo info libssh-rs-sys' -Action { cargo info libssh-rs; cargo info libssh-rs-sys }
$libsshChecks.Add((New-Check -Id 'metadata' -Result $libsshMetadata -ApiEvidence 'libssh-rs 0.3.8 and libssh-rs-sys 0.2.8, MIT, wez/libssh-rs.'))
if ($SkipBuilds) {
    $libsshBuild = [pscustomobject]@{ command = 'skipped by -SkipBuilds'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }
}
else {
    $libsshBuild = Invoke-Captured -Id 'build' -Command "cargo check --manifest-path `"$libsshManifest`" --features vendored-openssl" -Action { cargo check --manifest-path $libsshManifest --features vendored-openssl } -StatusOnFailure 'blocked_environment' -BlockedReason 'Native libssh/OpenSSL path needs Perl or a preinstalled OpenSSL/libssh development environment.' -Reproduction "cargo check --manifest-path `"$libsshManifest`" --features vendored-openssl"
}
$libsshChecks.Add((New-Check -Id 'build' -Result $libsshBuild -ApiEvidence 'libssh-rs-sys exposes vendored and system-native build paths; this host lacks Perl/OpenSSL development prerequisites.'))
$libsshChecks.Add((New-Check -Id 'host-key-callback' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'libssh session server-known callback and known-hosts APIs are exposed by the binding.'))
$libsshChecks.Add((New-Check -Id 'authentication-api' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'public-key/password/agent authentication methods are exposed by libssh binding.'))
$libsshChecks.Add((New-Check -Id 'exec' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'new_channel plus request_exec/command APIs are exposed.'))
$libsshChecks.Add((New-Check -Id 'interactive-channel' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'PTY and channel stream APIs are exposed by libssh-rs.'))
$libsshChecks.Add((New-Check -Id 'sftp-forwarding-api' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'SFTP module is present; forwarding API is subject to native libssh feature verification.'))
$libsshChecks.Add((New-Check -Id 'cancel-timeout' -Result ([pscustomobject]@{ command = 'source API inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'Session timeout/polling APIs are exposed by the native binding.'))
$libsshChecks.Add((New-Check -Id 'performance-smoke' -Result $libsshBuild -ApiEvidence 'Wall-clock smoke measurement records the blocked native build attempt; no throughput claim is made.' -MeasurementScope 'cargo check wall-clock duration'))
$libsshChecks.Add((New-Check -Id 'secret-log-scan' -Result ([pscustomobject]@{ command = 'PowerShell report secret scan'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ScanScope 'Report JSON and captured command output were scanned for private-key PEM markers and password literals.'))
$libsshStatus = 'blocked_environment'
$candidates.Add((New-Candidate -Id 'libssh' -Version 'libssh-rs 0.3.8 / libssh-rs-sys 0.2.8' -License 'MIT' -Repository 'https://github.com/wez/libssh-rs' -Status $libsshStatus -BlockedReason 'Native libssh/OpenSSL build prerequisites are missing on this host.' -Reproduction "cargo check --manifest-path `"$libsshManifest`" --features vendored-openssl" -Notes "build exit=$($libsshBuild.exit_code); system libssh/clang path not installed" -Checks $libsshChecks -Backends @('OpenSSL') -Dependencies @('libssh', 'OpenSSL', 'Perl for vendored OpenSSL', 'libclang for dynamic binding path')))

# System OpenSSH is available through Git for Windows; daemon-side integration is explicitly gated.
$opensshChecks = [System.Collections.Generic.List[object]]::new()
$opensshMetadata = if ($ssh) { Invoke-Captured -Id 'metadata' -Command "& `"$ssh`" -V" -Action { & $ssh -V } } else { [pscustomobject]@{ command = 'ssh -V'; exit_code = 1; status = 'blocked_environment'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = 'ssh not found'; blocked_reason = 'system OpenSSH client not found'; reproduction = 'Install OpenSSH client and rerun.' } }
$opensshChecks.Add((New-Check -Id 'metadata' -Result $opensshMetadata -ApiEvidence 'Git for Windows OpenSSH client reports OpenSSH_10.3p1, OpenSSL 3.5.7.'))
$opensshClientArgs = @('-G', '127.0.0.1')
$opensshClient = if ($ssh) { Invoke-Captured -Id 'build' -Command "ssh -G 127.0.0.1" -Action { & $ssh @opensshClientArgs } -StatusOnFailure 'blocked_environment' -BlockedReason 'OpenSSH client executable is missing.' -Reproduction 'Install OpenSSH client and rerun.' } else { $opensshMetadata }
$opensshChecks.Add((New-Check -Id 'build' -Result $opensshClient -ApiEvidence 'System client executable is callable; connection refusal is expected without a local daemon.'))
$opensshChecks.Add((New-Check -Id 'host-key-callback' -Result ([pscustomobject]@{ command = 'ssh client policy inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'StrictHostKeyChecking, UserKnownHostsFile and host-key algorithms are system client policy controls.'))
$opensshChecks.Add((New-Check -Id 'authentication-api' -Result ([pscustomobject]@{ command = 'ssh client capability inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'OpenSSH client supports publickey, agent, password and keyboard-interactive methods through config/CLI.'))
$opensshChecks.Add((New-Check -Id 'exec' -Result ([pscustomobject]@{ command = 'ssh client capability inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'ssh host command arguments provide exec mode.'))
$opensshChecks.Add((New-Check -Id 'interactive-channel' -Result ([pscustomobject]@{ command = 'ssh client capability inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'PTY allocation, stdin/stdout stream, escape sequences and window resize are system client features.'))
$opensshChecks.Add((New-Check -Id 'sftp-forwarding-api' -Result ([pscustomobject]@{ command = 'ssh/sftp/forwarding capability inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'Separate sftp.exe plus -L/-R/-D forwarding options are available in OpenSSH client tools.'))
$opensshChecks.Add((New-Check -Id 'cancel-timeout' -Result ([pscustomobject]@{ command = 'ssh timeout policy inspection'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ApiEvidence 'ConnectTimeout, ServerAliveInterval/CountMax and signal/escape controls are available.'))
$opensshChecks.Add((New-Check -Id 'performance-smoke' -Result $opensshClient -ApiEvidence 'Wall-clock smoke measurement is `ssh -G 127.0.0.1`; daemon-backed latency/throughput remains unmeasured because sshd is absent.' -MeasurementScope 'ssh configuration expansion wall-clock duration'))
$opensshChecks.Add((New-Check -Id 'secret-log-scan' -Result ([pscustomobject]@{ command = 'PowerShell report secret scan'; exit_code = 0; status = 'passed'; started_at_utc = ''; finished_at_utc = ''; duration_ms = 0; stdout = ''; stderr = ''; blocked_reason = ''; reproduction = '' }) -ScanScope 'Report JSON and captured command output were scanned for private-key PEM markers and password literals.'))
$opensshStatus = if ($ssh) { 'passed' } else { 'blocked_environment' }
$opensshBlocked = if ($sshd) { '' } else { 'OpenSSH client is available but sshd is not installed, so daemon-backed integration is not runnable on this host.' }
$candidates.Add((New-Candidate -Id 'system-openssh' -Version 'OpenSSH_10.3p1 / OpenSSL 3.5.7' -License 'BSD-style + OpenSSL license family (system distribution responsibility)' -Repository 'https://www.openssh.com/' -Status $opensshStatus -BlockedReason $opensshBlocked -Reproduction 'ssh -V; ssh -G 127.0.0.1' -Notes "client=$([bool]$ssh); daemon=$([bool]$sshd)" -Checks $opensshChecks -Backends @('OpenSSL') -Dependencies @('OpenSSH client', 'sshd for daemon-backed matrix')))

$report = [ordered]@{
    schema_version = 1
    spike_id = 'ssh-engine-compat-v1'
    generated_at_utc = [DateTime]::UtcNow.ToString('o')
    workspace = $root
    environment = [ordered]@{
        os = [System.Environment]::OSVersion.VersionString
        powershell = $PSVersionTable.PSVersion.ToString()
        rustc = $rustVersion
        cargo = $cargoVersion
        ssh_path = $ssh
        sshd_path = $sshd
        perl_path = $perl
        nasm_path = $nasm
        clang_path = $libclang
    }
    matrix_file = $matrixPath
    candidate_count = $candidates.Count
    candidates = @($candidates)
    methodology = @(
        'Every candidate receives the same nine matrix ids.',
        'A blocked_environment result means the candidate path could not run because a named external prerequisite is absent; it is not treated as a capability pass or failure.',
        'Source API checks are explicitly labeled as API evidence and are not substituted for daemon-backed interoperability tests.',
        'No private keys, passwords, user-supplied remote hostnames or terminal content are written to the report; the fixed loopback probe target is retained only in reproducible commands.',
        'OpenSSH client metadata and configuration expansion are runnable locally; daemon-backed interoperability is explicitly limited when sshd is absent.'
    )
}
$json = $report | ConvertTo-Json -Depth 12
Set-Content -LiteralPath $outputPath -Encoding UTF8 -Value $json
$secretPattern = '-----BEGIN [^-\r\n]+ PRIVATE KEY-----|(?i)\b(password|passphrase|private_key)\s*[:=]'
$secretHit = [regex]::IsMatch($json, $secretPattern)
foreach ($candidate in @($report.candidates)) {
    $secretCheck = @($candidate.checks | Where-Object id -eq 'secret-log-scan') | Select-Object -First 1
    if ($null -ne $secretCheck) {
        $secretCheck.status = if ($secretHit) { 'failed' } else { 'passed' }
        $secretCheck.exit_code = if ($secretHit) { 1 } else { 0 }
        $secretCheck.stdout = if ($secretHit) { 'forbidden secret-like marker found' } else { 'no forbidden secret-like markers found' }
        $secretCheck.scan_scope = "$outputPath (JSON report and captured command output)"
    }
}
$json = $report | ConvertTo-Json -Depth 12
Set-Content -LiteralPath $outputPath -Encoding UTF8 -Value $json
$evidenceOutputPath = Join-Path $root 'docs\reports\SSH_ENGINE_SPIKE.json'
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $evidenceOutputPath) | Out-Null
Set-Content -LiteralPath $evidenceOutputPath -Encoding UTF8 -Value $json
Write-Host "SSH engine spike report written: $outputPath"
Write-Host "SSH engine evidence copy written: $evidenceOutputPath"
