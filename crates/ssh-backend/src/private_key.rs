use base64::Engine as _;
use pkcs8::SecretDocument;
use secret::SecretString;
use ssh_key::PrivateKey as SshPrivateKey;

/// Detected private key container format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateKeyFormat {
    /// OpenSSH format (`BEGIN OPENSSH PRIVATE KEY`).
    OpenSsh,
    /// PKCS#8 (unencrypted) `BEGIN PRIVATE KEY`.
    Pkcs8,
    /// PKCS#8 (encrypted) `BEGIN ENCRYPTED PRIVATE KEY`.
    EncryptedPkcs8,
    /// Not a supported container.
    Unknown,
}

/// Detects the private key container format from PEM text.
pub fn detect_format(pem: &str) -> PrivateKeyFormat {
    if pem.contains("BEGIN OPENSSH PRIVATE KEY") {
        PrivateKeyFormat::OpenSsh
    } else if pem.contains("BEGIN ENCRYPTED PRIVATE KEY") {
        PrivateKeyFormat::EncryptedPkcs8
    } else if pem.contains("BEGIN PRIVATE KEY") {
        PrivateKeyFormat::Pkcs8
    } else {
        PrivateKeyFormat::Unknown
    }
}

/// Key algorithm family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAlgorithm {
    /// Ed25519.
    Ed25519,
    /// ECDSA NIST P-256.
    EcdsaP256,
    /// ECDSA NIST P-384.
    EcdsaP384,
    /// ECDSA NIST P-521.
    EcdsaP521,
    /// RSA.
    Rsa,
    /// Unsupported algorithm.
    Unsupported,
}

impl KeyAlgorithm {
    /// Stable name.
    pub const fn as_str(self) -> &'static str {
        match self {
            KeyAlgorithm::Ed25519 => "ssh-ed25519",
            KeyAlgorithm::EcdsaP256 => "ecdsa-sha2-nistp256",
            KeyAlgorithm::EcdsaP384 => "ecdsa-sha2-nistp384",
            KeyAlgorithm::EcdsaP521 => "ecdsa-sha2-nistp521",
            KeyAlgorithm::Rsa => "ssh-rsa",
            KeyAlgorithm::Unsupported => "unsupported",
        }
    }
}

/// Private key load error (no secret context).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    /// The key is encrypted and no passphrase was provided.
    Encrypted,
    /// The provided passphrase is wrong.
    WrongPassphrase,
    /// The container format is not supported (e.g. PKCS#1 PEM).
    UnsupportedFormat,
    /// The key material is malformed.
    Malformed,
    /// The key algorithm is unsupported.
    UnsupportedAlgorithm,
}

impl KeyError {
    /// Stable string code.
    pub const fn stable_code(self) -> &'static str {
        match self {
            KeyError::Encrypted => "E_KEY_ENCRYPTED",
            KeyError::WrongPassphrase => "E_KEY_WRONG_PASSPHRASE",
            KeyError::UnsupportedFormat => "E_KEY_UNSUPPORTED_FORMAT",
            KeyError::Malformed => "E_KEY_MALFORMED",
            KeyError::UnsupportedAlgorithm => "E_KEY_UNSUPPORTED_ALGORITHM",
        }
    }
}

/// A parsed private key handle.
///
/// Secret material is held in zeroizing containers (ssh-key `PrivateKey`
/// zeroizes on drop; PKCS#8 uses `pkcs8::SecretDocument`). The passphrase is
/// never retained.
#[derive(Debug)]
pub enum PrivateKeyHandle {
    /// OpenSSH-format key.
    OpenSsh(Box<SshPrivateKey>),
    /// PKCS#8-format key (zeroizing secret document).
    Pkcs8 {
        /// Algorithm family.
        algorithm: KeyAlgorithm,
        /// Zeroizing PKCS#8 document.
        secret: pkcs8::SecretDocument,
    },
}

impl PrivateKeyHandle {
    /// The key algorithm family.
    pub fn algorithm(&self) -> KeyAlgorithm {
        match self {
            PrivateKeyHandle::OpenSsh(key) => map_ssh_algorithm(key.algorithm()),
            PrivateKeyHandle::Pkcs8 { algorithm, .. } => *algorithm,
        }
    }

