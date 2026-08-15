# Benchmarks (T156)

Version: 1.0.0. Deterministic benchmarks for the T003 metrics that can be
measured on this host, persisted per platform/device, with a statistics
gate that blocks significant regressions (>10% or absolute budget).

## Metrics

| Metric | Method | Budget |
| --- | --- | --- |
| Startup | `ssh-cli --version` wall time, 30 repeats | P95 <= 500ms |
| Parse throughput | 10 MB VT corpus through terminal-parser, 30 repeats | p50 >= 40 MB/s |
| Input-to-pixel | bridge push + snapshot latency, 30 repeats | P95 <= 45ms |
| Scrollback | 10k lines into scrollback, 30 repeats | P95 <= 100ms |
| Memory | desktop runtime measurement | blocked_unavailable_toolchain |
| Power | mobile device measurement | blocked_unavailable_toolchain |

Statistics: P50/P95/P99 + mean over 30 repeats per the T003 measurement
principle. Results are persisted to `benchmarks/results/<platform>/<device>/`
and compared against `benchmarks/baseline.json`; a regression beyond 10% or
an absolute budget breach fails the gate.