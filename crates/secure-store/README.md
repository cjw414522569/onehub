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