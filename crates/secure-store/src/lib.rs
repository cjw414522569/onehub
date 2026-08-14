#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # secure-store
//!
//! Platform secure-storage adapter (T085): the SecureStore contract and the
//! Windows adapter targeting Credential Manager / DPAPI.

pub mod android;
pub mod apple;
pub mod erasure;
pub mod linux;
pub mod store;

pub use android::{
    select_hardware, AndroidKeystoreStore, HardwareProtection, InvalidationState,
    KeystoreCapabilities, MemoryAndroidKeystore,
};
pub use apple::{
    migrate_keychain, AccessControl, AccessPolicy, AppleKeychainStore, Biometrics,
    MemoryKeychainStore, MigrationOutcome,
};
pub use erasure::{
    forensic_scan, BackupStore, CryptoErasure, DataStore, ErasurePlan, ErasureReport, ErasureScope,
    MemoryBackupStore, MemoryDataStore, ACCOUNT_SECRET_PREFIX,
};
pub use linux::{
    detect_environment, FallbackPolicy, LinuxSecretStore, MemoryLinuxStore, SecretEnvironment,
};
pub use store::{
    MemorySecureStore, ProtectionMechanism, SecureStore, StoreError, SystemCredentialBackend,
    WindowsSecureStore,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "secure-store";
