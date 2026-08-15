#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # host-library
//!
//! Host library model: list, grouping, tags, search, and sorting (T102).

pub mod editor;
pub mod host;

pub use editor::{
    default_spec, state_map, AccessibilityReport, FieldKind, FieldSpec, FieldState, HostEditorForm,
    ReviewRow, SectionReview, SectionSpec, SectionState, PASSWORD_MASK,
};
pub use host::{
    GroupSummary, HostLibrary, HostRecord, SelectionModel, SortField, SortOrder, TagSummary,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "host-library";
