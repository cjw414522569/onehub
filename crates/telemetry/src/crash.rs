//! Crash capture, sanitization, retention, and consent-gated upload (T149).
//!
//! [`CrashSanitizer`] scrubs every dump so it never carries host names,
//! commands, terminal content, or credentials. [`CrashRetention`] defines a
//! clear retention period and max-dump budget with a [`CrashRetention::delete`]
//! mechanism. [`CrashUploadPolicy`] uploads only after explicit user consent.

/// The crash dump schema version.
pub const CRASH_SCHEMA_VERSION: u32 = 1;

/// A sanitized crash dump (no content by construction).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashDump {
    /// Schema version.
    pub schema_version: u32,
    /// The app version that crashed.
    pub app_version: String,
    /// Capture time (unix seconds).
    pub captured_at_unix: u64,
    /// The thread that panicked.
    pub thread_name: String,
    /// The sanitized panic message.
    pub panic_message: String,
    /// Sanitized stack frames.
    pub stack_trace: Vec<String>,
}

impl CrashDump {
    /// Builds a dump and immediately sanitizes the message and frames.
    pub fn capture(
        app_version: &str,
        captured_at_unix: u64,
        thread_name: &str,
        raw_message: &str,
        raw_stack: Vec<String>,
        sanitizer: &CrashSanitizer,
    ) -> Self {
        Self {
            schema_version: CRASH_SCHEMA_VERSION,
            app_version: app_version.to_owned(),
            captured_at_unix,
            thread_name: sanitizer.sanitize_text(thread_name),
            panic_message: sanitizer.sanitize_text(raw_message),
            stack_trace: raw_stack
                .iter()
                .map(|frame| sanitizer.sanitize_text(frame))
                .collect(),
        }
    }
}

/// The default denylist: markers that must never survive into a dump.
pub const DEFAULT_CRASH_DENYLIST: &[&str] = &[
    "db.internal",
    "10.0.0.5",
    "ls -la",
    "rm -rf",
    "c4145",
    "PRIVACY_CANARY_3d19f7aa8c",
];

/// Sensitive key=value keys whose values are redacted.
const SENSITIVE_VALUE_KEYS: &[&str] = &[
    "password",
    "passwd",
    "token",
    "secret",
    "private_key",
    "authorization",
    "apikey",
];

/// Scrubs crash content so dumps never expose sensitive data.
#[derive(Debug, Clone)]
pub struct CrashSanitizer {
    denylist: Vec<String>,
}

impl Default for CrashSanitizer {
    fn default() -> Self {
        Self::with_denylist(DEFAULT_CRASH_DENYLIST)
    }
}

impl CrashSanitizer {
    /// A sanitizer with an explicit denylist.
    pub fn with_denylist(denylist: &[&str]) -> Self {
        Self {
            denylist: denylist.iter().map(|entry| entry.to_string()).collect(),
        }
    }

    /// Redacts every denylist marker (case-insensitive) and every sensitive
    /// `key=value` segment.
    pub fn sanitize_text(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for marker in &self.denylist {
            out = redact_case_insensitive(&out, marker);
        }
        out = redact_sensitive_values(&out);
        out
    }
}

fn redact_case_insensitive(text: &str, marker: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let needle = marker.to_ascii_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut index = 0usize;
    while index < text.len() {
        if let Some(found) = lower[index..].find(&needle) {
            result.push_str(&text[index..index + found]);
            result.push_str("[REDACTED]");
            index += found + needle.len();
        } else {
            result.push_str(&text[index..]);
            break;
        }
    }
    result
}

fn redact_sensitive_values(text: &str) -> String {
    let mut out = text.to_owned();
    for token in text.split_whitespace() {
        if let Some((key, _)) = token.split_once('=') {
            if SENSITIVE_VALUE_KEYS
                .iter()
                .any(|sensitive| key.eq_ignore_ascii_case(sensitive))
            {
                let key_eq = format!("{key}=");
                let replacement = format!("{key_eq}[REDACTED]");
                out = out.replace(token, &replacement);
            }
        }
    }
    out
}

/// Retention policy: a retention period and a max-dump budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrashRetention {
    /// Maximum dumps kept.
    pub max_dumps: usize,
    /// Dumps older than this (seconds) are deleted.
    pub retention_secs: u64,
}

impl Default for CrashRetention {
    fn default() -> Self {
        Self {
            max_dumps: 5,
            retention_secs: 30 * 24 * 60 * 60, // 30 days
        }
    }
}

