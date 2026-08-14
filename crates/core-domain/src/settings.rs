use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// Scope of a settings entry.
///
/// Window-level settings apply to a single window, account-level settings to
/// the signed-in account, and global settings to every window on the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SettingsScope {
    /// Machine-wide default.
    Global,
    /// Per-account.
    Account,
    /// Per-window.
    Window,
}

/// A settings value (typed, non-sensitive by construction policy).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingsValue {
    /// Boolean.
    Bool(bool),
    /// Integer.
    Int(i64),
    /// String.
    String(String),
    /// String list.
    StringList(Vec<String>),
}

/// A single settings entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsEntry {
    /// Setting key.
    pub key: String,
    /// Value.
    pub value: SettingsValue,
    /// Scope.
    pub scope: SettingsScope,
}

/// Local settings store.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSettings {
    entries: Vec<SettingsEntry>,
}

/// Current settings schema version.
pub const SETTINGS_SCHEMA_VERSION: u32 = 2;

impl LocalSettings {
    /// An empty settings store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The built-in defaults (global scope).
    pub fn defaults() -> Self {
        Self {
            entries: vec![
                SettingsEntry {
                    key: "terminal.scrollback_lines".to_owned(),
                    value: SettingsValue::Int(10_000),
                    scope: SettingsScope::Global,
                },
                SettingsEntry {
                    key: "terminal.font_size".to_owned(),
                    value: SettingsValue::Int(12),
                    scope: SettingsScope::Global,
                },
                SettingsEntry {
                    key: "security.confirm_close".to_owned(),
                    value: SettingsValue::Bool(true),
                    scope: SettingsScope::Global,
                },
            ],
        }
    }

    /// Adds or replaces an entry.
    pub fn set(&mut self, entry: SettingsEntry) {
        self.entries
            .retain(|existing| !(existing.key == entry.key && existing.scope == entry.scope));
        self.entries.push(entry);
    }

    /// Returns the most specific value for a key using scope precedence
    /// (window > account > global).
    pub fn effective(&self, key: &str) -> Option<&SettingsValue> {
        let mut best: Option<&SettingsEntry> = None;
        for entry in &self.entries {
            if entry.key != key {
                continue;
            }
            let better = match (best, entry.scope) {
                (None, _) => true,
                (Some(current), scope) => scope_precedence(scope) > scope_precedence(current.scope),
            };
            if better {
                best = Some(entry);
            }
        }
        best.map(|entry| &entry.value)
    }

    /// Returns the value at exactly one scope.
    pub fn get_at_scope(&self, key: &str, scope: SettingsScope) -> Option<&SettingsValue> {
        self.entries
            .iter()
            .find(|entry| entry.key == key && entry.scope == scope)
            .map(|entry| &entry.value)
    }

    /// Returns all entries.
    pub fn entries(&self) -> &[SettingsEntry] {
        &self.entries
    }
}

fn scope_precedence(scope: SettingsScope) -> u8 {
    match scope {
        SettingsScope::Global => 0,
        SettingsScope::Account => 1,
        SettingsScope::Window => 2,
    }
}

/// A versioned settings document with migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsDocument {
    /// Schema version.
    pub schema_version: u32,
    /// Settings.
    #[serde(default)]
    pub settings: LocalSettings,
}

impl SettingsDocument {
    /// A fresh document with defaults at the current version.
    pub fn fresh() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            settings: LocalSettings::defaults(),
        }
    }
}

/// Migrates a settings document to the current schema version, applying
/// defaults for missing global keys. Idempotent at the current version.
pub fn migrate_settings(document: SettingsDocument) -> SettingsDocument {
    let mut settings = document.settings;
    if document.schema_version < SETTINGS_SCHEMA_VERSION {
        for default in LocalSettings::defaults().entries() {
            if settings
                .get_at_scope(&default.key, SettingsScope::Global)
                .is_none()
            {
                settings.set(default.clone());
            }
        }
    }
    SettingsDocument {
        schema_version: SETTINGS_SCHEMA_VERSION,
        settings,
    }
}

