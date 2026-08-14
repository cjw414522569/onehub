//! OSC policy and security filtering (T066).
//!
//! OSC sequences can set the window title (OSC 0/2), the working directory
//! (OSC 7), or request a desktop notification (OSC 9 / OSC 777;notify).
//! Untrusted terminal output must not bypass the title / notification
//! policy: [`OscPolicy`] gates each kind, strips control characters, and caps
//! lengths before the model stores anything. Notifications are denied by
//! default (secure default; opt in explicitly).

/// A desktop notification requested by an OSC sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// Short summary line.
    pub summary: String,
    /// Optional body text.
    pub body: String,
}

/// Policy for OSC-derived state and side effects.
///
/// Denied kinds are dropped entirely (no title change, no stored directory,
/// no notification). Allowed payloads are sanitized (control characters
/// removed) and length-capped so untrusted input cannot grow state without
/// bound or smuggle terminal control bytes into the UI layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OscPolicy {
    /// Allow OSC 0/2 window-title changes.
    pub allow_title: bool,
    /// Allow OSC 7 working-directory updates.
    pub allow_working_directory: bool,
    /// Allow OSC 9 / OSC 777 desktop notifications.
    pub allow_notifications: bool,
    /// Maximum title length (chars).
    pub max_title_len: usize,
    /// Maximum working-directory length (chars).
    pub max_working_directory_len: usize,
    /// Maximum notification summary length (chars).
    pub max_notification_summary_len: usize,
    /// Maximum notification body length (chars).
    pub max_notification_body_len: usize,
}

impl Default for OscPolicy {
    /// Secure defaults: titles and the working directory are allowed (and
    /// sanitized); notifications are denied until explicitly enabled.
    fn default() -> Self {
        Self {
            allow_title: true,
            allow_working_directory: true,
            allow_notifications: false,
            max_title_len: 512,
            max_working_directory_len: 4096,
            max_notification_summary_len: 256,
            max_notification_body_len: 1024,
        }
    }
}

/// Removes control characters and caps `raw` at `max_len` chars.
fn sanitize_text(raw: &str, max_len: usize) -> String {
    raw.chars()
        .filter(|ch| !ch.is_control())
        .take(max_len)
        .collect()
}

impl OscPolicy {
    /// Sanitizes a window title, or `None` when titles are denied.
    pub fn sanitize_title(&self, raw: &str) -> Option<String> {
        if !self.allow_title {
            return None;
        }
        let title = sanitize_text(raw, self.max_title_len);
        if title.is_empty() {
            None
        } else {
            Some(title)
        }
    }

    /// Sanitizes a working-directory payload, or `None` when denied.
    pub fn sanitize_working_directory(&self, raw: &str) -> Option<String> {
        if !self.allow_working_directory {
            return None;
        }
        let directory = sanitize_text(raw, self.max_working_directory_len);
        if directory.is_empty() {
            None
        } else {
            Some(directory)
        }
    }

    /// Sanitizes a notification, or `None` when notifications are denied.
    pub fn sanitize_notification(&self, summary: &str, body: &str) -> Option<Notification> {
        if !self.allow_notifications {
            return None;
        }
        let summary = sanitize_text(summary, self.max_notification_summary_len);
        let body = sanitize_text(body, self.max_notification_body_len);
        if summary.is_empty() && body.is_empty() {
            return None;
        }
        Some(Notification { summary, body })
    }
}

#[cfg(test)]
mod tests {
    use super::{Notification, OscPolicy};

    #[test]
    fn title_is_sanitized_and_capped() {
        let policy = OscPolicy::default();
        // Control characters (ESC, BEL, newline) are stripped; the remaining
        // text (including literal `]0;`) is kept as plain data.
        assert_eq!(
            policy
                .sanitize_title("prod\x1b]0;injected\x07shell\n")
                .as_deref(),
            Some("prod]0;injectedshell")
        );
        // Length cap.
        let long = "x".repeat(1000);
        assert_eq!(policy.sanitize_title(&long).unwrap().chars().count(), 512);
    }

    #[test]
    fn title_denied_when_policy_blocks() {
        let policy = OscPolicy {
            allow_title: false,
            ..OscPolicy::default()
        };
        assert_eq!(policy.sanitize_title("anything"), None);
    }

    #[test]
    fn working_directory_is_sanitized() {
        let policy = OscPolicy::default();
        assert_eq!(
            policy
                .sanitize_working_directory("file:///home/user/project\x00hidden")
                .as_deref(),
            Some("file:///home/user/projecthidden")
        );
        assert_eq!(policy.sanitize_working_directory(""), None);
    }

    #[test]
    fn notifications_denied_by_default() {
        let policy = OscPolicy::default();
        assert!(!policy.allow_notifications);
        assert_eq!(
            policy.sanitize_notification("spam", "click here"),
            None,
            "default policy must not surface untrusted notifications"
        );
    }

    #[test]
    fn notifications_sanitized_when_allowed() {
        let policy = OscPolicy {
            allow_notifications: true,
            ..OscPolicy::default()
        };
        assert_eq!(
            policy.sanitize_notification("build\x1b done", "0 errors\x00"),
            Some(Notification {
                summary: "build done".to_owned(),
                body: "0 errors".to_owned(),
            })
        );
        assert_eq!(policy.sanitize_notification("", ""), None);
        // Caps apply per field.
        let summary = "s".repeat(1000);
        let body = "b".repeat(2000);
        let notification = policy
            .sanitize_notification(&summary, &body)
            .expect("allowed");
        assert_eq!(notification.summary.chars().count(), 256);
        assert_eq!(notification.body.chars().count(), 1024);
    }
}
