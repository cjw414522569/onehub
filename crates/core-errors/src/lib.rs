#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # core-errors
//!
//! Stable, language-neutral error identifiers and metadata that cross FFI
//! boundaries without language exceptions or sensitive context.

pub mod code;
pub mod info;
pub mod mapping;

pub use code::{ErrorCode, Recoverability, RetrySuggestion};
pub use info::{ErrorInfo, MessageParam};
pub use mapping::all_domain_errors;

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "core-errors";
