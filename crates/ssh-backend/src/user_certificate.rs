use ed25519_dalek::VerifyingKey;
use sha2::{Digest, Sha256};
use ssh_key::certificate::Certificate;
use ssh_key::{Fingerprint, HashAlg};

/// Outcome of verifying an OpenSSH user certificate.
///
/// Each failure is diagnosable so the UI can explain exactly why a
/// certificate was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserCertVerification {
    /// The certificate is valid for the requested principal now.
    Valid,
    /// The certificate text could not be parsed.
    Malformed,
    /// The current time is before `valid_after`.
    NotYetValid,
    /// The current time is at or after `valid_before`.
    Expired,
    /// The certificate signature does not verify against the embedded CA key.
    SignatureInvalid,
    /// The embedded CA key is not one of the trusted CAs.
    UntrustedCa,
    /// The certificate does not list the requested principal.
    PrincipalMismatch {
        /// Principals the certificate allows.
        allowed: Vec<String>,
        /// The requested principal.
        requested: String,
    },
}

impl UserCertVerification {
    /// Whether the certificate is accepted.
    pub fn is_valid(&self) -> bool {
        matches!(self, UserCertVerification::Valid)
    }
}

/// SHA-256 fingerprint of an Ed25519 CA key in OpenSSH public-key blob form.
///
/// Matches `ssh-key`'s `KeyData::fingerprint(HashAlg::Sha256)` so it can be
/// compared against the fingerprint embedded in a certificate.
pub fn ed25519_ca_fingerprint(ca: &VerifyingKey) -> Fingerprint {
    let name = b"ssh-ed25519";
    let mut blob = Vec::with_capacity(4 + name.len() + 4 + 32);
    blob.extend_from_slice(&(name.len() as u32).to_be_bytes());
    blob.extend_from_slice(name);
    blob.extend_from_slice(&(32u32).to_be_bytes());
    blob.extend_from_slice(&ca.to_bytes());
    Fingerprint::Sha256(Sha256::digest(&blob).into())
}

/// Verifies an OpenSSH user certificate against trusted CA fingerprints.
///
/// Checks, in order: parse, validity window (diagnosable), embedded signature,
/// CA trust, and requested principal.
pub fn verify_user_certificate(
    certificate_pem: &str,
    ca_fingerprints: &[Fingerprint],
    now_unix: u64,
    requested_principal: &str,
) -> UserCertVerification {
    let Ok(certificate) = Certificate::from_openssh(certificate_pem) else {
        return UserCertVerification::Malformed;
    };

    if now_unix < certificate.valid_after() {
        return UserCertVerification::NotYetValid;
    }
    if now_unix >= certificate.valid_before() {
        return UserCertVerification::Expired;
    }

    if certificate.verify_signature().is_err() {
        return UserCertVerification::SignatureInvalid;
    }

    let cert_fingerprint = certificate.signature_key().fingerprint(HashAlg::Sha256);
    if !ca_fingerprints.contains(&cert_fingerprint) {
        return UserCertVerification::UntrustedCa;
    }

    let allowed = certificate.valid_principals();
    if !allowed.is_empty()
        && !allowed
            .iter()
            .any(|principal| principal == requested_principal)
    {
        return UserCertVerification::PrincipalMismatch {
            allowed: allowed.to_vec(),
            requested: requested_principal.to_owned(),
        };
    }

    UserCertVerification::Valid
}

#[cfg(test)]
mod tests {
    use super::{ed25519_ca_fingerprint, verify_user_certificate, UserCertVerification};
    use rand::rngs::OsRng;
    use ssh_key::certificate::{Builder, CertType};
    use ssh_key::{Algorithm, PrivateKey};

    const NOW: u64 = 1_700_000_000;

    struct Keys {
        ca: PrivateKey,
        user: PrivateKey,
    }

    fn keys() -> Keys {
        Keys {
            ca: PrivateKey::random(&mut OsRng, Algorithm::Ed25519).expect("ca key"),
            user: PrivateKey::random(&mut OsRng, Algorithm::Ed25519).expect("user key"),
        }
    }

