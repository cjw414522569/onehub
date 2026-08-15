//! Secure paste confirmation, multi-line warning, and bracketed paste (T110).
//!
//! [`PasteContent::analyze`] inspects clipboard text for newlines, control
//! characters, suspicious shell fragments, and size. [`SecurePasteFlow`]
//! applies a configurable [`PastePolicy`] and returns an
//! [`Allow` / `Confirm` / `Block`] decision with a **preview** of the exact
//! payload (control chars escaped, truncated) so a potential command
//! injection is visible before it is pasted. Password pasting has its own
//! policy. When the terminal supports bracketed paste, the payload is wrapped
//! in `ESC[200~ ... ESC[201~`.

/// How risky a paste is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteRisk {
    /// No risk flags.
    Safe,
    /// Contains newlines (multi-line).
    ContainsNewlines,
    /// Contains control characters.
    ContainsControlChars,
    /// Looks like a shell command injection.
    SuspiciousCommand,
    /// Larger than the configured maximum.
    Huge,
}

/// Analyzed clipboard content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteContent {
    /// The raw text.
    pub text: String,
    /// Byte length.
    pub len: usize,
    /// Number of newline characters.
    pub newlines: usize,
    /// Number of control characters (excluding newline / tab).
    pub control_chars: usize,
    /// Whether a suspicious shell fragment was found.
    pub suspicious: bool,
}

/// Shell fragments that suggest command injection.
const SUSPICIOUS_FRAGMENTS: &[&str] = &[
    "rm ", "&&", "||", "$(", "`", "| sh", "|bash", "sudo ", "wget ", "curl ",
];

impl PasteContent {
    /// Analyzes clipboard text.
    pub fn analyze(text: &str) -> Self {
        let mut newlines = 0;
        let mut control_chars = 0;
        for character in text.chars() {
            match character {
                '\n' => newlines += 1,
                '\t' | '\r' => {}
                character if (character as u32) < 0x20 => control_chars += 1,
                _ => {}
            }
        }
        let suspicious = SUSPICIOUS_FRAGMENTS
            .iter()
            .any(|fragment| text.contains(fragment));
        Self {
            text: text.to_owned(),
            len: text.len(),
            newlines,
            control_chars,
            suspicious,
        }
    }

    /// The highest-risk flag for a policy's maximum paste size.
    pub fn risk(&self, max_bytes: usize) -> PasteRisk {
        if self.len > max_bytes {
            return PasteRisk::Huge;
        }
        if self.suspicious {
            return PasteRisk::SuspiciousCommand;
        }
        if self.control_chars > 0 {
            return PasteRisk::ContainsControlChars;
        }
        if self.newlines > 0 {
            return PasteRisk::ContainsNewlines;
        }
        PasteRisk::Safe
    }

    /// A preview with control characters escaped and long content truncated.
    pub fn preview(&self, max_chars: usize) -> String {
        let escaped: String = self
            .text
            .chars()
            .take(max_chars)
            .map(|character| match character {
                '\x1b' => "␛".to_owned(),
                '\n' => "␊".to_owned(),
                '\t' => "␉".to_owned(),
                '\r' => "␍".to_owned(),
                character if (character as u32) < 0x20 => {
                    format!("^{}", (character as u8 + 0x40) as char)
                }
                other => other.to_string(),
            })
            .collect();
        if self.len > max_chars {
            format!("{escaped}… ({} bytes)", self.len)
        } else {
            escaped
        }
    }
}

/// How password pasting is treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordPastePolicy {
    /// Paste without confirmation.
    Allow,
    /// Require confirmation.
    Confirm,
    /// Block entirely.
    Block,
}

/// The paste policy (configurable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PastePolicy {
    /// Confirm multi-line pastes.
    pub confirm_newlines: bool,
    /// Confirm pastes with control characters.
    pub confirm_control_chars: bool,
    /// Confirm pastes with suspicious shell fragments.
    pub confirm_suspicious: bool,
    /// Maximum paste size in bytes.
    pub max_paste_bytes: usize,
    /// Password-paste policy.
    pub password_policy: PasswordPastePolicy,
}

