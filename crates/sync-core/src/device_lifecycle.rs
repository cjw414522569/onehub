//! Device lifecycle: pairing, recovery codes, revocation, and key rotation
//! (T095).
//!
//! A [`KeyManager`] (the primary device) owns a chain of generation-tagged
//! data keys. Every authorized device holds its own [`DeviceKey`] (stored in
//! that device's secure store) plus wrapped copies of the data keys it is
//! allowed to use. Wrapping is AEAD (random nonce + Poly1305 tag), so only a
//! device holding the right key can unwrap a generation's data key.
//!
//! Security model (the "lost device" acceptance):
//! - Pairing uses a one-time [`PairingCode`]; a new device receives only the
//!   *current* generation's wrapped key, so it cannot read older data.
//! - [`KeyManager::revoke_device`] marks a lost device revoked; the next
//!   [`KeyManager::rotate_keys`] issues a fresh generation and wraps it only
//!   for non-revoked devices. The revoked device keeps old keys (it may still
//!   read data it already had) but never receives the new generation, so it
//!   cannot read new data.
//! - An offline device that misses a rotation similarly cannot read the new
//!   generation until it syncs the new wrapped key.
//! - [`RecoveryCode`] restores the current data key after a lost primary
//!   device ([`KeyManager::recover_data_key`]).

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

use crate::sync_protocol::{decrypt_envelope, DeviceIdentity, SyncEnvelope};

/// Length of a device key (32 bytes).
pub const DEVICE_KEY_LEN: usize = 32;
/// Length of a data key (32 bytes).
pub const DATA_KEY_LEN: usize = 32;
/// Length of a recovery code (32 bytes).
pub const RECOVERY_CODE_LEN: usize = 32;
/// Length of a pairing code (16 bytes).
pub const PAIRING_CODE_LEN: usize = 16;

/// AEAD associated data for device-key wrapping.
const WRAP_AAD: &[u8] = b"sync-core:device-key-wrap:v1";
/// Wrapped layout: 12-byte nonce || AEAD ciphertext (32-byte key + 16-byte tag).
const WRAP_NONCE_LEN: usize = 12;
const WRAP_TAG_LEN: usize = 16;

/// A per-device symmetric key (secret; lives in the device's secure store).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceKey(pub [u8; 32]);

/// A generation-tagged data-encryption key (secret; held by authorized
/// devices, wrapped per device).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataKey(pub [u8; 32]);

/// A one-time pairing code that authorizes a new device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairingCode([u8; PAIRING_CODE_LEN]);

/// A recovery code: restores the current data key after a lost primary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryCode([u8; RECOVERY_CODE_LEN]);

impl DeviceKey {
    /// A fresh random device key.
    pub fn generate() -> Self {
        Self(random_bytes::<DEVICE_KEY_LEN>())
    }

    /// A key from explicit bytes (for deterministic tests / import).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl DataKey {
    /// A fresh random data key.
    pub fn generate() -> Self {
        Self(random_bytes::<DATA_KEY_LEN>())
    }

    /// A key from explicit bytes (for deterministic tests / import).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl PairingCode {
    /// A fresh random pairing code.
    pub fn generate() -> Self {
        Self(random_bytes::<PAIRING_CODE_LEN>())
    }

    /// A code from explicit bytes.
    pub fn from_bytes(bytes: [u8; PAIRING_CODE_LEN]) -> Self {
        Self(bytes)
    }
}

impl RecoveryCode {
    /// A fresh random recovery code (shown once at setup).
    pub fn generate() -> Self {
        Self(random_bytes::<RECOVERY_CODE_LEN>())
    }

    /// A code from explicit bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw code bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for PairingCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", format_hex_groups(&self.0))
    }
}

impl fmt::Display for RecoveryCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", format_hex_groups(&self.0))
    }
}

impl FromStr for PairingCode {
    type Err = LifecycleError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let bytes = parse_hex(source).ok_or(LifecycleError::MalformedCode)?;
        let array: [u8; PAIRING_CODE_LEN] = bytes
            .try_into()
            .map_err(|_| LifecycleError::MalformedCode)?;
        Ok(Self(array))
    }
}

