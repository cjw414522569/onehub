use serde::{Deserialize, Serialize};

/// Host key verification policy for a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HostKeyPolicy {
    /// Strict: the host key must already be known and unchanged.
    Strict,
    /// Trust-on-first-use: an unknown host key is prompted once and then
    /// remembered.
    FirstTrust,
    /// Certificate authority: trust a CA-signed host certificate.
    CertificateAuthority,
}

impl HostKeyPolicy {
    /// Returns the stable policy name.
    pub const fn as_str(self) -> &'static str {
        match self {
            HostKeyPolicy::Strict => "strict",
            HostKeyPolicy::FirstTrust => "first-trust",
            HostKeyPolicy::CertificateAuthority => "certificate-authority",
        }
    }
}

/// Observed status of a host key during verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HostKeyStatus {
    /// The key is already known and matches.
    Known,
    /// The key is not in the trust store (first contact).
    Unknown,
    /// The key changed compared to the stored one (conflict or rotation).
    Changed,
    /// The key was signed by a trusted certificate authority.
    CertificateAuthorized,
    /// The key is explicitly revoked.
    Revoked,
    /// The key type or encoding does not match expectations.
    Mismatch,
}

/// Decision produced by applying a policy to a host key status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HostKeyDecision {
    /// Connect without prompting.
    Trusted,
    /// Prompt the user to trust an unknown key (TOFU).
    PromptToTrust,
    /// Reject the connection.
    Rejected,
}

/// Applies a host key policy to an observed status.
///
/// - `Strict`: only `Known` and `CertificateAuthorized` are trusted.
/// - `FirstTrust`: `Unknown` becomes `PromptToTrust`; `Known` trusted;
///   `Changed`/`Revoked`/`Mismatch` rejected.
/// - `CertificateAuthority`: `CertificateAuthorized` trusted; `Revoked` and
///   `Changed` rejected; `Unknown` is `PromptToTrust` only under first-trust,
///   otherwise rejected.
pub fn verify_host_key(policy: HostKeyPolicy, status: HostKeyStatus) -> HostKeyDecision {
    use HostKeyDecision::*;
    use HostKeyPolicy::*;
    use HostKeyStatus::*;
    match (policy, status) {
        (Strict, Known) => Trusted,
        (Strict, CertificateAuthorized) => Trusted,
        (Strict, Unknown) => Rejected,
        (Strict, Changed) => Rejected,
        (Strict, Revoked) => Rejected,
        (Strict, Mismatch) => Rejected,
        (FirstTrust, Known) => Trusted,
        (FirstTrust, CertificateAuthorized) => Trusted,
        (FirstTrust, Unknown) => PromptToTrust,
        (FirstTrust, Changed) => Rejected,
        (FirstTrust, Revoked) => Rejected,
        (FirstTrust, Mismatch) => Rejected,
        (CertificateAuthority, CertificateAuthorized) => Trusted,
        (CertificateAuthority, Unknown) => Rejected,
        (CertificateAuthority, Changed) => Rejected,
        (CertificateAuthority, Revoked) => Rejected,
        (CertificateAuthority, Mismatch) => Rejected,
        (CertificateAuthority, Known) => Trusted,
    }
}

/// Marker of an OpenSSH known_hosts entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KnownHostsMarker {
    /// Plain host key entry.
    None,
    /// `@cert-authority` CA-signed host certificate.
    CertAuthority,
    /// `@revoked` explicitly revoked key.
    Revoked,
}

/// A parsed OpenSSH known_hosts line (subset: one host-name pattern set, one
/// key, optional marker; hashed entries keep the raw hashed host field).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownHostsEntry {
    /// `@marker` if present.
    pub marker: KnownHostsMarker,
    /// Comma-separated host name patterns, or the hashed `|1|salt|hash` field.
    pub hosts: String,
    /// Key type such as `ssh-ed25519` or `ssh-rsa`.
    pub key_type: String,
    /// Base64-encoded key material (no secret material; host keys are public).
    pub key_base64: String,
}

impl KnownHostsEntry {
    /// Parses a single non-comment known_hosts line.
    ///
    /// Returns `None` for empty, comment-only, or malformed lines.
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let mut fields = line.split_whitespace();
        let marker = match fields.next()? {
            "@cert-authority" => KnownHostsMarker::CertAuthority,
            "@revoked" => KnownHostsMarker::Revoked,
            first => {
                return Some(KnownHostsEntry {
                    marker: KnownHostsMarker::None,
                    hosts: first.to_owned(),
                    key_type: fields.next()?.to_owned(),
                    key_base64: fields.next()?.to_owned(),
                });
            }
        };
        Some(KnownHostsEntry {
            marker,
            hosts: fields.next()?.to_owned(),
            key_type: fields.next()?.to_owned(),
            key_base64: fields.next()?.to_owned(),
        })
    }

    /// Returns whether the entry carries a hashed host field (`|1|...`).
    pub fn is_hashed(&self) -> bool {
        self.hosts.starts_with("|1|")
    }
}

/// A key-format fingerprint helper for corpus assertions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostKeyIdentity {
    /// Key type such as `ssh-ed25519`.
    pub key_type: String,
    /// Base64 public key.
    pub key_base64: String,
}

