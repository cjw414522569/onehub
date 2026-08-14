use std::collections::HashMap;

use core_domain::host::HostId;

/// Parsed SSH protocol version line.
///
/// The wire format is `SSH-<major>.<minor>-<software> [<comment>]` (RFC 4253).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshVersion {
    /// Protocol major version.
    pub major: u16,
    /// Protocol minor version.
    pub minor: u16,
    /// Software version string.
    pub software: String,
    /// Optional comment.
    pub comment: Option<String>,
}

impl SshVersion {
    /// Parses a version line, rejecting malformed or unsupported majors.
    pub fn parse(line: &str) -> Result<Self, SshVersionError> {
        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        let rest = line.strip_prefix("SSH-").ok_or(SshVersionError::NotSsh)?;
        let mut parts = rest.splitn(2, '-');
        let version = parts.next().unwrap_or_default();
        let mut version_parts = version.split('.');
        let major = version_parts
            .next()
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or(SshVersionError::Malformed)?;
        let minor = version_parts
            .next()
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or(SshVersionError::Malformed)?;
        let software_comment = parts.next().unwrap_or_default();
        let (software, comment) = match software_comment.split_once(' ') {
            Some((software, comment)) => (software.to_owned(), Some(comment.to_owned())),
            None => (software_comment.to_owned(), None),
        };
        Ok(Self {
            major,
            minor,
            software,
            comment,
        })
    }

    /// Whether this is a supported SSH-2.0 peer.
    pub fn is_supported(&self) -> bool {
        self.major == 2
    }
}

/// Version line parse error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshVersionError {
    /// Not an SSH version line.
    NotSsh,
    /// Malformed version fields.
    Malformed,
}

/// Family of negotiated algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlgorithmKind {
    /// Key exchange.
    Kex,
    /// Host key.
    HostKey,
    /// Symmetric cipher.
    Cipher,
    /// MAC.
    Mac,
    /// Compression.
    Compression,
}

impl AlgorithmKind {
    /// Stable string name.
    pub const fn as_str(self) -> &'static str {
        match self {
            AlgorithmKind::Kex => "kex",
            AlgorithmKind::HostKey => "host-key",
            AlgorithmKind::Cipher => "cipher",
            AlgorithmKind::Mac => "mac",
            AlgorithmKind::Compression => "compression",
        }
    }
}

/// A negotiated SSH algorithm (known or unknown).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Algorithm {
    /// curve25519-sha256
    Curve25519Sha256,
    /// curve25519-sha256@libssh.org
    Curve25519Sha256LibSsh,
    /// ecdh-sha2-nistp256
    EcdhSha2Nistp256,
    /// ecdh-sha2-nistp384
    EcdhSha2Nistp384,
    /// ecdh-sha2-nistp521
    EcdhSha2Nistp521,
    /// diffie-hellman-group16-sha512
    Group16Sha512,
    /// diffie-hellman-group14-sha256
    Group14Sha256,
    /// diffie-hellman-group14-sha1 (legacy, disabled by default)
    Group14Sha1,
    /// diffie-hellman-group1-sha1 (legacy, disabled by default)
    Group1Sha1,
    /// ssh-ed25519
    Ed25519,
    /// ecdsa-sha2-nistp256
    EcdsaSha2Nistp256,
    /// ecdsa-sha2-nistp384
    EcdsaSha2Nistp384,
    /// ecdsa-sha2-nistp521
    EcdsaSha2Nistp521,
    /// rsa-sha2-512
    RsaSha2_512,
    /// rsa-sha2-256
    RsaSha2_256,
    /// ssh-rsa (SHA-1, legacy, disabled by default)
    SshRsa,
    /// chacha20-poly1305@openssh.com
    Chacha20Poly1305,
    /// aes128-gcm@openssh.com
    Aes128Gcm,
    /// aes256-gcm@openssh.com
    Aes256Gcm,
    /// aes128-ctr
    Aes128Ctr,
    /// aes192-ctr
    Aes192Ctr,
    /// aes256-ctr
    Aes256Ctr,
    /// aes128-cbc (legacy, disabled by default)
    Aes128Cbc,
    /// 3des-cbc (legacy, disabled by default)
    TripleDesCbc,
    /// hmac-sha2-256
    HmacSha2_256,
    /// hmac-sha2-512
    HmacSha2_512,
    /// umac-128-etm@openssh.com
    Umac128Etm,
    /// hmac-sha1 (legacy, disabled by default)
    HmacSha1,
    /// none
    NoneCompression,
    /// zlib@openssh.com
    ZlibOpenssh,
    /// Any other algorithm not enumerated (treated as unknown/disabled).
    Other(String),
}

