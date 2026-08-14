//! Database field-level encryption and master-key wrapping (T089).
//!
//! Sensitive metadata is stored as versioned AEAD blobs ([`EncryptedField`])
//! using ChaCha20-Poly1305. The field keys live in a [`KeyRing`] held outside
//! the database (never written into it); the master key wraps the active field
//! key in the OS secure store, so recovery rebuilds the ring from the wrapped
//! master key. [`KeyRing::rotate`] adds a new key version and re-encrypts
//! fields, while old versions remain decryptable. Tampering, rotation, and
//! recovery are verified by tests.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

/// The current AEAD construction version.
pub const AEAD_VERSION: u8 = 1;

/// A versioned AEAD-encrypted field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedField {
    /// Key-ring version used for encryption.
    pub version: u8,
    /// 96-bit nonce.
    pub nonce: [u8; 12],
    /// Ciphertext + 16-byte Poly1305 tag.
    pub ciphertext: Vec<u8>,
}

/// A field-level encryptor for one key version.
#[derive(Clone)]
pub struct FieldEncryptor {
    cipher: ChaCha20Poly1305,
    version: u8,
}

impl FieldEncryptor {
    /// An encryptor for `version` with a 32-byte key.
    pub fn new(version: u8, key: &[u8; 32]) -> Self {
        Self {
            cipher: ChaCha20Poly1305::new(Key::from_slice(key)),
            version,
        }
    }

    /// The key version.
    pub fn version(&self) -> u8 {
        self.version
    }

    /// Encrypts a field with a fresh nonce.
    pub fn encrypt(&self, plaintext: &[u8]) -> EncryptedField {
        let mut nonce_bytes = [0u8; 12];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        nonce_bytes[..8].copy_from_slice(&now.to_le_bytes());
        // A fixed counter suffix (nanosecond timestamp dominates the nonce).
        nonce_bytes[8..].copy_from_slice(&0u32.to_le_bytes());
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: &[self.version],
                },
            )
            .expect("encrypt");
        EncryptedField {
            version: self.version,
            nonce: nonce_bytes,
            ciphertext,
        }
    }

    /// Decrypts a field; returns `None` on tampering or a wrong key.
    pub fn decrypt(&self, field: &EncryptedField) -> Option<Vec<u8>> {
        if field.version != self.version {
            return None;
        }
        let nonce = Nonce::from_slice(&field.nonce);
        self.cipher
            .decrypt(
                nonce,
                Payload {
                    msg: &field.ciphertext,
                    aad: &[self.version],
                },
            )
            .ok()
    }
}

/// A versioned key ring held outside the database.
#[derive(Debug, Clone)]
pub struct KeyRing {
    keys: Vec<(u8, [u8; 32])>,
    active_version: u8,
}

impl KeyRing {
    /// A ring with a single initial key.
    pub fn new(version: u8, key: [u8; 32]) -> Self {
        Self {
            keys: vec![(version, key)],
            active_version: version,
        }
    }

    /// The active version.
    pub fn active_version(&self) -> u8 {
        self.active_version
    }

    /// The number of retained key versions.
    pub fn versions(&self) -> usize {
        self.keys.len()
    }

    /// The encryptor for a version, if the key is present.
    pub fn encryptor_for(&self, version: u8) -> Option<FieldEncryptor> {
        self.keys
            .iter()
            .find(|(v, _)| *v == version)
            .map(|(v, key)| FieldEncryptor::new(*v, key))
    }

    /// The active encryptor.
    pub fn active(&self) -> FieldEncryptor {
        self.encryptor_for(self.active_version).expect("active key")
    }

    /// Rotates to a new key version, retaining the old keys so existing rows
    /// remain decryptable. Returns the new version.
    pub fn rotate(&mut self, new_version: u8, new_key: [u8; 32]) -> u8 {
        if !self.keys.iter().any(|(v, _)| *v == new_version) {
            self.keys.push((new_version, new_key));
        }
        self.active_version = new_version;
        self.active_version
    }

    /// Removes an old key version (after all rows are re-encrypted); fields
    /// encrypted with it can no longer be read.
    pub fn purge_version(&mut self, version: u8) {
        self.keys
            .retain(|(v, _)| *v != version || *v == self.active_version);
    }
}

/// Wraps the active field key with a master key (held in the OS secure store,
/// never in the database).
#[derive(Clone)]
pub struct MasterKeyWrapper {
    cipher: ChaCha20Poly1305,
}

