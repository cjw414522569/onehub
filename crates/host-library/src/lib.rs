#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # host-library
//!
//! Host library model: list, grouping, tags, search, and sorting (T102).

pub mod accessibility;
pub mod android_integration;
pub mod android_security;
pub mod auth_prompt;
pub mod backend_gate;
pub mod command_palette;
pub mod diagnostics;
pub mod editor;
pub mod file_manager;
pub mod fingerprint;
pub mod gestures;
pub mod host;
pub mod ios_integration;
pub mod keyboard;
pub mod linux_integration;
pub mod macos_entitlements;
pub mod macos_integration;
pub mod mobile;
pub mod paste;
pub mod port_forwarding;
pub mod session_status;
pub mod settings;
pub mod snippets;
pub mod transfer_queue;
pub mod windows_integration;
pub mod workspace;

pub use accessibility::{
    screen_reader_checklist, A11yNode, A11yRole, A11yTree, A11yViolation, ChecklistItem,
    MotionPreference, ReduceMotionPolicy, TerminalAccessibleMode, ViolationSeverity,
};
pub use android_integration::{AppState, LifecycleModel, NetworkState};
pub use android_security::{
    BiometricPrompt, BiometricState, FileSelection, KeyImport, KeyImportFlow, ShareSheet,
};
pub use auth_prompt::{
    AuthPrompt, ConfirmState, HardwareConfirmation, KeyOption, KeySelection, PromptError,
    PromptKind, PromptState, SelectionError, SelectionState,
};
pub use backend_gate::{
    BackendComparison, BackendGate, BackendSelection, Feature, FeatureSupport, TerminalBackend,
};
pub use command_palette::{CommandPalette, FlowKey, KeyboardFlow, PaletteAction, PaletteCommand};
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
pub use ios_integration::{RecoveryState, Scene, SceneCollection, SceneState, SuspensionModel};
pub use keyboard::{
    key_label, parse_key, Chord, Direction, KeyAction, KeyBindingConfig, KeyCode, KeyEvent, KeyMap,
    ModifierKey, Modifiers, Platform, PlatformSemantics, PrimaryModifier,
};
pub use linux_integration::{
    DesktopEntry, DesktopEnvironment, DisplayServer, LinuxNotification, NoKeyringPolicy,
    ScalingPolicy, SecretServiceState,
};
pub use macos_entitlements::{AuditIssue, Entitlement, EntitlementSet, NotarizationAudit};
pub use macos_integration::{
    AppNapPolicy, MacArch, MacMenu, MacMenuAction, MacNotification, RetinaScale,
};
pub use mobile::{
    effective_safe_area, BarLayout, BottomActionBar, FormFactor, Orientation, SafeAreaInsets,
    SessionStack, SystemBack, Viewport,
};
pub use paste::{
    PasswordPastePolicy, PasteContent, PasteDecision, PastePayload, PastePolicy, PasteRisk,
    SecurePasteFlow, BRACKETED_PASTE_BEGIN, BRACKETED_PASTE_END,
};
pub use windows_integration::{
    parse_ssh_link, DpiContext, LinkError, Monitor, MonitorLayout, ProtocolLink, Rect, Size,
    SleepWakePolicy, TrayAction, WindowsArch, WindowsNotification,
};
pub use workspace::{
    FocusLocation, PaneModel, PaneSnapshot, RestoreError, ShortcutAction, ShortcutMap,
    SplitDirection, TabModel, TabSnapshot, WindowModel, WindowSnapshot, Workspace,
    WorkspaceSnapshot,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "host-library";
