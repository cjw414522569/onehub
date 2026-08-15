//! Accessibility semantics, screen readers, and reduce-motion (T119).
//!
//! [`A11yTree`] is a semantic tree (roles + names + states) consumed by
//! platform screen readers. [`A11yTree::audit`] runs the WCAG 2.2 AA
//! critical-path checks that are modelable here: every interactive node has
//! an accessible name (4.1.2), focus order is deterministic (2.4.3), and
//! interactive roles are keyboard-reachable (2.1.1). [`ReduceMotionPolicy`]
//! disables animation / smooth scrolling / cursor blink when the OS requests
//! reduced motion (2.3.3). [`TerminalAccessibleMode`] exposes the visible
//! screen as a screen-reader text buffer with a cursor announcement.

/// The severity of an accessibility violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViolationSeverity {
    /// WCAG A / AA critical-path.
    Critical,
    /// Best-effort.
    Warning,
}

/// A role in the semantic tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A11yRole {
    /// Button.
    Button,
    /// Checkbox.
    Checkbox,
    /// Text field.
    TextField,
    /// Plain text.
    Text,
    /// List.
    List,
    /// Tab.
    Tab,
    /// Link.
    Link,
    /// Heading.
    Heading,
}

impl A11yRole {
    /// Whether the role is interactive (must have an accessible name).
    pub fn interactive(&self) -> bool {
        matches!(
            self,
            A11yRole::Button
                | A11yRole::Checkbox
                | A11yRole::TextField
                | A11yRole::Tab
                | A11yRole::Link
        )
    }
}

/// A semantic node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A11yNode {
    /// Stable id.
    pub id: u64,
    /// The role.
    pub role: A11yRole,
    /// The accessible label.
    pub label: String,
    /// An optional value (e.g. a text field's content).
    pub value: Option<String>,
    /// Disabled state.
    pub disabled: bool,
    /// Focused state.
    pub focused: bool,
    /// Selected state.
    pub selected: bool,
}

impl A11yNode {
    /// The accessible name (label, falling back to the value).
    pub fn accessible_name(&self) -> String {
        if !self.label.trim().is_empty() {
            self.label.clone()
        } else {
            self.value.clone().unwrap_or_default()
        }
    }
}

/// An accessibility audit finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A11yViolation {
    /// Severity.
    pub severity: ViolationSeverity,
    /// A stable code.
    pub code: &'static str,
    /// The node id (if any).
    pub node_id: Option<u64>,
    /// A human message.
    pub message: String,
}

/// The semantic tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A11yTree {
    /// Nodes in document order.
    pub nodes: Vec<A11yNode>,
}

impl A11yTree {
    /// A tree over the given nodes.
    pub fn new(nodes: Vec<A11yNode>) -> Self {
        Self { nodes }
    }

    /// The deterministic focus order (document order, interactive first).
    pub fn focus_order(&self) -> Vec<u64> {
        let mut interactive: Vec<u64> = self
            .nodes
            .iter()
            .filter(|node| node.role.interactive() && !node.disabled)
            .map(|node| node.id)
            .collect();
        let mut others: Vec<u64> = self
            .nodes
            .iter()
            .filter(|node| !node.role.interactive() || node.disabled)
            .map(|node| node.id)
            .collect();
        interactive.append(&mut others);
        interactive
    }