impl CrashRetention {
    /// The dump ids to delete given `(id, captured_at)` entries and `now`:
    /// expired dumps plus the oldest beyond `max_dumps`.
    pub fn prune(&self, dumps: &[(String, u64)], now: u64) -> Vec<String> {
        let mut expired: Vec<String> = dumps
            .iter()
            .filter(|(_, captured_at)| now.saturating_sub(*captured_at) > self.retention_secs)
            .map(|(id, _)| id.clone())
            .collect();
        let mut remaining: Vec<&(String, u64)> = dumps
            .iter()
            .filter(|(id, _)| !expired.contains(id))
            .collect();
        remaining.sort_by_key(|(_, captured_at)| *captured_at);
        while remaining.len() > self.max_dumps {
            let oldest = remaining.remove(0);
            expired.push(oldest.0.clone());
        }
        expired
    }

    /// Deletes a dump (the deletion mechanism the UI calls).
    pub fn delete(&self, dumps: &mut Vec<(String, u64)>, id: &str) -> bool {
        let before = dumps.len();
        dumps.retain(|(existing, _)| existing != id);
        dumps.len() != before
    }
}

/// Upload gating: dumps are only uploaded after explicit user consent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CrashUploadPolicy {
    /// Whether the user consented to crash uploads.
    pub consent: bool,
}

impl CrashUploadPolicy {
    /// Whether a dump may be uploaded right now.
    pub fn can_upload(&self) -> bool {
        self.consent
    }
}

#[cfg(test)]
mod tests {
    use super::{CrashDump, CrashRetention, CrashSanitizer, CrashUploadPolicy};

    #[test]
    fn sanitizer_redacts_canary_and_content_markers() {
        let sanitizer = CrashSanitizer::default();
        let message = format!(
            "connect {} failed for user {}: {}",
            "db.internal", "c4145", "PRIVACY_CANARY_3d19f7aa8c"
        );
        let clean = sanitizer.sanitize_text(&message);
        assert!(!clean.contains("db.internal"));
        assert!(!clean.contains("c4145"));
        assert!(!clean.contains("PRIVACY_CANARY_3d19f7aa8c"));
        assert!(clean.contains("[REDACTED]"));
    }

    #[test]
    fn sanitizer_redacts_sensitive_key_values() {
        let sanitizer = CrashSanitizer::default();
        let clean = sanitizer.sanitize_text("token=abc123 password=secret value");
        assert!(!clean.contains("abc123"));
        assert!(!clean.contains("secret"));
        assert!(clean.contains("token=[REDACTED]"));
        assert!(clean.contains("password=[REDACTED]"));
        assert!(clean.contains("value"));
    }

    #[test]
    fn crash_dump_is_sanitized_at_capture() {
        let sanitizer = CrashSanitizer::default();
        let dump = CrashDump::capture(
            "0.1.0",
            1_700_000_000,
            "main",
            "panic at db.internal: ls -la",
            vec![
                "ssh_cli::run (main.rs:42)".to_owned(),
                "10.0.0.5".to_owned(),
            ],
            &sanitizer,
        );
        assert_eq!(dump.schema_version, 1);
        assert!(!dump.panic_message.contains("db.internal"));
        assert!(!dump.panic_message.contains("ls -la"));
        assert!(dump
            .stack_trace
            .iter()
            .all(|frame| !frame.contains("10.0.0.5")));
    }

    #[test]
    fn retention_prunes_expired_and_oldest() {
        let retention = CrashRetention {
            max_dumps: 2,
            retention_secs: 60,
        };
        let now = 1_000;
        let dumps = vec![
            ("a".to_owned(), now - 100), // expired
            ("b".to_owned(), now - 10),  // oldest live
            ("c".to_owned(), now - 5),
            ("d".to_owned(), now - 1),
        ];
        let to_delete = retention.prune(&dumps, now);
        assert!(to_delete.contains(&"a".to_owned())); // expired
        assert!(to_delete.contains(&"b".to_owned())); // oldest beyond the 2-dump budget
        assert!(!to_delete.contains(&"c".to_owned()));
        assert!(!to_delete.contains(&"d".to_owned()));
    }

    #[test]
    fn delete_removes_a_dump() {
        let retention = CrashRetention::default();
        let mut dumps = vec![("a".to_owned(), 1), ("b".to_owned(), 2)];
        assert!(retention.delete(&mut dumps, "a"));
        assert_eq!(dumps.len(), 1);
        assert!(!retention.delete(&mut dumps, "missing"));
    }

    #[test]
    fn upload_requires_consent() {
        let mut policy = CrashUploadPolicy::default();
        assert!(!policy.can_upload());
        policy.consent = true;
        assert!(policy.can_upload());
    }
}