/// Stable tab identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TabId(pub String);

impl TabId {
    /// Creates a tab id.
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DomainError::EmptyId);
        }
        Ok(Self(value))
    }
}

/// A terminal tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    /// Tab id.
    pub id: TabId,
    /// Display title.
    pub title: String,
    /// Optional session profile id.
    pub profile_id: Option<String>,
}

impl Tab {
    /// Creates a tab.
    pub fn new(id: TabId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            profile_id: None,
        }
    }

    /// Attaches a session profile.
    pub fn with_profile(mut self, profile_id: impl Into<String>) -> Self {
        self.profile_id = Some(profile_id.into());
        self
    }
}

/// Split direction for a layout node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SplitDirection {
    /// Side-by-side.
    Horizontal,
    /// Stacked.
    Vertical,
}

/// A layout tree: leaves are tabs, internal nodes are splits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutNode {
    /// A tab leaf.
    Leaf { tab: Tab },
    /// A split with children.
    Split {
        /// Split direction.
        direction: SplitDirection,
        /// Child nodes (at least two).
        children: Vec<LayoutNode>,
    },
}

impl LayoutNode {
    /// Returns all tab leaves in traversal order.
    pub fn tabs(&self) -> Vec<&Tab> {
        let mut result = Vec::new();
        self.collect_tabs(&mut result);
        result
    }

    fn collect_tabs<'a>(&'a self, out: &mut Vec<&'a Tab>) {
        match self {
            LayoutNode::Leaf { tab } => out.push(tab),
            LayoutNode::Split { children, .. } => {
                for child in children {
                    child.collect_tabs(out);
                }
            }
        }
    }
}

/// A window layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowLayout {
    /// Window id.
    pub id: String,
    /// Layout tree root.
    pub root: LayoutNode,
    /// Active tab id.
    pub active_tab_id: TabId,
}

/// A workspace (collection of windows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// Workspace id.
    pub id: String,
    /// Windows.
    pub windows: Vec<WindowLayout>,
}

#[cfg(test)]
mod tests {
    use super::{
        migrate_settings, LayoutNode, LocalSettings, SettingsDocument, SettingsEntry,
        SettingsScope, SettingsValue, SplitDirection, Tab, TabId, WindowLayout, Workspace,
        SETTINGS_SCHEMA_VERSION,
    };

    #[test]
    fn defaults_are_applied_and_effective() {
        let settings = LocalSettings::defaults();
        assert_eq!(
            settings.effective("terminal.scrollback_lines"),
            Some(&SettingsValue::Int(10_000))
        );
        assert_eq!(settings.effective("missing.key"), None);
    }

    #[test]
    fn scope_precedence_resolves_window_over_account_over_global() {
        let mut settings = LocalSettings::defaults();
        settings.set(SettingsEntry {
            key: "terminal.font_size".to_owned(),
            value: SettingsValue::Int(14),
            scope: SettingsScope::Account,
        });
        settings.set(SettingsEntry {
            key: "terminal.font_size".to_owned(),
            value: SettingsValue::Int(16),
            scope: SettingsScope::Window,
        });
        assert_eq!(
            settings.effective("terminal.font_size"),
            Some(&SettingsValue::Int(16))
        );
        assert_eq!(
            settings.get_at_scope("terminal.font_size", SettingsScope::Account),
            Some(&SettingsValue::Int(14))
        );
        assert_eq!(
            settings.get_at_scope("terminal.font_size", SettingsScope::Global),
            Some(&SettingsValue::Int(12))
        );
    }

