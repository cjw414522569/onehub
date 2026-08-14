#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # secure-store
//!
//! Platform secure-storage adapter (T085): the SecureStore contract and the
//! Windows adapter targeting Credential Manager / DPAPI.

pub mod store;

pub use store::{
    MemorySecureStore, ProtectionMechanism, SecureStore, StoreError, SystemCredentialBackend,
    WindowsSecureStore,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "secure-store";
