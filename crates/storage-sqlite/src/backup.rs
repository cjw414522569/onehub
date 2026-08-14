//! Secure import / export and encrypted backup format (T090).
//!
//! [`BackupArchive`] is a versioned, password-encrypted backup: the payload is
//! encrypted with ChaCha20-Poly1305 under a key derived from the passphrase
//! via scrypt with explicit, sufficient KDF parameters (salt, log_n, r, p),
//! and the archive declares its [`ExportScope`] so the export range is
//! explicit. Round-trip and wrong-passphrase behavior are verified by tests.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use scrypt::{scrypt, Params};

/// The current backup format version.
pub const BACKUP_VERSION: u8 = 1;

/// scrypt KDF parameters (sufficient defaults).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    /// 16-byte random salt.
    pub salt: [u8; 16],
    /// log2 of the N parameter.
    pub log_n: u8,
    /// r parameter.
    pub r: u32,
    /// p parameter.
    pub p: u32,
}

impl Default for KdfParams {
    /// N=2^15, r=8, p=1 (memory ~32 MiB; sufficient for password KDF).
    fn default() -> Self {
        Self {
            salt: [0u8; 16],
            log_n: 15,
            r: 8,
            p: 1,
        }
    }
}

/// The explicit export scope (which categories are included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportScope {
    /// Included categories, e.g. "profiles", "hosts", "known_hosts".
    pub includes: Vec<String>,
}

impl ExportScope {
    /// A scope that includes every category.
    pub fn all() -> Self {
        Self {
            includes: vec![
                "profiles".to_owned(),
                "hosts".to_owned(),
                "known_hosts".to_owned(),
                "settings".to_owned(),
            ],
        }
    }

    /// Whether a category is included.
    pub fn includes_category(&self, category: &str) -> bool {
        self.includes.iter().any(|c| c == category)
    }
}

/// A versioned, password-encrypted backup archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupArchive {
    /// Format version.
    pub version: u8,
    /// KDF parameters.
    pub kdf: KdfParams,
    /// The export scope.
    pub scope: ExportScope,
    /// Nonce.
    pub nonce: [u8; 12],
    /// Ciphertext + Poly1305 tag.
    pub ciphertext: Vec<u8>,
}

/// A backup error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupError {
    /// The passphrase is wrong (AEAD authentication failed).
    BadPassphrase,
    /// The archive version is unsupported.
    UnsupportedVersion(u8),
    /// The KDF parameters are insufficient.
    WeakKdf,
}

/// A random 16-byte salt (cryptographic randomness via getrandom).
pub fn random_salt() -> [u8; 16] {
    let mut salt = [0u8; 16];
    let _ = getrandom::getrandom(&mut salt);
    salt
}

/// Derives a 32-byte key from the passphrase using scrypt.
pub fn derive_key(passphrase: &[u8], kdf: &KdfParams) -> Result<[u8; 32], BackupError> {
    let log_n = kdf.log_n;
    if log_n < 15 || kdf.r < 8 || kdf.p < 1 {
        return Err(BackupError::WeakKdf);
    }
    let params = Params::new(log_n, kdf.r, kdf.p, 32).map_err(|_| BackupError::WeakKdf)?;
    let mut key = [0u8; 32];
    scrypt(passphrase, &kdf.salt, &params, &mut key).map_err(|_| BackupError::WeakKdf)?;
    Ok(key)
}

/// Encrypts a payload into a versioned, password-protected backup.
pub fn encrypt_backup(
    passphrase: &[u8],
    salt: [u8; 16],
    scope: ExportScope,
    payload: &[u8],
) -> Result<BackupArchive, BackupError> {
    let kdf = KdfParams {
        salt,
        ..KdfParams::default()
    };
    let key = derive_key(passphrase, &kdf)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let nonce_bytes = [0u8; 12];
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: payload,
                aad: &[BACKUP_VERSION],
            },
        )
        .expect("encrypt backup");
    Ok(BackupArchive {
        version: BACKUP_VERSION,
        kdf,
        scope,
        nonce: nonce_bytes,
        ciphertext,
    })
}

/// Decrypts a backup; a wrong passphrase yields [`BackupError::BadPassphrase`].
pub fn decrypt_backup(passphrase: &[u8], archive: &BackupArchive) -> Result<Vec<u8>, BackupError> {
    if archive.version != BACKUP_VERSION {
        return Err(BackupError::UnsupportedVersion(archive.version));
    }
    let key = derive_key(passphrase, &archive.kdf)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    cipher
        .decrypt(
            Nonce::from_slice(&archive.nonce),
            Payload {
                msg: &archive.ciphertext,
                aad: &[archive.version],
            },
        )
        .map_err(|_| BackupError::BadPassphrase)
}

#[cfg(test)]
mod tests {
    use super::{decrypt_backup, encrypt_backup, random_salt, BackupError, ExportScope};

    #[test]
    fn round_trip_encrypt_decrypt() {
        let scope = ExportScope::all();
        let payload = b"profiles, hosts, known_hosts";
        let salt = [3u8; 16];
        let archive = encrypt_backup(b"hunter2", salt, scope.clone(), payload).unwrap();
        assert_eq!(archive.version, 1);
        assert_eq!(archive.scope, scope);
        assert_eq!(
            decrypt_backup(b"hunter2", &archive).unwrap(),
            payload.to_vec()
        );
        // The scope is explicit and includes the expected categories.
        assert!(archive.scope.includes_category("hosts"));
        assert!(archive.scope.includes_category("known_hosts"));
    }

    #[test]
    fn wrong_passphrase_fails() {
        let archive = encrypt_backup(b"correct", [1u8; 16], ExportScope::all(), b"data").unwrap();
        assert_eq!(
            decrypt_backup(b"wrong", &archive),
            Err(BackupError::BadPassphrase)
        );
        assert_eq!(
            decrypt_backup(b"correct", &archive).unwrap(),
            b"data".to_vec()
        );
    }

    #[test]
    fn unsupported_version_fails() {
        let archive = encrypt_backup(b"pw", [1u8; 16], ExportScope::all(), b"data").unwrap();
        let mut future = archive.clone();
        future.version = 99;
        assert_eq!(
            decrypt_backup(b"pw", &future),
            Err(BackupError::UnsupportedVersion(99))
        );
    }

    #[test]
    fn weak_kdf_parameters_are_rejected() {
        let kdf = super::KdfParams {
            log_n: 5, // too small
            ..super::KdfParams::default()
        };
        let key = super::derive_key(b"pw", &kdf);
        assert_eq!(key, Err(BackupError::WeakKdf));
    }

    #[test]
    fn random_salt_is_nonzero_and_16_bytes() {
        let salt = random_salt();
        assert_eq!(salt.len(), 16);
        assert!(salt.iter().any(|b| *b != 0), "salt should not be all zeros");
    }
}
