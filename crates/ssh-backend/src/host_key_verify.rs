use base64::Engine as _;
use core_domain::host_key::{HostKeyPolicy, HostKeyStatus, KnownHostsMarker};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::known_hosts::KnownHostsStore;

/// SHA-256 fingerprint of a host key blob, in OpenSSH `SHA256:<base64>`
/// presentation form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyFingerprint {
    /// Base64 (no padding) SHA-256 digest.
    pub sha256_b64: String,
}

impl HostKeyFingerprint {
    /// Computes the fingerprint from a key blob.
    pub fn sha256(key_blob: &[u8]) -> Self {
        let digest = Sha256::digest(key_blob);
        let sha256_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest);
        Self { sha256_b64 }
    }

    /// The OpenSSH presentation form: `SHA256:<base64>`.
    pub fn display(&self) -> String {
        format!("SHA256:{}", self.sha256_b64)
    }
}

/// A host certificate presented during the handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCertificate {
    /// The public key embedded in the certificate (matches the presented
    /// host key).
    pub cert_key_blob: Vec<u8>,
    /// CA signature over `cert_key_blob`.
    pub signature: Vec<u8>,
    /// Validity start (unix seconds).
    pub valid_after: u64,
    /// Validity end (unix seconds).
    pub valid_before: u64,
    /// Optional principals (empty = any host).
    pub principals: Vec<String>,
}

impl HostCertificate {
    /// Whether the certificate is within its validity window (using the
    /// provided current time in unix seconds).
    pub fn is_valid_now(&self, now_unix: u64) -> bool {
        self.valid_after <= now_unix && now_unix < self.valid_before
    }
}

/// Trusted certificate-authority public keys.
#[derive(Debug, Clone, Default)]
pub struct CaStore {
    /// Trusted Ed25519 CA verifying keys.
    pub keys: Vec<VerifyingKey>,
}

impl CaStore {
    /// An empty CA store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a trusted CA key.
    pub fn add(&mut self, key: VerifyingKey) {
        self.keys.push(key);
    }
}

/// Outcome of verifying a presented host key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyVerification {
    /// Known and unchanged.
    Trusted { fingerprint: HostKeyFingerprint },
    /// First contact; the UI must prompt with the fingerprint.
    Unknown { fingerprint: HostKeyFingerprint },
    /// The key changed versus the stored key; must block.
    Changed {
        fingerprint: HostKeyFingerprint,
        stored_fingerprint: HostKeyFingerprint,
    },
    /// Signed by a trusted CA within its validity window.
    CertificateAuthorized { fingerprint: HostKeyFingerprint },
    /// Explicitly revoked.
    Revoked,
    /// Key type or blob mismatch.
    Mismatch,
}

impl HostKeyVerification {
    /// Whether the connection may proceed without prompting.
    pub fn is_trusted(&self) -> bool {
        matches!(
            self,
            HostKeyVerification::Trusted { .. } | HostKeyVerification::CertificateAuthorized { .. }
        )
    }

    /// Whether the UI must prompt the user.
    pub fn requires_prompt(&self) -> bool {
        matches!(self, HostKeyVerification::Unknown { .. })
    }

    /// Whether the connection must be blocked (never silent).
    pub fn is_blocked(&self) -> bool {
        matches!(
            self,
            HostKeyVerification::Changed { .. }
                | HostKeyVerification::Revoked
                | HostKeyVerification::Mismatch
        )
    }

    /// The status for the domain policy decision (T028).
    pub fn status(&self) -> HostKeyStatus {
        match self {
            HostKeyVerification::Trusted { .. } => HostKeyStatus::Known,
            HostKeyVerification::Unknown { .. } => HostKeyStatus::Unknown,
            HostKeyVerification::Changed { .. } => HostKeyStatus::Changed,
            HostKeyVerification::CertificateAuthorized { .. } => {
                HostKeyStatus::CertificateAuthorized
            }
            HostKeyVerification::Revoked => HostKeyStatus::Revoked,
            HostKeyVerification::Mismatch => HostKeyStatus::Mismatch,
        }
    }
}

