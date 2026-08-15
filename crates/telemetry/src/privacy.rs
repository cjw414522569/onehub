//! Privacy-preserving telemetry (T147): default-off consent and a public
//! collection dictionary.
//!
//! Telemetry is **off by default** ([`TelemetryConsent::DefaultOff`]) and
//! only leaves the machine after explicit user consent. The public schema
//! ([`TELEMETRY_SCHEMA`]) allowlists every event and field that may be
//! collected; the collector rejects any field outside the allowlist and any
//! field whose name touches terminal content, commands, identity, or host
//! data (defense in depth).

/// Consent state for telemetry collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TelemetryConsent {
    /// Telemetry is off by default: nothing is collected or sent.
    #[default]
    DefaultOff,
    /// The user explicitly opted in; allowlisted events may be collected.
    ExplicitConsent,
}

/// One event in the public collection dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TelemetryEventSpec {
    /// Stable event name.
    pub name: &'static str,
    /// Allowlisted field names for this event.
    pub fields: &'static [&'static str],
}

/// The public collection dictionary (the "采集字典"). It NEVER contains
/// terminal content, commands, identity, or host data.
pub const TELEMETRY_SCHEMA: &[TelemetryEventSpec] = &[
    TelemetryEventSpec {
        name: "app_start",
        fields: &["platform", "app_version", "build_channel"],
    },
    TelemetryEventSpec {
        name: "app_crash",
        fields: &["crash_count"],
    },
    TelemetryEventSpec {
        name: "session_duration_secs",
        fields: &["duration_secs"],
    },
    TelemetryEventSpec {
        name: "feature_used",
        fields: &["feature"],
    },
    TelemetryEventSpec {
        name: "gateway_latency_ms",
        fields: &["latency_ms", "region"],
    },
];

/// The public "never collected" inventory: data classes that must never
/// appear in telemetry.
pub const NEVER_COLLECTED: &[&str] = &[
    "terminal content",
    "terminal cell data",
    "commands",
    "command history",
    "identity",
    "user identity",
    "host names",
    "host addresses",
    "credentials",
    "keys",
    "tokens",
    "session payload",
    "transfer payload",
];

/// A field that would be sent outbound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundEvent {
    /// Event name.
    pub name: String,
    /// Allowlisted fields (never sensitive).
    pub fields: Vec<(String, String)>,
}

/// Whether a field name touches terminal / command / identity / host data.
fn is_forbidden_field(field: &str) -> bool {
    let lower = field.to_ascii_lowercase();
    [
        "terminal", "command", "identity", "host", "user", "hostname", "key", "token", "secret",
        "password", "payload", "content",
    ]
    .iter()
    .any(|token| lower.contains(token))
}

/// The collector. `consent` defaults to off; only explicitly consented,
/// allowlisted, non-sensitive fields may be captured.
#[derive(Debug, Clone, Copy, Default)]
pub struct TelemetryCollector {
    consent: TelemetryConsent,
}

impl TelemetryCollector {
    /// A collector with the given consent (default-off).
    pub fn new(consent: TelemetryConsent) -> Self {
        Self { consent }
    }

    /// The current consent.
    pub fn consent(&self) -> TelemetryConsent {
        self.consent
    }

    /// Explicitly sets consent (default stays off).
    pub fn set_consent(&mut self, consent: TelemetryConsent) {
        self.consent = consent;
    }

    /// Captures an event for outbound transmission, or `None` when consent
    /// is off, the event is not in the public schema, or any field is
    /// outside the allowlist / touches forbidden data.
    pub fn collect(&self, event: &str, fields: &[(&str, &str)]) -> Option<OutboundEvent> {
        if self.consent == TelemetryConsent::DefaultOff {
            return None;
        }
        let spec = TELEMETRY_SCHEMA.iter().find(|spec| spec.name == event)?;
        let mut allowed: Vec<(String, String)> = Vec::new();
        for (name, value) in fields {
            // Defense in depth: any field touching terminal / command /
            // identity / host data hard-rejects the whole event.
            if is_forbidden_field(name) {
                return None;
            }
            if !spec.fields.contains(name) {
                continue; // not in the allowlist: dropped
            }
            allowed.push((name.to_string(), value.to_string()));
        }
        Some(OutboundEvent {
            name: event.to_owned(),
            fields: allowed,
        })
    }

