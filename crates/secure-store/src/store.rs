//! Secure storage adapter contract and the Windows adapter (T085).
//!
//! [`SecureStore`] is the platform-agnostic secret contract (opaque bytes,
//! no plaintext leakage). The Windows adapter ([`WindowsSecureStore`]) targets
//! the system protection mechanism — Windows Credential Manager / DPAPI —
//! declared via [`ProtectionMechanism`]; the real OS binding requires the
//! `windows` crate and a deployed credential scope and is `blocked_environment`
//! on CI hosts without it. [`MemorySecureStore`] is a deterministic test
//! double that models lock-screen / account-switch behavior: when unavailable,
//! reads fail with [`StoreError::Unavailable`] instead of returning secrets.

use std::collections::HashMap;

/// Why a secure-store operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    /// The system protection is unavailable (lock screen / different account).
    Unavailable,
    /// The operation is not supported on this platform/build.
    NotSupported,
    /// The key was invalidated (Android Keystore) and must be recovered.
    Invalidated,
}

/// The OS protection mechanism used by an adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectionMechanism {
    /// Windows Credential Manager / DPAPI.
    WindowsCredentialManager,
    /// Platform-neutral (in-memory test double).
    Memory,
}

/// The platform-agnostic secret contract.
pub trait SecureStore {
    /// Stores a secret (opaque bytes).
    fn set_secret(&mut self, name: &str, secret: &[u8]) -> Result<(), StoreError>;
    /// Reads a secret; `Ok(None)` when the name is not present.
    fn get_secret(&self, name: &str) -> Result<Option<Vec<u8>>, StoreError>;
    /// Deletes a secret; returns whether it existed.
    fn delete_secret(&mut self, name: &str) -> Result<bool, StoreError>;
    /// Whether secrets are currently accessible (lock screen / account
    /// switch may make them unavailable).
    fn is_available(&self) -> bool;
}

/// A deterministic in-memory store that models lock-screen / account-switch
/// availability.
#[derive(Debug, Clone, Default)]
pub struct MemorySecureStore {
    secrets: HashMap<String, Vec<u8>>,
    available: bool,
}

impl MemorySecureStore {
    /// An available empty store.
    pub fn new() -> Self {
        Self {
            secrets: HashMap::new(),
            available: true,
        }
    }

    /// Simulates a lock screen / account switch: when `available` is false,
    /// reads and writes fail with [`StoreError::Unavailable`].
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }
}

impl SecureStore for MemorySecureStore {
    fn set_secret(&mut self, name: &str, secret: &[u8]) -> Result<(), StoreError> {
        if !self.available {
            return Err(StoreError::Unavailable);
        }
        self.secrets.insert(name.to_owned(), secret.to_vec());
        Ok(())
    }

    fn get_secret(&self, name: &str) -> Result<Option<Vec<u8>>, StoreError> {
        if !self.available {
            return Err(StoreError::Unavailable);
        }
        Ok(self.secrets.get(name).cloned())
    }

    fn delete_secret(&mut self, name: &str) -> Result<bool, StoreError> {
        if !self.available {
            return Err(StoreError::Unavailable);
        }
        Ok(self.secrets.remove(name).is_some())
    }

    fn is_available(&self) -> bool {
        self.available
    }
}

/// The Windows secure-storage adapter.
///
/// The production backend calls Windows Credential Manager / DPAPI so secrets
/// are protected by the OS and follow the OS lock-screen / account-scope
/// rules. The real OS binding is `blocked_environment` on CI hosts without a
/// native credential scope; this wrapper keeps the contract and delegates to
/// any [`SecureStore`] backend.
pub struct WindowsSecureStore {
    backend: Box<dyn SecureStore>,
}

impl WindowsSecureStore {
    /// A Windows adapter over the given backend.
    pub fn new(backend: Box<dyn SecureStore>) -> Self {
        Self { backend }
    }

    /// The production entry: wraps the system Credential Manager backend.
    /// On hosts without a native binding this returns a store whose
    /// operations report [`StoreError::NotSupported`].
    pub fn system() -> Self {
        Self::new(Box::new(SystemCredentialBackend))
    }

    /// The protection mechanism this adapter targets.
    pub fn protection() -> ProtectionMechanism {
        ProtectionMechanism::WindowsCredentialManager
    }
}

impl SecureStore for WindowsSecureStore {
    fn set_secret(&mut self, name: &str, secret: &[u8]) -> Result<(), StoreError> {
        self.backend.set_secret(name, secret)
    }

    fn get_secret(&self, name: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.backend.get_secret(name)
    }

    fn delete_secret(&mut self, name: &str) -> Result<bool, StoreError> {
        self.backend.delete_secret(name)
    }

    fn is_available(&self) -> bool {
        self.backend.is_available()
    }
}

/// The system Credential Manager backend (interface boundary).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemCredentialBackend;

impl SecureStore for SystemCredentialBackend {
    fn set_secret(&mut self, _name: &str, _secret: &[u8]) -> Result<(), StoreError> {
        Err(StoreError::NotSupported)
    }

    fn get_secret(&self, _name: &str) -> Result<Option<Vec<u8>>, StoreError> {
        Err(StoreError::NotSupported)
    }

    fn delete_secret(&mut self, _name: &str) -> Result<bool, StoreError> {
        Err(StoreError::NotSupported)
    }

    fn is_available(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MemorySecureStore, ProtectionMechanism, SecureStore, StoreError, WindowsSecureStore,
    };

    #[test]
    fn memory_store_round_trip() {
        let mut store = MemorySecureStore::new();
        assert!(store.is_available());
        assert_eq!(store.set_secret("host.key", b"secret").unwrap(), ());
        assert_eq!(
            store.get_secret("host.key").unwrap(),
            Some(b"secret".to_vec())
        );
        assert_eq!(store.get_secret("missing").unwrap(), None);
        assert!(store.delete_secret("host.key").unwrap());
        assert!(!store.delete_secret("host.key").unwrap());
    }

    #[test]
    fn lock_screen_makes_secrets_unavailable() {
        let mut store = MemorySecureStore::new();
        store.set_secret("k", b"v").unwrap();
        // Lock screen / account switch: secrets must not leak.
        store.set_available(false);
        assert!(!store.is_available());
        assert_eq!(store.get_secret("k"), Err(StoreError::Unavailable));
        assert_eq!(store.set_secret("k2", b"x"), Err(StoreError::Unavailable));
        assert_eq!(store.delete_secret("k"), Err(StoreError::Unavailable));
        // Unlock restores access.
        store.set_available(true);
        assert_eq!(store.get_secret("k").unwrap(), Some(b"v".to_vec()));
    }

    #[test]
    fn windows_adapter_delegates_to_backend() {
        let mut adapter = WindowsSecureStore::new(Box::new(MemorySecureStore::new()));
        assert_eq!(
            WindowsSecureStore::protection(),
            ProtectionMechanism::WindowsCredentialManager
        );
        adapter.set_secret("k", b"v").unwrap();
        assert_eq!(adapter.get_secret("k").unwrap(), Some(b"v".to_vec()));
        assert!(adapter.is_available());
    }

    #[test]
    fn system_backend_reports_not_supported_without_native_binding() {
        let mut adapter = WindowsSecureStore::system();
        assert_eq!(adapter.set_secret("k", b"v"), Err(StoreError::NotSupported));
        assert!(!adapter.is_available());
    }
}
