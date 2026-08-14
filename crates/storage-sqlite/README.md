# storage-sqlite

- Layer: L3 (infrastructure)
- Dependencies: `core-domain`, `core-protocol`
- Scope: SQLite persistence adapter behind repository traits.

## T083: schema, migration, backup, downgrade policy

`crates/storage-sqlite/src/migration.rs`:

- `Migrator` - applies `Migration` steps in order, each transactionally; a
  failed step rolls back its own record; already-applied versions are skipped
  (idempotent). `rollback` runs revert steps in reverse with `NoRevert`
  detection.
- `BackupPolicy` - `None` / `BeforeMigration` / `EveryStep`; sets the backup
  path in the migration context.
- `OpenPolicy` / `open_strategy` - explicit old-version open strategy:
  `Upgrade` (migrate), `ReadOnly` (open newer DB read-only), `Reject` (refuse
  a newer DB).

The actual SQL dialect is deferred to the repository layer (T084); this row
locks the schema/versioning contract.