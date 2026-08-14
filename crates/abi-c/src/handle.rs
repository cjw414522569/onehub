//! Opaque handle lifecycle and cross-ABI resource ownership (T098).
//!
//! Resources cross the ABI as opaque `u64` handles (never raw pointers).
//! Ownership is creator-owned: [`HandleTable`] hands out handles, and
//! release is **idempotent** — a double release (or a managed runtime's GC /
//! ARC finalizer racing with an explicit release) is a safe no-op, never a
//! use-after-free. Stale handles are rejected by [`HandleTable::contains`] /
//! [`HandleTable::get`] instead of dereferencing freed memory. Dropping the
//! table (process exit) drops every remaining resource, so cancellation and
//! exit leak nothing.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// The reserved "no handle" value. Valid handles are never `0`.
pub const INVALID_HANDLE: u64 = 0;

/// An opaque resource stored behind a handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandleResource {
    /// Resource type discriminator (caller-defined).
    pub resource_type: u32,
    /// Opaque payload (never crosses the ABI).
    pub payload: Vec<u8>,
    /// Whether the operation was cancelled; resources are still released on
    /// exit (cancellation never leaks).
    pub cancelled: bool,
}

/// A handle table with idempotent release and stale-handle rejection.
#[derive(Debug, Clone, Default)]
pub struct HandleTable<T> {
    slots: HashMap<u64, T>,
    next: u64,
}

impl<T> HandleTable<T> {
    /// An empty table.
    pub fn new() -> Self {
        Self {
            slots: HashMap::new(),
            next: 1,
        }
    }

    /// Inserts a resource and returns its opaque handle (never `0`).
    pub fn insert(&mut self, resource: T) -> u64 {
        let handle = self.next;
        self.next = self.next.saturating_add(1);
        self.slots.insert(handle, resource);
        handle
    }

    /// Reads a resource by handle; `None` for stale/unknown handles.
    pub fn get(&self, handle: u64) -> Option<&T> {
        self.slots.get(&handle)
    }

    /// Mutates a resource by handle; `None` for stale/unknown handles.
    pub fn get_mut(&mut self, handle: u64) -> Option<&mut T> {
        self.slots.get_mut(&handle)
    }

    /// Releases a resource. **Idempotent**: the first call returns
    /// `Some(resource)` and removes it; a second call for the same handle
    /// returns `None` (safe no-op, no double-free).
    pub fn remove(&mut self, handle: u64) -> Option<T> {
        self.slots.remove(&handle)
    }

    /// Whether a handle is currently live.
    pub fn contains(&self, handle: u64) -> bool {
        self.slots.contains_key(&handle)
    }

    /// The number of live resources.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether the table holds no resources.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

static REGISTRY: OnceLock<Mutex<HandleTable<HandleResource>>> = OnceLock::new();

fn registry() -> &'static Mutex<HandleTable<HandleResource>> {
    REGISTRY.get_or_init(|| Mutex::new(HandleTable::new()))
}

/// Creates an opaque handle for a resource. Returns [`INVALID_HANDLE`] on
/// failure (never returned for a successful creation).
#[no_mangle]
pub extern "C" fn ssh_abi_handle_create(resource_type: u32, payload_len: u64) -> u64 {
    let resource = HandleResource {
        resource_type,
        payload: vec![0u8; payload_len as usize],
        cancelled: false,
    };
    registry().lock().expect("registry lock").insert(resource)
}

/// Releases a resource idempotently: `0` on the first release, `-1` when the
/// handle is already released or unknown.
#[no_mangle]
pub extern "C" fn ssh_abi_handle_release(handle: u64) -> i32 {
    if registry()
        .lock()
        .expect("registry lock")
        .remove(handle)
        .is_some()
    {
        0
    } else {
        -1
    }
}

/// Whether a handle is live: `1` live, `0` stale/unknown.
#[no_mangle]
pub extern "C" fn ssh_abi_handle_is_valid(handle: u64) -> i32 {
    if registry().lock().expect("registry lock").contains(handle) {
        1
    } else {
        0
    }
}

/// The number of live handles (used by leak/stress checks).
#[no_mangle]
pub extern "C" fn ssh_abi_handle_count() -> u64 {
    registry().lock().expect("registry lock").len() as u64
}

/// Marks a resource cancelled: `0` ok, `-1` stale/unknown. Cancelled
/// resources are still released on exit (cancellation never leaks).
#[no_mangle]
pub extern "C" fn ssh_abi_handle_cancel(handle: u64) -> i32 {
    let mut registry = registry().lock().expect("registry lock");
    match registry.get_mut(handle) {
        Some(resource) => {
            resource.cancelled = true;
            0
        }
        None => -1,
    }
}

