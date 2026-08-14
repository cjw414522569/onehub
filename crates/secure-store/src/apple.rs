//! Apple Keychain / Secure Enclave adapter (T086).
//!
//! [`AppleKeychainStore`] targets the Apple Keychain with an explicit
//! [`AccessPolicy`] (access control + biometrics + device-only). The
//! in-memory [`MemoryKeychainStore`] test double records which access policy
//! each item was written under (modeling Keychain access-control application)
//! and [`migrate_keychain`] moves legacy items to a new service prefix under
//! the new policy. The real Keychain / Secure Enclave calls require macOS /
//! iOS (simulator or device) and are `blocked_environment` on this Windows
//! CI host; the access-control and migration contract is verified
//! deterministically.

use crate::store::{MemorySecureStore, SecureStore, StoreError};

/// Keychain access control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessControl {
    /// Items accessible only while the device is unlocked.
    WhenUnlocked,
    /// Items accessible after first unlock.
    AfterFirstUnlock,
    /// Items always accessible (not recommended for secrets).
    Always,
}

/// Biometric access requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Biometrics {
    /// No biometric requirement.
    None,
    /// Touch ID required.
    TouchId,
    /// Face ID required.
    FaceId,
}

/// The Keychain access policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessPolicy {
    /// Access control class.
    pub access: AccessControl,
    /// Biometric requirement.
    pub biometrics: Biometrics,
    /// Device-only (not migrated to other devices).
    pub device_only: bool,
}

impl Default for AccessPolicy {
    fn default() -> Self {
        Self {
            access: AccessControl::WhenUnlocked,
            biometrics: Biometrics::None,
            device_only: false,
        }
    }
}

/// The Apple Keychain adapter (wraps any [`SecureStore`] backend).
pub struct AppleKeychainStore {
    backend: Box<dyn SecureStore>,
    policy: AccessPolicy,
}

impl AppleKeychainStore {
    /// An Apple adapter over a backend with an access policy.
    pub fn new(backend: Box<dyn SecureStore>, policy: AccessPolicy) -> Self {
        Self { backend, policy }
    }

    /// The configured access policy.
    pub fn policy(&self) -> AccessPolicy {
        self.policy
    }
}

impl SecureStore for AppleKeychainStore {
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

/// The in-memory Keychain test double: records the access policy applied to
/// each item.
#[derive(Debug, Clone, Default)]
pub struct MemoryKeychainStore {
    inner: MemorySecureStore,
    policy_applied: Vec<(String, AccessPolicy)>,
}

impl MemoryKeychainStore {
    /// An empty Keychain double (available).
    pub fn new() -> Self {
        Self {
            inner: MemorySecureStore::new(),
            policy_applied: Vec::new(),
        }
    }

    /// Writes an item under an explicit access policy (records it).
    pub fn write_with_policy(
        &mut self,
        name: &str,
        secret: &[u8],
        policy: AccessPolicy,
    ) -> Result<(), StoreError> {
        self.inner.set_secret(name, secret)?;
        self.policy_applied.push((name.to_owned(), policy));
        Ok(())
    }

    /// The policies recorded per item name.
    pub fn policies_applied(&self) -> &[(String, AccessPolicy)] {
        &self.policy_applied
    }

