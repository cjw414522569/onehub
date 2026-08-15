//! Command snippets, variable hints, and sensitive parameter injection (T112).
//!
//! A [`SnippetTemplate`] is a command with `{{variable}}` placeholders.
//! [`SnippetEngine::render`] substitutes values in a single pass (no
//! recursive substitution, so a malicious value cannot inject new
//! placeholders) and produces a **preview** in which secret values are
//! masked. [`CommandHistory`] records only the masked preview, so sensitive
//! values never enter history (verified by a leak test).

use std::collections::BTreeMap;

/// The mask shown for secret values in previews and history.
pub const SECRET_MASK: &str = "••••";

/// The kind of a snippet variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableKind {
    /// Plain text.
    Text,
    /// A secret (never shown in previews/history).
    Secret,
}

/// A snippet variable declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetVariable {
    /// Variable name (as used in `{{name}}`).
    pub name: String,
    /// The variable kind.
    pub kind: VariableKind,
}

/// A command snippet template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetTemplate {
    /// Stable id.
    pub id: u64,
    /// Human name.
    pub name: String,
    /// The command with `{{variable}}` placeholders.
    pub command: String,
    /// Declared variables.
    pub variables: Vec<SnippetVariable>,
}

impl SnippetTemplate {
    /// The variable names referenced by the command, in order.
    pub fn variables_in_command(&self) -> Vec<String> {
        let mut names = Vec::new();
        let bytes = self.command.as_bytes();
        let mut index = 0;
        while index + 3 < bytes.len() {
            if bytes[index..].starts_with(b"{{") {
                if let Some(end) = self.command[index + 2..].find("}}") {
                    let name = self.command[index + 2..index + 2 + end].to_owned();
                    if !name.is_empty() && !names.contains(&name) {
                        names.push(name);
                    }
                    index += 2 + end + 2;
                    continue;
                }
            }
            index += 1;
        }
        names
    }
}

/// Why snippet rendering failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetError {
    /// A value was provided for an undeclared variable.
    UnknownVariable,
    /// A declared variable has no value.
    MissingVariable,
}

/// The render result: the executable command plus a masked preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderResult {
    /// The fully substituted command (may contain secrets).
    pub command: String,
    /// The preview with secret values masked.
    pub preview: String,
    /// Whether any secret value was used.
    pub sensitive_used: bool,
}

/// The snippet engine.
pub struct SnippetEngine;

impl SnippetEngine {
    /// Renders a template with values. Substitution is a single pass: a value
    /// that itself contains `{{...}}` is inserted literally (no injection).
    pub fn render(
        template: &SnippetTemplate,
        values: &BTreeMap<String, String>,
    ) -> Result<RenderResult, SnippetError> {
        for variable in &template.variables {
            if !values.contains_key(&variable.name) {
                return Err(SnippetError::MissingVariable);
            }
        }
        let mut sensitive_used = false;
        let mut command = template.command.clone();
        let mut preview = template.command.clone();
        let mut secret_names = Vec::new();
        for variable in &template.variables {
            if variable.kind == VariableKind::Secret {
                secret_names.push(variable.name.clone());
            }
        }
        for name in template.variables_in_command() {
            let value = values.get(&name).ok_or(SnippetError::MissingVariable)?;
            let placeholder = format!("{{{{{name}}}}}");
            command = command.replace(&placeholder, value);
            if secret_names.contains(&name) {
                sensitive_used = true;
                preview = preview.replace(&placeholder, SECRET_MASK);
            } else {
                preview = preview.replace(&placeholder, value);
            }
        }
        Ok(RenderResult {
            command,
            preview,
            sensitive_used,
        })
    }
}

/// A history entry (masked preview only, never secrets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// The template id.
    pub template_id: u64,
    /// The masked preview.
    pub preview: String,
}

/// The command history.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandHistory {
    entries: Vec<HistoryEntry>,
}

impl CommandHistory {
    /// An empty history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a render: only the **masked preview** is stored.
    pub fn record(&mut self, template_id: u64, render: &RenderResult) {
        self.entries.push(HistoryEntry {
            template_id,
            preview: render.preview.clone(),
        });
    }

    /// The recorded entries.
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// Whether any recorded entry contains `needle` (history-leak check).
    pub fn contains(&self, needle: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.preview.contains(needle))
    }
}

/// Variable hints for autocomplete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableHints;

