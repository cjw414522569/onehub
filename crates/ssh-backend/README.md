# ssh-backend

- Layer: L2 (protocol adapters)
- Dependencies: `core-domain`, `core-protocol`, `secret`, `session-orchestrator`, `tokio`, `base64`, `hmac`, `sha1`, `sha2`, `ed25519-dalek`, `pkcs8`, `pem-rfc7468`, `ssh-key`; no concrete SSH library yet.
- Scope: SSH backend adapter boundary with an injectable transport.

## T037: adapter skeleton and injectable transport

`SessionTarget`, `TransportError`, `SessionHandle`, `SshTransport`, `FakeTransport`.

## T038: DNS, Happy Eyeballs v2, IPv4/IPv6, connection timeout

`Resolver`, `Connector`, `ConnectionGuard`, `happy_eyeballs_connect`.

## T039: version exchange, algorithm negotiation, secure defaults

`SshVersion`, `Algorithm`, `AlgorithmPolicy`, `negotiate_algorithm`, `HostAlgorithmPolicy`.

## T040: known_hosts reading, hashed hosts, wildcards, port rules

`hostname_matches_pattern`, `hashed_host_matches`, `host_field_matches`, `KnownHostsStore`.

## T041: host key verification (first connect, change blocking, CA certs)

`HostKeyFingerprint`, `HostCertificate`, `CaStore`, `HostKeyVerification`, `HostKeyVerifier`.

## T042: password and keyboard-interactive authentication

`AuthMethod`, `AuthPrompt`, `AuthChallenge`, `AuthResponse`, `KeyboardInteractiveHandler`, `run_keyboard_interactive`, `PasswordAuthenticator`.

## T043: Ed25519/ECDSA/RSA private keys and encrypted keys

`PrivateKeyFormat`, `KeyAlgorithm`, `KeyError`, `PrivateKeyHandle`, `load_private_key`.

## T044: OpenSSH user certificate authentication

`UserCertVerification`, `ed25519_ca_fingerprint`, `verify_user_certificate`.

## T045: ssh-agent / Pageant / macOS agent adapters

`frame_message`, `parse_frame`, `AgentIdentity`, `AgentStream`, `AgentClient`, `FakeAgentServer`, `AgentError`.

## T046: FIDO2/PKCS#11 hardware key capability gate

`HardwareKeyKind`, `HardwareKeyGate`, `hardware_key_gate`, `HardwareKeyBackend`, `effective_gate`.

## T047: keepalive, network-loss detection, exponential-backoff reconnect

| Model | Purpose |
|---|---|
| `KeepaliveConfig` | Probe interval + response timeout. |
| `LivenessProbe` / `probe_with_timeout` | Injectable keepalive probe with a deadline. |
| `ReconnectBackoff` | Exponential backoff (base, factor, cap) with `would_storm` budget check. |
| `run_reconnect_loop` | Keepalive + reconnect loop; attempt resets on recovery; observes cancellation. |
| `MonitorState` / `MonitorStateHandle` | Visible state: alive, reconnect attempt, next backoff ms. |

Fault-injection tests use scripted probes (flaky / always-dead / hanging): a flaky probe recovers after backoff, an always-dead connection grows attempts 3..20 within one virtual second (exponential cap, no storm), a hanging probe times out, and cancellation stops the loop.


## T048: PTY, shell, exec, environment, window-size change, exit status

| Model | Purpose |
|---|---|
| `PtyConfig` | PTY request config (RFC 4254 §6.2): term, columns, rows, pixel size. |
| `ExecCommand` | Single command with an environment (`EnvVar` list). |
| `ExitStatus` | Exit code and/or terminating signal; `is_success` = code 0 and no signal. |
| `ChannelEvent` | Output data and/or exit-status events replayed by a channel. |
| `ChannelError` | Protocol / closed / I/O channel errors (no secret context). |
| `SessionChannel` | Injectable channel contract: request_pty, start_shell, exec, set_window_size, read_event, close. |
| `ScriptedChannel` | Records calls and replays scripted events for the shell/exec/resize matrix. |
| `run_interactive_shell` | Drives PTY → shell → optional resize → collect output → exit status → close. |
| `run_exec_command` | Drives exec → collect output → exit status → close. |

Both drivers propagate the final exit status (code or signal) and reject empty
TERM / empty command at construction. Tests cover the multi-shell TERM matrix,
resize propagation, nonzero/signal exit status, and input validation.

## T049: multi-channel fair scheduling and flow-window management

| Model | Purpose |
|---|---|
| `TrafficClass` | Control / Interactive / Bulk classification with priority rank. |
| `FlowWindow` | Per-channel SSH send window (RFC 4254 5.2): consume on send, replenish on peer `WINDOW_ADJUST`, capped at `max_window`. |
| `QosError` | Unknown/duplicate channel, window-exceeded, invalid config (no secret context). |
| `SchedulerConfig` | Bulk DRR quantum, interactive quantum, round budget, initial/max window, max packet. |
| `Scheduler` | Strict priority (Control, then Interactive, then Bulk DRR), deterministic id order, budget-bounded `drain`, per-channel snapshot. |
| `ScheduledSend` / `ChannelSnapshot` / `SchedulerSnapshot` | Authorized sends and visible per-channel state for QoS benchmarks. |

