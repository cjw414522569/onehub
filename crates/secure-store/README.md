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