    /// SHA-256 fingerprint of the public key, when derivable.
    pub fn public_fingerprint_sha256(&self) -> Option<String> {
        match self {
            PrivateKeyHandle::OpenSsh(key) => {
                use sha2::{Digest, Sha256};
                let Ok(blob) = key.public_key().to_bytes() else {
                    return None;
                };
                let digest = Sha256::digest(&blob);
                Some(base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest))
            }
            PrivateKeyHandle::Pkcs8 { .. } => None,
        }
    }
}

fn map_ssh_algorithm(algorithm: ssh_key::Algorithm) -> KeyAlgorithm {
    use ssh_key::Algorithm;
    match algorithm {
        Algorithm::Ed25519 => KeyAlgorithm::Ed25519,
        Algorithm::Ecdsa { curve } => match curve {
            ssh_key::EcdsaCurve::NistP256 => KeyAlgorithm::EcdsaP256,
            ssh_key::EcdsaCurve::NistP384 => KeyAlgorithm::EcdsaP384,
            ssh_key::EcdsaCurve::NistP521 => KeyAlgorithm::EcdsaP521,
        },
        Algorithm::Rsa { .. } => KeyAlgorithm::Rsa,
        _ => KeyAlgorithm::Unsupported,
    }
}

fn classify_pkcs8_algorithm(info: &pkcs8::PrivateKeyInfo<'_>) -> KeyAlgorithm {
    use pkcs8::ObjectIdentifier;
    let oid = info.algorithm.oid;
    if oid == ObjectIdentifier::new_unwrap("1.3.101.112") {
        return KeyAlgorithm::Ed25519;
    }
    if oid == ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1") {
        return KeyAlgorithm::Rsa;
    }
    if oid == ObjectIdentifier::new_unwrap("1.2.840.10045.2.1") {
        let curve = info.algorithm.parameters_oid().ok();
        if curve == Some(ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7")) {
            return KeyAlgorithm::EcdsaP256;
        }
        if curve == Some(ObjectIdentifier::new_unwrap("1.3.132.0.34")) {
            return KeyAlgorithm::EcdsaP384;
        }
        if curve == Some(ObjectIdentifier::new_unwrap("1.3.132.0.35")) {
            return KeyAlgorithm::EcdsaP521;
        }
        return KeyAlgorithm::Unsupported;
    }
    KeyAlgorithm::Unsupported
}

