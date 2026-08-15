# Versioning, compatibility windows, and deprecation policy (T164)

Version: 1.0.0.

## Policy

- Every independently-shipped component (client, gateway, sync, FFI/WASM)
  uses SemVer and can upgrade independently.
- Versioned wire/ABI boundaries reject unknown newer versions cleanly (a
  strict v1 today; the window widens as protocols mature). The database and
  sync schema support an N-1/N-2 window and migrate older versions forward.
- Deprecated features are removed only after N+2 (two releases after the
  deprecation notice) and continue to honor the compatibility window until
  removal.

## N / N-1 / N-2 matrix

`versioning/versioning.json` records the per-component matrix: current
protocols (gateway `SESSION_PROTOCOL_VERSION=1`, ABI `ABI_SCHEMA_VERSION=1`,
WASM `WASM_BOUNDARY_VERSION=1`, sync `SYNC_PROTOCOL_VERSION=1`) are
`compatible` at N; N-1/N-2 are not-yet-applicable for v1 protocols; the
database schema is `compatible` at N-1 (2->3) and N-2 (1->3) via the
storage-sqlite migrator. Newer versions are rejected everywhere (no silent
downgrade).

## Contract

`scripts/test-versioning.mjs` validates the matrix against the real source
constants, runs the versioned-boundary tests (gateway, ABI, WASM, sync,
storage-sqlite migrations), and checks the deprecation policy.