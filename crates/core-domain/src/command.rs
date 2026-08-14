use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// An environment variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVar {
    /// Variable name.
    pub name: String,
    /// Value (may be secret).
    pub value: String,
    /// Whether the value is sensitive and must not enter history/telemetry.
    pub sensitive: bool,
}

impl EnvVar {
    /// Creates an environment variable, rejecting an empty name.
    pub fn new(
        name: impl Into<String>,
        value: impl Into<String>,
        sensitive: bool,
    ) -> Result<Self, DomainError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(DomainError::EmptyName);
        }
        Ok(Self {
            name,
            value: value.into(),
            sensitive,
        })
    }
}

/// An immutable environment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    vars: Vec<EnvVar>,
}

impl Environment {
    /// An empty environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds an environment from variables.
    pub fn from_vars(vars: Vec<EnvVar>) -> Self {
        Self { vars }
    }

    /// Looks up a variable by name.
    pub fn get(&self, name: &str) -> Option<&EnvVar> {
        self.vars.iter().find(|var| var.name == name)
    }

    /// Returns all variables.
    pub fn vars(&self) -> &[EnvVar] {
        &self.vars
    }
}

/// A placeholder inside a command template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceholderDef {
    /// Placeholder name, e.g. `user`.
    pub name: String,
    /// Whether this field's value is sensitive.
    pub sensitive: bool,
}

/// A named command snippet (template + declared placeholders).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSnippet {
    /// Stable snippet id.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Template with `{placeholder}` slots, e.g. `ssh {user}@{host} {command}`.
    pub template: String,
    /// Declared placeholders.
    pub placeholders: Vec<PlaceholderDef>,
}

impl CommandSnippet {
    /// Creates a snippet.
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        template: impl Into<String>,
        placeholders: Vec<PlaceholderDef>,
    ) -> Result<Self, DomainError> {
        let id = id.into();
        let label = label.into();
        let template = template.into();
        if id.trim().is_empty() || label.trim().is_empty() || template.trim().is_empty() {
            return Err(DomainError::EmptyName);
        }
        Ok(Self {
            id,
            label,
            template,
            placeholders,
        })
    }

    /// Whether any declared placeholder is sensitive.
    pub fn has_sensitive_fields(&self) -> bool {
        self.placeholders
            .iter()
            .any(|placeholder| placeholder.sensitive)
    }
}

/// A macro that expands to text, optionally sensitive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Macro {
    /// Macro name, e.g. `$TOKEN`.
    pub name: String,
    /// Expansion text (may be secret).
    pub expansion: String,
    /// Whether the expansion is sensitive.
    pub sensitive: bool,
}

/// A resolved command with an explicit sensitivity flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCommand {
    /// The final command text.
    pub text: String,
    /// Whether the command (or any input that produced it) is sensitive.
    pub sensitive: bool,
}

impl ResolvedCommand {
    /// Whether the command may be stored in shell history.
    pub fn history_allowed(&self) -> bool {
        !self.sensitive
    }

    /// Whether the command may be included in telemetry.
    pub fn telemetry_allowed(&self) -> bool {
        !self.sensitive
    }
}

