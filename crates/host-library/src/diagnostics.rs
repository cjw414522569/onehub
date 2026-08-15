//! Diagnostic package export with sensitive-information redaction (T117).
//!
//! [`DiagnosticExporter::preview`] shows exactly which categories will be
//! included / excluded so the user can confirm, and the default
//! [`RedactionPolicy`] excludes commands, hosts, usernames, session bodies,
//! and keys. [`Redactor`] scrubs secrets, `user@host` tokens, and private-key
//! blocks from the included categories, so a canary-secret scan finds nothing
//! in the exported bundle by default.

/// The redaction marker.
pub const REDACTED: &str = "[REDACTED]";

/// A diagnostic category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCategory {
    /// Application logs.
    Logs,
    /// A redacted config summary.
    ConfigSummary,
    /// System information.
    SystemInfo,
    /// Command history (excluded by default).
    CommandHistory,
    /// Hosts (excluded by default).
    Hosts,
    /// Session body excerpts (excluded by default).
    SessionBody,
    /// Key material (excluded by default).
    Keys,
}

impl DiagnosticCategory {
    /// A human label.
    pub fn label(&self) -> &'static str {
        match self {
            DiagnosticCategory::Logs => "logs",
            DiagnosticCategory::ConfigSummary => "config summary",
            DiagnosticCategory::SystemInfo => "system info",
            DiagnosticCategory::CommandHistory => "command history",
            DiagnosticCategory::Hosts => "hosts",
            DiagnosticCategory::SessionBody => "session body",
            DiagnosticCategory::Keys => "keys",
        }
    }
}

/// Which categories to include in the export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactionPolicy {
    /// Included categories.
    pub include: Vec<DiagnosticCategory>,
}

impl RedactionPolicy {
    /// The default policy: only logs, config summary, and system info; never
    /// commands, hosts, usernames, session bodies, or keys.
    pub fn defaults() -> Self {
        Self {
            include: vec![
                DiagnosticCategory::Logs,
                DiagnosticCategory::ConfigSummary,
                DiagnosticCategory::SystemInfo,
            ],
        }
    }

    /// Adds a category to the include set (explicit user opt-in).
    pub fn including(mut self, category: DiagnosticCategory) -> Self {
        if !self.include.contains(&category) {
            self.include.push(category);
        }
        self
    }
}

/// The diagnostic inputs (may contain sensitive data).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticInput {
    /// Logs.
    pub logs: String,
    /// Config summary.
    pub config_summary: String,
    /// System info.
    pub system_info: String,
    /// Command history.
    pub command_history: String,
    /// Hosts.
    pub hosts: String,
    /// Session body.
    pub session_body: String,
    /// Key material.
    pub keys: String,
}

impl DiagnosticInput {
    fn text_for(&self, category: DiagnosticCategory) -> &str {
        match category {
            DiagnosticCategory::Logs => &self.logs,
            DiagnosticCategory::ConfigSummary => &self.config_summary,
            DiagnosticCategory::SystemInfo => &self.system_info,
            DiagnosticCategory::CommandHistory => &self.command_history,
            DiagnosticCategory::Hosts => &self.hosts,
            DiagnosticCategory::SessionBody => &self.session_body,
            DiagnosticCategory::Keys => &self.keys,
        }
    }
}

/// What an export would contain (shown to the user for confirmation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticPreview {
    /// Categories that will be included.
    pub included: Vec<DiagnosticCategory>,
    /// Categories that will be excluded.
    pub excluded: Vec<DiagnosticCategory>,
    /// Total bytes of the included raw text.
    pub included_bytes: usize,
}

/// One redacted section of the bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticSection {
    /// The category.
    pub category: DiagnosticCategory,
    /// The redacted text.
    pub redacted_text: String,
}

/// The exported diagnostic bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticBundle {
    /// Redacted sections (only the included categories).
    pub sections: Vec<DiagnosticSection>,
    /// Excluded categories.
    pub excluded: Vec<DiagnosticCategory>,
}

