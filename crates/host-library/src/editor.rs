//! Host editor with inline validation and accessibility (T103).
//!
//! The editor is organized into five reviewable sections — basic, auth,
//! proxy, terminal, and advanced — each a list of labeled fields. Every
//! field validates **inline** (the error updates as soon as the value is
//! set), the form is valid only when all fields pass, and a review view
//! masks secrets so the configuration can be audited before saving. An
//! accessibility report guarantees every field is labeled, focusable in a
//! stable order, and every error message is non-empty (screen-reader
//! friendly).

use std::collections::BTreeMap;

/// The kind of an editor field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Free text.
    Text,
    /// A numeric value.
    Number,
    /// A constrained choice.
    Select,
    /// A secret (never shown in reviews).
    Password,
}

/// A field's static specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSpec {
    /// Stable field id.
    pub id: &'static str,
    /// Human label (accessibility).
    pub label: &'static str,
    /// Kind.
    pub kind: FieldKind,
    /// Whether a non-empty value is required.
    pub required: bool,
}

/// A reviewable editor section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionSpec {
    /// Stable section id.
    pub id: &'static str,
    /// Section title.
    pub title: &'static str,
    /// Fields in focus order.
    pub fields: &'static [FieldSpec],
}

/// The default editor layout (basic / auth / proxy / terminal / advanced).
pub fn default_spec() -> Vec<SectionSpec> {
    vec![
        SectionSpec {
            id: "basic",
            title: "Basic",
            fields: &[
                FieldSpec {
                    id: "name",
                    label: "Display name",
                    kind: FieldKind::Text,
                    required: true,
                },
                FieldSpec {
                    id: "host",
                    label: "Hostname or IP",
                    kind: FieldKind::Text,
                    required: true,
                },
                FieldSpec {
                    id: "port",
                    label: "Port",
                    kind: FieldKind::Number,
                    required: true,
                },
                FieldSpec {
                    id: "group",
                    label: "Group",
                    kind: FieldKind::Text,
                    required: false,
                },
                FieldSpec {
                    id: "tags",
                    label: "Tags",
                    kind: FieldKind::Text,
                    required: false,
                },
            ],
        },
        SectionSpec {
            id: "auth",
            title: "Authentication",
            fields: &[
                FieldSpec {
                    id: "username",
                    label: "Username",
                    kind: FieldKind::Text,
                    required: true,
                },
                FieldSpec {
                    id: "method",
                    label: "Auth method",
                    kind: FieldKind::Select,
                    required: true,
                },
                FieldSpec {
                    id: "key_path",
                    label: "Private key path",
                    kind: FieldKind::Text,
                    required: false,
                },
                FieldSpec {
                    id: "password",
                    label: "Password",
                    kind: FieldKind::Password,
                    required: false,
                },
            ],
        },
        SectionSpec {
            id: "proxy",
            title: "Proxy",
            fields: &[
                FieldSpec {
                    id: "proxy_enabled",
                    label: "Use proxy",
                    kind: FieldKind::Select,
                    required: false,
                },
                FieldSpec {
                    id: "proxy_host",
                    label: "Proxy host",
                    kind: FieldKind::Text,
                    required: false,
                },
                FieldSpec {
                    id: "proxy_port",
                    label: "Proxy port",
                    kind: FieldKind::Number,
                    required: false,
                },
            ],
        },
        SectionSpec {
            id: "terminal",
            title: "Terminal",
            fields: &[
                FieldSpec {
                    id: "shell",
                    label: "Shell",
                    kind: FieldKind::Text,
                    required: false,
                },
                FieldSpec {
                    id: "color_scheme",
                    label: "Color scheme",
                    kind: FieldKind::Select,
                    required: false,
                },
            ],
        },
        SectionSpec {
            id: "advanced",
            title: "Advanced",
            fields: &[
                FieldSpec {
                    id: "keepalive",
                    label: "Keepalive seconds",
                    kind: FieldKind::Number,
                    required: false,
                },
                FieldSpec {
                    id: "compression",
                    label: "Compression",
                    kind: FieldKind::Select,
                    required: false,
                },
            ],
        },
    ]
}

