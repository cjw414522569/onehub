//! Android Keystore adapter (T087).
//!
//! [`AndroidKeystoreStore`] targets the Android Keystore with an explicit
//! [`KeystoreCapabilities`] (StrongBox / TEE / Software hardware protection).
//! Android keys can be invalidated (e.g. by a screen-lock change); the
//! in-memory [`MemoryAndroidKeystore`] double models invalidation and
//! recovery: while invalidated, reads fail with [`StoreError::Invalidated`]
//! and the store is recoverable by re-writing under a fresh key. Real
//! multi-API emulator / device tests are `blocked_environment` on this host;
//! the hardware-protection and invalidation-recovery contract is verified
//! deterministically.

use crate::store::{MemorySecureStore, SecureStore, StoreError};

/// Android Keystore hardware protection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareProtection {
    /// StrongBox (dedicated secure hardware).
    StrongBox,
    /// Trusted Execution Environment.
    TrustedExecutionEnvironment,
    /// Software-backed (no dedicated hardware).
    Software,
}

/// The Keystore capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeystoreCapabilities {
    /// The hardware protection in use.
    pub hardware: HardwareProtection,
    /// Whether keys can be invalidated (and must be recovered).
    pub invalidatable: bool,
}

/// Selects hardware protection: StrongBox when supported, else TEE, else
/// software (fallback).
pub fn select_hardware(strongbox: bool, tee: bool) -> HardwareProtection {
    if strongbox {
        HardwareProtection::StrongBox
    } else if tee {
        HardwareProtection::TrustedExecutionEnvironment
    } else {
        HardwareProtection::Software
    }
}

/// The Android Keystore adapter (wraps any [`SecureStore`] backend).
pub struct AndroidKeystoreStore {
    backend: Box<dyn SecureStore>,
    capabilities: KeystoreCapabilities,
}

impl AndroidKeystoreStore {
    /// An Android adapter over a backend with explicit capabilities.
    pub fn new(backend: Box<dyn SecureStore>, capabilities: KeystoreCapabilities) -> Self {
        Self {
            backend,
            capabilities,
        }
    }

    /// The Keystore capabilities.
    pub fn capabilities(&self) -> KeystoreCapabilities {
        self.capabilities
    }
}

impl SecureStore for AndroidKeystoreStore {
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

/// The invalidation state of the Keystore keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidationState {
    /// Keys valid.
    Valid,
    /// Keys invalidated (screen-lock change, etc.); recovery required.
    Invalidated,
}

/// The in-memory Android Keystore double: models hardware protection and
/// invalidation / recovery.
#[derive(Debug, Clone)]
pub struct MemoryAndroidKeystore {
    inner: MemorySecureStore,
    capabilities: KeystoreCapabilities,
    state: InvalidationState,
}

impl MemoryAndroidKeystore {
    /// A store with the given capabilities (valid).
    pub fn new(capabilities: KeystoreCapabilities) -> Self {
        Self {
            inner: MemorySecureStore::new(),
            capabilities,
            state: InvalidationState::Valid,
        }
    }

    /// The capabilities.
    pub fn capabilities(&self) -> KeystoreCapabilities {
        self.capabilities
    }

    /// The invalidation state.
    pub fn state(&self) -> InvalidationState {
        self.state
    }

    /// Simulates key invalidation (e.g. a screen-lock change): reads now fail
    /// with [`StoreError::Invalidated`].
    pub fn invalidate(&mut self) {
        self.state = InvalidationState::Invalidated;
    }

    /// Recovers by regenerating keys; the store becomes valid again. Items
    /// must be re-written (the caller re-encrypts).
    pub fn recover(&mut self) {
        self.state = InvalidationState::Valid;
    }
}

impl SecureStore for MemoryAndroidKeystore {
    fn set_secret(&mut self, name: &str, secret: &[u8]) -> Result<(), StoreError> {
        if self.state == InvalidationState::Invalidated {
            return Err(StoreError::Invalidated);
        }
        self.inner.set_secret(name, secret)
    }

    fn get_secret(&self, name: &str) -> Result<Option<Vec<u8>>, StoreError> {
        if self.state == InvalidationState::Invalidated {
            return Err(StoreError::Invalidated);
        }
        self.inner.get_secret(name)
    }

    fn delete_secret(&mut self, name: &str) -> Result<bool, StoreError> {
        if self.state == InvalidationState::Invalidated {
            return Err(StoreError::Invalidated);
        }
        self.inner.delete_secret(name)
    }

    fn is_available(&self) -> bool {
        self.state == InvalidationState::Valid
    }
}

#[cfg(test)]
mod tests {
    use super::{
        select_hardware, AndroidKeystoreStore, HardwareProtection, InvalidationState,
        KeystoreCapabilities, MemoryAndroidKeystore,
    };
    use crate::store::{SecureStore, StoreError};

    #[test]
    fn hardware_protection_is_selected() {
        assert_eq!(select_hardware(true, true), HardwareProtection::StrongBox);
        assert_eq!(
            select_hardware(false, true),
            HardwareProtection::TrustedExecutionEnvironment
        );
        assert_eq!(select_hardware(false, false), HardwareProtection::Software);
    }

    #[test]
    fn android_adapter_delegates_and_keeps_capabilities() {
        let capabilities = KeystoreCapabilities {
            hardware: HardwareProtection::StrongBox,
            invalidatable: true,
        };
        let mut store = AndroidKeystoreStore::new(
            Box::new(MemoryAndroidKeystore::new(capabilities)),
            capabilities,
        );
        assert_eq!(store.capabilities(), capabilities);
        store.set_secret("k", b"v").unwrap();
        assert_eq!(store.get_secret("k").unwrap(), Some(b"v".to_vec()));
    }

    #[test]
    fn invalidation_is_recoverable() {
        let capabilities = KeystoreCapabilities {
            hardware: HardwareProtection::TrustedExecutionEnvironment,
            invalidatable: true,
        };
        let mut store = MemoryAndroidKeystore::new(capabilities);
        store.set_secret("k", b"v").unwrap();
        // Screen-lock change invalidates the keys.
        store.invalidate();
        assert_eq!(store.state(), InvalidationState::Invalidated);
        assert!(!store.is_available());
        assert_eq!(store.get_secret("k"), Err(StoreError::Invalidated));
        assert_eq!(store.set_secret("k2", b"x"), Err(StoreError::Invalidated));
        // Recovery regenerates keys; the caller re-encrypts.
        store.recover();
        assert_eq!(store.state(), InvalidationState::Valid);
        assert!(store.is_available());
        store.set_secret("k", b"v2").unwrap();
        assert_eq!(store.get_secret("k").unwrap(), Some(b"v2".to_vec()));
    }
}