    /// Runs the WCAG 2.2 AA critical-path audit that is modelable here.
    pub fn audit(&self) -> Vec<A11yViolation> {
        let mut violations = Vec::new();
        // 4.1.2 Name: interactive nodes need an accessible name.
        for node in &self.nodes {
            if node.role.interactive() && node.accessible_name().trim().is_empty() {
                violations.push(A11yViolation {
                    severity: ViolationSeverity::Critical,
                    code: "WCAG_4_1_2_NAME",
                    node_id: Some(node.id),
                    message: format!("interactive {:?} has no accessible name", node.role),
                });
            }
        }
        // 2.4.3 Focus order: every node appears exactly once.
        let order = self.focus_order();
        let unique: std::collections::HashSet<u64> = order.iter().copied().collect();
        if unique.len() != self.nodes.len() {
            violations.push(A11yViolation {
                severity: ViolationSeverity::Critical,
                code: "WCAG_2_4_3_FOCUS_ORDER",
                node_id: None,
                message: "focus order must contain every node exactly once".to_owned(),
            });
        }
        // 2.1.1 Keyboard: no interactive node may be unfocusable.
        for node in &self.nodes {
            if node.role.interactive() && node.disabled {
                violations.push(A11yViolation {
                    severity: ViolationSeverity::Warning,
                    code: "WCAG_2_1_1_KEYBOARD",
                    node_id: Some(node.id),
                    message: "interactive node is disabled (unreachable by keyboard)".to_owned(),
                });
            }
        }
        violations
    }
}

/// The OS motion preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionPreference {
    /// No preference.
    NoPreference,
    /// The user requested reduced motion.
    Reduce,
}

/// The reduce-motion policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReduceMotionPolicy {
    /// Whether animations are enabled.
    pub animations_enabled: bool,
    /// Whether smooth scrolling is enabled.
    pub smooth_scrolling: bool,
    /// Whether the cursor blinks.
    pub cursor_blink: bool,
}

impl ReduceMotionPolicy {
    /// The policy for a motion preference (WCAG 2.3.3).
    pub fn for_preference(preference: MotionPreference) -> Self {
        match preference {
            MotionPreference::NoPreference => Self {
                animations_enabled: true,
                smooth_scrolling: true,
                cursor_blink: true,
            },
            MotionPreference::Reduce => Self {
                animations_enabled: false,
                smooth_scrolling: false,
                cursor_blink: false,
            },
        }
    }
}

/// The terminal's accessible mode.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerminalAccessibleMode {
    /// Whether accessible mode is enabled.
    pub enabled: bool,
    /// The visible screen lines as text.
    pub buffer: Vec<String>,
    /// The cursor (row, column), 1-based.
    pub cursor: (u16, u16),
}

impl TerminalAccessibleMode {
    /// Enables accessible mode.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Sets the visible screen (line by line).
    pub fn set_screen(&mut self, lines: &[String]) {
        self.buffer = lines.to_vec();
    }

    /// Sets the cursor position (1-based row, column).
    pub fn set_cursor(&mut self, row: u16, column: u16) {
        self.cursor = (row, column);
    }

    /// The screen-reader text: the buffer plus a cursor announcement.
    pub fn screen_reader_text(&self) -> String {
        let mut text = String::new();
        for (index, line) in self.buffer.iter().enumerate() {
            text.push_str(&format!("line {}: {}\n", index + 1, line));
        }
        text.push_str(&format!(
            "cursor at row {}, column {}",
            self.cursor.0, self.cursor.1
        ));
        text
    }
}

/// A screen-reader checklist item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecklistItem {
    /// The screen reader (VoiceOver / NVDA / TalkBack).
    pub reader: &'static str,
    /// The check description.
    pub description: String,
    /// Whether it passed here (automated) or must run on a native host.
    pub automated: bool,
}

