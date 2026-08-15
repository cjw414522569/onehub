# telemetry

- Layer: L3 (infrastructure)
- Dependencies: `core-domain`, `core-protocol`
- Scope: allowlisted, secret-free telemetry and diagnostics adapter.
- T016 status: buildable workspace skeleton; event policy is deferred to its control row.

## T146: structured logging, trace ids, dynamic level control

`crates/telemetry/src/log.rs`:

- `Logger` - emits deterministic structured entries (`level trace target
  message k=v ...`). `set_level` controls the dynamic threshold at runtime.
- `TraceId` / `LogContext` - 16-hex correlation ids; `child()` derives
  nested-operation ids so entries correlate across layers.
- `SensitiveFieldPolicy` - default denylist (token, password, secret, key,
  credential, host, username, user, terminal, command, history, transfer);
  any matching field is dropped, so default logs contain no sensitive
  fields. Values are escaped (one entry = one line).
- `examples/canary.rs` - attempts to log a canary secret under sensitive
  field names; `scripts/test-telemetry-logging.mjs` scans all emitted bytes
  and fails if the canary leaks.