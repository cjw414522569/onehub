# forwarding

- Layer: L1 (core services)
- Dependencies: `core-domain`, `core-protocol`, `tokio`
- Scope: local / remote / dynamic port forwarding engines.

## T052: local port forwarding

| Model | Purpose |
|---|---|
| `BindScope` | Classifies the listen address (Loopback / Wildcard / Other) for the bind-range warning. |
| `LocalForwardConfig` | Listen address, target host/port, concurrent-connection cap, graceful-shutdown timeout. |
| `ForwardError` | Bind-failed / T031 conflict / target-unreachable / not-started (no secret context). |
| `TargetConnector` / `TcpConnector` | Injectable channel opener (SSH direct-tcpip plugs into the same trait); TCP for tests. |
| `LocalForwarder` | Binds the listener, accepts with a connection cap, pipes bidirectionally; `start` / `close_listener` / `drain` / `stop`. |

Tests: TCP echo round trip through the forwarder, 16 concurrent echoes under a
cap, excess connections refused with EOF, graceful shutdown (listener closes
immediately, in-flight drains), bind failure on an occupied port and on a
non-local address, bind-scope warning policy, and T031 table conflict
detection.

## T053: remote port forwarding

| Model | Purpose |
|---|---|
| `RemoteForwardConfig` | Server-side listen host/port (0 = dynamic) + local target. |
| `RemoteForwardReply` | `Accepted { allocated_port }` (dynamic) / `Rejected { detail }`. |
| `RemoteForwardEvent` | `ListenerClosed` (server close visible) / `IncomingConnection { connection_id }`. |
| `RemoteForwardError` | Io / Timeout / Protocol (no secret context). |
| `RemoteForwardPeer` / `WirePeer` | Injectable peer; `WirePeer` performs the real RFC 4254 7.1 `tcpip-forward` exchange over a stream. |
| `RemoteForwarder` | `establish` (applies reply to state), `pipe_incoming` (pipes to local target via `TargetConnector`), `mark_server_closed`. |

Tests: dynamic port allocation over the real wire (request port 0, server replies
allocated 30000), rejection visibility (`REQUEST_FAILURE`), server-close
visibility, incoming connection piped to a local TCP echo target, and the
RFC 4254 7.1 codec round trips. Real OpenSSH `-R` integration is
`blocked_environment` on this host (no `ssh` binary); the wire format is
exercised over `duplex`.

## Validation

```text
cargo test -p forwarding --locked
node .\scripts\test-local-forwarding.mjs .
node .\scripts\test-remote-forwarding.mjs .
```
