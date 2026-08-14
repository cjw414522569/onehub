use base64::Engine as _;
use core_domain::host_key::KnownHostsEntry;
use hmac::{Hmac, Mac};
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

/// Matches a hostname against an OpenSSH `*`/`?` pattern (whole-string
/// match, OpenSSH `match_pattern` semantics).
pub fn hostname_matches_pattern(pattern: &str, hostname: &str) -> bool {
    fn match_at(pattern: &[char], hostname: &[char]) -> bool {
        match (pattern.first(), hostname.first()) {
            (None, None) => true,
            (Some('*'), _) => {
                match_at(&pattern[1..], hostname)
                    || (!hostname.is_empty() && match_at(pattern, &hostname[1..]))
            }
            (Some('?'), _) if !hostname.is_empty() => match_at(&pattern[1..], &hostname[1..]),
            (Some(pattern_char), Some(host_char)) if pattern_char == host_char => {
                match_at(&pattern[1..], &hostname[1..])
            }
            _ => false,
        }
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let hostname: Vec<char> = hostname.chars().collect();
    match_at(&pattern, &hostname)
}

/// Computes the OpenSSH hashed-host digest: HMAC-SHA1 with the salt as key
/// over the hostname (20-byte output).
fn hash_host(salt: &[u8], hostname: &str) -> Vec<u8> {
    let mut mac = HmacSha1::new_from_slice(salt).expect("hmac accepts any key length");
    mac.update(hostname.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// Constant-time-ish equality (XOR accumulate).
fn ct_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

/// Whether a `|1|salt|hash|` host field matches a hostname.
pub fn hashed_host_matches(field: &str, hostname: &str) -> bool {
    let Some(rest) = field.strip_prefix("|1|") else {
        return false;
    };
    let Some(rest) = rest.strip_suffix('|') else {
        return false;
    };
    let Some((salt_b64, hash_b64)) = rest.split_once('|') else {
        return false;
    };
    let engine = base64::engine::general_purpose::STANDARD;
    let Ok(salt) = engine.decode(salt_b64) else {
        return false;
    };
    let Ok(expected) = engine.decode(hash_b64) else {
        return false;
    };
    ct_eq(&hash_host(&salt, hostname), &expected)
}

/// Whether a host field matches `host` and `port`.
///
/// Handles hashed fields (`|1|...|`), bracketed host+port (`[host]:port`),
/// plain hostnames, and `*`/`?` wildcard patterns.
pub fn host_field_matches(field: &str, host: &str, port: u16) -> bool {
    if field.starts_with('|') {
        return hashed_host_matches(field, host);
    }
    if let Some(rest) = field.strip_prefix('[') {
        if let Some(end) = rest.find("]:") {
            let field_host = &rest[..end];
            let Ok(field_port) = rest[end + 2..].parse::<u16>() else {
                return false;
            };
            return field_port == port && hostname_matches_pattern(field_host, host);
        }
        // Unbracketed-with-port is invalid for the bracket form.
        return false;
    }
    hostname_matches_pattern(field, host)
}

/// An OpenSSH known_hosts store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownHostsStore {
    entries: Vec<KnownHostsEntry>,
}

impl KnownHostsStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses known_hosts text (one entry per line, comments/blank skipped).
    pub fn from_text(text: &str) -> Self {
        let entries = text.lines().filter_map(KnownHostsEntry::parse).collect();
        Self { entries }
    }

    /// Returns all entries whose host field matches `host` and `port`.
    pub fn lookup(&self, host: &str, port: u16) -> Vec<&KnownHostsEntry> {
        self.entries
            .iter()
            .filter(|entry| host_field_matches(&entry.hosts, host, port))
            .collect()
    }

    /// All entries.
    pub fn entries(&self) -> &[KnownHostsEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::{
        hashed_host_matches, host_field_matches, hostname_matches_pattern, KnownHostsStore,
    };

    fn key(key_type: &str) -> String {
        format!("{key_type} AAAAC3NzaC1lZDI1NTE5AAAAIGZgM3exampleKeyMaterial")
    }

    #[test]
    fn wildcard_pattern_matches_openssh_semantics() {
        assert!(hostname_matches_pattern("example.com", "example.com"));
        assert!(hostname_matches_pattern("*.example.com", "a.example.com"));
        assert!(hostname_matches_pattern("*.example.com", "a.b.example.com"));
        assert!(!hostname_matches_pattern("*.example.com", "example.com"));
        assert!(hostname_matches_pattern("host?", "host1"));
        assert!(!hostname_matches_pattern("host?", "host12"));
        assert!(!hostname_matches_pattern("example.com", "example.org"));
    }

    #[test]
    fn plain_host_field_matches() {
        assert!(host_field_matches("example.com", "example.com", 22));
        assert!(host_field_matches("example.com", "example.com", 2222));
        assert!(!host_field_matches("example.com", "other.com", 22));
        assert!(host_field_matches("*.example.com", "a.example.com", 22));
    }

    #[test]
    fn bracket_port_rule_matches() {
        assert!(host_field_matches(
            "[db.internal]:2222",
            "db.internal",
            2222
        ));
        assert!(!host_field_matches("[db.internal]:2222", "db.internal", 22));
        assert!(host_field_matches("[::1]:22", "::1", 22));
        assert!(!host_field_matches("[::1]:22", "::1", 2222));
    }

    #[test]
    fn hashed_host_matches_openssh_vector() {
        // Salt "salt" (4 bytes) hashing hostname "example.com" via
        // HMAC-SHA1(salt, host). Precomputed with OpenSSH's hash_host.
        let salt = "c2FsdA=="; // "salt"
        let expected_hash = "Jq5lAYgyR+u24cwfPUp92/fOJcs="; // HMAC-SHA1("salt","example.com")
        let field = format!("|1|{salt}|{expected_hash}|");
        assert!(hashed_host_matches(&field, "example.com"));
        assert!(!hashed_host_matches(&field, "other.com"));
        assert!(!hashed_host_matches("|1|bad", "example.com"));
    }

    #[test]
    fn store_parses_and_looks_up_with_all_rules() {
        let text = format!(
            "# comment\n\
             example.com {key1}\n\
             *.example.com {key2}\n\
             [db.internal]:2222 {key3}\n\
             |1|c2FsdA==|Jq5lAYgyR+u24cwfPUp92/fOJcs=| {key4}",
            key1 = key("ssh-ed25519"),
            key2 = key("ssh-ed25519"),
            key3 = key("ssh-ed25519"),
            key4 = key("ssh-ed25519"),
        );
        let store = KnownHostsStore::from_text(&text);
        assert_eq!(store.entries().len(), 4);

        // Plain exact match plus the hashed entry for the same host.
        let exact = store.lookup("example.com", 22);

        assert_eq!(
            exact.len(),
            2,
            "plain + hashed entries both match example.com"
        );
        assert!(exact.iter().any(|entry| entry.is_hashed()));
        // Wildcard subdomain match.
        assert_eq!(store.lookup("a.example.com", 22).len(), 1);
        // Bracket+port rule.
        assert_eq!(store.lookup("db.internal", 2222).len(), 1);
        assert_eq!(store.lookup("db.internal", 22).len(), 0);
        // Hashed host match.
        assert_eq!(
            store
                .lookup("example.com", 22)
                .iter()
                .filter(|e| e.is_hashed())
                .count(),
            1
        );
        // Non-matching host has no entries.
        assert_eq!(store.lookup("nowhere.invalid", 22).len(), 0);
    }

    #[test]
    fn store_ignores_comments_and_blank_lines() {
        let store = KnownHostsStore::from_text("  \n# comment\n\n");
        assert!(store.entries().is_empty());
    }

    #[test]
    fn open_ssh_compat_corpus() {
        // A small corpus of real known_hosts shapes.
        let corpus = format!(
            "github.com {key1}\n\
             gitlab.com {key2}\n\
             [127.0.0.1]:2222 {key3}\n\
             *.internal.example {key4}\n\
             @revoked oldhost.example.com {key5}",
            key1 = key("ssh-ed25519"),
            key2 = key("ssh-rsa"),
            key3 = key("ecdsa-sha2-nistp256"),
            key4 = key("ssh-ed25519"),
            key5 = key("ssh-ed25519"),
        );
        let store = KnownHostsStore::from_text(&corpus);
        assert_eq!(store.lookup("github.com", 22).len(), 1);
        assert_eq!(store.lookup("gitlab.com", 22).len(), 1);
        assert_eq!(store.lookup("127.0.0.1", 2222).len(), 1);
        assert_eq!(store.lookup("svc.internal.example", 22).len(), 1);
        // Revoked entries are returned as matches too; policy decides.
        assert_eq!(store.lookup("oldhost.example.com", 22).len(), 1);
    }
}