impl FromStr for RecoveryCode {
    type Err = LifecycleError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let bytes = parse_hex(source).ok_or(LifecycleError::MalformedCode)?;
        let array: [u8; RECOVERY_CODE_LEN] = bytes
            .try_into()
            .map_err(|_| LifecycleError::MalformedCode)?;
        Ok(Self(array))
    }
}

/// Why a device-lifecycle operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleError {
    /// The device id is not registered.
    UnknownDevice,
    /// The device is already registered (no duplicate pairings).
    AlreadyRegistered,
    /// The device is revoked and cannot be re-paired.
    Revoked,
    /// The pairing code is missing, already used, or wrong.
    InvalidPairingCode,
    /// The recovery code is wrong.
    InvalidRecoveryCode,
    /// A code string could not be parsed.
    MalformedCode,
}

/// A registered device's lifecycle record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecord {
    /// The device identity.
    pub device: DeviceIdentity,
    /// Whether the device has been revoked (lost).
    pub revoked: bool,
}

/// The outcome of a key rotation: the new generation plus the wrapped keys
/// that must be delivered to each non-revoked device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotatedKeys {
    /// The new generation number.
    pub generation: u64,
    /// `(device_id, wrapped_data_key)` for every non-revoked device.
    pub bundles: Vec<(u64, Vec<u8>)>,
    /// The new data key wrapped under the recovery key.
    pub recovery_wrapped: Vec<u8>,
}

/// Wraps a data key for a device (random nonce + Poly1305 tag).
pub fn wrap_data_key(device_key: &DeviceKey, data_key: &DataKey) -> Vec<u8> {
    let nonce = random_bytes::<WRAP_NONCE_LEN>();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&device_key.0));
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &data_key.0,
                aad: WRAP_AAD,
            },
        )
        .expect("wrap data key");
    let mut wrapped = Vec::with_capacity(WRAP_NONCE_LEN + ciphertext.len());
    wrapped.extend_from_slice(&nonce);
    wrapped.extend_from_slice(&ciphertext);
    wrapped
}

/// Unwraps a data key; `None` on a wrong key or any tampering.
pub fn unwrap_data_key(device_key: &DeviceKey, wrapped: &[u8]) -> Option<DataKey> {
    if wrapped.len() < WRAP_NONCE_LEN + DATA_KEY_LEN + WRAP_TAG_LEN {
        return None;
    }
    let nonce = &wrapped[..WRAP_NONCE_LEN];
    let ciphertext = &wrapped[WRAP_NONCE_LEN..];
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&device_key.0));
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: WRAP_AAD,
            },
        )
        .ok()?;
    let bytes: [u8; 32] = plaintext.try_into().ok()?;
    Some(DataKey(bytes))
}

/// The primary device's key lifecycle manager.
#[derive(Debug, Clone)]
pub struct KeyManager {
    generation: u64,
    data_keys: HashMap<u64, DataKey>,
    recovery_wrapped: HashMap<u64, Vec<u8>>,
    devices: HashMap<u64, DeviceRecord>,
    device_keys: HashMap<u64, DeviceKey>,
    wrapped: HashMap<u64, HashMap<u64, Vec<u8>>>,
    recovery: RecoveryCode,
    pairing_codes: Vec<PairingCode>,
}

impl KeyManager {
    /// A fresh manager at generation 1 with the given recovery code and
    /// initial data key.
    pub fn new(recovery: RecoveryCode, initial_data_key: DataKey) -> Self {
        let recovery_key = DeviceKey(recovery.0);
        let mut recovery_wrapped = HashMap::new();
        recovery_wrapped.insert(1, wrap_data_key(&recovery_key, &initial_data_key));
        let mut data_keys = HashMap::new();
        data_keys.insert(1, initial_data_key);
        Self {
            generation: 1,
            data_keys,
            recovery_wrapped,
            devices: HashMap::new(),
            device_keys: HashMap::new(),
            wrapped: HashMap::new(),
            recovery,
            pairing_codes: Vec::new(),
        }
    }