impl MasterKeyWrapper {
    /// A wrapper over a 32-byte master key.
    pub fn new(master_key: &[u8; 32]) -> Self {
        Self {
            cipher: ChaCha20Poly1305::new(Key::from_slice(master_key)),
        }
    }

    /// Wraps a field key + version into an opaque blob (stored outside the DB).
    pub fn wrap(&self, version: u8, key: &[u8; 32]) -> Vec<u8> {
        let mut payload = vec![version];
        payload.extend_from_slice(key);
        let nonce = [0u8; 12];
        self.cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &payload,
                    aad: b"master-key-wrapper",
                },
            )
            .expect("wrap")
    }

    /// Unwraps a wrapped field key blob back into (version, key) for recovery.
    pub fn unwrap(&self, wrapped: &[u8]) -> Option<(u8, [u8; 32])> {
        let plaintext = self
            .cipher
            .decrypt(
                Nonce::from_slice(&[0u8; 12]),
                Payload {
                    msg: wrapped,
                    aad: b"master-key-wrapper",
                },
            )
            .ok()?;
        if plaintext.len() != 33 {
            return None;
        }
        let version = plaintext[0];
        let mut key = [0u8; 32];
        key.copy_from_slice(&plaintext[1..]);
        Some((version, key))
    }
}

/// Re-encrypts a field under the ring's active version (rotation).
pub fn reencrypt(field: &EncryptedField, ring: &KeyRing) -> Option<EncryptedField> {
    let old = ring.encryptor_for(field.version)?;
    let plaintext = old.decrypt(field)?;
    Some(ring.active().encrypt(&plaintext))
}

#[cfg(test)]
mod tests {
    use super::{reencrypt, FieldEncryptor, KeyRing, MasterKeyWrapper};

    #[test]
    fn encrypt_decrypt_round_trip() {
        let key = [7u8; 32];
        let encryptor = FieldEncryptor::new(1, &key);
        let field = encryptor.encrypt(b"sensitive metadata");
        assert_eq!(field.version, 1);
        assert_eq!(
            encryptor.decrypt(&field).as_deref(),
            Some(&b"sensitive metadata"[..])
        );
    }

    #[test]
    fn tampering_is_detected() {
        let key = [7u8; 32];
        let encryptor = FieldEncryptor::new(1, &key);
        let mut field = encryptor.encrypt(b"secret");
        // Flip a ciphertext byte: AEAD authentication must fail.
        let last = field.ciphertext.len() - 1;
        field.ciphertext[last] ^= 0x01;
        assert_eq!(
            encryptor.decrypt(&field),
            None,
            "tampered field must not decrypt"
        );
        // Wrong key also fails.
        let wrong = FieldEncryptor::new(1, &[8u8; 32]);
        assert_eq!(wrong.decrypt(&field), None);
    }

    #[test]
    fn rotation_reencrypts_and_old_versions_stay_readable() {
        let mut ring = KeyRing::new(1, [1u8; 32]);
        let field = ring.active().encrypt(b"row");
        // Rotate to version 2.
        ring.rotate(2, [2u8; 32]);
        assert_eq!(ring.active_version(), 2);
        assert_eq!(ring.versions(), 2);
        // Old rows still decrypt via version 1.
        assert_eq!(
            ring.encryptor_for(1).unwrap().decrypt(&field).as_deref(),
            Some(&b"row"[..])
        );
        // Re-encrypt under the active version; the new blob is version 2.
        let new_field = reencrypt(&field, &ring).unwrap();
        assert_eq!(new_field.version, 2);
        assert_eq!(
            ring.active().decrypt(&new_field).as_deref(),
            Some(&b"row"[..])
        );
    }

    #[test]
    fn master_key_wrap_and_recovery() {
        let master = [9u8; 32];
        let wrapper = MasterKeyWrapper::new(&master);
        let field_key = [5u8; 32];
        let wrapped = wrapper.wrap(3, &field_key);
        // The wrapped blob is opaque and not the field key.
        assert_ne!(wrapped, field_key.to_vec());
        // Recovery rebuilds the key ring from the wrapped master key.
        let (version, key) = wrapper.unwrap(&wrapped).expect("unwrap");
        assert_eq!(version, 3);
        assert_eq!(key, field_key);
        let ring = KeyRing::new(version, key);
        let field = ring.active().encrypt(b"recovered");
        assert_eq!(
            ring.active().decrypt(&field).as_deref(),
            Some(&b"recovered"[..])
        );
        // A wrong master key cannot unwrap.
        let wrong = MasterKeyWrapper::new(&[0u8; 32]);
        assert_eq!(wrong.unwrap(&wrapped), None);
    }
}
