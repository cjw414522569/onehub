//! Windows ConPTY / system OpenSSH compatible-backend capability gate (T122).
//!
//! [`BackendGate`] only enables the system OpenSSH backend when it is
//! **explicitly selected**; the built-in backend is the default and the
//! system backend is never enabled implicitly. [`BackendComparison`] exposes
//! the feature-support matrix so behavior differences between the built-in
//! and the system backend are visible to the user (e.g. true color,
//! bracketed paste, mouse, OSC 52).

/// A terminal backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalBackend {
    /// The built-in terminal engine (own ConPTY / renderer).
    BuiltIn,
    /// The system OpenSSH `ssh.exe` backend.
    SystemOpenSsh,
}

/// Whether a backend was explicitly selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendSelection {
    /// Nothing explicitly selected (built-in is the default).
    NotSelected,
    /// The user explicitly selected a backend.
    ExplicitlySelected(TerminalBackend),
}

/// The capability gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendGate {
    /// The explicit selection, if any.
    pub selection: BackendSelection,
}

impl BackendGate {
    /// The default gate: built-in active, system backend not selected.
    pub fn new() -> Self {
        Self {
            selection: BackendSelection::NotSelected,
        }
    }

    /// Explicitly selects a backend (the only way to enable the system one).
    pub fn select(&mut self, backend: TerminalBackend) {
        self.selection = BackendSelection::ExplicitlySelected(backend);
    }

    /// Clears the selection (back to the built-in default).
    pub fn reset(&mut self) {
        self.selection = BackendSelection::NotSelected;
    }

    /// The active backend: built-in unless the system backend was explicitly
    /// selected.
    pub fn active_backend(&self) -> TerminalBackend {
        match self.selection {
            BackendSelection::ExplicitlySelected(TerminalBackend::SystemOpenSsh) => {
                TerminalBackend::SystemOpenSsh
            }
            _ => TerminalBackend::BuiltIn,
        }
    }

    /// Whether the system OpenSSH backend is enabled (only on explicit
    /// selection, never implicitly).
    pub fn system_enabled(&self) -> bool {
        self.active_backend() == TerminalBackend::SystemOpenSsh
    }
}

impl Default for BackendGate {
    fn default() -> Self {
        Self::new()
    }
}

/// A terminal feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// UTF-8.
    Utf8,
    /// True color (24-bit).
    TrueColor,
    /// Bracketed paste.
    BracketedPaste,
    /// Mouse reporting.
    Mouse,
    /// Resize.
    Resize,
    /// Correct Unicode width.
    UnicodeWidth,
    /// OSC 52 clipboard.
    Osc52Clipboard,
    /// Bell.
    Bell,
}

impl Feature {
    /// A human label.
    pub fn label(&self) -> &'static str {
        match self {
            Feature::Utf8 => "UTF-8",
            Feature::TrueColor => "true color",
            Feature::BracketedPaste => "bracketed paste",
            Feature::Mouse => "mouse reporting",
            Feature::Resize => "resize",
            Feature::UnicodeWidth => "unicode width",
            Feature::Osc52Clipboard => "OSC 52 clipboard",
            Feature::Bell => "bell",
        }
    }
}

/// Feature support for both backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureSupport {
    /// The feature.
    pub feature: Feature,
    /// Whether the built-in backend supports it.
    pub built_in: bool,
    /// Whether the system OpenSSH backend supports it.
    pub system_open_ssh: bool,
}

impl FeatureSupport {
    /// Whether support differs between the backends.
    pub fn differs(&self) -> bool {
        self.built_in != self.system_open_ssh
    }
}

/// The backend comparison (the visible behavior-difference surface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendComparison {
    /// The support matrix.
    pub rows: Vec<FeatureSupport>,
}

impl BackendComparison {
    /// The known support matrix for the built-in vs system backends.
    pub fn compare() -> Self {
        let rows = vec![
            FeatureSupport {
                feature: Feature::Utf8,
                built_in: true,
                system_open_ssh: true,
            },
            FeatureSupport {
                feature: Feature::TrueColor,
                built_in: true,
                system_open_ssh: false,
            },
            FeatureSupport {
                feature: Feature::BracketedPaste,
                built_in: true,
                system_open_ssh: false,
            },
            FeatureSupport {
                feature: Feature::Mouse,
                built_in: true,
                system_open_ssh: false,
            },
            FeatureSupport {
                feature: Feature::Resize,
                built_in: true,
                system_open_ssh: true,
            },
            FeatureSupport {
                feature: Feature::UnicodeWidth,
                built_in: true,
                system_open_ssh: false,
            },
            FeatureSupport {
                feature: Feature::Osc52Clipboard,
                built_in: true,
                system_open_ssh: false,
            },
            FeatureSupport {
                feature: Feature::Bell,
                built_in: true,
                system_open_ssh: true,
            },
        ];
        Self { rows }
    }

    /// The rows where support differs (visible to the user).
    pub fn differences(&self) -> Vec<FeatureSupport> {
        self.rows
            .iter()
            .copied()
            .filter(FeatureSupport::differs)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{BackendComparison, BackendGate, BackendSelection, Feature, TerminalBackend};

    #[test]
    fn system_backend_is_only_enabled_when_explicitly_selected() {
        let mut gate = BackendGate::new();
        // Default: built-in active, system never implicitly enabled.
        assert_eq!(gate.active_backend(), TerminalBackend::BuiltIn);
        assert!(!gate.system_enabled());
        // Explicitly selecting the system backend enables it.
        gate.select(TerminalBackend::SystemOpenSsh);
        assert!(gate.system_enabled());
        assert_eq!(gate.active_backend(), TerminalBackend::SystemOpenSsh);
        assert_eq!(
            gate.selection,
            BackendSelection::ExplicitlySelected(TerminalBackend::SystemOpenSsh)
        );
        // Reset returns to the built-in default.
        gate.reset();
        assert!(!gate.system_enabled());
        assert_eq!(gate.active_backend(), TerminalBackend::BuiltIn);
        // Explicitly selecting the built-in also stays built-in.
        gate.select(TerminalBackend::BuiltIn);
        assert!(!gate.system_enabled());
    }

    #[test]
    fn behavior_differences_are_visible() {
        let comparison = BackendComparison::compare();
        assert_eq!(comparison.rows.len(), 8);
        let differences = comparison.differences();
        // True color, bracketed paste, mouse, unicode width, and OSC 52 all
        // differ; the differences are visible to the user.
        assert!(differences.len() >= 5);
        for row in &differences {
            assert!(row.differs());
        }
        assert!(differences
            .iter()
            .any(|row| row.feature == Feature::BracketedPaste && !row.system_open_ssh));
        assert!(differences
            .iter()
            .any(|row| row.feature == Feature::TrueColor && !row.system_open_ssh));
        // Shared features are not reported as differences.
        assert!(comparison
            .rows
            .iter()
            .all(|row| row.feature != Feature::Resize || !row.differs()));
    }

    #[test]
    fn feature_labels_are_readable() {
        assert_eq!(Feature::BracketedPaste.label(), "bracketed paste");
        assert_eq!(Feature::TrueColor.label(), "true color");
    }
}
