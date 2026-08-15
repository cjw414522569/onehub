#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # host-library
//!
//! Host library model: list, grouping, tags, search, and sorting (T102).

pub mod auth_prompt;
pub mod diagnostics;
pub mod editor;
pub mod file_manager;
pub mod fingerprint;
pub mod gestures;
pub mod host;
pub mod keyboard;
pub mod mobile;
pub mod paste;
pub mod port_forwarding;
pub mod session_status;
pub mod settings;
pub mod snippets;
pub mod transfer_queue;
pub mod workspace;

pub use auth_prompt::{
    AuthPrompt, ConfirmState, HardwareConfirmation, KeyOption, KeySelection, PromptError,
    PromptKind, PromptState, SelectionError, SelectionState,
};
pub use diagnostics::{
    DiagnosticBundle, DiagnosticCategory, DiagnosticExporter, DiagnosticInput, DiagnosticPreview,
    DiagnosticSection, RedactionPolicy, Redactor, REDACTED,
};
pub use editor::{
    default_spec, state_map, AccessibilityReport, FieldKind, FieldSpec, FieldState, HostEditorForm,
    ReviewRow, SectionReview, SectionSpec, SectionState, PASSWORD_MASK,
};
pub use file_manager::{
    ConflictAction, FileKind, FileOperationManager, FilePane, OpError, OpState, RemoteFile,
    TransferKind, TransferOp, TransferProgress,
};
pub use fingerprint::{
    ChangeNotice, FingerprintReview, FingerprintSource, HostKeyFingerprint, KeyAlgorithm,
    ReviewDecision, ReviewState, ReviewView, RiskLevel, SHA256_FINGERPRINT_LEN,
};
pub use gestures::{
    ExtendedKey, ExtendedKeyboard, Gesture, GestureRecognizer, InputMode, KeyChord, TouchPoint,
    LONG_PRESS_MS, SCROLL_THRESHOLD,
};
pub use host::{
    GroupSummary, HostLibrary, HostRecord, SelectionModel, SortField, SortOrder, TagSummary,
};
pub use keyboard::{
    key_label, parse_key, Chord, Direction, KeyAction, KeyBindingConfig, KeyCode, KeyEvent, KeyMap,
    ModifierKey, Modifiers, Platform, PlatformSemantics, PrimaryModifier,
};
pub use mobile::{
    effective_safe_area, BarLayout, BottomActionBar, FormFactor, Orientation, SafeAreaInsets,
    SessionStack, SystemBack, Viewport,
};
pub use paste::{
    PasswordPastePolicy, PasteContent, PasteDecision, PastePayload, PastePolicy, PasteRisk,
    SecurePasteFlow, BRACKETED_PASTE_BEGIN, BRACKETED_PASTE_END,
};
pub use workspace::{
    FocusLocation, PaneModel, PaneSnapshot, RestoreError, ShortcutAction, ShortcutMap,
    SplitDirection, TabModel, TabSnapshot, WindowModel, WindowSnapshot, Workspace,
    WorkspaceSnapshot,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "host-library";