Guarantees verified by tests: interactive bytes are always scheduled in the same
`drain` they are enqueued (zero waiting rounds behind bulk data); bulk channels
share via deficit round robin with no starvation; a full flow window blocks a
channel until `window_adjust` arrives; window adjusts are capped; round budget
bounds each drain. The acceptance benchmark runs one interactive terminal + two
SFTP transfers + two port-forward streams and asserts interactive latency 0,
per-round bulk progress, and full 8 MiB/channel completion under flow control.

## T050: ProxyJump multi-hop connection

| Model | Purpose |
|---|---|
| `HopEndpoint` | Resolved hop host + port. |
| `HopErrorKind` / `HopError` | Stable per-hop failure codes; `hop` index localizes the failing hop (final index = target). |
| `HopSession` | Established hop: verified fingerprint, auth method, direct-tcpip tunnel opener. |
| `MultiHopBackend` | Injectable backend: `connect_first` / `connect_next` (through a tunnel). |
| `HopRecord` / `MultiHopReport` | Per-hop host/port/fingerprint/auth for diagnostics. |
| `connect_chain` | Walks the chain (T030 `ProxyChain`), verifies and authenticates each hop independently, honours per-hop timeout, returns the report. |

1/2/3-hop topologies are exercised in-process with a fake backend that reuses
the T028 host-key policy decision and T042 constant-time password
authentication. Each hop verifies its own host key and uses its own credential;
failures (connect, resolve, host-key, auth, tunnel, timeout, cancellation,
cyclic chain) are localized to the failing hop. Real container topologies
(Docker + sshd) are recorded as `blocked_environment` on this host.
## Validation

```text
cargo test -p ssh-backend --locked
cargo check -p ssh-backend --locked
node scripts/test-ssh-adapter.mjs .
node scripts/test-connectivity.mjs .
node scripts/test-ssh-algorithms.mjs .
node scripts/test-known-hosts.mjs .
node scripts/test-host-key-verify.mjs .
node scripts/test-ssh-auth.mjs .
node scripts/test-private-key.mjs .
node scripts/test-user-certificate.mjs .
node scripts/test-agent.mjs .
node scripts/test-hardware-key.mjs .
node scripts/test-keepalive.mjs .
node scripts/test-session-channel.mjs .
node scripts/test-channel-qos.mjs .
node scripts/test-proxy-jump.mjs .
```

## T055: agent forwarding (off by default)

| Model | Purpose |
|---|---|
| `AgentForwardController` | Off by default; explicit per-session authorization; revoke. |
| `AgentForwardRisk` | UI risk notice (`AGENT_FORWARD_RISK`) requiring confirmation. |
| `AgentForwardState` | Disabled / Authorized / Rejected / Closed. |
| `AgentForwardOutcome` | Enabled / Rejected { reason } / Disconnected. |
| `AgentForwardTransport` | Opens the `auth-agent@openssh.com` channel (RFC 4254), forwards agent frames (T045 codec), observes close. |

Tests: off by default, per-session authorization isolation, risk notice for
the UI, channel-open codec round trips, wire confirmation (Enabled), wire
rejection (Rejected), channel close (Disconnected), and an agent-frame
round trip (REQUEST_IDENTITIES -> IDENTITIES_ANSWER) through the forwarded
channel over a duplex stream.

## T060: OpenSSH server version / OS / algorithm compatibility matrix

| Model | Purpose |
|---|---|
| `ServerCombo` | Documented OpenSSH server combination: platform x OS x version x default kex/cipher/MAC/host-key offers. |
| `check_combo` | Local, always-runnable check: T039 `negotiate_algorithm` against the server offers in every category. |
| `ChosenAlgorithms` | Mutually acceptable kex/host-key/cipher/MAC by local preference. |
| `run_compat_matrix` | Runs every combo; records live-server execution as `blocked_environment` on hosts without ssh/sshd/docker. |
| `CompatMatrixReport` | Per-combo verdict + execution mode + platform coverage. |

The matrix covers Linux (Ubuntu 24.04/9.6, Debian 12/9.2, RHEL 9/8.7), macOS
14/9.4, Windows 10/11 in-box OpenSSH, FreeBSD 13.2/9.0 and OpenBSD 7.4/9.5,
plus a legacy RHEL 6/5.3 SHA-1-only combo that is rejected under secure
defaults and connects only with an explicit legacy policy. The full nightly
live-server matrix (provisioned sshd across platforms) is `blocked_environment`
on this host and is meant for CI.

## T061: continuous fuzzing of SSH decoders, auth messages, channel data

`src/fuzz.rs` (test-only) adds deterministic, bounded fuzz targets over the
SSH wire surface: agent frame parsing (`parse_frame`, size-bounded),
auth-agent channel-open decoding, known_hosts host/hashed/wildcard matching,
QoS scheduler op sequences (no panic / no livelock / bounded queues), session
channel validators, and random `|1|salt|hash|` fields. Each target uses a
fixed seed from `fuzz/smoke-corpus.json`; any panic, out-of-bounds access,
livelock or unbounded memory growth fails the test and the failing input must
be persisted back to the corpus. `run-fuzz-smoke.ps1` runs the core + SSH
targets, accumulates CPU time via `-IterationsMultiplier` for the nightly
budget, and archives the corpus under `artifacts/fuzz/corpus-archive/`.
