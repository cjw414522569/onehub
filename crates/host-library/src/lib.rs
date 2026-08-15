#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # host-library
//!
//! Host library model: list, grouping, tags, search, and sorting (T102).

pub mod host;

pub use host::{
    GroupSummary, HostLibrary, HostRecord, SelectionModel, SortField, SortOrder, TagSummary,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "host-library";