    #[test]
    fn account_scope_overrides_global_without_window() {
        let mut settings = LocalSettings::new();
        settings.set(SettingsEntry {
            key: "theme".to_owned(),
            value: SettingsValue::String("dark".to_owned()),
            scope: SettingsScope::Global,
        });
        settings.set(SettingsEntry {
            key: "theme".to_owned(),
            value: SettingsValue::String("light".to_owned()),
            scope: SettingsScope::Account,
        });
        assert_eq!(
            settings.effective("theme"),
            Some(&SettingsValue::String("light".to_owned()))
        );
    }

    #[test]
    fn migration_applies_defaults_and_bumps_version() {
        let old = SettingsDocument {
            schema_version: 1,
            settings: LocalSettings::new(),
        };
        let migrated = migrate_settings(old);
        assert_eq!(migrated.schema_version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(
            migrated.settings.effective("terminal.scrollback_lines"),
            Some(&SettingsValue::Int(10_000))
        );
        assert_eq!(
            migrated.settings.effective("security.confirm_close"),
            Some(&SettingsValue::Bool(true))
        );
    }

    #[test]
    fn migration_preserves_user_settings() {
        let mut settings = LocalSettings::new();
        settings.set(SettingsEntry {
            key: "terminal.font_size".to_owned(),
            value: SettingsValue::Int(18),
            scope: SettingsScope::Account,
        });
        let migrated = migrate_settings(SettingsDocument {
            schema_version: 1,
            settings,
        });
        assert_eq!(
            migrated.settings.effective("terminal.font_size"),
            Some(&SettingsValue::Int(18))
        );
    }

    #[test]
    fn migration_is_idempotent() {
        let first = migrate_settings(SettingsDocument {
            schema_version: 1,
            settings: LocalSettings::new(),
        });
        let second = migrate_settings(first.clone());
        assert_eq!(first, second);
    }

    #[test]
    fn layout_tree_exposes_tabs_in_order() {
        let tab_a = Tab::new(TabId::new("a").expect("id"), "A");
        let tab_b = Tab::new(TabId::new("b").expect("id"), "B").with_profile("profile-1");
        let tab_c = Tab::new(TabId::new("c").expect("id"), "C");
        let root = LayoutNode::Split {
            direction: SplitDirection::Horizontal,
            children: vec![
                LayoutNode::Leaf { tab: tab_a },
                LayoutNode::Split {
                    direction: SplitDirection::Vertical,
                    children: vec![
                        LayoutNode::Leaf { tab: tab_b },
                        LayoutNode::Leaf { tab: tab_c },
                    ],
                },
            ],
        };
        let tabs = root.tabs();
        assert_eq!(tabs.len(), 3);
        assert_eq!(tabs[0].id, TabId("a".to_owned()));
        assert_eq!(tabs[1].profile_id.as_deref(), Some("profile-1"));

        let window = WindowLayout {
            id: "w1".to_owned(),
            root,
            active_tab_id: TabId("b".to_owned()),
        };
        let workspace = Workspace {
            id: "ws-1".to_owned(),
            windows: vec![window],
        };
        assert_eq!(workspace.windows.len(), 1);
        assert_eq!(workspace.windows[0].active_tab_id, TabId("b".to_owned()));
    }

    #[test]
    fn settings_and_layout_serde_round_trip() {
        let document = SettingsDocument::fresh();
        let json = serde_json::to_string(&document).expect("serialize document");
        let decoded: SettingsDocument = serde_json::from_str(&json).expect("deserialize document");
        assert_eq!(decoded, document);

        let workspace = Workspace {
            id: "ws-2".to_owned(),
            windows: vec![WindowLayout {
                id: "w2".to_owned(),
                root: LayoutNode::Leaf {
                    tab: Tab::new(TabId::new("t1").expect("id"), "T1"),
                },
                active_tab_id: TabId("t1".to_owned()),
            }],
        };
        let json = serde_json::to_string(&workspace).expect("serialize workspace");
        let decoded: Workspace = serde_json::from_str(&json).expect("deserialize workspace");
        assert_eq!(decoded, workspace);
    }
}
