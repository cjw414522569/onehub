# Release Candidate feature freeze and defect-zero rules (T172)

## Feature freeze

After the freeze commit, no new features are merged — only defect fixes,
security patches, and release tooling changes.

## Defect zero

A Release Candidate ships only with zero blocker and zero critical defects.
The severity scale is blocker / critical / high / medium / low; blocker and
critical are blocking. The security pipeline (T160) and supply-chain gate
enforce this.

## Same commit

Every RC artifact must be built from the exact same source commit; the
artifact provenance (T169) records it, and the RC gate verifies the full
matrix + artifact hashes against that commit.

`scripts/test-rc-gate.mjs` runs the RC full matrix and archives
`release/rc/rc-gate.report.json`.