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

## T137: authentication, short-lived tokens, session isolation

`services/gateway/src/auth.rs`:

- `TokenIssuer` - issues short-lived (default 300s), single-use,
  tenant-bound tokens; secrets are derived in memory only, never persisted.
- `SessionRegistry` - owns every session; `authenticate` rejects expired,
  replayed, unknown, and cross-tenant tokens; `access` re-checks the tenant
  boundary on every session-bound operation (no cross-tenant reach).
- `CredentialPolicy` - the gateway rejects persisting long-lived SSH keys
  (keys stay client-side); only short-lived in-memory session keys within
  the lifetime budget are accepted.
- Tests cover the authorization-bypass, replay, and token-expiry sets.

## T152: deterministic fake server + cross-language harness

`services/gateway/examples/fake-server.rs` - a deterministic fake gateway
that speaks the versioned session protocol (T135) with the auth registry
(T137) and address policy (T136). It runs the full scenario suite
(handshake, auth, capabilities, data, cancel, backpressure, resume, error
paths, SSRF denial, close, isolation) 100 consecutive times and prints a
stable summary; `scripts/test-cross-language.mjs` runs it repeatedly and
asserts byte-identical output (no flakiness).
