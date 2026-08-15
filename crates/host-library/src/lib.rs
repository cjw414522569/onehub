#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # host-library
//!
//! Host library model: list, grouping, tags, search, and sorting (T102).

pub mod auth_prompt;
pub mod editor;
pub mod fingerprint;
pub mod host;
pub mod workspace;

pub use auth_prompt::{
    AuthPrompt, ConfirmState, HardwareConfirmation, KeyOption, KeySelection, PromptError,
    PromptKind, PromptState, SelectionError, SelectionState,
};
pub use editor::{
    default_spec, state_map, AccessibilityReport, FieldKind, FieldSpec, FieldState, HostEditorForm,
    ReviewRow, SectionReview, SectionSpec, SectionState, PASSWORD_MASK,
};
pub use fingerprint::{
    ChangeNotice, FingerprintReview, FingerprintSource, HostKeyFingerprint, KeyAlgorithm,
    ReviewDecision, ReviewState, ReviewView, RiskLevel, SHA256_FINGERPRINT_LEN,
};
pub use host::{
    GroupSummary, HostLibrary, HostRecord, SelectionModel, SortField, SortOrder, TagSummary,
};
pub use workspace::{
    FocusLocation, PaneModel, PaneSnapshot, RestoreError, ShortcutAction, ShortcutMap,
    SplitDirection, TabModel, TabSnapshot, WindowModel, WindowSnapshot, Workspace,
    WorkspaceSnapshot,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "host-library";