    /// The rendered outbound line (deterministic, escaped).
    pub fn render(&self, event: &OutboundEvent) -> String {
        let fields = event
            .fields
            .iter()
            .map(|(name, value)| format!("{name}={}", escape(value)))
            .collect::<Vec<_>>()
            .join(" ");
        format!("telemetry:event={} {}", event.name, fields)
            .trim_end()
            .to_owned()
    }
}

/// Escapes an outbound value (no whitespace injection).
fn escape(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_owned();
    }
    value
        .chars()
        .map(|ch| match ch {
            ' ' | '\t' | '\n' | '\r' | '=' => '_',
            ch => ch,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{TelemetryCollector, TelemetryConsent, TELEMETRY_SCHEMA};

    #[test]
    fn telemetry_is_off_by_default() {
        let collector = TelemetryCollector::default();
        assert_eq!(collector.consent(), TelemetryConsent::DefaultOff);
        // Nothing is captured while consent is off.
        assert!(collector
            .collect(
                "app_start",
                &[("platform", "windows"), ("app_version", "0.1.0")]
            )
            .is_none());
    }

    #[test]
    fn explicit_consent_collects_only_allowlisted_fields() {
        let mut collector = TelemetryCollector::new(TelemetryConsent::ExplicitConsent);
        collector.set_consent(TelemetryConsent::ExplicitConsent);
        let event = collector
            .collect(
                "app_start",
                &[
                    ("platform", "windows"),
                    ("app_version", "0.1.0"),
                    ("build_channel", "stable"),
                    ("extra_not_in_schema", "dropped"),
                ],
            )
            .unwrap();
        assert_eq!(event.name, "app_start");
        assert_eq!(event.fields.len(), 3);
        assert!(event
            .fields
            .iter()
            .all(|(name, _)| name != "extra_not_in_schema"));
    }

    #[test]
    fn unknown_event_is_rejected() {
        let collector = TelemetryCollector::new(TelemetryConsent::ExplicitConsent);
        assert!(collector.collect("not_in_schema", &[]).is_none());
    }

    #[test]
    fn terminal_command_identity_host_fields_are_rejected() {
        let collector = TelemetryCollector::new(TelemetryConsent::ExplicitConsent);
        // These would violate the public dictionary: they must never be
        // captured even though `feature_used` is a real event.
        for forbidden in [
            "terminal_text",
            "command",
            "command_history",
            "user_identity",
            "host_name",
            "host_address",
            "private_key",
            "auth_token",
        ] {
            assert!(
                collector
                    .collect("feature_used", &[(forbidden, "secret-value")])
                    .is_none(),
                "{forbidden} must never be captured"
            );
        }
    }

    #[test]
    fn render_and_escape_are_deterministic() {
        let collector = TelemetryCollector::new(TelemetryConsent::ExplicitConsent);
        let event = collector
            .collect(
                "app_start",
                &[("platform", "windows"), ("app_version", "x y=z")],
            )
            .unwrap();
        let line = collector.render(&event);
        // Values are escaped: no whitespace injection, empty values render.
        assert_eq!(
            line,
            "telemetry:event=app_start platform=windows app_version=x_y_z"
        );
        let empty = collector
            .collect("feature_used", &[("feature", "")])
            .unwrap();
        assert_eq!(
            collector.render(&empty),
            "telemetry:event=feature_used feature=\"\""
        );
    }

    #[test]
    fn consent_getter_and_setter_are_reflected() {
        let mut collector = TelemetryCollector::default();
        assert_eq!(collector.consent(), TelemetryConsent::DefaultOff);
        collector.set_consent(TelemetryConsent::ExplicitConsent);
        assert_eq!(collector.consent(), TelemetryConsent::ExplicitConsent);
        collector.set_consent(TelemetryConsent::DefaultOff);
        assert!(collector.collect("app_start", &[]).is_none());
    }

    #[test]
    fn schema_is_public_and_clean() {
        // The dictionary never contains terminal/command/identity/host data.
        let forbidden = [
            "terminal", "command", "identity", "host", "user", "key", "token", "secret",
            "password", "payload",
        ];
        assert!(!TELEMETRY_SCHEMA.is_empty());
        for spec in TELEMETRY_SCHEMA {
            for field in spec.fields {
                let lower = field.to_ascii_lowercase();
                for token in &forbidden {
                    assert!(
                        !lower.contains(token),
                        "schema field '{field}' in {} touches forbidden data ({token})",
                        spec.name
                    );
                }
            }
        }
    }
}