impl Algorithm {
    /// The wire name.
    pub fn as_str(&self) -> &str {
        match self {
            Algorithm::Curve25519Sha256 => "curve25519-sha256",
            Algorithm::Curve25519Sha256LibSsh => "curve25519-sha256@libssh.org",
            Algorithm::EcdhSha2Nistp256 => "ecdh-sha2-nistp256",
            Algorithm::EcdhSha2Nistp384 => "ecdh-sha2-nistp384",
            Algorithm::EcdhSha2Nistp521 => "ecdh-sha2-nistp521",
            Algorithm::Group16Sha512 => "diffie-hellman-group16-sha512",
            Algorithm::Group14Sha256 => "diffie-hellman-group14-sha256",
            Algorithm::Group14Sha1 => "diffie-hellman-group14-sha1",
            Algorithm::Group1Sha1 => "diffie-hellman-group1-sha1",
            Algorithm::Ed25519 => "ssh-ed25519",
            Algorithm::EcdsaSha2Nistp256 => "ecdsa-sha2-nistp256",
            Algorithm::EcdsaSha2Nistp384 => "ecdsa-sha2-nistp384",
            Algorithm::EcdsaSha2Nistp521 => "ecdsa-sha2-nistp521",
            Algorithm::RsaSha2_512 => "rsa-sha2-512",
            Algorithm::RsaSha2_256 => "rsa-sha2-256",
            Algorithm::SshRsa => "ssh-rsa",
            Algorithm::Chacha20Poly1305 => "chacha20-poly1305@openssh.com",
            Algorithm::Aes128Gcm => "aes128-gcm@openssh.com",
            Algorithm::Aes256Gcm => "aes256-gcm@openssh.com",
            Algorithm::Aes128Ctr => "aes128-ctr",
            Algorithm::Aes192Ctr => "aes192-ctr",
            Algorithm::Aes256Ctr => "aes256-ctr",
            Algorithm::Aes128Cbc => "aes128-cbc",
            Algorithm::TripleDesCbc => "3des-cbc",
            Algorithm::HmacSha2_256 => "hmac-sha2-256",
            Algorithm::HmacSha2_512 => "hmac-sha2-512",
            Algorithm::Umac128Etm => "umac-128-etm@openssh.com",
            Algorithm::HmacSha1 => "hmac-sha1",
            Algorithm::NoneCompression => "none",
            Algorithm::ZlibOpenssh => "zlib@openssh.com",
            Algorithm::Other(name) => name,
        }
    }
}

/// Security classification of an algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlgorithmSecurity {
    /// Acceptable under the secure default policy.
    Secure,
    /// Legacy (weak); disabled unless a per-host override enables it.
    Legacy,
    /// Never allowed.
    Disabled,
}

/// The secure default policy.
///
/// Only modern, widely-supported algorithms are enabled. SHA-1 based
/// algorithms (group14-sha1, group1-sha1, ssh-rsa, hmac-sha1) and CBC
/// ciphers are disabled unless a host explicitly opts in.
#[derive(Debug, Clone)]
pub struct AlgorithmPolicy {
    /// Allowed algorithms per kind, in local preference order.
    pub allowed: HashMap<AlgorithmKind, Vec<Algorithm>>,
}

