//! OSC 8 hyperlink policy and explicit open-confirmation support (T067).
//!
//! Terminal output can embed hyperlinks via `OSC 8;;<uri>`. Before a link is
//! stored (and later opened), [`HyperlinkPolicy`] enforces a scheme whitelist
//! and a length cap, and exposes [`HyperlinkPolicy::review`] so the UI can
//! show the *effective* target (the host actually reached) for an explicit
//! open confirmation. Dangerous schemes (`javascript:`, `data:`, ...) are
//! forbidden outright.

/// A human-reviewable view of a hyperlink for the open-confirmation dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyperlinkReview {
    /// The raw URI exactly as received (shown so the user can inspect it).
    pub uri: String,
    /// Lowercased URI scheme.
    pub scheme: String,
    /// The host actually reached after stripping userinfo and port, if any.
    pub effective_host: Option<String>,
}

/// Policy controlling which OSC 8 hyperlinks are stored and openable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyperlinkPolicy {
    /// Lowercase schemes that may be opened (whitelist).
    pub allowed_schemes: Vec<String>,
    /// Maximum URI length (chars); longer URIs are rejected.
    pub max_uri_len: usize,
}

impl Default for HyperlinkPolicy {
    /// Safe default whitelist: no `javascript:`, `data:`, `vbscript:`, or
    /// `file:` execution vectors.
    fn default() -> Self {
        Self {
            allowed_schemes: vec![
                "https".to_owned(),
                "http".to_owned(),
                "ssh".to_owned(),
                "sftp".to_owned(),
                "mailto".to_owned(),
            ],
            max_uri_len: 2048,
        }
    }
}

/// Extracts the lowercased scheme of a URI (`scheme:` prefix), if valid.
pub fn scheme_of(uri: &str) -> Option<&str> {
    let end = uri.find(':')?;
    let scheme = &uri[..end];
    if scheme.is_empty() {
        return None;
    }
    let mut chars = scheme.chars();
    let first_ok = chars
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false);
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if first_ok && rest_ok {
        Some(scheme)
    } else {
        None
    }
}

/// Extracts the effective host of a hierarchical URI (`scheme://authority/`),
/// stripping userinfo (the `user@` prefix) and port. Non-hierarchical URIs
/// (e.g. `mailto:`) return `None`.
pub fn effective_host(uri: &str) -> Option<String> {
    let after_scheme = uri.split_once(':')?.1;
    let authority = after_scheme.strip_prefix("//")?;
    let authority = authority.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

impl HyperlinkPolicy {
    /// Whether a URI scheme is on the whitelist.
    pub fn scheme_allowed(&self, scheme: &str) -> bool {
        self.allowed_schemes
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(scheme))
    }

    /// Whether the URI may be stored and opened (scheme whitelist + length
    /// cap). Dangerous schemes never pass.
    pub fn can_open(&self, uri: &str) -> bool {
        if uri.chars().count() > self.max_uri_len {
            return false;
        }
        match scheme_of(uri) {
            Some(scheme) => self.scheme_allowed(scheme),
            None => false,
        }
    }

    /// Builds the review data for an explicit open confirmation. Returns
    /// `None` for forbidden URIs, so the UI never offers to open them.
    pub fn review(&self, uri: &str) -> Option<HyperlinkReview> {
        if !self.can_open(uri) {
            return None;
        }
        Some(HyperlinkReview {
            uri: uri.to_owned(),
            scheme: scheme_of(uri).unwrap_or_default().to_ascii_lowercase(),
            effective_host: effective_host(uri),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{effective_host, scheme_of, HyperlinkPolicy};

    #[test]
    fn scheme_whitelist_blocks_dangerous_schemes() {
        let policy = HyperlinkPolicy::default();
        for allowed in [
            "https://example.com/a",
            "http://example.com",
            "ssh://host",
            "sftp://host/p",
            "mailto:user@example.com",
        ] {
            assert!(policy.can_open(allowed), "{allowed} should be allowed");
        }
        for denied in [
            "javascript:alert(1)",
            "data:text/html;base64,PHNjcmlwdD4=",
            "vbscript:msgbox(1)",
            "file:///etc/passwd",
            "ftp://host/file",
        ] {
            assert!(!policy.can_open(denied), "{denied} must be denied");
        }
    }

    #[test]
    fn scheme_and_host_parsing() {
        assert_eq!(scheme_of("HTTPS://Example.COM/path"), Some("HTTPS"));
        assert_eq!(
            effective_host("https://user:pass@Evil.com:8443/path").as_deref(),
            Some("evil.com")
        );
        assert_eq!(effective_host("ssh://host").as_deref(), Some("host"));
        assert_eq!(effective_host("mailto:user@example.com"), None);
        assert_eq!(scheme_of("no-scheme"), None);
    }

    #[test]
    fn phishing_url_surfaces_effective_host() {
        let policy = HyperlinkPolicy::default();
        // The visible label claims example.com but the link targets evil.com;
        // the review must surface evil.com for the open confirmation.
        let review = policy
            .review("https://example.com@evil.com/path")
            .expect("scheme allowed");
        assert_eq!(review.scheme, "https");
        assert_eq!(review.effective_host.as_deref(), Some("evil.com"));
        assert_eq!(review.uri, "https://example.com@evil.com/path");
    }

    #[test]
    fn forbidden_and_oversized_uris_have_no_review() {
        let policy = HyperlinkPolicy::default();
        assert_eq!(policy.review("javascript:alert(1)"), None);
        let long = format!("https://example.com/{}", "x".repeat(3000));
        assert!(!policy.can_open(&long));
        assert_eq!(policy.review(&long), None);
    }
}