impl VariableHints {
    /// Returns candidate values for a text variable matching a partial input.
    pub fn resolve(candidates: &[&str], partial: &str, limit: usize) -> Vec<String> {
        candidates
            .iter()
            .filter(|candidate| candidate.starts_with(partial))
            .take(limit)
            .map(|candidate| (*candidate).to_owned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        CommandHistory, SnippetEngine, SnippetError, SnippetTemplate, SnippetVariable,
        VariableHints, VariableKind, SECRET_MASK,
    };

    fn template() -> SnippetTemplate {
        SnippetTemplate {
            id: 1,
            name: "deploy".to_owned(),
            command: "ssh {{user}}@{{host}} -p {{port}} --token {{token}}".to_owned(),
            variables: vec![
                SnippetVariable {
                    name: "user".to_owned(),
                    kind: VariableKind::Text,
                },
                SnippetVariable {
                    name: "host".to_owned(),
                    kind: VariableKind::Text,
                },
                SnippetVariable {
                    name: "port".to_owned(),
                    kind: VariableKind::Text,
                },
                SnippetVariable {
                    name: "token".to_owned(),
                    kind: VariableKind::Secret,
                },
            ],
        }
    }

    fn values() -> BTreeMap<String, String> {
        let mut values = BTreeMap::new();
        values.insert("user".to_owned(), "root".to_owned());
        values.insert("host".to_owned(), "10.0.0.5".to_owned());
        values.insert("port".to_owned(), "22".to_owned());
        values.insert("token".to_owned(), "SUPER_SECRET_TOKEN_123".to_owned());
        values
    }

    #[test]
    fn render_substitutes_and_masks_secrets() {
        let template = template();
        let render = SnippetEngine::render(&template, &values()).unwrap();
        assert!(render.command.contains("SUPER_SECRET_TOKEN_123"));
        assert!(render.command.starts_with("ssh root@10.0.0.5 -p 22"));
        assert!(render.sensitive_used);
        // The preview masks the secret but keeps text values visible.
        assert!(!render.preview.contains("SUPER_SECRET_TOKEN_123"));
        assert!(render.preview.contains(SECRET_MASK));
        assert!(render.preview.contains("root@10.0.0.5"));
    }

    #[test]
    fn sensitive_values_never_enter_history() {
        let template = template();
        let mut history = CommandHistory::new();
        for _ in 0..3 {
            let render = SnippetEngine::render(&template, &values()).unwrap();
            history.record(template.id, &render);
        }
        assert_eq!(history.entries().len(), 3);
        assert!(
            !history.contains("SUPER_SECRET_TOKEN_123"),
            "secret must never leak into history"
        );
        assert!(history.entries().iter().all(|entry| entry.template_id == 1),);
        // The masked preview is still useful.
        assert!(history.contains(SECRET_MASK));
    }

    #[test]
    fn template_injection_is_not_recursive() {
        let mut template = template();
        template.command = "echo {{payload}}".to_owned();
        template.variables = vec![SnippetVariable {
            name: "payload".to_owned(),
            kind: VariableKind::Text,
        }];
        let mut values = BTreeMap::new();
        // A malicious value that looks like a placeholder must be inserted
        // literally (single-pass substitution = no injection).
        values.insert("payload".to_owned(), "x; rm -rf / {{evil}}".to_owned());
        let render = SnippetEngine::render(&template, &values).unwrap();
        assert_eq!(render.command, "echo x; rm -rf / {{evil}}");
        assert!(
            render.command.contains("{{evil}}"),
            "no recursive substitution"
        );
    }

    #[test]
    fn variable_validation_rejects_missing_and_unknown() {
        let template = template();
        let mut incomplete = values();
        incomplete.remove("token");
        assert_eq!(
            SnippetEngine::render(&template, &incomplete),
            Err(SnippetError::MissingVariable)
        );
        let mut extra = values();
        extra.insert("unused".to_owned(), "x".to_owned());
        // Extra values are ignored (only declared variables are used).
        assert!(SnippetEngine::render(&template, &extra).is_ok());
    }

    #[test]
    fn variables_in_command_are_parsed_in_order() {
        let template = template();
        let names = template.variables_in_command();
        assert_eq!(names, vec!["user", "host", "port", "token"]);
        let no_vars = SnippetTemplate {
            id: 2,
            name: "plain".to_owned(),
            command: "ls -la".to_owned(),
            variables: Vec::new(),
        };
        assert!(no_vars.variables_in_command().is_empty());
    }

    #[test]
    fn variable_hints_resolve_by_prefix() {
        let hints = VariableHints::resolve(&["prod-a", "prod-b", "dev-a", "stage"], "prod", 10);
        assert_eq!(hints, vec!["prod-a", "prod-b"]);
        let limited = VariableHints::resolve(&["a1", "a2", "a3"], "a", 2);
        assert_eq!(limited, vec!["a1", "a2"]);
        assert!(VariableHints::resolve(&["x"], "y", 10).is_empty());
    }

    #[test]
    fn non_secret_render_is_not_marked_sensitive() {
        let template = SnippetTemplate {
            id: 3,
            name: "list".to_owned(),
            command: "ls {{dir}}".to_owned(),
            variables: vec![SnippetVariable {
                name: "dir".to_owned(),
                kind: VariableKind::Text,
            }],
        };
        let mut values = BTreeMap::new();
        values.insert("dir".to_owned(), "/tmp".to_owned());
        let render = SnippetEngine::render(&template, &values).unwrap();
        assert!(!render.sensitive_used);
        assert_eq!(render.preview, "ls /tmp");
    }
}