/// The screen-reader checklist (automated parts here; VoiceOver / NVDA /
/// TalkBack live checks run on native hosts).
pub fn screen_reader_checklist() -> Vec<ChecklistItem> {
    vec![
        ChecklistItem {
            reader: "all",
            description: "interactive nodes have accessible names (WCAG 4.1.2)".to_owned(),
            automated: true,
        },
        ChecklistItem {
            reader: "all",
            description: "focus order is deterministic (WCAG 2.4.3)".to_owned(),
            automated: true,
        },
        ChecklistItem {
            reader: "all",
            description: "reduced motion disables animation (WCAG 2.3.3)".to_owned(),
            automated: true,
        },
        ChecklistItem {
            reader: "all",
            description: "terminal accessible mode announces screen and cursor".to_owned(),
            automated: true,
        },
        ChecklistItem {
            reader: "VoiceOver",
            description: "read current line / word / navigate by line on macOS/iOS".to_owned(),
            automated: false,
        },
        ChecklistItem {
            reader: "NVDA",
            description: "read current line / word / navigate by line on Windows".to_owned(),
            automated: false,
        },
        ChecklistItem {
            reader: "TalkBack",
            description: "read current line / word / navigate by line on Android".to_owned(),
            automated: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        screen_reader_checklist, A11yNode, A11yRole, A11yTree, ChecklistItem, MotionPreference,
        ReduceMotionPolicy, TerminalAccessibleMode, ViolationSeverity,
    };

    fn node(id: u64, role: A11yRole, label: &str) -> A11yNode {
        A11yNode {
            id,
            role,
            label: label.to_owned(),
            value: None,
            disabled: false,
            focused: false,
            selected: false,
        }
    }

    #[test]
    fn audit_finds_missing_names_and_keyboard_issues() {
        // An unnamed button is a critical 4.1.2 violation.
        let unnamed = node(1, A11yRole::Button, "");
        let tree = A11yTree::new(vec![unnamed]);
        let violations = tree.audit();
        assert!(violations
            .iter()
            .any(|v| v.code == "WCAG_4_1_2_NAME" && v.severity == ViolationSeverity::Critical));
        // A disabled interactive node is a keyboard warning.
        let mut disabled = node(2, A11yRole::Button, "OK");
        disabled.disabled = true;
        let tree = A11yTree::new(vec![disabled]);
        assert!(tree.audit().iter().any(|v| v.code == "WCAG_2_1_1_KEYBOARD"));
        // A fully named, focusable tree passes.
        let tree = A11yTree::new(vec![
            node(1, A11yRole::Button, "Connect"),
            node(2, A11yRole::TextField, "Host"),
            node(3, A11yRole::Text, "No hosts yet"),
        ]);
        assert!(tree.audit().is_empty());
    }

    #[test]
    fn focus_order_is_deterministic_and_complete() {
        let tree = A11yTree::new(vec![
            node(1, A11yRole::Text, "a"),
            node(2, A11yRole::Button, "b"),
            node(3, A11yRole::TextField, "c"),
            node(4, A11yRole::List, "d"),
        ]);
        // Interactive nodes come first, then the rest, all in document order.
        assert_eq!(tree.focus_order(), vec![2, 3, 1, 4]);
        let mut unique = tree.focus_order();
        unique.sort();
        assert_eq!(unique, vec![1, 2, 3, 4]);
    }

    #[test]
    fn reduce_motion_disables_animation() {
        let normal = ReduceMotionPolicy::for_preference(MotionPreference::NoPreference);
        assert!(normal.animations_enabled && normal.cursor_blink);
        let reduced = ReduceMotionPolicy::for_preference(MotionPreference::Reduce);
        assert!(!reduced.animations_enabled);
        assert!(!reduced.smooth_scrolling);
        assert!(!reduced.cursor_blink);
    }

    #[test]
    fn terminal_accessible_mode_announces_screen_and_cursor() {
        let mut mode = TerminalAccessibleMode::default();
        mode.enable();
        mode.set_screen(&["hello".to_owned(), "world".to_owned()]);
        mode.set_cursor(2, 3);
        let text = mode.screen_reader_text();
        assert!(text.contains("line 1: hello"));
        assert!(text.contains("line 2: world"));
        assert!(text.contains("cursor at row 2, column 3"));
    }

    #[test]
    fn screen_reader_checklist_covers_all_three_readers() {
        let checklist = screen_reader_checklist();
        assert!(checklist.iter().any(|item| item.reader == "VoiceOver"));
        assert!(checklist.iter().any(|item| item.reader == "NVDA"));
        assert!(checklist.iter().any(|item| item.reader == "TalkBack"));
        // The in-model checks are automated; the platform checks are not.
        assert!(checklist.iter().filter(|item| item.automated).count() >= 4);
        assert!(checklist.iter().any(|item| !item.automated));
        let _: &ChecklistItem = &checklist[0];
    }
}
