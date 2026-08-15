# Network fault injection matrix (T154)

Version: 1.0.0. The matrix deterministically covers the six required fault
classes against the real gateway models (`GatewaySession` T135,
`SessionRegistry` T137, `AddressPolicy` T136).

| Fault kind | Coverage | Recovery |
| --- | --- | --- |
| Latency | per-message delay within budget; all data delivered | completes |
| Packet loss | 25% first-transmission loss; retransmission | all payloads delivered |
| Reordering | shuffled delivery; sequence watermark reaches max | all messages processed |
| Disconnect | mid-stream break | resume token -> Ready without re-auth |
| DNS failure | empty resolution | stable `NoAddresses` error |
| Network switch | disconnect on one network, resume on a fresh channel | Ready + data flows |

`services/gateway/examples/fault-matrix.rs` runs the matrix 50 times and
prints a stable report; `scripts/test-fault-matrix.mjs` runs it three times,
asserts byte-identical output, and regenerates `fault-matrix.report.json`
(byte-identical).