    /// The current generation number.
    pub fn current_generation(&self) -> u64 {
        self.generation
    }

    /// The current generation's data key (for encrypting new data).
    pub fn current_data_key(&self) -> DataKey {
        self.data_keys[&self.generation]
    }

    /// The data key for an explicit generation (the primary holds all).
    pub fn data_key_for(&self, generation: u64) -> Option<DataKey> {
        self.data_keys.get(&generation).copied()
    }

    /// The wrapped data key for a device and generation, if issued.
    pub fn wrapped_key_for(&self, device: u64, generation: u64) -> Option<&[u8]> {
        self.wrapped
            .get(&device)
            .and_then(|generations| generations.get(&generation))
            .map(Vec::as_slice)
    }

    /// Whether a device holds (or was issued) a wrapped key for a generation.
    pub fn has_wrapped_key(&self, device: u64, generation: u64) -> bool {
        self.wrapped_key_for(device, generation).is_some()
    }

    /// Whether a device is revoked.
    pub fn is_revoked(&self, device: u64) -> bool {
        self.devices
            .get(&device)
            .map(|record| record.revoked)
            .unwrap_or(false)
    }

    /// All registered device records.
    pub fn devices(&self) -> Vec<DeviceRecord> {
        self.devices.values().cloned().collect()
    }

    /// Issues a fresh one-time pairing code.
    pub fn create_pairing_code(&mut self) -> PairingCode {
        let code = PairingCode::generate();
        self.pairing_codes.push(code);
        code
    }

    /// Pairs a new device with a one-time code. Returns the wrapped *current*
    /// data key to deliver to the device's vault. A new device only receives
    /// the current generation, never older data keys.
    pub fn pair_device(
        &mut self,
        device: DeviceIdentity,
        device_key: &DeviceKey,
        code: &PairingCode,
    ) -> Result<Vec<u8>, LifecycleError> {
        let index = self
            .pairing_codes
            .iter()
            .position(|candidate| candidate == code)
            .ok_or(LifecycleError::InvalidPairingCode)?;
        if self.is_revoked(device.device_id) {
            return Err(LifecycleError::Revoked);
        }
        if self.devices.contains_key(&device.device_id) {
            return Err(LifecycleError::AlreadyRegistered);
        }
        self.pairing_codes.remove(index);
        self.devices.insert(
            device.device_id,
            DeviceRecord {
                device,
                revoked: false,
            },
        );
        self.device_keys.insert(device.device_id, *device_key);
        let wrapped = wrap_data_key(device_key, &self.current_data_key());
        self.wrapped
            .entry(device.device_id)
            .or_default()
            .insert(self.generation, wrapped.clone());
        Ok(wrapped)
    }

    /// Revokes a lost device (idempotent). The next rotation excludes it.
    pub fn revoke_device(&mut self, device: u64) -> Result<(), LifecycleError> {
        let record = self
            .devices
            .get_mut(&device)
            .ok_or(LifecycleError::UnknownDevice)?;
        record.revoked = true;
        Ok(())
    }

    /// Rotates to a fresh generation: every non-revoked device gets a wrapped
    /// copy of the new data key; the recovery-wrapped copy is also refreshed.
    pub fn rotate_keys(&mut self) -> Result<RotatedKeys, LifecycleError> {
        self.generation += 1;
        let generation = self.generation;
        let new_key = DataKey::generate();
        self.data_keys.insert(generation, new_key);
        let mut bundles = Vec::new();
        for (device_id, record) in &self.devices {
            if record.revoked {
                continue;
            }
            let device_key = self
                .device_keys
                .get(device_id)
                .copied()
                .ok_or(LifecycleError::UnknownDevice)?;
            let wrapped = wrap_data_key(&device_key, &new_key);
            self.wrapped
                .entry(*device_id)
                .or_default()
                .insert(generation, wrapped.clone());
            bundles.push((*device_id, wrapped));
        }
        let recovery_wrapped = wrap_data_key(&DeviceKey(self.recovery.0), &new_key);
        self.recovery_wrapped
            .insert(generation, recovery_wrapped.clone());
        Ok(RotatedKeys {
            generation,
            bundles,
            recovery_wrapped,
        })
    }

