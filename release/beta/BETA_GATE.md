# Beta entry gate (T171)

The Beta gate confirms the product meets Beta standard across six-platform
critical paths, migration, crash rate, and performance — and the resulting
test report is signed with the release signing pipeline (T165).

## Gate areas

| Area | Evidence |
| --- | --- |
| Six-platform critical paths | e2e smoke matrix (T153) |
| Migration | startup/migration (T101) + DB migration drill (T168) |
| Crash rate / leaks | crash sanitization (T149) + 10k resource soak (T155) + 72h soak (T163) |
| Performance | benchmarks (T156) + input-to-pixel (T157) + high-speed stress (T158) |
| Signed report | Beta test report signed via the T165 signing chain |

`scripts/test-beta-gate.mjs` runs the suite and archives
`release/beta/beta-gate.report.json` + the signed `beta-test-report.signed.json`.