/// Resolves a snippet into a command by substituting placeholders from
/// `arguments` and environment variables, and expanding macros.
///
/// Sensitivity propagates monotonically: if the snippet declares a sensitive
/// placeholder, any used environment variable is sensitive, any macro used is
/// sensitive, or the resulting text would contain a sensitive value, the
/// resolved command is sensitive and must be excluded from history and
/// telemetry.
pub fn resolve_command(
    snippet: &CommandSnippet,
    arguments: &[(String, String)],
    environment: &Environment,
    macros: &[Macro],
) -> ResolvedCommand {
    let mut sensitive = snippet.has_sensitive_fields();
    let mut text = snippet.template.clone();

    for (name, value) in arguments {
        let declared = snippet.placeholders.iter().find(|p| p.name == *name);
        if declared.map(|p| p.sensitive).unwrap_or(false) {
            sensitive = true;
        }
        text = text.replace(&format!("{{{name}}}"), value);
    }

    // Environment substitution: `$NAME` references expand to values; using a
    // sensitive variable marks the command sensitive.
    for var in environment.vars() {
        let token = format!("${}", var.name);
        if text.contains(&token) {
            text = text.replace(&token, &var.value);
            if var.sensitive {
                sensitive = true;
            }
        }
    }

    // Macro expansion: `$NAME` macros expand; sensitive macros propagate.
    for macro_def in macros {
        let token = format!("${}", macro_def.name);
        if text.contains(&token) {
            text = text.replace(&token, &macro_def.expansion);
            if macro_def.sensitive {
                sensitive = true;
            }
        }
    }

    ResolvedCommand { text, sensitive }
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_command, CommandSnippet, EnvVar, Environment, Macro, PlaceholderDef,
        ResolvedCommand,
    };

    fn snippet() -> CommandSnippet {
        CommandSnippet::new(
            "ssh-basic",
            "SSH Basic",
            "ssh {user}@{host} $COMMAND",
            vec![
                PlaceholderDef {
                    name: "user".to_owned(),
                    sensitive: false,
                },
                PlaceholderDef {
                    name: "host".to_owned(),
                    sensitive: false,
                },
            ],
        )
        .expect("valid snippet")
    }

    fn snippet_with_secret() -> CommandSnippet {
        CommandSnippet::new(
            "db-query",
            "DB Query",
            "psql postgresql://{user}@{host} -c $PASSWORD",
            vec![
                PlaceholderDef {
                    name: "user".to_owned(),
                    sensitive: false,
                },
                PlaceholderDef {
                    name: "host".to_owned(),
                    sensitive: false,
                },
            ],
        )
        .expect("valid snippet")
    }

    #[test]
    fn non_sensitive_command_is_allowed_in_history_and_telemetry() {
        let command = resolve_command(
            &snippet(),
            &[
                ("user".to_owned(), "alice".to_owned()),
                ("host".to_owned(), "prod".to_owned()),
            ],
            &Environment::from_vars(vec![EnvVar::new("COMMAND", "uptime", false).expect("var")]),
            &[],
        );
        assert_eq!(command.text, "ssh alice@prod uptime");
        assert!(!command.sensitive);
        assert!(command.history_allowed());
        assert!(command.telemetry_allowed());
    }

    #[test]
    fn sensitive_placeholder_propagates_sensitivity() {
        let mut secret_snippet = snippet_with_secret();
        secret_snippet.placeholders[0].sensitive = true;
        let command = resolve_command(
            &secret_snippet,
            &[
                ("user".to_owned(), "alice".to_owned()),
                ("host".to_owned(), "prod".to_owned()),
            ],
            &Environment::new(),
            &[],
        );
        assert!(command.sensitive);
        assert!(!command.history_allowed());
        assert!(!command.telemetry_allowed());
    }

    #[test]
    fn sensitive_environment_variable_propagates_sensitivity() {
        let command = resolve_command(
            &snippet_with_secret(),
            &[
                ("user".to_owned(), "alice".to_owned()),
                ("host".to_owned(), "prod".to_owned()),
            ],
            &Environment::from_vars(vec![EnvVar::new("PASSWORD", "hunter2", true).expect("var")]),
            &[],
        );
        assert!(command.sensitive);
        assert!(!command.history_allowed());
        assert!(!command.telemetry_allowed());
        assert!(command.text.contains("hunter2"));
    }

    #[test]
    fn non_sensitive_environment_variable_does_not_propagate() {
        let command = resolve_command(
            &snippet(),
            &[
                ("user".to_owned(), "alice".to_owned()),
                ("host".to_owned(), "prod".to_owned()),
            ],
            &Environment::from_vars(vec![EnvVar::new("COMMAND", "df -h", false).expect("var")]),
            &[],
        );
        assert!(!command.sensitive);
        assert_eq!(command.text, "ssh alice@prod df -h");
    }

    #[test]
    fn sensitive_macro_propagates_sensitivity() {
        let command = resolve_command(
            &snippet(),
            &[
                ("user".to_owned(), "alice".to_owned()),
                ("host".to_owned(), "prod".to_owned()),
            ],
            &Environment::new(),
            &[Macro {
                name: "COMMAND".to_owned(),
                expansion: "secret-command".to_owned(),
                sensitive: true,
            }],
        );
        assert!(command.sensitive);
        assert!(!command.history_allowed());
        assert!(!command.telemetry_allowed());
    }

    #[test]
    fn sensitivity_is_monotonic() {
        // Once any input is sensitive, the result is sensitive regardless of
        // later non-sensitive inputs.
        let base = resolve_command(
            &snippet_with_secret(),
            &[
                ("user".to_owned(), "alice".to_owned()),
                ("host".to_owned(), "prod".to_owned()),
            ],
            &Environment::from_vars(vec![EnvVar::new("PASSWORD", "hunter2", true).expect("var")]),
            &[],
        );
        assert!(base.sensitive);
        let again = resolve_command(
            &snippet_with_secret(),
            &[
                ("user".to_owned(), "bob".to_owned()),
                ("host".to_owned(), "staging".to_owned()),
            ],
            &Environment::from_vars(vec![EnvVar::new("PASSWORD", "hunter2", true).expect("var")]),
            &[],
        );
        assert!(again.sensitive);
    }

    #[test]
    fn models_round_trip_serde() {
        let snippet = snippet_with_secret();
        let json = serde_json::to_string(&snippet).expect("serialize snippet");
        let decoded: CommandSnippet = serde_json::from_str(&json).expect("deserialize snippet");
        assert_eq!(decoded, snippet);

        let env = Environment::from_vars(vec![EnvVar::new("PASSWORD", "x", true).expect("var")]);
        let json = serde_json::to_string(&env).expect("serialize env");
        let decoded: Environment = serde_json::from_str(&json).expect("deserialize env");
        assert_eq!(decoded, env);

        let command: ResolvedCommand = resolve_command(
            &snippet,
            &[
                ("user".to_owned(), "alice".to_owned()),
                ("host".to_owned(), "prod".to_owned()),
            ],
            &env,
            &[],
        );
        assert!(command.sensitive);
    }
}
