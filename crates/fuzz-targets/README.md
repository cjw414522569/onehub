# fuzz-targets

- Layer: L1 (test infrastructure; no runtime behavior)
- Dependencies: `core-domain`, `core-errors`, `core-protocol`, `session-orchestrator`, `serde`.
- Scope: deterministic, bounded fuzz-style smoke targets for the core models.

## T036

Each target in `fuzz/smoke-corpus.json` runs with a fixed seed and iteration budget over a deterministic PRNG (no external dependency). Targets:

| Target | Focus |
|---|---|
| `fuzz_known_hosts_parse` | known_hosts parser must never panic. |
| `fuzz_terminal_snapshot_deserialize` | arbitrary JSON must not panic; unknown extensions ignored. |
| `fuzz_proxy_chain_validation` | cycle detection total and deterministic. |
| `fuzz_session_state_machine` | every event sequence yields a defined outcome; Closed terminal. |
| `fuzz_forwarding_table` | conflict detection consistent. |
| `fuzz_settings_migration` | migration never panics and is idempotent. |
| `fuzz_command_resolution` | resolution never panics; sensitivity boolean consistent. |

Run via `scripts/run-fuzz-smoke.ps1` (time-limited). Crash inputs must be added back to the corpus for regression.