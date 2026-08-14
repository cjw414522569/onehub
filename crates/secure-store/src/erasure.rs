//! Local data deletion, account sign-out, and cryptographic erasure (T096).
//!
//! The flow is confirmation-first: [`CryptoErasure::plan`] computes the exact
//! [`ErasurePlan`] (secret names, local-data keys, backup archives) for a
//! user-selected [`ErasureScope`] *before* anything is touched, so the caller
//! can present the plan and get confirmation. [`CryptoErasure::erase`] then
//! removes exactly the planned items and returns an [`ErasureReport`].
//!
//! After erasure, [`forensic_scan`] re-reads every store and reports any
//! remaining occurrence of known plaintext markers or secret names — the
//! "forensic check" that nothing recoverable is left behind. `Account`
//! scope is the sign-out flow: it clears the sync account's secrets
//! (device key, wrapped keys, recovery code, account token) while leaving
//! unrelated local data and backups intact.

use std::collections::HashMap;

use crate::store::{SecureStore, StoreError};

/// Naming convention for sync-account secrets in the secure store.
pub const ACCOUNT_SECRET_PREFIX: &str = "account:";

/// A local key/value data store (config, settings, session data).
pub trait DataStore {
    /// All keys currently present.
    fn keys(&self) -> Vec<String>;
    /// A value by key.
    fn get(&self, key: &str) -> Option<Vec<u8>>;
    /// Whether a key is present.
    fn contains(&self, key: &str) -> bool;
    /// Deletes a key; returns whether it existed.
    fn delete(&mut self, key: &str) -> bool;
}

/// An encrypted-backup archive store.
pub trait BackupStore {
    /// All archive names currently present.
    fn names(&self) -> Vec<String>;
    /// An archive's contents by name.
    fn contents(&self, name: &str) -> Option<Vec<u8>>;
    /// Whether an archive is present.
    fn contains(&self, name: &str) -> bool;
    /// Deletes an archive; returns whether it existed.
    fn delete(&mut self, name: &str) -> bool;
}

/// The user-confirmable erasure scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErasureScope {
    /// Sign-out: the sync account's secrets (account:*) and nothing else.
    Account,
    /// Local configuration/data (no secrets, no backups).
    LocalData,
    /// Encrypted backup archives.
    Backups,
    /// Full wipe: every secret, data key, and backup.
    Everything,
}

/// The exact items an erasure will remove — the confirmation artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErasurePlan {
    /// The scope the user selected.
    pub scope: ErasureScope,
    /// Secure-store secret names to erase.
    pub secrets: Vec<String>,
    /// Local data keys to erase.
    pub data_keys: Vec<String>,
    /// Backup archive names to erase.
    pub backups: Vec<String>,
    /// Cryptographic erasure is always irreversible.
    pub irreversible: bool,
}

/// What an erasure actually removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ErasureReport {
    /// Secrets erased.
    pub secrets_erased: usize,
    /// Local data keys erased.
    pub data_keys_erased: usize,
    /// Backup archives erased.
    pub backups_erased: usize,
}

/// A deterministic in-memory data store (test double / local data model).
#[derive(Debug, Clone, Default)]
pub struct MemoryDataStore {
    entries: HashMap<String, Vec<u8>>,
}

impl MemoryDataStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a value.
    pub fn insert(&mut self, key: &str, value: Vec<u8>) {
        self.entries.insert(key.to_owned(), value);
    }
}

impl DataStore for MemoryDataStore {
    fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.entries.keys().cloned().collect();
        keys.sort();
        keys
    }

    fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.entries.get(key).cloned()
    }

    fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    fn delete(&mut self, key: &str) -> bool {
        self.entries.remove(key).is_some()
    }
}

/// A deterministic in-memory backup-archive store (test double).
#[derive(Debug, Clone, Default)]
pub struct MemoryBackupStore {
    archives: HashMap<String, Vec<u8>>,
}

impl MemoryBackupStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts an archive.
    pub fn insert(&mut self, name: &str, contents: Vec<u8>) {
        self.archives.insert(name.to_owned(), contents);
    }
}

impl BackupStore for MemoryBackupStore {
    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.archives.keys().cloned().collect();
        names.sort();
        names
    }

    fn contents(&self, name: &str) -> Option<Vec<u8>> {
        self.archives.get(name).cloned()
    }

    fn contains(&self, name: &str) -> bool {
        self.archives.contains_key(name)
    }

    fn delete(&mut self, name: &str) -> bool {
        self.archives.remove(name).is_some()
    }
}