/// Loads a private key from PEM text.
///
/// If the key is encrypted, a passphrase is required; the passphrase is used
/// transiently and never retained.
pub fn load_private_key(
    pem: &str,
    passphrase: Option<&SecretString>,
) -> Result<PrivateKeyHandle, KeyError> {
    match detect_format(pem) {
        PrivateKeyFormat::OpenSsh => {
            let key = SshPrivateKey::from_openssh(pem).map_err(|_| KeyError::Malformed)?;
            if key.is_encrypted() {
                let passphrase = passphrase.ok_or(KeyError::Encrypted)?;
                let decrypted = key
                    .decrypt(passphrase.expose_secret())
                    .map_err(|_| KeyError::WrongPassphrase)?;
                Ok(PrivateKeyHandle::OpenSsh(Box::new(decrypted)))
            } else {
                Ok(PrivateKeyHandle::OpenSsh(Box::new(key)))
            }
        }
        PrivateKeyFormat::Pkcs8 => {
            let (label, der) =
                pem_rfc7468::decode_vec(pem.as_bytes()).map_err(|_| KeyError::Malformed)?;
            if label != "PRIVATE KEY" {
                return Err(KeyError::Malformed);
            }
            let info =
                pkcs8::PrivateKeyInfo::try_from(der.as_slice()).map_err(|_| KeyError::Malformed)?;
            let algorithm = classify_pkcs8_algorithm(&info);
            if algorithm == KeyAlgorithm::Unsupported {
                return Err(KeyError::UnsupportedAlgorithm);
            }
            let secret = SecretDocument::try_from(&info).map_err(|_| KeyError::Malformed)?;
            Ok(PrivateKeyHandle::Pkcs8 { algorithm, secret })
        }
        PrivateKeyFormat::EncryptedPkcs8 => {
            let passphrase = passphrase.ok_or(KeyError::Encrypted)?;
            let (label, der) =
                pem_rfc7468::decode_vec(pem.as_bytes()).map_err(|_| KeyError::Malformed)?;
            if label != "ENCRYPTED PRIVATE KEY" {
                return Err(KeyError::Malformed);
            }
            let encrypted = pkcs8::EncryptedPrivateKeyInfo::try_from(der.as_slice())
                .map_err(|_| KeyError::Malformed)?;
            let secret = encrypted
                .decrypt(passphrase.expose_secret().as_bytes())
                .map_err(|_| KeyError::WrongPassphrase)?;
            let info = pkcs8::PrivateKeyInfo::try_from(secret.as_bytes())
                .map_err(|_| KeyError::Malformed)?;
            let algorithm = classify_pkcs8_algorithm(&info);
            if algorithm == KeyAlgorithm::Unsupported {
                return Err(KeyError::UnsupportedAlgorithm);
            }
            Ok(PrivateKeyHandle::Pkcs8 { algorithm, secret })
        }
        PrivateKeyFormat::Unknown => Err(KeyError::UnsupportedFormat),
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_format, load_private_key, KeyAlgorithm, KeyError, PrivateKeyFormat};
    use rand::rngs::OsRng;
    use secret::SecretString;
    use ssh_key::{Algorithm, LineEnding, PrivateKey};

    // Real PKCS#8 fixtures from the RustCrypto pkcs8 crate test suite.
    const ED25519_PKCS8_PEM: &str = include_str!("../tests/fixtures/ed25519-priv-pkcs8v2.pem");
    const ED25519_ENC_PKCS8_PEM: &str =
        include_str!("../tests/fixtures/ed25519-encpriv-aes256-pbkdf2-sha256.pem");
    const P256_PKCS8_PEM: &str = include_str!("../tests/fixtures/p256-priv.pem");

    fn pass(value: &str) -> SecretString {
        SecretString::from(value)
    }

    fn ssh_pem(algorithm: Algorithm) -> String {
        let key = PrivateKey::random(&mut OsRng, algorithm).expect("generate key");
        key.to_openssh(LineEnding::LF).expect("encode").to_string()
    }

    #[test]
    fn format_detection_handles_all_supported_containers() {
        assert_eq!(
            detect_format("-----BEGIN OPENSSH PRIVATE KEY-----\n"),
            PrivateKeyFormat::OpenSsh
        );
        assert_eq!(
            detect_format("-----BEGIN PRIVATE KEY-----\n"),
            PrivateKeyFormat::Pkcs8
        );
        assert_eq!(
            detect_format("-----BEGIN ENCRYPTED PRIVATE KEY-----\n"),
            PrivateKeyFormat::EncryptedPkcs8
        );
        assert_eq!(
            detect_format("-----BEGIN RSA PRIVATE KEY-----\n"),
            PrivateKeyFormat::Unknown
        );
        assert_eq!(detect_format("junk"), PrivateKeyFormat::Unknown);
    }

    #[test]
    fn openssh_key_matrix_parses_all_algorithms() {
        // Fast algorithms are generated in-test.
        for (algorithm, expected) in [
            (Algorithm::Ed25519, KeyAlgorithm::Ed25519),
            (
                Algorithm::Ecdsa {
                    curve: ssh_key::EcdsaCurve::NistP256,
                },
                KeyAlgorithm::EcdsaP256,
            ),
            (
                Algorithm::Ecdsa {
                    curve: ssh_key::EcdsaCurve::NistP384,
                },
                KeyAlgorithm::EcdsaP384,
            ),
            (
                Algorithm::Ecdsa {
                    curve: ssh_key::EcdsaCurve::NistP521,
                },
                KeyAlgorithm::EcdsaP521,
            ),
        ] {
            let pem = ssh_pem(algorithm);
            let handle = load_private_key(&pem, None).expect("parse");
            assert_eq!(handle.algorithm(), expected, "algorithm mismatch");
            assert!(
                handle.public_fingerprint_sha256().is_some(),
                "OpenSSH keys must expose a SHA-256 public fingerprint"
            );
        }
        // RSA key generation is slow; use a pre-generated real fixture.
        const RSA_OPENSSH_PEM: &str = include_str!("../tests/fixtures/rsa-openssh.pem");
        let handle = load_private_key(RSA_OPENSSH_PEM, None).expect("parse rsa fixture");
        assert_eq!(handle.algorithm(), KeyAlgorithm::Rsa);
        assert!(handle.public_fingerprint_sha256().is_some());
    }

    #[test]
    fn encrypted_openssh_key_requires_and_uses_passphrase() {
        let pem = ssh_pem(Algorithm::Ed25519);
        let key = PrivateKey::from_openssh(&pem).expect("parse plain");
        let encrypted = key.encrypt(&mut OsRng, "correct horse").expect("encrypt");
        let encrypted_pem = encrypted
            .to_openssh(LineEnding::LF)
            .expect("encode")
            .to_string();

        // Without a passphrase -> Encrypted.
        assert!(matches!(
            load_private_key(&encrypted_pem, None),
            Err(KeyError::Encrypted)
        ));
        // Wrong passphrase -> WrongPassphrase.
        assert!(matches!(
            load_private_key(&encrypted_pem, Some(&pass("wrong"))),
            Err(KeyError::WrongPassphrase)
        ));
        // Correct passphrase -> decrypted with matching algorithm.
        let handle =
            load_private_key(&encrypted_pem, Some(&pass("correct horse"))).expect("decrypt");
        assert_eq!(handle.algorithm(), KeyAlgorithm::Ed25519);
    }

    #[test]
    fn pkcs8_plain_fixtures_parse() {
        let handle = load_private_key(ED25519_PKCS8_PEM, None).expect("parse ed25519 pkcs8");
        assert_eq!(handle.algorithm(), KeyAlgorithm::Ed25519);

        let p256 = load_private_key(P256_PKCS8_PEM, None).expect("parse p256 pkcs8");
        assert_eq!(p256.algorithm(), KeyAlgorithm::EcdsaP256);
    }

    #[test]
    fn encrypted_pkcs8_fixture_requires_and_uses_passphrase() {
        assert_eq!(
            detect_format(ED25519_ENC_PKCS8_PEM),
            PrivateKeyFormat::EncryptedPkcs8
        );
        // Without a passphrase -> Encrypted.
        assert!(matches!(
            load_private_key(ED25519_ENC_PKCS8_PEM, None),
            Err(KeyError::Encrypted)
        ));
        // Wrong passphrase -> WrongPassphrase.
        assert!(matches!(
            load_private_key(ED25519_ENC_PKCS8_PEM, Some(&pass("wrong"))),
            Err(KeyError::WrongPassphrase)
        ));
        // Correct passphrase (the fixture password is "hunter42").
        let handle = load_private_key(ED25519_ENC_PKCS8_PEM, Some(&pass("hunter42")))
            .expect("decrypt pkcs8");
        assert_eq!(handle.algorithm(), KeyAlgorithm::Ed25519);
    }

    #[test]
    fn unsupported_formats_are_rejected_with_clear_errors() {
        // PKCS#1 RSA PEM is not in the supported set.
        let pkcs1 =
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----\n";
        assert!(matches!(
            load_private_key(pkcs1, None),
            Err(KeyError::UnsupportedFormat)
        ));
        // Malformed OpenSSH.
        let malformed =
            "-----BEGIN OPENSSH PRIVATE KEY-----\nnot-base64\n-----END OPENSSH PRIVATE KEY-----\n";
        assert!(matches!(
            load_private_key(malformed, None),
            Err(KeyError::Malformed)
        ));
    }

    #[test]
    fn error_codes_are_unique() {
        let codes = [
            KeyError::Encrypted.stable_code(),
            KeyError::WrongPassphrase.stable_code(),
            KeyError::UnsupportedFormat.stable_code(),
            KeyError::Malformed.stable_code(),
            KeyError::UnsupportedAlgorithm.stable_code(),
        ];
        let mut seen = std::collections::HashSet::new();
        for code in codes {
            assert!(code.starts_with("E_KEY_"), "prefix required: {code}");
            assert!(seen.insert(code), "duplicate code: {code}");
        }
    }
}