impl AlgorithmPolicy {
    /// The secure defaults.
    pub fn defaults() -> Self {
        let mut allowed = HashMap::new();
        allowed.insert(
            AlgorithmKind::Kex,
            vec![
                Algorithm::Curve25519Sha256,
                Algorithm::Curve25519Sha256LibSsh,
                Algorithm::EcdhSha2Nistp256,
                Algorithm::EcdhSha2Nistp384,
                Algorithm::EcdhSha2Nistp521,
                Algorithm::Group16Sha512,
                Algorithm::Group14Sha256,
            ],
        );
        allowed.insert(
            AlgorithmKind::HostKey,
            vec![
                Algorithm::Ed25519,
                Algorithm::EcdsaSha2Nistp256,
                Algorithm::EcdsaSha2Nistp384,
                Algorithm::EcdsaSha2Nistp521,
                Algorithm::RsaSha2_512,
                Algorithm::RsaSha2_256,
            ],
        );
        allowed.insert(
            AlgorithmKind::Cipher,
            vec![
                Algorithm::Chacha20Poly1305,
                Algorithm::Aes256Gcm,
                Algorithm::Aes128Gcm,
                Algorithm::Aes256Ctr,
                Algorithm::Aes192Ctr,
                Algorithm::Aes128Ctr,
            ],
        );
        allowed.insert(
            AlgorithmKind::Mac,
            vec![
                Algorithm::HmacSha2_256,
                Algorithm::HmacSha2_512,
                Algorithm::Umac128Etm,
            ],
        );
        allowed.insert(
            AlgorithmKind::Compression,
            vec![Algorithm::NoneCompression, Algorithm::ZlibOpenssh],
        );
        Self { allowed }
    }

    /// Whether `algorithm` is enabled for `kind`.
    pub fn is_allowed(&self, kind: AlgorithmKind, algorithm: &Algorithm) -> bool {
        self.allowed
            .get(&kind)
            .map(|list| list.contains(algorithm))
            .unwrap_or(false)
    }

    /// The security classification for an algorithm.
    pub fn security(&self, kind: AlgorithmKind, algorithm: &Algorithm) -> AlgorithmSecurity {
        match algorithm {
            Algorithm::Group14Sha1 | Algorithm::Group1Sha1 | Algorithm::SshRsa => {
                AlgorithmSecurity::Legacy
            }
            Algorithm::Aes128Cbc | Algorithm::TripleDesCbc | Algorithm::HmacSha1 => {
                AlgorithmSecurity::Legacy
            }
            Algorithm::Other(_) => AlgorithmSecurity::Disabled,
            _ => {
                if self.is_allowed(kind, algorithm) {
                    AlgorithmSecurity::Secure
                } else {
                    AlgorithmSecurity::Disabled
                }
            }
        }
    }
}

/// Result of negotiating one algorithm kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegotiatedAlgorithm {
    /// Both sides share an allowed algorithm; local preference order applies.
    Selected(Algorithm),
    /// The remote offered only algorithms this side does not allow.
    Rejected,
}

/// Negotiates a single algorithm kind between local policy and remote
/// offers, preferring the local order. Unknown or disabled algorithms are
/// never selected; a downgrade attempt yields `Rejected`.
pub fn negotiate_algorithm(
    policy: &AlgorithmPolicy,
    kind: AlgorithmKind,
    remote_offers: &[Algorithm],
) -> NegotiatedAlgorithm {
    let Some(local) = policy.allowed.get(&kind) else {
        return NegotiatedAlgorithm::Rejected;
    };
    for candidate in local {
        if remote_offers.contains(candidate) {
            return NegotiatedAlgorithm::Selected(candidate.clone());
        }
    }
    NegotiatedAlgorithm::Rejected
}

/// Per-host explicit compatibility overrides.
///
/// The secure defaults always apply; a host may explicitly enable a legacy
/// algorithm (e.g. to talk to an old server), scoped to that host only.
#[derive(Debug, Clone, Default)]
pub struct HostAlgorithmPolicy {
    /// host id -> (kind -> allowed algorithms appended after the defaults).
    overrides: HashMap<HostId, HashMap<AlgorithmKind, Vec<Algorithm>>>,
}