impl PastePolicy {
    /// The default policy: confirm newlines/control/suspicious, 1 MiB max,
    /// password paste requires confirmation.
    pub fn defaults() -> Self {
        Self {
            confirm_newlines: true,
            confirm_control_chars: true,
            confirm_suspicious: true,
            max_paste_bytes: 1024 * 1024,
            password_policy: PasswordPastePolicy::Confirm,
        }
    }
}

/// The bracketed-paste begin marker.
pub const BRACKETED_PASTE_BEGIN: &[u8] = b"\x1b[200~";
/// The bracketed-paste end marker.
pub const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";

/// The payload to paste (bracketed when the terminal supports it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastePayload {
    /// The bracketed-paste payload bytes.
    pub bracketed: Vec<u8>,
    /// The raw text.
    pub raw: String,
}

/// The paste decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteDecision {
    /// Paste directly.
    Allow(PastePayload),
    /// Show a preview and ask the user to confirm.
    Confirm {
        /// The preview string.
        preview: String,
        /// Reasons for the confirmation.
        reasons: Vec<&'static str>,
    },
    /// Refuse the paste.
    Block(&'static str),
}

/// The secure paste flow.
pub struct SecurePasteFlow;

impl SecurePasteFlow {
    /// Wraps text in bracketed-paste markers.
    pub fn bracketed_payload(text: &str) -> Vec<u8> {
        let mut payload = Vec::with_capacity(
            BRACKETED_PASTE_BEGIN.len() + text.len() + BRACKETED_PASTE_END.len(),
        );
        payload.extend_from_slice(BRACKETED_PASTE_BEGIN);
        payload.extend_from_slice(text.as_bytes());
        payload.extend_from_slice(BRACKETED_PASTE_END);
        payload
    }

