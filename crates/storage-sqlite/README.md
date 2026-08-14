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
## T084: config repository and atomic transactions

`crates/storage-sqlite/src/repository.rs`:

- `ConfigRepository` - the SQL-free domain contract (get/set/delete/contains).
- `AtomicStore` - in-memory store with per-key versions and
  `compare_and_swap` (stale writers get `VersionMismatch`, no silent lost
  update).
- `AtomicTransaction` - snapshot-isolated atomic transactions with conflict
  detection on commit.

Concurrent modifications never lose data (8 threads x 250 CAS increments all
preserved); the domain layer never depends on SQL.

## T089: database field-level encryption and master-key wrapping

`crates/storage-sqlite/src/crypto.rs`:

- `EncryptedField` - a versioned ChaCha20-Poly1305 AEAD blob (version + nonce +
  ciphertext + tag).
- `FieldEncryptor` / `KeyRing` - field keys live outside the database;
  `KeyRing::rotate` adds a new key version (old versions stay decryptable) and
  `reencrypt` moves rows to the active version.
- `MasterKeyWrapper` - wraps the active field key with a master key held in
  the OS secure store (never in the database); `unwrap` rebuilds the key ring
  for recovery.

Tampering, rotation, and recovery are verified by tests.

## T090: secure import/export and encrypted backup format

`crates/storage-sqlite/src/backup.rs`:

- `BackupArchive` - a versioned (BACKUP_VERSION=1), password-encrypted backup
  with an explicit `ExportScope` (which categories are included).
- `KdfParams` - scrypt parameters (salt, log_n>=15, r>=8, p>=1); `random_salt`
  uses getrandom.
- `encrypt_backup` / `decrypt_backup` - ChaCha20-Poly1305 under the derived
  key; a wrong passphrase yields `BadPassphrase`.

Round-trip and wrong-passphrase behavior are verified by tests.
