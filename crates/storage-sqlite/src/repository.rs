//! Config repository and atomic transactions (T084).
//!
//! The domain-facing [`ConfigRepository`] contract is SQL-free (pure Rust
//! key/value types), so the domain layer never depends on SQL. The
//! [`AtomicStore`] implements it with optimistic concurrency control: every
//! value carries a version and [`AtomicStore::compare_and_swap`] refuses
//! conflicting writes, so concurrent modifications never silently lose data.
//! [`AtomicTransaction`] provides snapshot-isolated atomic transactions with
//! conflict detection on commit.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// The domain-facing repository contract (no SQL types).
pub trait ConfigRepository {
    /// Reads a value.
    fn get(&self, key: &str) -> Option<Vec<u8>>;
    /// Writes a value (overwrites).
    fn set(&self, key: &str, value: Vec<u8>);
    /// Deletes a key; returns whether it existed.
    fn delete(&self, key: &str) -> bool;
    /// Whether a key exists.
    fn contains(&self, key: &str) -> bool;
}

/// Why a compare-and-swap failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasError {
    /// The key's current version differs from the expected version.
    VersionMismatch {
        /// Expected version.
        expected: u64,
        /// Actual version.
        actual: u64,
    },
}

/// A key/value entry with an optimistic-concurrency version.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    value: Vec<u8>,
    version: u64,
}

/// An in-memory atomic store with per-key versions.
#[derive(Debug, Clone, Default)]
pub struct AtomicStore {
    inner: Arc<RwLock<HashMap<String, Entry>>>,
}

impl AtomicStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The current version of a key (0 when absent).
    pub fn version(&self, key: &str) -> u64 {
        self.inner
            .read()
            .expect("store lock")
            .get(key)
            .map(|entry| entry.version)
            .unwrap_or(0)
    }

    /// Compares-and-swaps: writes `value` only when the key's current version
    /// equals `expected_version`; the new version is `expected_version + 1`.
    /// Concurrent conflicting writers get [`CasError::VersionMismatch`] and
    /// retry — no update is silently lost.
    pub fn compare_and_swap(
        &self,
        key: &str,
        expected_version: u64,
        value: Vec<u8>,
    ) -> Result<u64, CasError> {
        let mut guard = self.inner.write().expect("store lock");
        let actual = guard.get(key).map(|entry| entry.version).unwrap_or(0);
        if actual != expected_version {
            return Err(CasError::VersionMismatch {
                expected: expected_version,
                actual,
            });
        }
        let new_version = expected_version + 1;
        guard.insert(
            key.to_owned(),
            Entry {
                value,
                version: new_version,
            },
        );
        Ok(new_version)
    }

    /// Begins an atomic transaction (snapshot isolation).
    pub fn begin(&self) -> AtomicTransaction {
        let snapshot = self.inner.read().expect("store lock").clone();
        AtomicTransaction {
            snapshot,
            writes: HashMap::new(),
            deletes: Vec::new(),
        }
    }
}

impl ConfigRepository for AtomicStore {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.inner
            .read()
            .expect("store lock")
            .get(key)
            .map(|entry| entry.value.clone())
    }

    fn set(&self, key: &str, value: Vec<u8>) {
        let mut guard = self.inner.write().expect("store lock");
        let version = guard.get(key).map(|entry| entry.version).unwrap_or(0);
        guard.insert(
            key.to_owned(),
            Entry {
                value,
                version: version + 1,
            },
        );
    }

    fn delete(&self, key: &str) -> bool {
        self.inner
            .write()
            .expect("store lock")
            .remove(key)
            .is_some()
    }

    fn contains(&self, key: &str) -> bool {
        self.inner.read().expect("store lock").contains_key(key)
    }
}

/// A transaction error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionError {
    /// A written key changed concurrently; the transaction was aborted.
    Conflict,
}

/// A snapshot-isolated atomic transaction.
#[derive(Debug, Clone)]
pub struct AtomicTransaction {
    snapshot: HashMap<String, Entry>,
    writes: HashMap<String, Vec<u8>>,
    deletes: Vec<String>,
}

impl AtomicTransaction {
    /// Reads a key from the transaction snapshot (uncommitted writes win).
    pub fn read(&self, key: &str) -> Option<Vec<u8>> {
        if self.deletes.contains(&key.to_owned()) {
            return None;
        }
        if let Some(value) = self.writes.get(key) {
            return Some(value.clone());
        }
        self.snapshot.get(key).map(|entry| entry.value.clone())
    }