    /// Evaluates a paste against a policy.
    pub fn evaluate(
        content: &PasteContent,
        policy: &PastePolicy,
        is_password_field: bool,
    ) -> PasteDecision {
        let payload = || PastePayload {
            bracketed: Self::bracketed_payload(&content.text),
            raw: content.text.clone(),
        };
        let risk = content.risk(policy.max_paste_bytes);

        // Password fields follow the dedicated policy.
        if is_password_field {
            return match policy.password_policy {
                PasswordPastePolicy::Allow => PasteDecision::Allow(payload()),
                PasswordPastePolicy::Confirm => PasteDecision::Confirm {
                    preview: content.preview(80),
                    reasons: vec!["password paste requires confirmation"],
                },
                PasswordPastePolicy::Block => {
                    PasteDecision::Block("password paste is disabled by policy")
                }
            };
        }

        // Non-password fields.
        let mut reasons = Vec::new();
        match risk {
            PasteRisk::Huge => reasons.push("clipboard is larger than the configured maximum"),
            PasteRisk::SuspiciousCommand if policy.confirm_suspicious => {
                reasons.push("content looks like a shell command injection")
            }
            PasteRisk::ContainsControlChars if policy.confirm_control_chars => {
                reasons.push("content contains control characters")
            }
            PasteRisk::ContainsNewlines if policy.confirm_newlines => {
                reasons.push("content contains multiple lines")
            }
            _ => {}
        }
        if reasons.is_empty() {
            PasteDecision::Allow(payload())
        } else {
            PasteDecision::Confirm {
                preview: content.preview(120),
                reasons,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PasswordPastePolicy, PasteContent, PasteDecision, PastePolicy, PasteRisk, SecurePasteFlow,
        BRACKETED_PASTE_BEGIN, BRACKETED_PASTE_END,
    };

    #[test]
    fn analyze_detects_newlines_control_and_suspicious() {
        let multi_line = PasteContent::analyze("line1\nline2");
        assert_eq!(multi_line.newlines, 1);
        assert_eq!(multi_line.control_chars, 0);
        assert!(!multi_line.suspicious);

        let control = PasteContent::analyze("abc\x1b[2J");
        assert_eq!(control.control_chars, 1);
        assert_eq!(control.newlines, 0);

        let injected = PasteContent::analyze("echo x; rm -rf /");
        assert!(injected.suspicious);
        assert_eq!(injected.risk(1_000_000), PasteRisk::SuspiciousCommand);
    }

    #[test]
    fn preview_escapes_and_truncates() {
        let control = PasteContent::analyze("a\x1bb\nc");
        assert_eq!(control.preview(100), "a␛b␊c");
        let huge = PasteContent::analyze(&"x".repeat(1000));
        let preview = huge.preview(10);
        assert!(preview.starts_with("xxxxxxxxxx"));
        assert!(preview.contains("1000 bytes"));
    }

    #[test]
    fn decision_matrix_follows_policy() {
        let policy = PastePolicy::defaults();
        // Multi-line -> confirm.
        let multi = PasteContent::analyze("a\nb");
        assert!(matches!(
            SecurePasteFlow::evaluate(&multi, &policy, false),
            PasteDecision::Confirm { reasons, .. } if reasons.contains(&"content contains multiple lines")
        ));
        // Safe -> allow.
        let safe = PasteContent::analyze("hello");
        assert!(matches!(
            SecurePasteFlow::evaluate(&safe, &policy, false),
            PasteDecision::Allow(_)
        ));
        // Suspicious -> confirm with preview.
        let injected = PasteContent::analyze("x && rm -rf /");
        match SecurePasteFlow::evaluate(&injected, &policy, false) {
            PasteDecision::Confirm { preview, reasons } => {
                assert!(reasons.iter().any(|r| r.contains("injection")));
                assert!(!preview.is_empty());
            }
            other => panic!("expected confirm, got {other:?}"),
        }
        // Disabling confirmation allows the same content.
        let lenient = PastePolicy {
            confirm_newlines: false,
            confirm_control_chars: false,
            confirm_suspicious: false,
            ..policy
        };
        assert!(matches!(
            SecurePasteFlow::evaluate(&injected, &lenient, false),
            PasteDecision::Allow(_)
        ));
    }

    #[test]
    fn password_paste_policy_is_configurable() {
        let content = PasteContent::analyze("p@ss word");
        for (policy, expected) in [
            (PasswordPastePolicy::Allow, true),
            (PasswordPastePolicy::Confirm, false),
            (PasswordPastePolicy::Block, false),
        ] {
            let config = PastePolicy {
                password_policy: policy,
                ..PastePolicy::defaults()
            };
            match SecurePasteFlow::evaluate(&content, &config, true) {
                PasteDecision::Allow(_) => assert!(expected, "Allow policy must allow"),
                PasteDecision::Confirm { .. } => assert!(!expected, "Confirm policy must confirm"),
                PasteDecision::Block(reason) => {
                    assert!(!expected);
                    assert!(reason.contains("disabled"));
                }
            }
        }
    }

    #[test]
    fn bracketed_paste_wraps_payload() {
        let payload = SecurePasteFlow::bracketed_payload("abc");
        let mut expected = Vec::new();
        expected.extend_from_slice(BRACKETED_PASTE_BEGIN);
        expected.extend_from_slice(b"abc");
        expected.extend_from_slice(BRACKETED_PASTE_END);
        assert_eq!(payload, expected);
        // An Allow decision carries the bracketed payload.
        let safe = PasteContent::analyze("abc");
        let policy = PastePolicy::defaults();
        match SecurePasteFlow::evaluate(&safe, &policy, false) {
            PasteDecision::Allow(payload) => assert_eq!(payload.bracketed, expected),
            other => panic!("expected allow, got {other:?}"),
        }
    }

    #[test]
    fn huge_clipboard_is_flagged() {
        let policy = PastePolicy {
            max_paste_bytes: 100,
            ..PastePolicy::defaults()
        };
        let huge = PasteContent::analyze(&"y".repeat(10_000));
        assert_eq!(huge.risk(100), PasteRisk::Huge);
        match SecurePasteFlow::evaluate(&huge, &policy, false) {
            PasteDecision::Confirm { preview, reasons } => {
                assert!(reasons.iter().any(|r| r.contains("larger")));
                assert!(preview.contains("10000 bytes"));
            }
            other => panic!("expected confirm for huge paste, got {other:?}"),
        }
    }
}
