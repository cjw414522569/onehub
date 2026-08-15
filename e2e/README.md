# e2e — cross-platform E2E harness (T153)

Page objects, test-account handling, and key rotation for the six-platform
E2E smoke matrix. Secrets never live in the repository: they are loaded from
the environment (`E2E_GATEWAY_TOKEN`, `E2E_TEST_KEY`) and rotated via
`src/key-rotation.ts`.

## Layout

- `src/page-objects.ts` - `ShellPageObject` + the six-platform matrix
  (windows / macos / linux / ios / android / web). Each page object runs
  the critical journey (launch -> select host -> connect -> type ->
  verify -> disconnect) against the deterministic fake gateway.
- `src/accounts.ts` - `SecretsProvider` reads env-only secrets and rejects
  placeholders.
- `src/key-rotation.ts` - `KeyRotationPolicy`: rotation interval, active
  key derivation (`base-gen<generation>`), and expiry.
- `accounts.schema.json` / `accounts.json.example` - the public account
  schema and a placeholder template (no real secrets).
- `src/smoke.ts` - the full smoke matrix runner.

## Run

```powershell
$env:E2E_GATEWAY_TOKEN = "short-lived-token-1"
$env:E2E_TEST_KEY = "short-lived-test-key-1"
node --experimental-strip-types e2e/src/smoke.ts
```

The contract (`scripts/test-e2e-smoke.mjs`) runs the type gate, the smoke
matrix, and a canary-secret scan that proves the env secret never enters
the repository tree.