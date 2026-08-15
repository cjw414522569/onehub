# Alpha entry gate (T170)

The Alpha gate confirms the core capabilities are safe and usable for
internal use: core connect, terminal, keys, and log security. Every area
must pass its contracts before Alpha is offered.

## Gate areas

| Area | Evidence (contracts) | Status |
| --- | --- | --- |
| Core connect | gateway session protocol (T135), auth/tokens/isolation (T137), address policy SSRF (T136), CLI config/connect/exec (T143), forwarding/SFTP/proxy shared core (T144), soak (T142) | pass |
| Terminal | parser/screen/render (T062-T077), WASM bridge (T138), input protocol | pass |
| Keys | secure-store erasure (T096), no long-term keys on disk (T137), platform keychain adapters (T126-T132) | pass |
| Log security | structured logs (T146), privacy telemetry (T147), diagnostics (T148), crash sanitization (T149) | pass |

`scripts/test-alpha-gate.mjs` runs the suite and archives
`release/alpha/alpha-gate.report.json`.