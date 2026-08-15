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