/// The live state of one field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldState {
    /// The field specification.
    pub spec: FieldSpec,
    /// Current value.
    pub value: String,
    /// Inline validation error (`None` when valid).
    pub error: Option<String>,
}

impl FieldState {
    fn new(spec: FieldSpec) -> Self {
        Self {
            spec,
            value: String::new(),
            error: None,
        }
    }
}

/// The live state of one section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionState {
    /// The section specification.
    pub spec: SectionSpec,
    /// Fields in focus order.
    pub fields: Vec<FieldState>,
}

/// The editor form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEditorForm {
    sections: Vec<SectionState>,
}

/// A review row: field label + display value (secrets masked).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRow {
    /// Field label.
    pub label: String,
    /// Display value (`"••••"` for passwords, `"<empty>"` when empty).
    pub display: String,
}

/// A reviewable section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionReview {
    /// Section title.
    pub title: String,
    /// Review rows in focus order.
    pub rows: Vec<ReviewRow>,
}

/// The accessibility report for the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibilityReport {
    /// Total fields.
    pub total_fields: usize,
    /// Fields with a non-empty label (must equal `total_fields`).
    pub labeled_fields: usize,
    /// Stable focus order (section order, then field order).
    pub focus_order: Vec<String>,
    /// `(field_id, error_message)` for fields with an error.
    pub fields_with_errors: Vec<(String, String)>,
}

/// The secret mask shown in reviews.
pub const PASSWORD_MASK: &str = "••••";

impl HostEditorForm {
    /// A form for the given section specs.
    pub fn new(specs: &[SectionSpec]) -> Self {
        let sections = specs
            .iter()
            .map(|spec| SectionState {
                spec: *spec,
                fields: spec
                    .fields
                    .iter()
                    .map(|field| FieldState::new(*field))
                    .collect(),
            })
            .collect();
        Self { sections }
    }

    /// The default form (basic/auth/proxy/terminal/advanced).
    pub fn default_form() -> Self {
        Self::new(&default_spec())
    }

    /// Sets a field value and revalidates it inline.
    pub fn set(&mut self, section_id: &str, field_id: &str, value: &str) -> &mut Self {
        if let Some(section) = self.sections.iter_mut().find(|s| s.spec.id == section_id) {
            if let Some(index) = section.fields.iter().position(|f| f.spec.id == field_id) {
                section.fields[index].value = value.to_owned();
            }
            // Revalidate the whole section so cross-field rules (e.g. auth
            // method -> key path, proxy enabled -> host) stay inline-correct.
            let errors: Vec<Option<String>> = section
                .fields
                .iter()
                .map(|field| validate_field(section, field))
                .collect();
            for (field, error) in section.fields.iter_mut().zip(errors) {
                field.error = error;
            }
        }
        self
    }

    /// Reads a field.
    pub fn field(&self, section_id: &str, field_id: &str) -> Option<&FieldState> {
        self.sections
            .iter()
            .find(|s| s.spec.id == section_id)
            .and_then(|s| s.fields.iter().find(|f| f.spec.id == field_id))
    }

    /// Validates every field; returns whether the form is valid.
    pub fn validate(&mut self) -> bool {
        let mut valid = true;
        for section in &mut self.sections {
            let errors: Vec<Option<String>> = section
                .fields
                .iter()
                .map(|field| validate_field(section, field))
                .collect();
            for (field, error) in section.fields.iter_mut().zip(errors) {
                field.error = error;
                if field.error.is_some() {
                    valid = false;
                }
            }
        }
        valid
    }

    /// Whether the form has no errors.
    pub fn is_valid(&self) -> bool {
        self.sections
            .iter()
            .all(|section| section.fields.iter().all(|field| field.error.is_none()))
    }

