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