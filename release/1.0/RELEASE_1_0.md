# 1.0 release readiness review (T173)

Every release area has an owner sign-off backed by the evidence from the
control ledger: product (Alpha/Beta/RC gates), security (pipeline +
independent review + signing), performance (benchmarks within budget),
licensing (SPDX compliance, 0 blockers), support (public docs + decision
tree + security channel), rollback (update + blue-green rollback), and
documentation (compat/troubleshooting/security + versioning + store
materials).

`scripts/test-release-1.0.mjs` validates the checklist (all seven areas
signed off) and archives `release/1.0/release-1.0.checklist.json`.