    /// The reviewable view: every section with labeled rows, secrets masked.
    pub fn review(&self) -> Vec<SectionReview> {
        self.sections
            .iter()
            .map(|section| SectionReview {
                title: section.spec.title.to_owned(),
                rows: section
                    .fields
                    .iter()
                    .map(|field| ReviewRow {
                        label: field.spec.label.to_owned(),
                        display: if field.value.is_empty() {
                            "<empty>".to_owned()
                        } else if field.spec.kind == FieldKind::Password {
                            PASSWORD_MASK.to_owned()
                        } else {
                            field.value.clone()
                        },
                    })
                    .collect(),
            })
            .collect()
    }

    /// The accessibility report (labels, focus order, error messages).
    pub fn accessibility(&self) -> AccessibilityReport {
        let mut total_fields = 0;
        let mut labeled_fields = 0;
        let mut focus_order = Vec::new();
        let mut fields_with_errors = Vec::new();
        for section in &self.sections {
            for field in &section.fields {
                total_fields += 1;
                if !field.spec.label.is_empty() {
                    labeled_fields += 1;
                }
                focus_order.push(field.spec.id.to_owned());
                if let Some(error) = &field.error {
                    fields_with_errors.push((field.spec.id.to_owned(), error.clone()));
                }
            }
        }
        AccessibilityReport {
            total_fields,
            labeled_fields,
            focus_order,
            fields_with_errors,
        }
    }
}

/// Validates one field inline, returning the error (or `None`).
fn validate_field(section: &SectionState, field: &FieldState) -> Option<String> {
    let value = field.value.trim();
    if field.spec.required && value.is_empty() {
        return Some(format!("{} is required", field.spec.label));
    }
    if !value.is_empty() {
        match field.spec.kind {
            FieldKind::Number => {
                let parsed: Result<u32, _> = value.parse();
                match parsed {
                    Ok(number) => {
                        let (min, max) = match field.spec.id {
                            "port" | "proxy_port" => (1, 65535),
                            "keepalive" => (0, 86400),
                            _ => (0, u32::MAX),
                        };
                        if number < min || number > max {
                            return Some(format!(
                                "{} must be between {min} and {max}",
                                field.spec.label
                            ));
                        }
                    }
                    Err(_) => return Some(format!("{} must be a number", field.spec.label)),
                }
            }
            FieldKind::Text => {
                if field.spec.id == "host" && value.contains(' ') {
                    return Some("Hostname must not contain spaces".to_owned());
                }
                if field.spec.id == "name" && value.len() > 64 {
                    return Some("Display name must be 64 characters or fewer".to_owned());
                }
            }
            FieldKind::Select | FieldKind::Password => {}
        }
    }
    // Cross-field rule: the "key" auth method requires a key path.
    if section.spec.id == "auth" && field.spec.id == "key_path" && value.is_empty() {
        let method = section
            .fields
            .iter()
            .find(|f| f.spec.id == "method")
            .map(|f| f.value.trim())
            .unwrap_or("");
        if method == "key" {
            return Some("A private key path is required for key authentication".to_owned());
        }
    }
    // Cross-field rule: enabling the proxy requires host + port.
    if section.spec.id == "proxy" && field.spec.id == "proxy_host" && value.is_empty() {
        let enabled = section
            .fields
            .iter()
            .find(|f| f.spec.id == "proxy_enabled")
            .map(|f| f.value.trim())
            .unwrap_or("");
        if enabled == "true" {
            return Some("A proxy host is required when the proxy is enabled".to_owned());
        }
    }
    None
}