/// Verifies a presented host key against the known_hosts store, the policy,
/// and the trusted CA store.
pub struct HostKeyVerifier<'a> {
    /// Known hosts store.
    pub store: &'a KnownHostsStore,
    /// Host key policy (used for the final decision via core-domain).
    pub policy: HostKeyPolicy,
    /// Trusted CAs.
    pub ca: &'a CaStore,
    /// Current unix time for certificate validity checks.
    pub now_unix: u64,
}

impl<'a> HostKeyVerifier<'a> {
    /// Verifies a presented key for `host:port`.
    ///
    /// Order: revoked -> CA certificate -> known/unknown -> changed/mismatch.
    /// A changed key is never silently accepted.
    pub fn verify(
        &self,
        host: &str,
        port: u16,
        key_type: &str,
        key_blob: &[u8],
        certificate: Option<&HostCertificate>,
    ) -> HostKeyVerification {
        let fingerprint = HostKeyFingerprint::sha256(key_blob);
        let matching = self.store.lookup(host, port);

        // 1. Explicitly revoked keys are rejected.
        for entry in &matching {
            if entry.marker == KnownHostsMarker::Revoked
                && entry.key_base64 == base64_encode(key_blob)
            {
                return HostKeyVerification::Revoked;
            }
        }

        // 2. CA-signed host certificates.
        if let Some(certificate) = certificate {
            if certificate.cert_key_blob == key_blob
                && certificate.is_valid_now(self.now_unix)
                && self.ca_verifies(certificate)
            {
                return HostKeyVerification::CertificateAuthorized { fingerprint };
            }
        }

        // 3. Known hosts: exact key match or changed fingerprint.
        for entry in &matching {
            if entry.key_type == key_type && entry.key_base64 == base64_encode(key_blob) {
                return HostKeyVerification::Trusted { fingerprint };
            }
        }
        if !matching.is_empty() {
            let stored = matching[0];
            let engine = base64::engine::general_purpose::STANDARD;
            let stored_bytes = engine
                .decode(&stored.key_base64)
                .unwrap_or_else(|_| stored.key_base64.as_bytes().to_vec());
            let stored_fingerprint = HostKeyFingerprint::sha256(&stored_bytes);
            return HostKeyVerification::Changed {
                fingerprint,
                stored_fingerprint,
            };
        }

        // 4. First contact.
        HostKeyVerification::Unknown { fingerprint }
    }

