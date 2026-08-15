//! Structured logging, trace ids, and dynamic level control (T146).
//!
//! [`Logger`] emits deterministic structured entries (level, trace id,
//! target, message, ordered fields). The [`SensitiveFieldPolicy`] drops any
//! field whose name matches the denylist (token, password, secret, key,
//! host, username, terminal, ...), so **default logs contain no sensitive
//! fields**. Levels are dynamic: [`Logger::set_level`] controls which
//! entries are emitted at runtime, and [`TraceId`] correlation lets entries
//! across layers be tied to one operation.

/// Log severity levels (ordered: Error < Warn < Info < Debug < Trace).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// Errors.
    Error = 1,
    /// Warnings.
    Warn = 2,
    /// Informational.
    Info = 3,
    /// Debug.
    Debug = 4,
    /// Trace.
    Trace = 5,
}

impl LogLevel {
    /// The stable lower-case name.
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}

/// A correlation id propagated across layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceId(u64);

impl TraceId {
    /// A trace id from an explicit value (tests and canary tooling).
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// A fresh trace id (time + atomic counter; deterministic enough for
    /// correlation without an external RNG dependency).
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0);
        Self(
            now ^ COUNTER
                .fetch_add(1, Ordering::Relaxed)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15),
        )
    }

    /// A derived child id for a nested operation (cross-layer correlation).
    pub fn child(&self) -> Self {
        Self(self.0.wrapping_add(0x9E37_79B9_7F4A_7C15).rotate_left(13))
    }

    /// The stable 16-hex-digit form.
    pub fn hex(&self) -> String {
        format!("{:016x}", self.0)
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

/// A structured log entry (no sensitive fields by construction).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Severity.
    pub level: LogLevel,
    /// Correlation id.
    pub trace_id: TraceId,
    /// The emitting component, e.g. `gateway.session`.
    pub target: String,
    /// The human message (no secrets by policy).
    pub message: String,
    /// Ordered structured fields (sensitive names already dropped).
    pub fields: Vec<(String, String)>,
}

/// A logging context carrying the trace id so child operations correlate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogContext {
    /// The trace id.
    pub trace_id: TraceId,
}

impl LogContext {
    /// A fresh context.
    pub fn new() -> Self {
        Self {
            trace_id: TraceId::new(),
        }
    }

    /// A child context for a nested operation (same trace, derived id).
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.child(),
        }
    }
}

impl Default for LogContext {
    fn default() -> Self {
        Self::new()
    }
}

/// The sensitive-field denylist. Any field whose lower-cased name contains
/// one of these tokens is dropped from the log (never emitted).
#[derive(Debug, Clone)]
pub struct SensitiveFieldPolicy {
    deny_tokens: Vec<String>,
}

impl Default for SensitiveFieldPolicy {
    fn default() -> Self {
        Self {
            deny_tokens: Self::default_denylist(),
        }
    }
}

impl SensitiveFieldPolicy {
    /// The default denylist tokens.
    pub fn default_denylist() -> Vec<String> {
        [
            "token",
            "password",
            "passwd",
            "secret",
            "private_key",
            "key",
            "credential",
            "host",
            "username",
            "user",
            "terminal",
            "command",
            "history",
            "transfer",
        ]
        .iter()
        .map(|token| token.to_string())
        .collect()
    }

    /// Whether a field name is sensitive.
    pub fn is_sensitive(&self, field: &str) -> bool {
        let lower = field.to_ascii_lowercase();
        self.deny_tokens.iter().any(|token| lower.contains(token))
    }
}

/// A structured logger with a dynamic level filter.
#[derive(Debug, Clone)]
pub struct Logger {
    /// The current level threshold (entries above this are dropped).
    level: LogLevel,
    /// The sensitive-field policy.
    policy: SensitiveFieldPolicy,
    /// The emitting target.
    target: String,
}

impl Logger {
    /// A logger at Info level with the default denylist.
    pub fn new(target: &str) -> Self {
        Self {
            level: LogLevel::Info,
            policy: SensitiveFieldPolicy::default(),
            target: target.to_owned(),
        }
    }

    /// The current dynamic level threshold.
    pub fn level(&self) -> LogLevel {
        self.level
    }

    /// Dynamically raises/lowers the level threshold at runtime.
    pub fn set_level(&mut self, level: LogLevel) {
        self.level = level;
    }

    /// Builds a structured entry for `level`, or `None` when the entry is
    /// below the current threshold. Sensitive fields are dropped.
    pub fn log(
        &self,
        level: LogLevel,
        trace_id: TraceId,
        message: &str,
        fields: &[(&str, &str)],
    ) -> Option<LogEntry> {
        if level > self.level {
            return None;
        }
        let fields = fields
            .iter()
            .filter(|(name, _)| !self.policy.is_sensitive(name))
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        Some(LogEntry {
            level,
            trace_id,
            target: self.target.clone(),
            message: message.to_owned(),
            fields,
        })
    }

