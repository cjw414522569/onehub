# secure-store

- Layer: L3 (infrastructure)
- Dependencies: `core-domain`
- Scope: platform secure-storage adapter returning opaque secret references.

## T085: Windows secure storage adapter

`crates/secure-store/src/store.rs`:

- `SecureStore` - the platform-agnostic secret contract (opaque bytes;
  set/get/delete/is_available).
- `WindowsSecureStore` - targets Windows Credential Manager / DPAPI
  (`ProtectionMechanism::WindowsCredentialManager`); the real OS binding is
  `blocked_environment` without a native credential scope.
- `MemorySecureStore` - deterministic test double modeling lock-screen /
  account-switch availability (`StoreError::Unavailable` on lock, restored on
  unlock).
- `SystemCredentialBackend` - the system backend boundary (reports
  `NotSupported` without a native binding).
## T086: Apple Keychain / Secure Enclave adapter

`crates/secure-store/src/apple.rs`:

- `AppleKeychainStore` - targets the Apple Keychain with an explicit
  `AccessPolicy` (access control class + biometrics + device-only).
- `MemoryKeychainStore` - in-memory double recording the access policy applied
  to each item.
- `migrate_keychain` - moves legacy items to a new service prefix under the
  new policy (legacy copy removed, unrelated items untouched).

Real Keychain / Secure Enclave calls require macOS/iOS (simulator or device)
and are `blocked_environment` on this Windows CI host; the access-control and
migration contract is verified deterministically.

## T087: Android Keystore adapter

`crates/secure-store/src/android.rs`:

- `AndroidKeystoreStore` - targets the Android Keystore with
  `KeystoreCapabilities` (StrongBox / TEE / Software hardware protection via
  `select_hardware`).
- `MemoryAndroidKeystore` - models key invalidation (e.g. a screen-lock
  change): while invalidated, reads/writes fail with `StoreError::Invalidated`;
  `recover()` regenerates keys so the store is usable again (recoverable on
  invalidation).

Real multi-API emulator / device tests are `blocked_environment` on this host;
the hardware-protection and invalidation-recovery contract is verified
deterministically.

## T088: Linux Secret Service adapter and no-service fallback

`crates/secure-store/src/linux.rs`:

- `LinuxSecretStore` - targets the D-Bus Secret Service (GNOME keyring / KDE
  Wallet) with explicit `SecretEnvironment` detection (gnome / kde / headless).
- `FallbackPolicy` - `Refuse` (headless never stores, no plaintext on disk) or
  `MemoryOnly` (secrets held in memory only; `persisted()` stays false).

Real keyring containers (with/without a Secret Service) are
`blocked_environment` on this host; the environment and no-plaintext contract
is verified deterministically.


## T096: local data deletion, account sign-out, and cryptographic erasure

`crates/secure-store/src/erasure.rs`:

- `CryptoErasure::plan` - computes the exact `ErasurePlan` (secret names,
  local-data keys, backup archives) for a user-selected `ErasureScope`
  *before* mutating anything, so the user can confirm the scope.
- `ErasureScope` - `Account` (sign-out: clears `account:*` sync secrets),
  `LocalData`, `Backups`, or `Everything` (full wipe).
- `CryptoErasure::erase` - removes exactly the confirmed items and returns an
  `ErasureReport` (counts erased).
- `forensic_scan` - the post-deletion forensic check: re-reads every store and
  reports any remaining occurrence of known plaintext markers / secret names
  (empty = nothing recoverable left).
- `DataStore` / `BackupStore` - generic local-data and backup-archive
  contracts with deterministic `MemoryDataStore` / `MemoryBackupStore`
  doubles.

`SecureStore` gained `names()` (default empty; `MemorySecureStore` enumerates
its keys) so erasure and forensic scans can enumerate secrets.