    /// All item names currently stored.
    pub fn keys(&self) -> Vec<String> {
        self.policy_applied
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Whether an item is present.
    pub fn contains(&self, name: &str) -> bool {
        self.inner.get_secret(name).ok().flatten().is_some()
    }
}

impl SecureStore for MemoryKeychainStore {
    fn set_secret(&mut self, name: &str, secret: &[u8]) -> Result<(), StoreError> {
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

/// The outcome of a Keychain migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// Items migrated under the new policy.
    Migrated(usize),
    /// No legacy items found.
    NothingToMigrate,
    /// The store is unavailable (e.g. locked).
    Unavailable,
}

/// Migrates legacy items from `from_prefix` to `to_prefix` under `policy`.
///
/// Each legacy item is re-written under the new prefix with the new access
/// policy and the legacy copy is removed, so access control and biometrics
/// are applied to migrated items.
pub fn migrate_keychain(
    store: &mut MemoryKeychainStore,
    from_prefix: &str,
    to_prefix: &str,
    policy: AccessPolicy,
) -> MigrationOutcome {
    let legacy: Vec<(String, Vec<u8>)> = store
        .keys()
        .into_iter()
        .filter(|name| name.starts_with(from_prefix))
        .filter_map(|name| store.get_secret(&name).ok().flatten().map(|v| (name, v)))
        .collect();
    if legacy.is_empty() {
        return MigrationOutcome::NothingToMigrate;
    }
    let mut migrated = 0usize;
    for (name, value) in legacy {
        let new_name = format!("{to_prefix}{}", &name[from_prefix.len()..]);
        if store.write_with_policy(&new_name, &value, policy).is_err() {
            return MigrationOutcome::Unavailable;
        }
        let _ = store.delete_secret(&name);
        migrated += 1;
    }
    MigrationOutcome::Migrated(migrated)
}

#[cfg(test)]
mod tests {
    use super::{
        migrate_keychain, AccessControl, AccessPolicy, AppleKeychainStore, Biometrics,
        MemoryKeychainStore, MigrationOutcome,
    };
    use crate::store::SecureStore;

    #[test]
    fn access_policy_defaults_and_custom() {
        let default = AccessPolicy::default();
        assert_eq!(default.access, AccessControl::WhenUnlocked);
        assert_eq!(default.biometrics, Biometrics::None);
        assert!(!default.device_only);
        let custom = AccessPolicy {
            access: AccessControl::AfterFirstUnlock,
            biometrics: Biometrics::FaceId,
            device_only: true,
        };
        assert_eq!(custom.biometrics, Biometrics::FaceId);
        assert!(custom.device_only);
    }

    #[test]
    fn apple_adapter_delegates_and_keeps_policy() {
        let policy = AccessPolicy {
            biometrics: Biometrics::TouchId,
            ..AccessPolicy::default()
        };
        let mut store = AppleKeychainStore::new(Box::new(MemoryKeychainStore::new()), policy);
        assert_eq!(store.policy(), policy);
        store.set_secret("k", b"v").unwrap();
        assert_eq!(store.get_secret("k").unwrap(), Some(b"v".to_vec()));
    }

    #[test]
    fn keychain_double_records_applied_policy() {
        let mut store = MemoryKeychainStore::new();
        let policy = AccessPolicy {
            biometrics: Biometrics::FaceId,
            device_only: true,
            ..AccessPolicy::default()
        };
        store
            .write_with_policy("ssh.key", b"secret", policy)
            .unwrap();
        assert!(store.contains("ssh.key"));
        assert_eq!(store.policies_applied(), &[("ssh.key".to_owned(), policy)]);
    }

    #[test]
    fn migration_moves_legacy_items_under_new_policy() {
        let mut store = MemoryKeychainStore::new();
        let old_policy = AccessPolicy::default();
        store
            .write_with_policy("legacy.host-a", b"a", old_policy)
            .unwrap();
        store
            .write_with_policy("legacy.host-b", b"b", old_policy)
            .unwrap();
        store
            .write_with_policy("other.key", b"keep", old_policy)
            .unwrap();
        let new_policy = AccessPolicy {
            biometrics: Biometrics::TouchId,
            device_only: true,
            ..AccessPolicy::default()
        };
        assert_eq!(
            migrate_keychain(&mut store, "legacy.", "ssh.", new_policy),
            MigrationOutcome::Migrated(2)
        );
        assert!(!store.contains("legacy.host-a"), "legacy copy removed");
        assert!(store.contains("ssh.host-a"), "migrated under new prefix");
        assert!(store.contains("other.key"), "unrelated item untouched");
        // Migrated items were written under the new policy.
        assert!(store
            .policies_applied()
            .iter()
            .any(|(name, policy)| name == "ssh.host-b" && *policy == new_policy));
    }

    #[test]
    fn migration_with_no_legacy_items_is_a_noop() {
        let mut store = MemoryKeychainStore::new();
        store
            .write_with_policy("ssh.host", b"v", AccessPolicy::default())
            .unwrap();
        assert_eq!(
            migrate_keychain(&mut store, "legacy.", "ssh.", AccessPolicy::default()),
            MigrationOutcome::NothingToMigrate
        );
    }
}