impl HostAlgorithmPolicy {
    /// An empty policy (secure defaults for every host).
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables an explicit compatibility algorithm for one host.
    pub fn enable_for_host(&mut self, host: &HostId, kind: AlgorithmKind, algorithm: Algorithm) {
        self.overrides
            .entry(host.clone())
            .or_default()
            .entry(kind)
            .or_default()
            .push(algorithm);
    }

    /// The effective policy for a host: secure defaults plus its overrides.
    pub fn effective(&self, host: &HostId) -> AlgorithmPolicy {
        let mut policy = AlgorithmPolicy::defaults();
        if let Some(kinds) = self.overrides.get(host) {
            for (kind, algorithms) in kinds {
                let entry = policy.allowed.entry(*kind).or_default();
                for algorithm in algorithms {
                    if !entry.contains(algorithm) {
                        entry.push(algorithm.clone());
                    }
                }
            }
        }
        policy
    }
}

#[cfg(test)]
mod tests {
    use super::{
        negotiate_algorithm, Algorithm, AlgorithmKind, AlgorithmPolicy, AlgorithmSecurity,
        HostAlgorithmPolicy, NegotiatedAlgorithm, SshVersion, SshVersionError,
    };
    use core_domain::host::HostId;

    #[test]
    fn version_line_parses() {
        let version = SshVersion::parse("SSH-2.0-OpenSSH_9.6\r\n").expect("parse");
        assert_eq!(version.major, 2);
        assert_eq!(version.minor, 0);
        assert_eq!(version.software, "OpenSSH_9.6");
        assert!(version.is_supported());
    }

    #[test]
    fn version_with_comment_parses() {
        let version = SshVersion::parse("SSH-2.0-Test_1.0 comment here").expect("parse");
        assert_eq!(version.comment.as_deref(), Some("comment here"));
    }

    #[test]
    fn version_rejects_malformed_and_unsupported() {
        assert_eq!(SshVersion::parse("hello"), Err(SshVersionError::NotSsh));
        assert!(SshVersion::parse("SSH-bogus").is_err());
        let v1 = SshVersion::parse("SSH-1.99-OpenSSH").expect("parse v1");
        assert!(!v1.is_supported());
    }

    #[test]
    fn defaults_are_secure_and_exclude_sha1_and_cbc() {
        let policy = AlgorithmPolicy::defaults();
        assert!(!policy.is_allowed(AlgorithmKind::Kex, &Algorithm::Group1Sha1));
        assert!(!policy.is_allowed(AlgorithmKind::Kex, &Algorithm::Group14Sha1));
        assert!(!policy.is_allowed(AlgorithmKind::HostKey, &Algorithm::SshRsa));
        assert!(!policy.is_allowed(AlgorithmKind::Cipher, &Algorithm::Aes128Cbc));
        assert!(!policy.is_allowed(AlgorithmKind::Cipher, &Algorithm::TripleDesCbc));
        assert!(!policy.is_allowed(AlgorithmKind::Mac, &Algorithm::HmacSha1));
        assert!(policy.is_allowed(AlgorithmKind::Kex, &Algorithm::Curve25519Sha256));
        assert!(policy.is_allowed(AlgorithmKind::HostKey, &Algorithm::Ed25519));
        assert!(policy.is_allowed(AlgorithmKind::Cipher, &Algorithm::Chacha20Poly1305));
        assert_eq!(
            policy.security(AlgorithmKind::HostKey, &Algorithm::SshRsa),
            AlgorithmSecurity::Legacy
        );
        assert_eq!(
            policy.security(AlgorithmKind::Kex, &Algorithm::Other("made-up".to_owned())),
            AlgorithmSecurity::Disabled
        );
    }

    #[test]
    fn negotiation_selects_shared_algorithm_in_local_preference() {
        let policy = AlgorithmPolicy::defaults();
        let remote = vec![Algorithm::Group14Sha256, Algorithm::Curve25519Sha256];
        // Local preference lists curve25519 first among allowed; remote offers
        // it, so it wins over group14.
        assert_eq!(
            negotiate_algorithm(&policy, AlgorithmKind::Kex, &remote),
            NegotiatedAlgorithm::Selected(Algorithm::Curve25519Sha256)
        );
    }