    fn ca_verifies(&self, certificate: &HostCertificate) -> bool {
        let Ok(signature) = Signature::from_slice(&certificate.signature) else {
            return false;
        };
        self.ca
            .keys
            .iter()
            .any(|ca| ca.verify(&certificate.cert_key_blob, &signature).is_ok())
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::{
        CaStore, HostCertificate, HostKeyFingerprint, HostKeyVerification, HostKeyVerifier,
    };
    use crate::known_hosts::KnownHostsStore;
    use base64::Engine as _;
    use core_domain::host_key::HostKeyPolicy;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn entry(hosts: &str, key_type: &str, blob: &[u8]) -> String {
        let b64 = base64::engine::general_purpose::STANDARD.encode(blob);
        format!("{hosts} {key_type} {b64}")
    }

    #[test]
    fn sha256_fingerprint_uses_openssh_presentation() {
        let blob = b"raw-host-key-bytes";
        let fingerprint = HostKeyFingerprint::sha256(blob);
        assert_eq!(fingerprint.sha256_b64.len(), 43); // 32 bytes -> 43 base64 chars no padding
        let display = fingerprint.display();
        assert!(display.starts_with("SHA256:"), "OpenSSH form: {display}");
        assert_eq!(display.len(), 50);
    }

    #[test]
    fn first_connect_reports_unknown_with_fingerprint() {
        let store = KnownHostsStore::new();
        let verifier = HostKeyVerifier {
            store: &store,
            policy: HostKeyPolicy::FirstTrust,
            ca: &CaStore::new(),
            now_unix: 1_700_000_000,
        };
        let result = verifier.verify("example.com", 22, "ssh-ed25519", b"key-A", None);
        assert!(matches!(result, HostKeyVerification::Unknown { .. }));
        assert!(result.requires_prompt());
        assert!(!result.is_blocked());
        assert_eq!(
            result.status(),
            core_domain::host_key::HostKeyStatus::Unknown
        );
    }

    #[test]
    fn mitm_changed_key_is_blocked_never_silent() {
        let text = entry("example.com", "ssh-ed25519", b"key-A");
        let store = KnownHostsStore::from_text(&text);
        let verifier = HostKeyVerifier {
            store: &store,
            policy: HostKeyPolicy::Strict,
            ca: &CaStore::new(),
            now_unix: 1_700_000_000,
        };
        // Attacker presents key-B.
        let result = verifier.verify("example.com", 22, "ssh-ed25519", b"key-B", None);
        assert!(matches!(result, HostKeyVerification::Changed { .. }));
        assert!(result.is_blocked());
        assert!(!result.is_trusted());
    }

    #[test]
    fn known_key_is_trusted() {
        let text = entry("example.com", "ssh-ed25519", b"key-A");
        let store = KnownHostsStore::from_text(&text);
        let verifier = HostKeyVerifier {
            store: &store,
            policy: HostKeyPolicy::Strict,
            ca: &CaStore::new(),
            now_unix: 1_700_000_000,
        };
        let result = verifier.verify("example.com", 22, "ssh-ed25519", b"key-A", None);
        assert!(result.is_trusted());
        assert!(matches!(result, HostKeyVerification::Trusted { .. }));
    }

    #[test]
    fn revoked_key_is_rejected() {
        let blob = b"key-R";
        let text = format!(
            "@revoked example.com ssh-ed25519 {}",
            base64::engine::general_purpose::STANDARD.encode(blob)
        );
        let store = KnownHostsStore::from_text(&text);
        let verifier = HostKeyVerifier {
            store: &store,
            policy: HostKeyPolicy::FirstTrust,
            ca: &CaStore::new(),
            now_unix: 1_700_000_000,
        };
        let result = verifier.verify("example.com", 22, "ssh-ed25519", blob, None);
        assert_eq!(result, HostKeyVerification::Revoked);
        assert!(result.is_blocked());
    }

    #[test]
    fn ca_signed_certificate_is_authorized() {
        let mut rng = OsRng;
        let ca_signing = SigningKey::generate(&mut rng);
        let ca_verifying = ca_signing.verifying_key();

        let host_key = b"host-key-under-certificate";
        let signature = ca_signing.sign(host_key);
        let certificate = HostCertificate {
            cert_key_blob: host_key.to_vec(),
            signature: signature.to_bytes().to_vec(),
            valid_after: 1_690_000_000,
            valid_before: 1_710_000_000,
            principals: vec![],
        };

        let mut ca = CaStore::new();
        ca.add(ca_verifying);
        let store = KnownHostsStore::new();
        let verifier = HostKeyVerifier {
            store: &store,
            policy: HostKeyPolicy::CertificateAuthority,
            ca: &ca,
            now_unix: 1_700_000_000,
        };
        let result = verifier.verify(
            "example.com",
            22,
            "ssh-ed25519",
            host_key,
            Some(&certificate),
        );
        assert!(matches!(
            result,
            HostKeyVerification::CertificateAuthorized { .. }
        ));
        assert!(result.is_trusted());
        assert!(!result.requires_prompt());
    }

    #[test]
    fn wrong_ca_or_expired_certificate_is_not_authorized() {
        let mut rng = OsRng;
        let trusted_ca = SigningKey::generate(&mut rng);
        let attacker_ca = SigningKey::generate(&mut rng);

        let host_key = b"host-key-under-certificate";
        // Signed by the attacker CA, not the trusted CA.
        let signature = attacker_ca.sign(host_key);
        let expired = HostCertificate {
            cert_key_blob: host_key.to_vec(),
            signature: signature.to_bytes().to_vec(),
            valid_after: 1_690_000_000,
            valid_before: 1_695_000_000, // expired by now (1_700_000_000)
            principals: vec![],
        };

        let mut ca = CaStore::new();
        ca.add(trusted_ca.verifying_key());
        let store = KnownHostsStore::new();
        let verifier = HostKeyVerifier {
            store: &store,
            policy: HostKeyPolicy::CertificateAuthority,
            ca: &ca,
            now_unix: 1_700_000_000,
        };
        // Not authorized -> falls through to first-contact Unknown.
        let result = verifier.verify("example.com", 22, "ssh-ed25519", host_key, Some(&expired));
        assert!(matches!(result, HostKeyVerification::Unknown { .. }));
        assert!(!result.is_trusted());
    }
}
