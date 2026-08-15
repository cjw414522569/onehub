//! Secure `ssh://` deep-link parsing and explicit confirmation (T132).
//!
//! [`parse_secure`] rejects links that carry a plaintext password in the
//! userinfo, validates the host strictly (no whitespace / control chars /
//! embedded `@` / scheme), strips path and query, and requires explicit
//! confirmation for external sources by default (never auto-connecting).
//! A fuzz corpus of malformed / injection URIs is rejected or sanitized.

/// Why a deep link was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRejection {
    /// The link embeds a plaintext password in the userinfo.
    PlaintextPassword,
    /// The scheme is not `ssh://`.
    UnsupportedScheme,
    /// The host is missing.
    MissingHost,
    /// The host contains invalid characters.
    InvalidHost,
    /// The port is out of range.
    InvalidPort,
}

/// A securely parsed link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecureLink {
    /// The host.
    pub host: String,
    /// The port (1..=65535).
    pub port: u16,
    /// The username, if present.
    pub username: Option<String>,
    /// Whether the user must explicitly confirm before connecting.
    pub requires_confirmation: bool,
}

/// The deep-link source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkSource {
    /// A browser / external app (never auto-connect by default).
    External,
    /// A link from within the app.
    InApp,
}

/// The deep-link policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeepLinkPolicy {
    /// Whether explicit confirmation is required.
    pub require_confirmation: bool,
}

impl DeepLinkPolicy {
    /// The policy for a source: external sources require confirmation.
    pub fn for_source(source: LinkSource) -> Self {
        Self {
            require_confirmation: matches!(source, LinkSource::External),
        }
    }
}

fn is_host_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '_' | '[' | ']' | ':')
}

/// Parses an `ssh://` link securely.
pub fn parse_secure(link: &str, policy: &DeepLinkPolicy) -> Result<SecureLink, LinkRejection> {
    let rest = link
        .strip_prefix("ssh://")
        .ok_or(LinkRejection::UnsupportedScheme)?;
    if rest.is_empty() {
        return Err(LinkRejection::MissingHost);
    }
    // Strip path / query / fragment.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err(LinkRejection::MissingHost);
    }
    let (userinfo, host_port) = match authority.rsplit_once('@') {
        Some((userinfo, host_port)) => (Some(userinfo), host_port),
        None => (None, authority),
    };
    // Plaintext passwords in the userinfo are rejected outright.
    if let Some(userinfo) = userinfo {
        if userinfo.contains(':') {
            return Err(LinkRejection::PlaintextPassword);
        }
    }
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) => (host, port),
        None => (host_port, "22"),
    };
    if host.is_empty() {
        return Err(LinkRejection::MissingHost);
    }
    // Strict host validation: no whitespace / control / '@' / scheme.
    if host.chars().any(|character| {
        character.is_whitespace()
            || (character as u32) < 0x20
            || matches!(character, '@' | '/' | '?' | '#')
    }) {
        return Err(LinkRejection::InvalidHost);
    }
    if !host.chars().all(is_host_char) {
        return Err(LinkRejection::InvalidHost);
    }
    let port: u16 = port.parse().map_err(|_| LinkRejection::InvalidPort)?;
    if port == 0 {
        return Err(LinkRejection::InvalidPort);
    }
    Ok(SecureLink {
        host: host.to_owned(),
        port,
        username: userinfo.map(str::to_owned),
        requires_confirmation: policy.require_confirmation,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_secure, DeepLinkPolicy, LinkRejection, LinkSource};

    fn external() -> DeepLinkPolicy {
        DeepLinkPolicy::for_source(LinkSource::External)
    }

    #[test]
    fn plaintext_passwords_are_rejected() {
        assert_eq!(
            parse_secure("ssh://user:p@ss@host", &external()),
            Err(LinkRejection::PlaintextPassword)
        );
        assert_eq!(
            parse_secure("ssh://user:secret@host:22", &external()),
            Err(LinkRejection::PlaintextPassword)
        );
    }

    #[test]
    fn external_links_require_confirmation_by_default() {
        let link = parse_secure("ssh://example.com", &external()).unwrap();
        assert!(
            link.requires_confirmation,
            "external links never auto-connect"
        );
        let in_app = parse_secure(
            "ssh://example.com",
            &DeepLinkPolicy::for_source(LinkSource::InApp),
        )
        .unwrap();
        assert!(!in_app.requires_confirmation);
    }

    #[test]
    fn injection_corpus_is_rejected_or_sanitized() {
        let corpus = [
            "ssh://ho st",          // whitespace in host
            "ssh://host\n",         // control char
            "ssh://user@host@evil", // embedded @ (host is evil, user is host@evil... actually host after last @)
            "ssh://host:0",         // port 0
            "ssh://host:99999",     // port overflow
            "ssh://host:notaport",  // non-numeric port
            "ssh://",               // missing host
            "https://host",         // wrong scheme
            "ssh://user:pw@h",      // password
            "ssh://-bad host",      // space
        ];
        for link in corpus {
            if let Ok(parsed) = parse_secure(link, &external()) {
                // Sanitized: host never contains whitespace/@/scheme.
                assert!(
                    !parsed.host.chars().any(|c| c.is_whitespace() || c == '@'),
                    "sanitized host must be clean for {link:?}: {}",
                    parsed.host
                );
                assert!(parsed.port >= 1, "port must be valid for {link:?}");
            }
        }
    }

    #[test]
    fn path_and_query_are_stripped() {
        let link =
            parse_secure("ssh://root@10.0.0.5:2222/some/path?x=1#frag", &external()).unwrap();
        assert_eq!(link.host, "10.0.0.5");
        assert_eq!(link.port, 2222);
        assert_eq!(link.username.as_deref(), Some("root"));
    }

    #[test]
    fn host_after_embedded_at_is_used() {
        // The authority's last '@' separates userinfo from host.
        let link = parse_secure("ssh://user%40example@real-host", &external()).unwrap();
        assert_eq!(link.host, "real-host");
        assert_eq!(link.username.as_deref(), Some("user%40example"));
    }
}