/// The confirmation-first erasure flow.
pub struct CryptoErasure;

impl CryptoErasure {
    /// Computes the exact items a scope will erase, without mutating
    /// anything. The caller presents this plan to the user for confirmation.
    pub fn plan(
        scope: ErasureScope,
        known_secrets: &[&str],
        secrets: &dyn SecureStore,
        data: &dyn DataStore,
        backups: &dyn BackupStore,
    ) -> ErasurePlan {
        let mut secret_names = Vec::new();
        if scope == ErasureScope::Account || scope == ErasureScope::Everything {
            for name in known_secrets {
                if scope == ErasureScope::Account && !name.starts_with(ACCOUNT_SECRET_PREFIX) {
                    continue;
                }
                // Only confirm items that actually exist in the store.
                if matches!(secrets.get_secret(name), Ok(Some(_))) {
                    secret_names.push((*name).to_owned());
                }
            }
        }
        let data_keys = match scope {
            ErasureScope::LocalData | ErasureScope::Everything => data.keys(),
            _ => Vec::new(),
        };
        let backups = match scope {
            ErasureScope::Backups | ErasureScope::Everything => backups.names(),
            _ => Vec::new(),
        };
        ErasurePlan {
            scope,
            secrets: secret_names,
            data_keys,
            backups,
            irreversible: true,
        }
    }

    /// Executes a confirmed plan: deletes exactly the planned secrets, data
    /// keys, and backups. Returns what was actually removed.
    pub fn erase(
        plan: &ErasurePlan,
        secrets: &mut dyn SecureStore,
        data: &mut dyn DataStore,
        backups: &mut dyn BackupStore,
    ) -> Result<ErasureReport, StoreError> {
        let mut secrets_erased = 0;
        for name in &plan.secrets {
            if secrets.delete_secret(name)? {
                secrets_erased += 1;
            }
        }
        let mut data_keys_erased = 0;
        for key in &plan.data_keys {
            if data.delete(key) {
                data_keys_erased += 1;
            }
        }
        let mut backups_erased = 0;
        for name in &plan.backups {
            if backups.delete(name) {
                backups_erased += 1;
            }
        }
        Ok(ErasureReport {
            secrets_erased,
            data_keys_erased,
            backups_erased,
        })
    }
}

/// Scans every store for any remaining occurrence of the given needles
/// (known plaintext markers or secret names). Returns one human-readable hit
/// per occurrence; an empty result means nothing recoverable remains.
pub fn forensic_scan(
    secrets: &dyn SecureStore,
    data: &dyn DataStore,
    backups: &dyn BackupStore,
    needles: &[&[u8]],
) -> Vec<String> {
    let mut hits = Vec::new();
    for name in secrets.names() {
        for needle in needles {
            if name
                .as_bytes()
                .windows(needle.len())
                .any(|window| window == *needle)
            {
                hits.push(format!("secret name '{name}' contains needle"));
            }
        }
        if let Ok(Some(value)) = secrets.get_secret(&name) {
            for needle in needles {
                if value.windows(needle.len()).any(|window| window == *needle) {
                    hits.push(format!("secret '{name}' value contains needle"));
                }
            }
        }
    }
    for key in data.keys() {
        for needle in needles {
            if key
                .as_bytes()
                .windows(needle.len())
                .any(|window| window == *needle)
            {
                hits.push(format!("data key '{key}' contains needle"));
            }
        }
        if let Some(value) = data.get(&key) {
            for needle in needles {
                if value.windows(needle.len()).any(|window| window == *needle) {
                    hits.push(format!("data key '{key}' value contains needle"));
                }
            }
        }
    }
    for name in backups.names() {
        for needle in needles {
            if name
                .as_bytes()
                .windows(needle.len())
                .any(|window| window == *needle)
            {
                hits.push(format!("backup '{name}' name contains needle"));
            }
        }
        if let Some(contents) = backups.contents(&name) {
            for needle in needles {
                if contents
                    .windows(needle.len())
                    .any(|window| window == *needle)
                {
                    hits.push(format!("backup '{name}' contents contain needle"));
                }
            }
        }
    }
    hits.sort();
    hits.dedup();
    hits
}

#[cfg(test)]
mod tests {
    use super::{
        forensic_scan, BackupStore, CryptoErasure, DataStore, ErasureScope, MemoryBackupStore,
        MemoryDataStore,
    };
    use crate::store::{MemorySecureStore, SecureStore};