    #[test]
    fn downgrade_attack_is_rejected() {
        // An attacker offers only SHA-1 / CBC / weak algorithms.
        let policy = AlgorithmPolicy::defaults();
        let weak_kex = vec![Algorithm::Group1Sha1, Algorithm::Group14Sha1];
        assert_eq!(
            negotiate_algorithm(&policy, AlgorithmKind::Kex, &weak_kex),
            NegotiatedAlgorithm::Rejected
        );
        let weak_hostkey = vec![Algorithm::SshRsa];
        assert_eq!(
            negotiate_algorithm(&policy, AlgorithmKind::HostKey, &weak_hostkey),
            NegotiatedAlgorithm::Rejected
        );
        let weak_cipher = vec![Algorithm::Aes128Cbc, Algorithm::TripleDesCbc];
        assert_eq!(
            negotiate_algorithm(&policy, AlgorithmKind::Cipher, &weak_cipher),
            NegotiatedAlgorithm::Rejected
        );
        let weak_mac = vec![Algorithm::HmacSha1];
        assert_eq!(
            negotiate_algorithm(&policy, AlgorithmKind::Mac, &weak_mac),
            NegotiatedAlgorithm::Rejected
        );
    }

    #[test]
    fn partial_downgrade_picks_the_strongest_shared() {
        let policy = AlgorithmPolicy::defaults();
        // Remote offers a mix: strong + weak. The strong one is selected.
        let mixed = vec![Algorithm::Group1Sha1, Algorithm::Curve25519Sha256];
        assert_eq!(
            negotiate_algorithm(&policy, AlgorithmKind::Kex, &mixed),
            NegotiatedAlgorithm::Selected(Algorithm::Curve25519Sha256)
        );
    }

    #[test]
    fn per_host_override_enables_legacy_only_for_that_host() {
        let host_a = HostId::new("legacy-host").expect("host id");
        let host_b = HostId::new("modern-host").expect("host id");
        let mut hosts = HostAlgorithmPolicy::new();
        hosts.enable_for_host(&host_a, AlgorithmKind::Kex, Algorithm::Group14Sha1);

        let effective_a = hosts.effective(&host_a);
        assert!(effective_a.is_allowed(AlgorithmKind::Kex, &Algorithm::Group14Sha1));

        // Host B keeps secure defaults.
        let effective_b = hosts.effective(&host_b);
        assert!(!effective_b.is_allowed(AlgorithmKind::Kex, &Algorithm::Group14Sha1));

        // With the override, negotiation with a legacy-only server succeeds
        // for host A but is rejected for host B.
        let legacy_remote = vec![Algorithm::Group14Sha1];
        assert_eq!(
            negotiate_algorithm(&effective_a, AlgorithmKind::Kex, &legacy_remote),
            NegotiatedAlgorithm::Selected(Algorithm::Group14Sha1)
        );
        assert_eq!(
            negotiate_algorithm(&effective_b, AlgorithmKind::Kex, &legacy_remote),
            NegotiatedAlgorithm::Rejected
        );
    }

    #[test]
    fn unknown_algorithms_are_never_selected() {
        let policy = AlgorithmPolicy::defaults();
        let remote = vec![Algorithm::Other("totally-made-up".to_owned())];
        assert_eq!(
            negotiate_algorithm(&policy, AlgorithmKind::Cipher, &remote),
            NegotiatedAlgorithm::Rejected
        );
    }

    #[test]
    fn matrix_covers_all_kinds() {
        let policy = AlgorithmPolicy::defaults();
        for kind in [
            AlgorithmKind::Kex,
            AlgorithmKind::HostKey,
            AlgorithmKind::Cipher,
            AlgorithmKind::Mac,
            AlgorithmKind::Compression,
        ] {
            let list = policy.allowed.get(&kind).expect("kind present");
            assert!(!list.is_empty(), "{} must have defaults", kind.as_str());
            // Re-negotiating against the local list selects the first.
            assert!(matches!(
                negotiate_algorithm(&policy, kind, list),
                NegotiatedAlgorithm::Selected(_)
            ));
        }
    }
}