    fn sign_user_cert(
        ca: &PrivateKey,
        user: &PrivateKey,
        after: u64,
        before: u64,
        principal: &str,
    ) -> String {
        let mut builder =
            Builder::new_with_random_nonce(&mut OsRng, user.public_key().clone(), after, before)
                .expect("builder");
        builder.cert_type(CertType::User).expect("cert type");
        if principal.is_empty() {
            builder.all_principals_valid().expect("all principals");
        } else {
            builder.valid_principal(principal).expect("principal");
        }
        let certificate = builder.sign(ca).expect("sign cert");
        certificate.to_openssh().expect("encode cert")
    }

    #[test]
    fn trusted_ca_valid_certificate_is_accepted() {
        let keys = keys();
        let pem = sign_user_cert(&keys.ca, &keys.user, NOW - 100, NOW + 100, "alice");
        let fingerprints = vec![ed25519_ca_fingerprint(&ca_verifying(&keys.ca))];
        assert_eq!(
            verify_user_certificate(&pem, &fingerprints, NOW, "alice"),
            UserCertVerification::Valid
        );
    }

    #[test]
    fn expired_certificate_is_diagnosed() {
        let keys = keys();
        let pem = sign_user_cert(&keys.ca, &keys.user, NOW - 200, NOW - 100, "alice");
        let fingerprints = vec![ed25519_ca_fingerprint(&ca_verifying(&keys.ca))];
        assert_eq!(
            verify_user_certificate(&pem, &fingerprints, NOW, "alice"),
            UserCertVerification::Expired
        );
    }

    #[test]
    fn not_yet_valid_certificate_is_diagnosed() {
        let keys = keys();
        let pem = sign_user_cert(&keys.ca, &keys.user, NOW + 100, NOW + 200, "alice");
        let fingerprints = vec![ed25519_ca_fingerprint(&ca_verifying(&keys.ca))];
        assert_eq!(
            verify_user_certificate(&pem, &fingerprints, NOW, "alice"),
            UserCertVerification::NotYetValid
        );
    }

    #[test]
    fn untrusted_ca_is_diagnosed() {
        let keys = keys();
        let attacker = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).expect("attacker ca");
        let pem = sign_user_cert(&attacker, &keys.user, NOW - 100, NOW + 100, "alice");
        let fingerprints = vec![ed25519_ca_fingerprint(&ca_verifying(&keys.ca))];
        assert_eq!(
            verify_user_certificate(&pem, &fingerprints, NOW, "alice"),
            UserCertVerification::UntrustedCa
        );
    }

    #[test]
    fn principal_mismatch_is_diagnosed() {
        let keys = keys();
        let pem = sign_user_cert(&keys.ca, &keys.user, NOW - 100, NOW + 100, "alice");
        let fingerprints = vec![ed25519_ca_fingerprint(&ca_verifying(&keys.ca))];
        assert_eq!(
            verify_user_certificate(&pem, &fingerprints, NOW, "bob"),
            UserCertVerification::PrincipalMismatch {
                allowed: vec!["alice".to_owned()],
                requested: "bob".to_owned(),
            }
        );
    }

    #[test]
    fn malformed_certificate_is_diagnosed() {
        assert_eq!(
            verify_user_certificate("not-a-certificate", &[], NOW, "alice"),
            UserCertVerification::Malformed
        );
    }

    #[test]
    fn wildcard_principal_certificate_accepts_any() {
        let keys = keys();
        let pem = sign_user_cert(&keys.ca, &keys.user, NOW - 100, NOW + 100, "");
        let fingerprints = vec![ed25519_ca_fingerprint(&ca_verifying(&keys.ca))];
        assert_eq!(
            verify_user_certificate(&pem, &fingerprints, NOW, "anyone"),
            UserCertVerification::Valid
        );
    }

    fn ca_verifying(key: &PrivateKey) -> VerifyingKey {
        // Reconstruct the Ed25519 verifying key from the ssh-key CA key.
        let bytes = key.public_key().to_bytes().expect("public bytes");
        // The public key blob for Ed25519 is `ssh-ed25519` || len(32) || key.
        let key_bytes = &bytes[bytes.len() - 32..];
        VerifyingKey::from_bytes(key_bytes.try_into().expect("32 bytes")).expect("verifying key")
    }

    use ed25519_dalek::VerifyingKey;
}
