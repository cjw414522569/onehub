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

## T147: default-off, explicit-consent privacy telemetry

`crates/telemetry/src/privacy.rs`:

- `TelemetryConsent` - DefaultOff (default) / ExplicitConsent; nothing
  leaves the machine without explicit consent.
- `TELEMETRY_SCHEMA` - the public collection dictionary (app_start,
  app_crash, session_duration_secs, feature_used, gateway_latency_ms) with
  allowlisted fields only; `NEVER_COLLECTED` lists the data classes that
  must never appear (terminal content, commands, identity, host data, ...).
- `TelemetryCollector::collect` - rejects unknown events, drops
  non-allowlisted fields, and hard-rejects any field touching forbidden
  data.
- `telemetry-schema.json` - the public schema (default off, events,
  never_collected), validated by the contract.
- `examples/privacy-canary.rs` - network-capture canary: off-mode capture
  is empty and 9/9 forbidden data attempts are rejected; the contract scans
  the outbound capture for zero leaks.

## T148: local performance sampling and user-exportable diagnostics

`crates/telemetry/src/diagnostics.rs`:

- `DiagnosticsSampler` - records numeric samples for the network / parse /
  render / memory paths (the API accepts only numbers, so no content can
  enter).
- `DiagnosticReport::export` - a versioned (schema_version 1) report of
  aggregates (count/mean/p50/p95/p99/min/max) with fixed metric labels;
  user-exportable.
- `examples/diagnostics.rs` - samples all four paths and prints the report;
  `scripts/test-diagnostics.mjs` runs the diagnostic data privacy scan (no
  host/command/terminal/payload content in the exported report).