    const MASTER_KEY_MARKER: &[u8] = b"MASTER_KEY_MARKER_DEADBEEF";
    const PASSWORD_MARKER: &[u8] = b"PASSWORD_PLAINTEXT_LEAK";
    const BACKUP_MARKER: &[u8] = b"BACKUP_PLAINTEXT_LEAK";

    fn populate() -> (
        MemorySecureStore,
        MemoryDataStore,
        MemoryBackupStore,
        Vec<String>,
    ) {
        let mut secrets = MemorySecureStore::new();
        let mut data = MemoryDataStore::new();
        let mut backups = MemoryBackupStore::new();
        // Sync-account secrets (sign-out scope).
        secrets
            .set_secret("account:device_key", MASTER_KEY_MARKER)
            .unwrap();
        secrets
            .set_secret("account:wrapped:1", b"wrapped-key-bytes")
            .unwrap();
        secrets
            .set_secret("account:recovery_code", b"RECOVERY-CODE-SECRET")
            .unwrap();
        secrets
            .set_secret("account:token", b"account-access-token")
            .unwrap();
        // A non-account secret (kept on sign-out).
        secrets
            .set_secret("ssh:host_key:example.com", PASSWORD_MARKER)
            .unwrap();
        // Local data.
        data.insert("settings", b"theme=dark".to_vec());
        data.insert("known_hosts", PASSWORD_MARKER.to_vec());
        data.insert("recent_sessions", b"[localhost]".to_vec());
        // Backup archives.
        backups.insert("backup-2026-08-01.bin", BACKUP_MARKER.to_vec());
        backups.insert("backup-2026-08-15.bin", b"encrypted-archive".to_vec());
        let known = vec![
            "account:device_key".to_owned(),
            "account:wrapped:1".to_owned(),
            "account:recovery_code".to_owned(),
            "account:token".to_owned(),
            "ssh:host_key:example.com".to_owned(),
        ];
        (secrets, data, backups, known)
    }

    fn known_refs(known: &[String]) -> Vec<&str> {
        known.iter().map(String::as_str).collect()
    }

    #[test]
    fn plan_lists_exact_scope_for_confirmation() {
        let (secrets, data, backups, known) = populate();
        let known = known_refs(&known);

        // Account (sign-out): only account: secrets, nothing else.
        let account = CryptoErasure::plan(ErasureScope::Account, &known, &secrets, &data, &backups);
        assert!(account.irreversible);
        assert_eq!(
            account.secrets,
            vec![
                "account:device_key",
                "account:wrapped:1",
                "account:recovery_code",
                "account:token",
            ]
        );
        assert!(account.data_keys.is_empty());
        assert!(account.backups.is_empty());

        // LocalData: only data keys.
        let local = CryptoErasure::plan(ErasureScope::LocalData, &known, &secrets, &data, &backups);
        assert!(local.secrets.is_empty());
        assert_eq!(
            local.data_keys,
            vec!["known_hosts", "recent_sessions", "settings"]
        );
        assert!(local.backups.is_empty());

        // Backups: only backups.
        let backup = CryptoErasure::plan(ErasureScope::Backups, &known, &secrets, &data, &backups);
        assert!(backup.secrets.is_empty());
        assert!(backup.data_keys.is_empty());
        assert_eq!(
            backup.backups,
            vec!["backup-2026-08-01.bin", "backup-2026-08-15.bin"]
        );

        // Everything: all present secrets, data, and backups.
        let everything =
            CryptoErasure::plan(ErasureScope::Everything, &known, &secrets, &data, &backups);
        assert_eq!(everything.secrets.len(), 5);
        assert_eq!(everything.data_keys.len(), 3);
        assert_eq!(everything.backups.len(), 2);
    }