    /// Restores the current data key from the recovery code.
    pub fn recover_data_key(&self, code: &RecoveryCode) -> Option<DataKey> {
        if code != &self.recovery {
            return None;
        }
        let recovery_key = DeviceKey(self.recovery.0);
        unwrap_data_key(&recovery_key, self.recovery_wrapped.get(&self.generation)?)
    }
}

/// One device's key vault: its own device key plus the wrapped data keys it
/// has received. In production the wrapped keys live in the device's secure
/// store; here they are held in memory.
#[derive(Debug, Clone)]
pub struct Device {
    /// The device identity.
    pub identity: DeviceIdentity,
    device_key: DeviceKey,
    wrapped: HashMap<u64, Vec<u8>>,
}

impl Device {
    /// A device with its own key.
    pub fn new(identity: DeviceIdentity, device_key: DeviceKey) -> Self {
        Self {
            identity,
            device_key,
            wrapped: HashMap::new(),
        }
    }

    /// Installs a wrapped data key for a generation (delivered by the
    /// manager). This is the "sync" step that decides what the device can
    /// read: a device that never installs generation G cannot read G data.
    pub fn install(&mut self, generation: u64, wrapped: Vec<u8>) {
        self.wrapped.insert(generation, wrapped);
    }

    /// Whether the device holds a wrapped key for a generation.
    pub fn can_read(&self, generation: u64) -> bool {
        self.wrapped.contains_key(&generation)
    }

    /// The number of wrapped generations installed.
    pub fn wrapped_len(&self) -> usize {
        self.wrapped.len()
    }