    /// Renders an entry as a deterministic structured line (values are
    /// escaped so a value can never inject a new log line).
    pub fn render(&self, entry: &LogEntry) -> String {
        let fields = entry
            .fields
            .iter()
            .map(|(name, value)| format!("{}={}", name, escape_value(value)))
            .collect::<Vec<_>>()
            .join(" ");
        let prefix = if fields.is_empty() {
            String::new()
        } else {
            format!(" {fields}")
        };
        format!(
            "level={} trace={} target={} message={}{}",
            entry.level.as_str(),
            entry.trace_id.hex(),
            entry.target,
            escape_value(&entry.message),
            prefix
        )
    }
}

/// Escapes a value for the structured line (no whitespace injection).
pub fn escape_value(value: &str) -> String {
    let escaped: String = value
        .chars()
        .map(|ch| match ch {
            ' ' | '\t' | '\n' | '\r' | '=' => '_',
            ch => ch,
        })
        .collect();
    if escaped.is_empty() {
        "\"\"".to_owned()
    } else {
        escaped
    }
}

#[cfg(test)]
mod tests {
    use super::{LogContext, LogLevel, Logger, SensitiveFieldPolicy, TraceId};

    #[test]
    fn dynamic_level_control_filters_below_threshold() {
        let mut logger = Logger::new("test.target");
        let trace = TraceId(1);
        assert!(logger.log(LogLevel::Info, trace, "visible", &[]).is_some());
        // Debug is below the Info threshold: dropped.
        assert!(logger.log(LogLevel::Debug, trace, "hidden", &[]).is_none());
        // Raise the threshold dynamically: everything is now emitted.
        logger.set_level(LogLevel::Trace);
        assert!(logger.log(LogLevel::Trace, trace, "trace", &[]).is_some());
        // Lower it: only errors survive.
        logger.set_level(LogLevel::Error);
        assert!(logger.log(LogLevel::Info, trace, "gone", &[]).is_none());
        assert!(logger.log(LogLevel::Error, trace, "boom", &[]).is_some());
        assert_eq!(logger.level(), LogLevel::Error);
    }

    #[test]
    fn sensitive_fields_are_dropped_by_default() {
        let logger = Logger::new("gateway.session");
        let trace = TraceId(7);
        let entry = logger
            .log(
                LogLevel::Info,
                trace,
                "session started",
                &[
                    ("session_id", "abc"),
                    ("auth_token", "SECRET_TOKEN_123"),
                    ("target_host", "db.internal"),
                    ("status", "ok"),
                ],
            )
            .unwrap();
        // The sensitive fields (auth_token, target_host) never appear.
        assert!(entry
            .fields
            .iter()
            .all(|(name, _)| name != "auth_token" && name != "target_host"));
        assert!(entry
            .fields
            .iter()
            .any(|(name, value)| name == "session_id" && value == "abc"));
        assert!(entry
            .fields
            .iter()
            .any(|(name, value)| name == "status" && value == "ok"));
        let rendered = logger.render(&entry);
        assert!(!rendered.contains("SECRET_TOKEN_123"));
        assert!(!rendered.contains("db.internal"));
    }

    #[test]
    fn policy_denylist_covers_credential_tokens() {
        let policy = SensitiveFieldPolicy::default();
        for sensitive in [
            "token",
            "password",
            "private_key",
            "credential",
            "username",
            "terminal_text",
            "command_history",
        ] {
            assert!(
                policy.is_sensitive(sensitive),
                "{sensitive} must be sensitive"
            );
        }
        for safe in ["status", "session_id", "latency_ms", "error_code", "bytes"] {
            assert!(!policy.is_sensitive(safe), "{safe} must not be sensitive");
        }
    }

    #[test]
    fn trace_ids_correlate_across_layers() {
        let context = LogContext::new();
        let parent = context.trace_id;
        let child = context.child().trace_id;
        assert_ne!(parent, child);
        assert_eq!(parent.hex().len(), 16);
        // The rendered entries share the correlation chain: child.hex() is
        // derived from parent so downstream layers can join on it.
        assert_ne!(parent.child().hex(), parent.hex());
        assert_eq!(parent.child().child().hex().len(), 16);
    }

    #[test]
    fn render_is_deterministic_and_line_safe() {
        let logger = Logger::new("cli.exec");
        let entry = logger
            .log(
                LogLevel::Info,
                TraceId(0x1234),
                "exec completed",
                &[("status", "ok"), ("bytes", "1024")],
            )
            .unwrap();
        let line = logger.render(&entry);
        assert_eq!(
            line,
            "level=info trace=0000000000001234 target=cli.exec message=exec_completed status=ok bytes=1024"
        );
        // Values with spaces/newlines are escaped so one entry = one line.
        let weird = logger
            .log(LogLevel::Warn, TraceId(2), "odd", &[("detail", "a b\nc=d")])
            .unwrap();
        let rendered = logger.render(&weird);
        assert!(!rendered.contains('\n'));
        assert!(rendered.contains("detail=a_b_c_d"));
    }
}