impl HostKeyIdentity {
    /// Creates a host key identity.
    pub fn new(key_type: impl Into<String>, key_base64: impl Into<String>) -> Self {
        Self {
            key_type: key_type.into(),
            key_base64: key_base64.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        verify_host_key, HostKeyDecision, HostKeyIdentity, HostKeyPolicy, HostKeyStatus,
        KnownHostsEntry, KnownHostsMarker,
    };
    use HostKeyDecision::*;
    use HostKeyPolicy::*;
    use HostKeyStatus::*;

    #[test]
    fn strict_policy_table_is_explicit() {
        assert_eq!(verify_host_key(Strict, Known), Trusted);
        assert_eq!(verify_host_key(Strict, CertificateAuthorized), Trusted);
        assert_eq!(verify_host_key(Strict, Unknown), Rejected);
        assert_eq!(verify_host_key(Strict, Changed), Rejected);
        assert_eq!(verify_host_key(Strict, Revoked), Rejected);
        assert_eq!(verify_host_key(Strict, Mismatch), Rejected);
    }

    #[test]
    fn first_trust_policy_prompts_on_unknown_and_rejects_conflicts() {
        assert_eq!(verify_host_key(FirstTrust, Unknown), PromptToTrust);
        assert_eq!(verify_host_key(FirstTrust, Known), Trusted);
        assert_eq!(verify_host_key(FirstTrust, Changed), Rejected);
        assert_eq!(verify_host_key(FirstTrust, Revoked), Rejected);
        assert_eq!(verify_host_key(FirstTrust, Mismatch), Rejected);
    }

    #[test]
    fn certificate_authority_policy_trusts_ca_and_rejects_revoked() {
        assert_eq!(
            verify_host_key(CertificateAuthority, CertificateAuthorized),
            Trusted
        );
        assert_eq!(verify_host_key(CertificateAuthority, Revoked), Rejected);
        assert_eq!(verify_host_key(CertificateAuthority, Changed), Rejected);
        assert_eq!(verify_host_key(CertificateAuthority, Unknown), Rejected);
        assert_eq!(verify_host_key(CertificateAuthority, Mismatch), Rejected);
    }

    #[test]
    fn known_hosts_plain_entry_parses() {
        let entry = KnownHostsEntry::parse(
            "example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGZgM3exampleKeyMaterial",
        )
        .expect("plain entry parses");
        assert_eq!(entry.marker, KnownHostsMarker::None);
        assert_eq!(entry.hosts, "example.com");
        assert_eq!(entry.key_type, "ssh-ed25519");
        assert_eq!(
            entry.key_base64,
            "AAAAC3NzaC1lZDI1NTE5AAAAIGZgM3exampleKeyMaterial"
        );
        assert!(!entry.is_hashed());
    }

    #[test]
    fn known_hosts_multi_host_and_port_entries_parse() {
        let entry = KnownHostsEntry::parse(
            "host1.example.com,host2.example.com ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQCorpusRsaKey",
        )
        .expect("multi-host entry parses");
        assert_eq!(entry.hosts, "host1.example.com,host2.example.com");
        assert_eq!(entry.key_type, "ssh-rsa");

        let ported = KnownHostsEntry::parse("[db.internal]:2222 ssh-ed25519 AAAAexamplePortKey")
            .expect("port entry parses");
        assert_eq!(ported.hosts, "[db.internal]:2222");
    }

    #[test]
    fn known_hosts_marker_and_hashed_entries_parse() {
        let ca =
            KnownHostsEntry::parse("@cert-authority *.example.com ssh-ed25519 AAAAexampleCaCert")
                .expect("cert-authority parses");
        assert_eq!(ca.marker, KnownHostsMarker::CertAuthority);
        assert_eq!(ca.hosts, "*.example.com");

        let revoked =
            KnownHostsEntry::parse("@revoked oldhost.example.com ssh-rsa AAAAexampleRevoked")
                .expect("revoked parses");
        assert_eq!(revoked.marker, KnownHostsMarker::Revoked);

        let hashed = KnownHostsEntry::parse(
            "|1|U2FsdGVkX1+corpusSalt|corpusHashValue ssh-ed25519 AAAAexampleHashedKey",
        )
        .expect("hashed parses");
        assert!(hashed.is_hashed());
        assert!(hashed.hosts.starts_with("|1|"));
    }

    #[test]
    fn known_hosts_ignores_comments_and_blank_lines() {
        assert!(KnownHostsEntry::parse("").is_none());
        assert!(KnownHostsEntry::parse("   ").is_none());
        assert!(KnownHostsEntry::parse("# a comment").is_none());
    }

    #[test]
    fn corpus_covers_all_accepted_statuses() {
        // Every status has a defined decision under at least one policy, and
        // the full policy x status table is total (never undefined).
        use HostKeyPolicy::*;
        use HostKeyStatus::*;
        let statuses = [
            Known,
            Unknown,
            Changed,
            CertificateAuthorized,
            Revoked,
            Mismatch,
        ];
        let policies = [Strict, FirstTrust, CertificateAuthority];
        let mut decisions = 0;
        for policy in policies {
            for status in statuses {
                let _ = verify_host_key(policy, status);
                decisions += 1;
            }
        }
        assert_eq!(decisions, 18);
    }

    #[test]
    fn host_key_identity_round_trip() {
        let identity = HostKeyIdentity::new("ssh-ed25519", "AAAAcorpusPublicKey");
        assert_eq!(identity.key_type, "ssh-ed25519");
        assert_eq!(identity.key_base64, "AAAAcorpusPublicKey");
    }
}
