//! End-to-end multi-device security scenario (T095).
//!
//! Combines the T092 envelope protocol, the T094 minimal trusted backend,
//! and the T095 device lifecycle to verify the acceptance criteria:
//! - a lost device can be revoked, and
//! - old/revoked devices cannot read new data.
//!
//! Each device's keys (device key + wrapped data keys) live in a
//! `secure_store::MemorySecureStore`, mirroring production device vaults.

use secure_store::{MemorySecureStore, SecureStore};
use sync_core::device_lifecycle::{DataKey, Device, DeviceKey, KeyManager, RecoveryCode};
use sync_core::sync_protocol::{decrypt_envelope, encrypt_envelope, DeviceIdentity};
use sync_service::{ServiceConfig, SyncBackend, SystemClock};

/// A simulated device: identity + its secure-store vault.
struct DeviceVault {
    identity: DeviceIdentity,
    store: MemorySecureStore,
}

impl DeviceVault {
    fn new(identity: DeviceIdentity, device_key: &DeviceKey) -> Self {
        let mut store = MemorySecureStore::new();
        store
            .set_secret("device_key", &device_key.0)
            .expect("store device key");
        Self { identity, store }
    }

    fn install_wrapped(&mut self, generation: u64, wrapped: &[u8]) {
        self.store
            .set_secret(&format!("wrapped:{generation}"), wrapped)
            .expect("store wrapped key");
    }

    /// Rebuilds the in-memory `Device` (for decryption) from the vault.
    fn device(&self) -> Device {
        let device_key_bytes: [u8; 32] = self
            .store
            .get_secret("device_key")
            .expect("vault available")
            .expect("device key present")
            .try_into()
            .expect("32-byte device key");
        let mut device = Device::new(self.identity, DeviceKey::from_bytes(device_key_bytes));
        let mut generation = 1u64;
        loop {
            let name = format!("wrapped:{generation}");
            match self.store.get_secret(&name).expect("vault available") {
                Some(wrapped) => {
                    device.install(generation, wrapped);
                    generation += 1;
                }
                None => break,
            }
        }
        device
    }
}

fn identity(device_id: u64) -> DeviceIdentity {
    DeviceIdentity {
        device_id,
        public_key: [device_id as u8; 32],
    }
}

#[test]
fn pairing_revocation_and_rotation_end_to_end() {
    let backend = SyncBackend::new(ServiceConfig::default(), std::sync::Arc::new(SystemClock));
    let recovery = RecoveryCode::from_bytes([0x11; 32]);
    let mut manager = KeyManager::new(recovery, DataKey::from_bytes([0x22; 32]));

    // Primary device (device 1, the manager) and device 2 with its own key.
    let device2_key = DeviceKey::from_bytes([0x32; 32]);
    let mut vault2 = DeviceVault::new(identity(2), &device2_key);

    // Pair device 2 with a one-time code; it receives the gen-1 wrapped key.
    let code2 = manager.create_pairing_code();
    let wrapped2 = manager
        .pair_device(identity(2), &device2_key, &code2)
        .expect("pair device 2");
    vault2.install_wrapped(1, &wrapped2);

    // Device 1 pushes a gen-1 envelope to device 2 through the backend.
    let env1 = encrypt_envelope(
        &manager.current_data_key().0,
        identity(1),
        2,
        b"hello from device 1",
    );
    backend
        .put(1, "msg-1", env1.clone())
        .expect("backend accepts ciphertext");

    // Device 2 pulls it and decrypts with its own key + gen-1 wrapped key.
    let fetched1 = backend
        .get(2, "msg-1")
        .expect("authorized")
        .expect("present");
    let device2 = vault2.device();
    assert!(device2.can_read(1));
    assert_eq!(
        device2.decrypt_envelope(1, &fetched1),
        Some(b"hello from device 1".to_vec())
    );

    // Device 3 pairs too and reads a gen-1 envelope addressed to it.
    let device3_key = DeviceKey::from_bytes([0x33; 32]);
    let mut vault3 = DeviceVault::new(identity(3), &device3_key);
    let code3 = manager.create_pairing_code();
    let wrapped3 = manager
        .pair_device(identity(3), &device3_key, &code3)
        .expect("pair device 3");
    vault3.install_wrapped(1, &wrapped3);
    let env1_to_3 = encrypt_envelope(
        &manager.current_data_key().0,
        identity(1),
        3,
        b"early secret for device 3",
    );
    backend.put(1, "msg-1-to-3", env1_to_3.clone()).unwrap();
    let fetched1_to_3 = backend.get(3, "msg-1-to-3").unwrap().unwrap();
    assert_eq!(
        vault3.device().decrypt_envelope(1, &fetched1_to_3),
        Some(b"early secret for device 3".to_vec())
    );

    // Device 3 is lost: revoke it and rotate to generation 2.
    manager.revoke_device(3).expect("revoke device 3");
    let rotated = manager.rotate_keys().expect("rotate keys");
    assert_eq!(rotated.generation, 2);
    assert!(
        rotated.bundles.iter().all(|(device_id, _)| *device_id != 3),
        "revoked device must not receive the gen-2 key"
    );
    // Deliver gen-2 wrapped keys to active devices' vaults.
    for (device_id, wrapped) in &rotated.bundles {
        match device_id {
            2 => vault2.install_wrapped(2, wrapped),
            other => panic!("unexpected bundle for device {other}"),
        }
    }
    // The primary device uses the manager's gen-2 data key directly.

    // Device 1 pushes a gen-2 envelope addressed to device 3 (the revoked
    // device can fetch the ciphertext, but must not be able to decrypt it).
    let env3 = encrypt_envelope(
        &manager.current_data_key().0,
        identity(1),
        3,
        b"new secret after revocation",
    );
    backend
        .put(1, "msg-3", env3.clone())
        .expect("backend accepts ciphertext");
    let fetched3 = backend
        .get(3, "msg-3")
        .expect("authorized")
        .expect("present");
    let device3 = vault3.device();
    assert!(!device3.can_read(2), "revoked device lacks the gen-2 key");
    assert_eq!(
        device3.decrypt_envelope(2, &fetched3),
        None,
        "revoked device cannot read new data"
    );
    // The primary still reads it with the current data key.
    assert_eq!(
        decrypt_envelope(&manager.current_data_key().0, &env3),
        Some(b"new secret after revocation".to_vec())
    );

    // Active device 2 reads a gen-2 envelope addressed to it.
    let env2_to_2 = encrypt_envelope(
        &manager.current_data_key().0,
        identity(1),
        2,
        b"gen2 for device 2",
    );
    backend.put(1, "msg-2", env2_to_2.clone()).unwrap();
    let fetched2 = backend.get(2, "msg-2").unwrap().unwrap();
    assert_eq!(
        vault2.device().decrypt_envelope(2, &fetched2),
        Some(b"gen2 for device 2".to_vec())
    );

    // The backend never stored plaintext: every stored value is an envelope.
    let metas = backend.list(1).unwrap();
    assert_eq!(metas.len(), 4); // msg-1, msg-1-to-3, msg-3, msg-2
    assert!(metas.iter().all(|meta| meta.byte_len >= 33));
}
