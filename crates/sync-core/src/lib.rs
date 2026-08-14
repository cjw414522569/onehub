#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # sync-core
//!
//! Synchronization-agnostic merge/conflict contracts plus the optional
//! end-to-end encrypted sync protocol design (T092).

pub mod crdt;
pub mod sync_protocol;

pub use crdt::{converge, CrdtEntry, CrdtState, LamportClock, ReplicaId};
pub use sync_protocol::{
    decrypt_envelope, encrypt_envelope, DeviceIdentity, RevocationList, RotateKey, SyncEnvelope,
    TestVector, ThreatModel, SYNC_PROTOCOL_VERSION,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "sync-core";
