# Test coverage baseline and differential gate (T151)

Version: 1.0.0. This document defines the per-language unit-test coverage
budget, the methodology, and the differential gate: new code must not lower
the committed baseline.

## Methodology

- Rust: `cargo llvm-cov --workspace --locked --summary-only` (line %).
- TypeScript: module-exercise coverage — the fraction of `web/app/src/*.ts`
  modules imported (transitively) by the test files.
- C# / Swift / Kotlin: `blocked_unavailable_toolchain` on this host (no
  .NET / Swift / Kotlin toolchains); their baselines are documented as
  interface-only and the gate runs on the native toolchains (per
  `bindings/CODEGEN.md`).

## Budgets

| Language | Overall threshold | Core security state machines |
| --- | --- | --- |
| Rust | >= 80% lines | >= 90% (gateway auth/session/address, telemetry crash/privacy/log, cli exit-code) |
| TypeScript | >= 90% modules exercised | n/a |
| C# | blocked_unavailable_toolchain | interface-only |
| Swift | blocked_unavailable_toolchain | interface-only |
| Kotlin | blocked_unavailable_toolchain | interface-only |

## Differential gate

`scripts/test-coverage.mjs` measures the current Rust line coverage and
compares it with `coverage/coverage-baseline.json`. A regression below
`baseline - 0.5` percentage points fails the gate; new code must never lower
the baseline. The current report is regenerated into
`coverage/coverage-report.json`.

## Baseline (committed)

- Rust workspace line coverage: 92.59% (2026-08-15).
- TypeScript module exercise: 100% (10/10 modules).
- C#/Swift/Kotlin: blocked_unavailable_toolchain.