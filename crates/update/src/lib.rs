#![forbid(unsafe_code)]

//! # update
//!
//! Automatic update and rollback for Windows / macOS / Linux (T166): signed
//! update metadata, anti-downgrade, staged rollout, and failure rollback.

pub mod updater;

pub use updater::{
    DigestVerifier, SignatureVerifier, StagedRollout, UpdateCoordinator, UpdateError,
    UpdateManifest, Version,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "update";