    /// Decrypts an envelope if the device holds the wrapped key for the
    /// given generation; `None` otherwise (wrong generation, revoked, or
    /// offline).
    pub fn decrypt_envelope(&self, generation: u64, envelope: &SyncEnvelope) -> Option<Vec<u8>> {
        let data_key = unwrap_data_key(&self.device_key, self.wrapped.get(&generation)?)?;
        decrypt_envelope(&data_key.0, envelope)
    }
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::getrandom(&mut bytes).expect("operating system randomness");
    bytes
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn format_hex_groups(bytes: &[u8]) -> String {
    let hex = to_hex(bytes);
    hex.as_bytes()
        .chunks(4)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

fn parse_hex(source: &str) -> Option<Vec<u8>> {
    let cleaned: String = source
        .chars()
        .filter(|character| *character != '-')
        .collect();
    if !cleaned.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(cleaned.len() / 2);
    for index in (0..cleaned.len()).step_by(2) {
        bytes.push(u8::from_str_radix(&cleaned[index..index + 2], 16).ok()?);
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{
        unwrap_data_key, wrap_data_key, DataKey, Device, DeviceIdentity, DeviceKey, KeyManager,
        LifecycleError, PairingCode, RecoveryCode,
    };
    use crate::sync_protocol::{decrypt_envelope, encrypt_envelope};

    fn manager() -> (KeyManager, RecoveryCode) {
        let recovery = RecoveryCode::from_bytes([0x11; 32]);
        let initial = DataKey::from_bytes([0x22; 32]);
        let manager = KeyManager::new(recovery, initial);
        (manager, recovery)
    }

    fn identity(device_id: u64) -> DeviceIdentity {
        DeviceIdentity {
            device_id,
            public_key: [device_id as u8; 32],
        }
    }

    #[test]
    fn recovery_and_pairing_codes_format_round_trip() {
        let recovery = RecoveryCode::from_bytes([0xAB; 32]);
        let text = recovery.to_string();
        assert!(text.contains('-'));
        assert_eq!(RecoveryCode::from_str(&text).unwrap(), recovery);
        assert_eq!(
            RecoveryCode::from_str(&text.to_lowercase()).unwrap(),
            recovery
        );
        assert_eq!(
            RecoveryCode::from_str("not-hex").unwrap_err(),
            LifecycleError::MalformedCode
        );
        let pairing = PairingCode::from_bytes([0xCD; 16]);
        let pairing_text = pairing.to_string();
        assert_eq!(PairingCode::from_str(&pairing_text).unwrap(), pairing);
    }

    #[test]
    fn wrap_unwrap_round_trip_and_tamper_rejection() {
        let device_key = DeviceKey::from_bytes([7; 32]);
        let data_key = DataKey::from_bytes([9; 32]);
        let wrapped = wrap_data_key(&device_key, &data_key);
        assert_eq!(wrapped.len(), 12 + 32 + 16);
        assert_eq!(unwrap_data_key(&device_key, &wrapped), Some(data_key));
        // Wrong key fails.
        assert_eq!(
            unwrap_data_key(&DeviceKey::from_bytes([8; 32]), &wrapped),
            None
        );
        // Tampering fails.
        let mut tampered = wrapped.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert_eq!(unwrap_data_key(&device_key, &tampered), None);
    }

    #[test]
    fn pairing_code_is_one_time_and_authorizes_new_device() {
        let (mut manager, _recovery) = manager();
        let code = manager.create_pairing_code();
        let device_key = DeviceKey::from_bytes([3; 32]);
        let wrapped = manager
            .pair_device(identity(2), &device_key, &code)
            .unwrap();
        assert!(manager.has_wrapped_key(2, 1));
        assert!(!manager.is_revoked(2));
        // The same code cannot be reused.
        assert_eq!(
            manager.pair_device(identity(2), &device_key, &code),
            Err(LifecycleError::InvalidPairingCode)
        );
        // A wrong code is refused.
        let wrong = PairingCode::from_bytes([0; 16]);
        assert_eq!(
            manager.pair_device(identity(3), &DeviceKey::from_bytes([4; 32]), &wrong),
            Err(LifecycleError::InvalidPairingCode)
        );
        // The delivered wrapped key decrypts current-generation envelopes.
        let mut device = Device::new(identity(2), device_key);
        device.install(1, wrapped);
        assert!(device.can_read(1));
        let envelope = encrypt_envelope(&manager.current_data_key().0, identity(1), 2, b"hello");
        assert_eq!(
            device.decrypt_envelope(1, &envelope),
            Some(b"hello".to_vec())
        );
        // A device that never installs the key cannot read.
        let stranger = Device::new(identity(9), DeviceKey::from_bytes([9; 32]));
        assert!(!stranger.can_read(1));
        assert_eq!(stranger.decrypt_envelope(1, &envelope), None);
    }

    #[test]
    fn revoked_device_cannot_read_post_rotation_data() {
        let (mut manager, _recovery) = manager();
        let mut b = Device::new(identity(2), DeviceKey::from_bytes([3; 32]));
        let mut c = Device::new(identity(3), DeviceKey::from_bytes([4; 32]));
        let gen1_key = manager.current_data_key();
        // Pair B and C at generation 1.
        let code_b = manager.create_pairing_code();
        let wrapped_b = manager
            .pair_device(identity(2), &DeviceKey::from_bytes([3; 32]), &code_b)
            .unwrap();
        let code_c = manager.create_pairing_code();
        let wrapped_c = manager
            .pair_device(identity(3), &DeviceKey::from_bytes([4; 32]), &code_c)
            .unwrap();
        b.install(1, wrapped_b);
        c.install(1, wrapped_c);
        let env1 = encrypt_envelope(&gen1_key.0, identity(1), 3, b"gen1 secret");
        assert_eq!(b.decrypt_envelope(1, &env1), Some(b"gen1 secret".to_vec()));
        assert_eq!(c.decrypt_envelope(1, &env1), Some(b"gen1 secret".to_vec()));
        // Rotate to generation 2: both devices receive the key.
        let rotated = manager.rotate_keys().unwrap();
        assert_eq!(rotated.generation, 2);
        for (device_id, wrapped) in &rotated.bundles {
            match device_id {
                2 => b.install(2, wrapped.clone()),
                3 => c.install(2, wrapped.clone()),
                other => panic!("unexpected device {other}"),
            }
        }
        // Device C is lost: revoke it, then rotate to generation 3.
        manager.revoke_device(3).unwrap();
        assert!(manager.is_revoked(3));
        let rotated3 = manager.rotate_keys().unwrap();
        assert_eq!(rotated3.generation, 3);
        assert!(
            rotated3
                .bundles
                .iter()
                .all(|(device_id, _)| *device_id != 3),
            "revoked device must not receive the new generation key"
        );
        for (device_id, wrapped) in &rotated3.bundles {
            if *device_id == 2 {
                b.install(3, wrapped.clone());
            }
        }
        let env3 = encrypt_envelope(
            &manager.current_data_key().0,
            identity(1),
            2,
            b"gen3 secret",
        );
        // Active device B reads the new data.
        assert_eq!(b.decrypt_envelope(3, &env3), Some(b"gen3 secret".to_vec()));
        // Revoked device C cannot read the new data.
        assert!(!c.can_read(3));
        assert_eq!(c.decrypt_envelope(3, &env3), None);
        // C may still read the old (pre-revocation) data it already held.
        assert_eq!(c.decrypt_envelope(1, &env1), Some(b"gen1 secret".to_vec()));
    }

    #[test]
    fn offline_old_device_cannot_read_new_data_until_it_syncs() {
        let (mut manager, _recovery) = manager();
        let device_key = DeviceKey::from_bytes([3; 32]);
        let mut b = Device::new(identity(2), device_key);
        let code = manager.create_pairing_code();
        let wrapped = manager
            .pair_device(identity(2), &device_key, &code)
            .unwrap();
        b.install(1, wrapped);
        // B goes offline; the manager rotates to generation 2 and wraps the
        // new key for B, but the bundle is never delivered.
        manager.rotate_keys().unwrap();
        let env2 = encrypt_envelope(&manager.current_data_key().0, identity(1), 2, b"gen2 for B");
        assert!(!b.can_read(2));
        assert_eq!(b.decrypt_envelope(2, &env2), None);
        // B comes back online and syncs the new wrapped key.
        let synced = manager.wrapped_key_for(2, 2).unwrap().to_vec();
        b.install(2, synced);
        assert_eq!(b.decrypt_envelope(2, &env2), Some(b"gen2 for B".to_vec()));
    }

    #[test]
    fn recovery_code_restores_current_data_key_after_loss() {
        let recovery = RecoveryCode::from_bytes([0x55; 32]);
        let initial = DataKey::from_bytes([0x66; 32]);
        let mut manager = KeyManager::new(recovery, initial);
        manager.rotate_keys().unwrap();
        // The recovery code restores the current (post-rotation) data key.
        let recovered = manager.recover_data_key(&recovery).unwrap();
        assert_eq!(recovered, manager.current_data_key());
        // Wrong code fails.
        let wrong = RecoveryCode::from_bytes([0x00; 32]);
        assert_eq!(manager.recover_data_key(&wrong), None);
        // The recovered key actually decrypts a current-generation envelope.
        let envelope =
            encrypt_envelope(&manager.current_data_key().0, identity(1), 2, b"recover me");
        assert_eq!(
            decrypt_envelope(&recovered.0, &envelope),
            Some(b"recover me".to_vec())
        );
    }

    #[test]
    fn pairing_after_revocation_is_refused() {
        let (mut manager, _recovery) = manager();
        let code = manager.create_pairing_code();
        let device_key = DeviceKey::from_bytes([3; 32]);
        manager
            .pair_device(identity(2), &device_key, &code)
            .unwrap();
        manager.revoke_device(2).unwrap();
        // A revoked device cannot be re-paired with a fresh code.
        let fresh = manager.create_pairing_code();
        assert_eq!(
            manager.pair_device(identity(2), &device_key, &fresh),
            Err(LifecycleError::Revoked)
        );
        // Unknown devices cannot be revoked.
        assert_eq!(
            manager.revoke_device(99),
            Err(LifecycleError::UnknownDevice)
        );
    }
}
