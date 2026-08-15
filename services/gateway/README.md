# gateway

- Layer: L6 (entrypoint)
- Dependencies: `session-orchestrator`, `ssh-backend`, `proxy`, `policy-engine`
- Scope: gateway service entrypoint over public interfaces.
- T016 status: buildable workspace skeleton; network service behavior is deferred to later control rows.



## T135: versioned gateway session protocol

`services/gateway/src/session_protocol.rs`:

- `GatewaySession` - handshake (version check) -> authentication -> capability
  negotiation -> ready, with per-message `MessageFlags` (cancel /
  backpressure) and a resume token so a session reconnects after a network
  failure without re-authenticating.
- `CapabilitySet::negotiate` - the intersection of client / server
  capabilities.
- Tests cover the protocol contract and network-fault recovery (resume).

## T136: target address policy and SSRF protection

`services/gateway/src/address_policy.rs`:

- `AddressPolicy` - configurable gate over every target: private /
  link-local / loopback / cloud-metadata / reserved address classes, a
  port policy (default SSH-only), and an optional host allowlist
  (exact or `*.suffix` wildcard).
- DNS rebinding guard - evaluation pins the validated addresses and
  `verify_still_valid` rejects a connect-time re-resolution that returns
  any address outside the pinned set.
- Tests cover the SSRF bypass set: private/link-local/loopback/metadata
  (IPv4, IPv6, IPv4-mapped), reserved and multicast ranges, port and
  allowlist policy, and DNS rebinding detection.