/// Whether a resource is cancelled: `1` cancelled, `0` not/unknown.
#[no_mangle]
pub extern "C" fn ssh_abi_handle_is_cancelled(handle: u64) -> i32 {
    let registry = registry().lock().expect("registry lock");
    match registry.get(handle) {
        Some(resource) if resource.cancelled => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{
        registry, ssh_abi_handle_cancel, ssh_abi_handle_count, ssh_abi_handle_create,
        ssh_abi_handle_is_cancelled, ssh_abi_handle_is_valid, ssh_abi_handle_release,
        HandleResource, HandleTable, INVALID_HANDLE,
    };

    /// Serializes the exported (global-registry) ABI tests.
    static ABI_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(test)]
    fn reset_registry() {
        *registry().lock().expect("registry lock") = HandleTable::new();
    }

    #[test]
    fn handle_table_release_is_idempotent() {
        let mut table = HandleTable::new();
        let handle = table.insert(42u32);
        assert_ne!(handle, INVALID_HANDLE);
        assert!(table.contains(handle));
        assert_eq!(table.remove(handle), Some(42));
        // Double release: safe no-op, no double-free.
        assert_eq!(table.remove(handle), None);
        assert!(!table.contains(handle));
        assert_eq!(table.get(handle), None);
        assert!(table.is_empty());
    }

    #[test]
    fn handle_table_drops_resources_on_drop() {
        struct Counted {
            drops: Arc<AtomicUsize>,
        }
        impl Drop for Counted {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let mut table = HandleTable::new();
        for _ in 0..5 {
            table.insert(Counted {
                drops: Arc::clone(&drops),
            });
        }
        assert_eq!(table.len(), 5);
        // Dropping the table (process exit) drops every remaining resource.
        drop(table);
        assert_eq!(
            drops.load(Ordering::SeqCst),
            5,
            "exit must not leak resources"
        );
    }

    #[test]
    fn exported_handle_abi_lifecycle_and_stress() {
        let _guard = ABI_TEST_LOCK.lock().unwrap();
        reset_registry();
        let handle = ssh_abi_handle_create(7, 128);
        assert_ne!(handle, INVALID_HANDLE);
        assert_eq!(ssh_abi_handle_is_valid(handle), 1);
        assert_eq!(ssh_abi_handle_count(), 1);
        assert_eq!(ssh_abi_handle_release(handle), 0);
        // Idempotent release: second call is a safe no-op.
        assert_eq!(ssh_abi_handle_release(handle), -1);
        assert_eq!(ssh_abi_handle_is_valid(handle), 0);
        assert_eq!(ssh_abi_handle_count(), 0);
        // Stale handles are rejected, never dereferenced.
        assert_eq!(ssh_abi_handle_cancel(handle), -1);
        assert_eq!(ssh_abi_handle_is_cancelled(handle), 0);

        // Stress: 10k create/release cycles leave zero live handles.
        for index in 0..10_000 {
            let created = ssh_abi_handle_create(1, 16);
            assert_eq!(ssh_abi_handle_release(created), 0);
            let _ = index;
        }
        assert_eq!(ssh_abi_handle_count(), 0, "stress must not leak handles");
        reset_registry();
    }

    #[test]
    fn exported_handle_abi_cancel_and_exit() {
        let _guard = ABI_TEST_LOCK.lock().unwrap();
        reset_registry();
        let handle = ssh_abi_handle_create(9, 64);
        assert_eq!(ssh_abi_handle_cancel(handle), 0);
        assert_eq!(ssh_abi_handle_is_cancelled(handle), 1);
        // Cancellation never leaks: the resource is still released.
        assert_eq!(ssh_abi_handle_release(handle), 0);
        assert_eq!(ssh_abi_handle_count(), 0);
        // A managed runtime's finalizer may race: releasing again is safe.
        assert_eq!(ssh_abi_handle_release(handle), -1);
        assert_eq!(ssh_abi_handle_count(), 0);
        reset_registry();
    }

    #[test]
    fn handle_ids_are_opaque_and_never_zero() {
        let _guard = ABI_TEST_LOCK.lock().unwrap();
        reset_registry();
        let first = ssh_abi_handle_create(1, 0);
        let second = ssh_abi_handle_create(1, 0);
        assert_ne!(first, INVALID_HANDLE);
        assert_ne!(second, INVALID_HANDLE);
        assert_ne!(first, second, "handles are unique opaque ids");
        ssh_abi_handle_release(first);
        ssh_abi_handle_release(second);
        reset_registry();
    }

    #[test]
    fn resource_payload_stays_opaque() {
        let mut table = HandleTable::new();
        let handle = table.insert(HandleResource {
            resource_type: 3,
            payload: vec![0xAB; 32],
            cancelled: false,
        });
        let resource = table.get(handle).expect("live handle");
        assert_eq!(resource.resource_type, 3);
        assert_eq!(resource.payload.len(), 32);
        assert!(!resource.cancelled);
        table.remove(handle);
    }
}
