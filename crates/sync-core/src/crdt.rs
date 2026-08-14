//! Local sync CRDT / conflict-merge core (T093).
//!
//! A per-key last-writer-wins register CRDT with Lamport clocks and
//! tombstones: offline concurrent edits deterministically converge (merge is
//! commutative and idempotent), and deletes are recoverable (a tombstone can
//! be superseded by a concurrent or later set). Random multi-replica property
//! tests verify convergence regardless of merge order.

use std::collections::HashMap;

/// A replica identifier.
pub type ReplicaId = u64;

/// A Lamport clock: (counter, replica id) orders events deterministically.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct LamportClock {
    /// Monotonic counter.
    pub counter: u64,
    /// Replica id (tie-breaker).
    pub replica: ReplicaId,
}

/// A CRDT entry: an optional value plus its version. `None` is a tombstone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrdtEntry {
    /// The value; `None` marks a delete (tombstone).
    pub value: Option<Vec<u8>>,
    /// The version that wrote this entry.
    pub version: LamportClock,
}

/// The CRDT state of one replica.
#[derive(Debug, Clone, Default)]
pub struct CrdtState {
    entries: HashMap<String, CrdtEntry>,
    clock: LamportClock,
}

impl CrdtState {
    /// A fresh state for a replica.
    pub fn new(replica: ReplicaId) -> Self {
        Self {
            entries: HashMap::new(),
            clock: LamportClock {
                counter: 0,
                replica,
            },
        }
    }

    /// The replica id.
    pub fn replica(&self) -> ReplicaId {
        self.clock.replica
    }

    /// Sets a key locally (bumps the clock).
    pub fn set(&mut self, key: &str, value: Vec<u8>) {
        self.clock.counter += 1;
        self.entries.insert(
            key.to_owned(),
            CrdtEntry {
                value: Some(value),
                version: self.clock,
            },
        );
    }

    /// Deletes a key locally by writing a tombstone (recoverable).
    pub fn delete(&mut self, key: &str) {
        self.clock.counter += 1;
        self.entries.insert(
            key.to_owned(),
            CrdtEntry {
                value: None,
                version: self.clock,
            },
        );
    }

    /// Reads a key; `None` when absent or deleted.
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.entries
            .get(key)
            .and_then(|entry| entry.value.as_deref())
    }

    /// Whether a key exists as a tombstone (deleted but recoverable).
    pub fn is_tombstone(&self, key: &str) -> bool {
        matches!(self.entries.get(key), Some(CrdtEntry { value: None, .. }))
    }

    /// Merges another replica's state: per key, the entry with the higher
    /// (counter, replica) version wins. Commutative and idempotent, so any
    /// merge order converges.
    pub fn merge(&mut self, other: &CrdtState) {
        for (key, entry) in &other.entries {
            let take = match self.entries.get(key) {
                Some(local) => entry.version > local.version,
                None => true,
            };
            if take {
                self.entries.insert(key.clone(), entry.clone());
            }
        }
        self.clock.counter = self.clock.counter.max(other.clock.counter);
    }

    /// The number of tracked keys (including tombstones).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the state has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Merges several replicas into one converged state (any order).
pub fn converge<'a>(
    replicas: impl IntoIterator<Item = &'a CrdtState>,
    replica: ReplicaId,
) -> CrdtState {
    let mut merged = CrdtState::new(replica);
    for state in replicas {
        merged.merge(state);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::{converge, CrdtState};

    #[test]
    fn set_get_delete_recover() {
        let mut replica = CrdtState::new(1);
        replica.set("host", b"example.com".to_vec());
        assert_eq!(replica.get("host"), Some(&b"example.com"[..]));
        // Delete writes a tombstone: still recoverable.
        replica.delete("host");
        assert_eq!(replica.get("host"), None);
        assert!(replica.is_tombstone("host"));
        // Re-set restores the value.
        replica.set("host", b"new.example.com".to_vec());
        assert_eq!(replica.get("host"), Some(&b"new.example.com"[..]));
        assert!(!replica.is_tombstone("host"));
    }

    #[test]
    fn offline_concurrent_edits_converge_deterministically() {
        // Two replicas edit offline and merge; the result is deterministic.
        let mut a = CrdtState::new(1);
        let mut b = CrdtState::new(2);
        a.set("x", b"from-a".to_vec());
        b.set("x", b"from-b".to_vec());
        b.set("y", b"only-b".to_vec());
        // Merge in either order -> same state.
        let mut ab = a.clone();
        ab.merge(&b);
        let mut ba = b.clone();
        ba.merge(&a);
        assert_eq!(ab.get("x"), ba.get("x"));
        assert_eq!(ab.get("y"), Some(&b"only-b"[..]));
        assert_eq!(
            ab.get("x"),
            Some(&b"from-b"[..]),
            "higher (counter,replica) wins"
        );
    }

    #[test]
    fn random_multi_replica_property_converges() {
        // Deterministic PRNG; 5 replicas with random edits merged pairwise in
        // random orders must all converge to the same state.
        let mut state = 0xdead_beef_cafe_f00du64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut replicas: Vec<CrdtState> = (0..5).map(CrdtState::new).collect();
        for replica in replicas.iter_mut() {
            for _ in 0..50 {
                let key = format!("k{}", next() % 8);
                if next() % 4 == 0 {
                    replica.delete(&key);
                } else {
                    replica.set(&key, next().to_be_bytes().to_vec());
                }
            }
        }
        // Merge replicas pairwise in several random orders; the result must
        // be identical every time.
        let mut orders = vec![
            vec![0usize, 1, 2, 3, 4],
            vec![4, 3, 2, 1, 0],
            vec![2, 4, 0, 3, 1],
        ];
        let _ = &mut orders;
        let first = converge(replicas.iter().rev(), 99);
        for order in orders {
            let mut merged = CrdtState::new(99);
            for index in order {
                merged.merge(&replicas[index]);
            }
            assert_eq!(merged.len(), first.len(), "converged length must match");
            for (key, entry) in first.entries.iter() {
                assert_eq!(
                    merged.entries.get(key),
                    Some(entry),
                    "replica {key} must converge to the same entry"
                );
            }
        }
    }

    #[test]
    fn delete_is_recoverable_under_concurrency() {
        // A deletes K; B concurrently sets K to a newer version -> after
        // merge, the set wins (delete recoverable / not sticky).
        let mut a = CrdtState::new(1);
        let mut b = CrdtState::new(2);
        a.set("k", b"v0".to_vec());
        b.merge(&a);
        a.delete("k");
        b.set("k", b"v2".to_vec());
        let mut merged = a;
        merged.merge(&b);
        assert_eq!(
            merged.get("k"),
            Some(&b"v2"[..]),
            "newer set must win over delete"
        );
    }
}
