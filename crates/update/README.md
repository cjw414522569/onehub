# update

- Layer: L3 (infrastructure)
- Dependencies: `core-domain`, `core-protocol`
- Scope: automatic update and rollback for Windows / macOS / Linux.

## T166: signed metadata, anti-downgrade, staged rollout, rollback

`crates/update/src/updater.rs`:

- `UpdateManifest` - signed update metadata (version, channel, rollout %,
  minimum version, artifact hash, signature).
- `UpdateCoordinator::apply` - verifies the signature (tamper), rejects
  downgrades and below-minimum targets, honors the staged rollout, and on a
  failed/interrupted install rolls back to the last-known-good version.
- `StagedRollout` - deterministic client buckets gate partial rollouts.
- `examples/update-matrix.rs` - upgrade / interrupt / tamper / downgrade /
  staged / rollback scenarios, 100 stable runs;
  `scripts/test-update-matrix.mjs` verifies and archives the report.