impl DiagnosticBundle {
    /// The concatenated redacted text of every section (for scans).
    pub fn text(&self) -> String {
        self.sections
            .iter()
            .map(|section| section.redacted_text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The diagnostic exporter.
pub struct DiagnosticExporter;

impl DiagnosticExporter {
    /// Builds the user-facing preview (nothing is exported yet).
    pub fn preview(input: &DiagnosticInput, policy: &RedactionPolicy) -> DiagnosticPreview {
        let mut excluded = Vec::new();
        let mut included_bytes = 0;
        for category in [
            DiagnosticCategory::Logs,
            DiagnosticCategory::ConfigSummary,
            DiagnosticCategory::SystemInfo,
            DiagnosticCategory::CommandHistory,
            DiagnosticCategory::Hosts,
            DiagnosticCategory::SessionBody,
            DiagnosticCategory::Keys,
        ] {
            if policy.include.contains(&category) {
                included_bytes += input.text_for(category).len();
            } else {
                excluded.push(category);
            }
        }
        DiagnosticPreview {
            included: policy.include.clone(),
            excluded,
            included_bytes,
        }
    }

    /// Exports the bundle: only included categories, each redacted.
    pub fn export(
        input: &DiagnosticInput,
        policy: &RedactionPolicy,
        secrets: &[&str],
    ) -> DiagnosticBundle {
        let mut sections = Vec::new();
        let mut excluded = Vec::new();
        for category in [
            DiagnosticCategory::Logs,
            DiagnosticCategory::ConfigSummary,
            DiagnosticCategory::SystemInfo,
            DiagnosticCategory::CommandHistory,
            DiagnosticCategory::Hosts,
            DiagnosticCategory::SessionBody,
            DiagnosticCategory::Keys,
        ] {
            if policy.include.contains(&category) {
                let raw = input.text_for(category);
                sections.push(DiagnosticSection {
                    category,
                    redacted_text: Redactor::scrub(raw, secrets),
                });
            } else {
                excluded.push(category);
            }
        }
        DiagnosticBundle { sections, excluded }
    }
}

/// The redactor.
pub struct Redactor;

impl Redactor {
    /// Replaces every exact secret substring with the redaction marker.
    pub fn redact_secrets(text: &str, secrets: &[&str]) -> String {
        let mut result = text.to_owned();
        for secret in secrets {
            if !secret.is_empty() {
                result = result.replace(secret, REDACTED);
            }
        }
        result
    }

    /// Redacts `user@host`-style tokens.
    pub fn redact_emailish(text: &str) -> String {
        text.split_whitespace()
            .map(|token| {
                if token.contains('@') && !token.starts_with(REDACTED) {
                    REDACTED.to_owned()
                } else {
                    token.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Redacts private-key blocks (`-----BEGIN ... PRIVATE KEY-----`).
    pub fn redact_key_blocks(text: &str) -> String {
        let mut result = String::new();
        let mut in_block = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("-----BEGIN") && trimmed.contains("PRIVATE KEY") {
                in_block = true;
                result.push_str(&format!("{REDACTED} key block\n"));
                continue;
            }
            if in_block {
                if trimmed.starts_with("-----END") {
                    in_block = false;
                }
                continue;
            }
            result.push_str(line);
            result.push('\n');
        }
        result
    }

    /// Applies all three redaction passes. Key blocks are removed first
    /// (line-based) so later whitespace collapsing cannot hide them.
    pub fn scrub(text: &str, secrets: &[&str]) -> String {
        let text = Self::redact_key_blocks(text);
        let text = Self::redact_secrets(&text, secrets);
        Self::redact_emailish(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DiagnosticCategory, DiagnosticExporter, DiagnosticInput, RedactionPolicy,
        Redactor, REDACTED,
    };

    fn canary_input() -> DiagnosticInput {
        DiagnosticInput {
            logs: "user admin@prod-box connected; CANARY_LOG=1; token CANARY_SECRET_LOGS".to_owned(),
            config_summary: "theme=dark; host CANARY_HOST_CONFIG".to_owned(),
            system_info: "os=windows; CANARY_SYS=1".to_owned(),
            command_history: "ssh root@CANARY_HOST; rm -rf /; CANARY_COMMAND=1".to_owned(),
            hosts: "CANARY_HOST_ALPHA 10.0.0.5".to_owned(),
            session_body: "output CANARY_BODY=1".to_owned(),
            keys: "-----BEGIN OPENSSH PRIVATE KEY-----\nCANARY_KEY_ABC\n-----END OPENSSH PRIVATE KEY-----".to_owned(),
        }
    }

    const CANARIES: &[&str] = &[
        "CANARY_LOG",
        "CANARY_HOST_CONFIG",
        "CANARY_SYS",
        "CANARY_COMMAND",
        "CANARY_HOST_ALPHA",
        "CANARY_BODY",
        "CANARY_KEY_ABC",
        "CANARY_SECRET_LOGS",
    ];

    #[test]
    fn preview_shows_included_and_excluded_categories() {
        let input = canary_input();
        let preview = DiagnosticExporter::preview(&input, &RedactionPolicy::defaults());
        assert!(preview.included.contains(&DiagnosticCategory::Logs));
        assert!(!preview.included.contains(&DiagnosticCategory::Keys));
        assert!(preview
            .excluded
            .contains(&DiagnosticCategory::CommandHistory));
        assert!(preview.excluded.contains(&DiagnosticCategory::Hosts));
        assert!(preview.excluded.contains(&DiagnosticCategory::SessionBody));
        assert!(preview.excluded.contains(&DiagnosticCategory::Keys));
        assert!(preview.included_bytes > 0);
    }

    #[test]
    fn default_export_passes_canary_secret_scan() {
        let input = canary_input();
        // The app knows the secrets that appear inside included categories;
        // excluded-category canaries are intentionally not in this list.
        let bundle = DiagnosticExporter::export(
            &input,
            &RedactionPolicy::defaults(),
            &[
                "CANARY_LOG",
                "CANARY_SYS",
                "CANARY_HOST_CONFIG",
                "CANARY_SECRET_LOGS",
            ],
        );
        let text = bundle.text();
        for canary in CANARIES {
            assert!(
                !text.contains(canary),
                "default export must not contain canary {canary}"
            );
        }
        // Excluded categories are listed.
        assert!(bundle.excluded.contains(&DiagnosticCategory::Keys));
        assert!(bundle.excluded.contains(&DiagnosticCategory::Hosts));
    }

    #[test]
    fn opt_in_includes_a_category() {
        let input = canary_input();
        let policy = RedactionPolicy::defaults().including(DiagnosticCategory::Hosts);
        let bundle = DiagnosticExporter::export(&input, &policy, &[]);
        let text = bundle.text();
        assert!(
            text.contains("CANARY_HOST_ALPHA"),
            "explicit opt-in includes the hosts category"
        );
    }

    #[test]
    fn redactor_scrubs_emailish_and_key_blocks() {
        let text = "user admin@prod-box ran a command\n-----BEGIN OPENSSH PRIVATE KEY-----\nsecretline\n-----END OPENSSH PRIVATE KEY-----\nafter";
        let scrubbed = Redactor::scrub(text, &[]);
        assert!(!scrubbed.contains("admin@prod-box"));
        assert!(!scrubbed.contains("secretline"));
        assert!(scrubbed.contains(REDACTED));
        assert!(scrubbed.contains("after"));
        // Explicit secrets are also replaced.
        let with_secret = Redactor::scrub("token ABC123 done", &["ABC123"]);
        assert!(!with_secret.contains("ABC123"));
        assert!(with_secret.contains(REDACTED));
    }
}
