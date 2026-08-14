# Workspace tools

T016 reserves this directory for repository-local, deterministic tooling.

- `scripts/validate-workspace.mjs` checks the workspace and platform boundary contract.
- `scripts/test-workspace-contract.mjs` runs positive and negative contract checks.
- Rust verification is performed with `cargo fmt`, `cargo check`, and `cargo build` from the repository root.
- Platform-specific commands must be added only with a toolchain lock and a control-row test; unavailable tools are recorded as `blocked_environment`.

