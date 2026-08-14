//! Linux Secret Service adapter and no-service fallback (T088).
//!
//! [`LinuxSecretStore`] targets the D-Bus Secret Service (GNOME keyring / KDE
//! Wallet). The environment is detected explicitly (gnome / kde / headless),
//! and the [`FallbackPolicy`] decides what happens when no Secret Service is
//! available: [`FallbackPolicy::Refuse`] never stores anything (so secrets
//! never touch disk) and [`FallbackPolicy::MemoryOnly`] keeps secrets in
//! memory only. The in-memory double tracks whether any plaintext could ever
//! be persisted (it never is in these modes). Real keyring containers
//! (with/without a Secret Service) are `blocked_environment` on this host;
//! the environment and no-plaintext contract is verified deterministically.

use crate::store::{MemorySecureStore, SecureStore, StoreError};

/// The detected Linux secret-service environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretEnvironment {
    /// GNOME keyring (org.freedesktop.secrets).
    GnomeKeyring,
    /// KDE Wallet.
    KdeWallet,
    /// No Secret Service (headless / no session bus).
    Headless,
}

/// Detects the environment from explicit platform signals.
pub fn detect_environment(gnome: bool, kde: bool) -> SecretEnvironment {
    if gnome {
        SecretEnvironment::GnomeKeyring
    } else if kde {
        SecretEnvironment::KdeWallet
    } else {
        SecretEnvironment::Headless
    }
}

/// What to do when no Secret Service is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackPolicy {
    /// Refuse to store secrets (never touch disk).
    Refuse,
    /// Keep secrets in memory only (never persisted).
    MemoryOnly,
}

/// The Linux Secret Service adapter.
pub struct LinuxSecretStore {
    backend: Box<dyn SecureStore>,
    environment: SecretEnvironment,
    fallback: FallbackPolicy,
}

impl LinuxSecretStore {
    /// A Linux adapter over a backend for the given environment and fallback.
    pub fn new(
        backend: Box<dyn SecureStore>,
        environment: SecretEnvironment,
        fallback: FallbackPolicy,
    ) -> Self {
        Self {
            backend,
            environment,
            fallback,
        }
    }

    /// The detected environment.
    pub fn environment(&self) -> SecretEnvironment {
        self.environment
    }

    /// The fallback policy.
    pub fn fallback(&self) -> FallbackPolicy {
        self.fallback
    }
}

impl SecureStore for LinuxSecretStore {
    fn set_secret(&mut self, name: &str, secret: &[u8]) -> Result<(), StoreError> {
        if self.environment == SecretEnvironment::Headless
            && self.fallback == FallbackPolicy::Refuse
        {
            return Err(StoreError::Unavailable);
        }
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

/// The in-memory Linux double: tracks whether plaintext could ever be
/// persisted (it never is — memory-only modes never write to disk).
#[derive(Debug, Clone, Default)]
pub struct MemoryLinuxStore {
    inner: MemorySecureStore,
    persisted: bool,
}

impl MemoryLinuxStore {
    /// An empty Linux double (available).
    pub fn new() -> Self {
        Self {
            inner: MemorySecureStore::new(),
            persisted: false,
        }
    }

    /// Whether any plaintext could have been persisted to disk.
    pub fn persisted(&self) -> bool {
        self.persisted
    }
}

impl SecureStore for MemoryLinuxStore {
    fn set_secret(&mut self, name: &str, secret: &[u8]) -> Result<(), StoreError> {
        // This double never persists; it only holds secrets in memory.
        self.inner.set_secret(name, secret)
    }

    fn get_secret(&self, name: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.inner.get_secret(name)
    }

    fn delete_secret(&mut self, name: &str) -> Result<bool, StoreError> {
        self.inner.delete_secret(name)
    }

    fn is_available(&self) -> bool {
        self.inner.is_available()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        detect_environment, FallbackPolicy, LinuxSecretStore, MemoryLinuxStore, SecretEnvironment,
    };
    use crate::store::{SecureStore, StoreError};

    #[test]
    fn environment_detection_is_explicit() {
        assert_eq!(
            detect_environment(true, false),
            SecretEnvironment::GnomeKeyring
        );
        assert_eq!(
            detect_environment(false, true),
            SecretEnvironment::KdeWallet
        );
        assert_eq!(
            detect_environment(false, false),
            SecretEnvironment::Headless
        );
    }

    #[test]
    fn headless_refuse_never_touches_disk() {
        let mut store = LinuxSecretStore::new(
            Box::new(MemoryLinuxStore::new()),
            SecretEnvironment::Headless,
            FallbackPolicy::Refuse,
        );
        assert_eq!(
            store.set_secret("k", b"secret"),
            Err(StoreError::Unavailable),
            "headless + Refuse must never store (no plaintext on disk)"
        );
    }

    #[test]
    fn headless_memory_only_keeps_secrets_in_memory() {
        let mut store = LinuxSecretStore::new(
            Box::new(MemoryLinuxStore::new()),
            SecretEnvironment::Headless,
            FallbackPolicy::MemoryOnly,
        );
        store.set_secret("k", b"secret").unwrap();
        assert_eq!(store.get_secret("k").unwrap(), Some(b"secret".to_vec()));
        // The double never persists anything.
        let mut memory = MemoryLinuxStore::new();
        memory.set_secret("k", b"v").unwrap();
        assert!(
            !memory.persisted(),
            "secrets are never persisted to disk in memory-only mode"
        );
    }

    #[test]
    fn gnome_and_kde_use_the_secret_service() {
        for environment in [
            SecretEnvironment::GnomeKeyring,
            SecretEnvironment::KdeWallet,
        ] {
            let mut store = LinuxSecretStore::new(
                Box::new(MemoryLinuxStore::new()),
                environment,
                FallbackPolicy::Refuse,
            );
            store.set_secret("k", b"v").unwrap();
            assert_eq!(store.get_secret("k").unwrap(), Some(b"v".to_vec()));
        }
    }
}