    /// Writes a key within the transaction.
    pub fn write(&mut self, key: &str, value: Vec<u8>) {
        self.deletes.retain(|k| k != key);
        self.writes.insert(key.to_owned(), value);
    }

    /// Deletes a key within the transaction.
    pub fn delete(&mut self, key: &str) {
        self.writes.remove(key);
        if !self.deletes.contains(&key.to_owned()) {
            self.deletes.push(key.to_owned());
        }
    }

    /// Commits atomically: every written/deleted key's version is re-checked
    /// against the snapshot; a concurrent change aborts with [`TransactionError::Conflict`].
    pub fn commit(self, store: &AtomicStore) -> Result<(), TransactionError> {
        for key in self.writes.keys().chain(self.deletes.iter()) {
            let snapshot_version = self
                .snapshot
                .get(key)
                .map(|entry| entry.version)
                .unwrap_or(0);
            if store.version(key) != snapshot_version {
                return Err(TransactionError::Conflict);
            }
        }
        let mut guard = store.inner.write().expect("store lock");
        for (key, value) in &self.writes {
            let version = guard.get(key).map(|entry| entry.version).unwrap_or(0);
            guard.insert(
                key.clone(),
                Entry {
                    value: value.clone(),
                    version: version + 1,
                },
            );
        }
        for key in &self.deletes {
            guard.remove(key);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::{AtomicStore, CasError, ConfigRepository, TransactionError};

    #[test]
    fn repository_contract_round_trip() {
        let store = AtomicStore::new();
        assert!(!store.contains("host"));
        store.set("host", b"example.com".to_vec());
        assert!(store.contains("host"));
        assert_eq!(store.get("host"), Some(b"example.com".to_vec()));
        assert!(store.delete("host"));
        assert!(!store.delete("host"));
        assert_eq!(store.get("host"), None);
    }

    #[test]
    fn compare_and_swap_rejects_stale_writers() {
        let store = AtomicStore::new();
        store.set("counter", vec![0]);
        let version = store.version("counter");
        // A concurrent writer bumps to version+1 first.
        store.set("counter", vec![1]);
        // The stale writer with the old version fails (no lost update).
        assert_eq!(
            store.compare_and_swap("counter", version, vec![99]),
            Err(CasError::VersionMismatch {
                expected: version,
                actual: version + 1,
            })
        );
        assert_eq!(store.get("counter"), Some(vec![1]));
        // Retry with the current version succeeds.
        let current = store.version("counter");
        assert_eq!(
            store.compare_and_swap("counter", current, vec![2]).unwrap(),
            current + 1
        );
        assert_eq!(store.get("counter"), Some(vec![2]));
    }

    #[test]
    fn concurrent_updates_do_not_lose_data() {
        // 8 threads x 250 compare-and-swap increments each: the final value
        // must equal 8*250 (no lost updates).
        let store = Arc::new(AtomicStore::new());
        // Start absent (version 0); every successful CAS bumps the version.
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let store = Arc::clone(&store);
                thread::spawn(move || {
                    for _ in 0..250 {
                        loop {
                            let version = store.version("counter");
                            let value = version as u32;
                            match store.compare_and_swap(
                                "counter",
                                version,
                                (value + 1).to_be_bytes().to_vec(),
                            ) {
                                Ok(_) => break,
                                Err(_) => continue,
                            }
                        }
                    }
                })
            })
            .collect();
        for handle in threads {
            handle.join().expect("thread");
        }
        let final_version = store.version("counter");
        assert_eq!(
            final_version,
            8 * 250,
            "every CAS increment must be preserved"
        );
    }

    #[test]
    fn transaction_commits_atomically_and_detects_conflict() {
        let store = AtomicStore::new();
        store.set("a", b"1".to_vec());
        let mut tx = store.begin();
        tx.write("a", b"2".to_vec());
        tx.write("b", b"3".to_vec());
        // Concurrent change to "a" before commit.
        store.set("a", b"x".to_vec());
        assert_eq!(tx.commit(&store), Err(TransactionError::Conflict));
        assert_eq!(
            store.get("a"),
            Some(b"x".to_vec()),
            "conflicting commit must not apply"
        );
        assert_eq!(store.get("b"), None);

        // A clean transaction commits atomically.
        let mut tx = store.begin();
        tx.write("a", b"10".to_vec());
        tx.write("b", b"20".to_vec());
        tx.delete("a");
        assert_eq!(tx.commit(&store), Ok(()));
        assert_eq!(store.get("a"), None);
        assert_eq!(store.get("b"), Some(b"20".to_vec()));
    }
}
