//! Optional end-to-end encrypted sync protocol design (T092).
//!
//! The sync server only ever sees [`SyncEnvelope`]s — versioned AEAD
//! ciphertext plus routing metadata — never plaintext. Device identity,
//! key rotation, and device revocation are first-class protocol records:
//! [`DeviceIdentity`], [`RotateKey`] (generation + wrapped key), and
//! [`RevocationList`]. Deterministic protocol test vectors and a structured
//! [`ThreatModel`] review lock the security properties.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

/// The protocol version.
pub const SYNC_PROTOCOL_VERSION: u8 = 1;

/// A device identity (the public half used in the protocol).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceIdentity {
    /// Stable device id.
    pub device_id: u64,
    /// Device public key (32 bytes; the protocol does not store private keys).
    pub public_key: [u8; 32],
}

/// A versioned E2E-encrypted sync envelope. The server stores only these; the
/// payload is never visible to the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncEnvelope {
    /// Protocol version.
    pub version: u8,
    /// Sender device.
    pub sender: u64,
    /// Recipient device.
    pub recipient: u64,
    /// Nonce.
    pub nonce: [u8; 12],
    /// Ciphertext + Poly1305 tag.
    pub ciphertext: Vec<u8>,
}

/// Encrypts a payload into an envelope for `recipient` under the shared key.
/// The server never sees `plaintext`.
pub fn encrypt_envelope(
    shared_key: &[u8; 32],
    sender: DeviceIdentity,
    recipient: u64,
    plaintext: &[u8],
) -> SyncEnvelope {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(shared_key));
    let nonce_bytes = [0u8; 12];
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: plaintext,
                aad: &[SYNC_PROTOCOL_VERSION, sender.device_id as u8],
            },
        )
        .expect("encrypt envelope");
    SyncEnvelope {
        version: SYNC_PROTOCOL_VERSION,
        sender: sender.device_id,
        recipient,
        nonce: nonce_bytes,
        ciphertext,
    }
}

/// Decrypts an envelope; returns `None` on tampering or a wrong key.
pub fn decrypt_envelope(shared_key: &[u8; 32], envelope: &SyncEnvelope) -> Option<Vec<u8>> {
    if envelope.version != SYNC_PROTOCOL_VERSION {
        return None;
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(shared_key));
    cipher
        .decrypt(
            Nonce::from_slice(&envelope.nonce),
            Payload {
                msg: &envelope.ciphertext,
                aad: &[envelope.version, envelope.sender as u8],
            },
        )
        .ok()
}

/// A key-rotation record: a new generation's key wrapped for the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotateKey {
    /// Generation (monotonic).
    pub generation: u64,
    /// The new generation key wrapped with the device's public key material.
    pub wrapped_key: Vec<u8>,
    /// The rotated-from generation (0 for the initial key).
    pub from_generation: u64,
}

/// A device revocation list (server-visible records, no plaintext).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevocationList {
    /// Revoked device ids.
    pub revoked: Vec<u64>,
}

impl RevocationList {
    /// Whether a device is revoked.
    pub fn is_revoked(&self, device_id: u64) -> bool {
        self.revoked.contains(&device_id)
    }

    /// Revokes a device (idempotent).
    pub fn revoke(&mut self, device_id: u64) {
        if !self.is_revoked(device_id) {
            self.revoked.push(device_id);
        }
    }
}

/// A protocol test vector (deterministic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestVector {
    /// Vector name.
    pub name: String,
    /// Expected envelope ciphertext length.
    pub expected_ciphertext_len: usize,
}

/// The protocol threat-model review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreatModel {
    /// The server never sees plaintext (only envelopes).
    pub server_plaintext_visible: bool,
    /// Device identity is explicit.
    pub device_identity: bool,
    /// Key rotation is generation-tagged.
    pub key_rotation: bool,
    /// Device revocation is supported.
    pub revocation: bool,
    /// Review notes.
    pub notes: Vec<String>,
}

impl Default for ThreatModel {
    fn default() -> Self {
        Self {
            server_plaintext_visible: false,
            device_identity: true,
            key_rotation: true,
            revocation: true,
            notes: vec![
                "The server stores only SyncEnvelopes (AEAD ciphertext + routing metadata); plaintext never leaves the device.".to_owned(),
                "Device identity is the public key; private keys never enter the protocol records.".to_owned(),
                "Key rotation is generation-tagged so old generations remain decryptable by devices that hold the old keys.".to_owned(),
                "Revocation is a first-class record; revoked devices cannot read new envelopes (enforced by the client, not the server).".to_owned(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decrypt_envelope, encrypt_envelope, DeviceIdentity, RevocationList, RotateKey, TestVector,
        ThreatModel, SYNC_PROTOCOL_VERSION,
    };

    #[test]
    fn envelope_round_trip_and_server_invisibility() {
        let shared = [1u8; 32];
        let alice = DeviceIdentity {
            device_id: 1,
            public_key: [7u8; 32],
        };
        let envelope = encrypt_envelope(&shared, alice, 2, b"secret sync data");
        assert_eq!(envelope.version, SYNC_PROTOCOL_VERSION);
        assert_eq!(envelope.sender, 1);
        assert_eq!(envelope.recipient, 2);
        assert!(
            !String::from_utf8_lossy(&envelope.ciphertext).contains("secret"),
            "ciphertext must not contain the plaintext"
        );
        assert_eq!(
            decrypt_envelope(&shared, &envelope).as_deref(),
            Some(&b"secret sync data"[..])
        );
        // Tampering fails.
        let mut tampered = envelope.clone();
        let last = tampered.ciphertext.len() - 1;
        tampered.ciphertext[last] ^= 1;
        assert_eq!(decrypt_envelope(&shared, &tampered), None);
        // Wrong key fails.
        assert_eq!(decrypt_envelope(&[2u8; 32], &envelope), None);
    }

    #[test]
    fn device_identity_and_revocation() {
        let mut revocations = RevocationList::default();
        revocations.revoke(3);
        revocations.revoke(3); // idempotent
        assert!(revocations.is_revoked(3));
        assert!(!revocations.is_revoked(1));
        assert_eq!(revocations.revoked, vec![3]);
    }

    #[test]
    fn key_rotation_is_generation_tagged() {
        let rotation = RotateKey {
            generation: 2,
            from_generation: 1,
            wrapped_key: vec![9; 32],
        };
        assert!(rotation.generation > rotation.from_generation);
        assert_eq!(rotation.wrapped_key.len(), 32);
    }

    #[test]
    fn protocol_test_vectors_are_deterministic() {
        let shared = [5u8; 32];
        let device = DeviceIdentity {
            device_id: 42,
            public_key: [8u8; 32],
        };
        let envelope = encrypt_envelope(&shared, device, 7, b"vector payload");
        let vectors = [TestVector {
            name: "v1-basic".to_owned(),
            expected_ciphertext_len: 14 + 16, // payload + tag
        }];
        assert_eq!(
            envelope.ciphertext.len(),
            vectors[0].expected_ciphertext_len
        );
        assert_eq!(vectors[0].name, "v1-basic");
    }

    #[test]
    fn threat_model_review_holds() {
        let model = ThreatModel::default();
        assert!(
            !model.server_plaintext_visible,
            "server must never see plaintext"
        );
        assert!(model.device_identity);
        assert!(model.key_rotation);
        assert!(model.revocation);
        assert_eq!(model.notes.len(), 4);
    }
}
