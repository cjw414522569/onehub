# cli

- Layer: L6 (entrypoint)
- Dependencies: `session-orchestrator`, `terminal-parser`, `transfer`, `forwarding`
- Scope: command-line entrypoint over public core contracts.
- T016 status: buildable workspace skeleton; command behavior is deferred to later control rows.



## T101: app startup, database unlock, and failure recovery

`apps/cli/src/startup.rs`:

- `StartupFlow::run` - opens/migrates the database (via storage-sqlite's
  `open_strategy` + `Migrator`) and checks the OS secure store, producing a
  structured `StartupOutcome` with **actionable prompts** for every failure
  mode:
  - `DB_CORRUPTED` - the database file is corrupted; restore a backup.
  - `DB_MIGRATION_FAILED` - a migration step failed; data is preserved at the
    pre-failure version; restore the pre-migration backup.
  - `DB_UNKNOWN_VERSION` - no migration path to the app's target version;
    install the matching app version.
  - `DB_REJECTED` / `DB_READ_ONLY` - version-policy outcomes.
  - `SECURE_STORE_LOCKED` - the OS keychain is locked; unlock and restart.
- Each prompt carries a stable code, severity, title, message, and the
  concrete user action.
- Tests cover the full startup failure matrix (corruption, migration failure,
  unknown version, reject/read-only policies, secure-store lock, healthy
  path).

Dependencies added: `storage-sqlite`, `secure-store` (declared in
`architecture/dependency-rules.json`).