    #[test]
    fn erase_executes_exactly_the_confirmed_plan() {
        let (mut secrets, mut data, mut backups, known) = populate();
        let known = known_refs(&known);

        // Sign-out first: account material gone, everything else intact.
        let account = CryptoErasure::plan(ErasureScope::Account, &known, &secrets, &data, &backups);
        let report = CryptoErasure::erase(&account, &mut secrets, &mut data, &mut backups).unwrap();
        assert_eq!(report.secrets_erased, 4);
        assert_eq!(report.data_keys_erased, 0);
        assert_eq!(report.backups_erased, 0);
        for name in &account.secrets {
            assert_eq!(secrets.get_secret(name).unwrap(), None);
        }
        assert_eq!(
            secrets.get_secret("ssh:host_key:example.com").unwrap(),
            Some(PASSWORD_MARKER.to_vec())
        );
        assert_eq!(data.keys().len(), 3);
        assert_eq!(backups.names().len(), 2);

        // Full wipe on the remaining material.
        let everything =
            CryptoErasure::plan(ErasureScope::Everything, &known, &secrets, &data, &backups);
        let report =
            CryptoErasure::erase(&everything, &mut secrets, &mut data, &mut backups).unwrap();
        assert_eq!(report.secrets_erased, 1);
        assert_eq!(report.data_keys_erased, 3);
        assert_eq!(report.backups_erased, 2);
        assert!(secrets.names().is_empty());
        assert!(data.keys().is_empty());
        assert!(backups.names().is_empty());
    }

    #[test]
    fn account_sign_out_clears_account_material_only() {
        let (mut secrets, mut data, mut backups, known) = populate();
        let known = known_refs(&known);
        let plan = CryptoErasure::plan(ErasureScope::Account, &known, &secrets, &data, &backups);
        let report = CryptoErasure::erase(&plan, &mut secrets, &mut data, &mut backups).unwrap();
        assert_eq!(report.secrets_erased, 4);
        // All account:* material is gone after sign-out.
        assert!(
            secrets
                .names()
                .iter()
                .all(|name| !name.starts_with(crate::erasure::ACCOUNT_SECRET_PREFIX)),
            "account secrets must be cleared"
        );
        // Non-account secret, local data, and backups are untouched.
        assert_eq!(
            secrets.get_secret("ssh:host_key:example.com").unwrap(),
            Some(PASSWORD_MARKER.to_vec())
        );
        assert_eq!(data.keys().len(), 3);
        assert_eq!(backups.names().len(), 2);
    }
    #[test]
    fn cryptographic_erasure_leaves_no_recoverable_material() {
        let (mut secrets, mut data, mut backups, known) = populate();
        let known = known_refs(&known);
        let needles = [MASTER_KEY_MARKER, PASSWORD_MARKER, BACKUP_MARKER];

        // Forensic scan finds the markers before erasure.
        let hits = forensic_scan(&secrets, &data, &backups, &needles);
        assert!(
            !hits.is_empty(),
            "markers must be detectable before erasure"
        );

        let plan = CryptoErasure::plan(ErasureScope::Everything, &known, &secrets, &data, &backups);
        CryptoErasure::erase(&plan, &mut secrets, &mut data, &mut backups).unwrap();

        // Forensic check: nothing recoverable remains anywhere.
        let hits = forensic_scan(&secrets, &data, &backups, &needles);
        assert!(
            hits.is_empty(),
            "no recoverable material may remain: {hits:?}"
        );
        assert!(secrets.names().is_empty());
        assert!(data.keys().is_empty());
        assert!(backups.names().is_empty());
    }

    #[test]
    fn forensic_scan_detects_leftovers_after_partial_erasure() {
        let (mut secrets, mut data, mut backups, known) = populate();
        let known = known_refs(&known);
        let needles = [PASSWORD_MARKER, BACKUP_MARKER];

        // Sign-out only: local data + backups still hold markers.
        let account = CryptoErasure::plan(ErasureScope::Account, &known, &secrets, &data, &backups);
        CryptoErasure::erase(&account, &mut secrets, &mut data, &mut backups).unwrap();
        let hits = forensic_scan(&secrets, &data, &backups, &needles);
        assert!(
            !hits.is_empty(),
            "leftovers outside the account scope must still be detected"
        );
        assert!(
            hits.iter().any(|hit| hit.contains("known_hosts")),
            "the data leftover must be reported: {hits:?}"
        );
        assert!(
            hits.iter().any(|hit| hit.contains("backup-2026-08-01.bin")),
            "the backup leftover must be reported: {hits:?}"
        );
    }

    #[test]
    fn memory_store_enumerates_names() {
        let mut secrets = MemorySecureStore::new();
        secrets.set_secret("b", b"1").unwrap();
        secrets.set_secret("a", b"2").unwrap();
        assert_eq!(secrets.names(), vec!["a", "b"]);
    }

    #[test]
    fn secure_store_names_defaults_to_empty_for_non_enumerable_backends() {
        let adapter = crate::store::WindowsSecureStore::system();
        assert!(adapter.names().is_empty());
        assert!(!adapter.is_available());
    }
}
