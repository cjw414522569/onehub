#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # cli
//!
//! CLI entrypoint (T101 adds the startup / database unlock / failure-recovery
//! flow).

pub mod startup;

pub use startup::{
    ActionablePrompt, DatabaseHealth, PromptSeverity, StartupConfig, StartupFlow, StartupOutcome,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "cli";