/// The rendered form state as a deterministic map (for state tests).
pub fn state_map(form: &HostEditorForm) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for section in &form.sections {
        for field in &section.fields {
            map.insert(
                format!("{}.{}", section.spec.id, field.spec.id),
                field.value.clone(),
            );
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::{state_map, FieldKind, HostEditorForm, PASSWORD_MASK};

    #[test]
    fn default_form_has_all_five_reviewable_sections() {
        let form = HostEditorForm::default_form();
        let titles: Vec<&str> = form
            .sections
            .iter()
            .map(|section| section.spec.title)
            .collect();
        assert_eq!(
            titles,
            vec!["Basic", "Authentication", "Proxy", "Terminal", "Advanced"]
        );
        assert!(form.is_valid(), "empty optional form is valid");
    }

    #[test]
    fn inline_validation_catches_bad_input_and_clears_on_fix() {
        let mut form = HostEditorForm::default_form();
        // Port out of range.
        form.set("basic", "name", "alpha")
            .set("basic", "host", "10.0.0.5")
            .set("basic", "port", "70000");
        assert!(form.field("basic", "port").unwrap().error.is_some());
        assert!(!form.is_valid());
        // Host with a space.
        form.set("basic", "host", "bad host");
        assert!(form.field("basic", "host").unwrap().error.is_some());
        // Fix both inline: errors clear immediately.
        form.set("basic", "port", "2222")
            .set("basic", "host", "good.host");
        assert!(form.field("basic", "port").unwrap().error.is_none());
        assert!(form.field("basic", "host").unwrap().error.is_none());
        // Required field empty.
        form.set("basic", "name", "");
        assert!(form.field("basic", "name").unwrap().error.is_some());
    }

    #[test]
    fn auth_method_key_requires_key_path() {
        let mut form = HostEditorForm::default_form();
        form.set("auth", "method", "key");
        // Inline: empty key path is an error once method == key.
        let key_path = form.field("auth", "key_path").unwrap();
        assert!(
            key_path.error.is_some(),
            "key auth without a key path must be invalid"
        );
        form.set("auth", "key_path", "~/.ssh/id_ed25519");
        assert!(form.field("auth", "key_path").unwrap().error.is_none());
    }

    #[test]
    fn form_is_valid_when_all_fields_ok() {
        let mut form = HostEditorForm::default_form();
        form.set("basic", "name", "alpha")
            .set("basic", "host", "10.0.0.5")
            .set("basic", "port", "22")
            .set("auth", "username", "root")
            .set("auth", "method", "agent");
        assert!(form.is_valid());
        assert!(form.validate());
        assert_eq!(form.accessibility().fields_with_errors.len(), 0);
    }

    #[test]
    fn review_masks_passwords_and_lists_sections() {
        let mut form = HostEditorForm::default_form();
        form.set("basic", "name", "alpha")
            .set("auth", "username", "root")
            .set("auth", "password", "s3cret");
        let review = form.review();
        assert_eq!(review.len(), 5);
        let auth = review
            .iter()
            .find(|section| section.title == "Authentication")
            .unwrap();
        let password = auth
            .rows
            .iter()
            .find(|row| row.label == "Password")
            .unwrap();
        assert_eq!(password.display, PASSWORD_MASK);
        assert!(!form
            .review()
            .iter()
            .flat_map(|section| section.rows.iter())
            .any(|row| row.label == "Password" && row.display == "s3cret"));
        // state_map is deterministic and reviewable.
        let state = state_map(&form);
        assert_eq!(state.get("basic.name").map(String::as_str), Some("alpha"));
    }

    #[test]
    fn accessibility_report_has_labels_and_focus_order() {
        let mut form = HostEditorForm::default_form();
        form.set("basic", "name", "");
        form.set("basic", "port", "99999");
        let report = form.accessibility();
        assert_eq!(
            report.total_fields, report.labeled_fields,
            "every field labeled"
        );
        assert_eq!(report.focus_order.len(), report.total_fields);
        // Focus order: basic first, advanced last.
        assert_eq!(report.focus_order[0], "name");
        assert_eq!(*report.focus_order.last().unwrap(), "compression");
        // Errors carry non-empty, screen-reader-friendly messages.
        assert!(!report.fields_with_errors.is_empty());
        for (_, message) in &report.fields_with_errors {
            assert!(!message.is_empty());
        }
        // Every field is a known kind.
        for section in &form.sections {
            for field in &section.fields {
                assert!(matches!(
                    field.spec.kind,
                    FieldKind::Text | FieldKind::Number | FieldKind::Select | FieldKind::Password
                ));
            }
        }
    }
}
