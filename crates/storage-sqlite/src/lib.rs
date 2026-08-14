#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # storage-sqlite
//!
//! SQLite persistence adapter: schema versioning, migration, backup, and
//! downgrade policy (T083).

pub mod migration;
pub mod repository;

pub use migration::{
    open_strategy, BackupMode, BackupPolicy, Migration, MigrationContext, MigrationError, Migrator,
    OpenDecision, OpenPolicy, SchemaVersion,
};
pub use repository::{
    AtomicStore, AtomicTransaction, CasError, ConfigRepository, TransactionError,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "storage-sqlite";
