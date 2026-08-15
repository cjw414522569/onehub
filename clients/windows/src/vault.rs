//! Secret vault (mxterm parity T002).
//!
//! Encrypts the secret-key material for credential secrets at rest. Two modes:
//! - master-password mode: an Argon2id KDF derives an AES-256-GCM key from the
//!   master password; the wrapped key blob is stored in a vault JSON file.
//! - local mode (no master password): a random key is protected with the OS
//!   DPAPI (CryptProtectData) so it unlocks automatically for the same user.
//!   `initialized` = the vault file exists; `unlocked` = the key is held in
//!   memory. Secrets are never stored in plaintext on disk.
//!

use aes_gcm::aead::{Aead, KeyInit, Nonce};
use aes_gcm::{Aes256Gcm, Key};

use argon2::Argon2;
use getrandom::getrandom;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Vault file name next to the store database.
const VAULT_FILE: &str = "ssh-client.vault.json";
/// KDF params: OWASP-recognized Argon2id defaults.
const ARGON2_M_COST: u32 = 19 * 1024;
const ARGON2_T_COST: u32 = 2;
const ARGON2_P_COST: u32 = 1;

/// The secret-vault status the UI expects (SecretVaultStatus).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultStatus {
    pub initialized: bool,
    pub unlocked: bool,
}

/// The in-memory vault handle.
#[derive(Debug)]
pub struct Vault {
    path: PathBuf,
    /// Whether the vault uses a master password (vs local DPAPI).
    master_password: bool,
    /// The active 32-byte key (None while locked).
    key: Option<[u8; 32]>,
}

fn b64(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn unb64(s: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

impl Vault {
    /// The vault file path for a given data directory.
    pub fn path_for(dir: &Path) -> PathBuf {
        dir.join(VAULT_FILE)
    }

    /// Loads vault metadata; key stays locked.
    pub fn open(dir: &Path) -> Self {
        Self {
            path: Self::path_for(dir),
            master_password: false,
            key: None,
        }
    }

    /// Whether the vault file exists.
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    fn read_blob(&self) -> Option<Value> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write_blob(&self, blob: &Value) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(blob).expect("vault json");
        std::fs::write(&self.path, json)
    }

    /// Current status: initialized + unlocked.
    pub fn status(&self) -> VaultStatus {
        VaultStatus {
            initialized: self.exists(),
            unlocked: self.key.is_some(),
        }
    }

    /// Enables a master password (creates or rekeys the vault).
    pub fn enable_master_password(&mut self, master_password: &str) -> Result<VaultStatus, String> {
        if master_password.trim().is_empty() {
            return Err("master password is empty".to_string());
        }
        let key = if let Some(k) = self.key {
            k
        } else {
            let mut k = [0u8; 32];
            getrandom(&mut k).map_err(|e| e.to_string())?;
            k
        };
        self.write_master_blob(master_password, &key)?;
        self.master_password = true;
        self.key = Some(key);
        Ok(self.status())
    }

    /// Disables the master password, switching to local DPAPI protection.
    pub fn disable_master_password(&mut self) -> Result<VaultStatus, String> {
        let key = self.require_unlocked()?;
        self.write_local_blob(&key)?;
        self.master_password = false;
        Ok(self.status())
    }

    /// Unlocks with a master password (verifies the password by decrypting
    /// the wrapped key blob).
    pub fn unlock(&mut self, master_password: &str) -> Result<VaultStatus, String> {
        if master_password.trim().is_empty() {
            return Err("master password is empty".to_string());
        }
        let derived = self.derive_master_key(master_password)?;
        let wrapped = self
            .read_blob()
            .ok_or_else(|| "vault not initialized".to_string())?;
        let cipher_b64 = wrapped
            .get("cipher")
            .and_then(Value::as_str)
            .ok_or_else(|| "vault missing cipher".to_string())?;
        let nonce_b64 = wrapped
            .get("nonce")
            .and_then(Value::as_str)
            .ok_or_else(|| "vault missing nonce".to_string())?;
        let cipher_bytes = unb64(cipher_b64).ok_or_else(|| "vault cipher invalid".to_string())?;
        let nonce = unb64(nonce_b64).ok_or_else(|| "vault nonce invalid".to_string())?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&derived));
        let decrypted = cipher
            .decrypt(
                aes_gcm::aead::Nonce::<Aes256Gcm>::from_slice(&nonce),
                cipher_bytes.as_slice(),
            )
            .map_err(|_| "invalid master password".to_string())?;
        let mut key = [0u8; 32];
        if decrypted.len() != 32 {
            return Err("vault key length mismatch".to_string());
        }
        key.copy_from_slice(&decrypted);
        self.key = Some(key);
        self.master_password = true;
        Ok(self.status())
    }

    /// Unlocks using the local DPAPI-protected key (no master password).
    pub fn unlock_local(&mut self) -> Result<VaultStatus, String> {
        if !self.exists() {
            // First run: create a local-protected vault.
            let mut key = [0u8; 32];
            getrandom(&mut key).map_err(|e| e.to_string())?;
            self.write_local_blob(&key)?;
            self.master_password = false;
            self.key = Some(key);
            return Ok(self.status());
        }
        let key = self.read_local_blob()?;
        self.master_password = false;
        self.key = Some(key);
        Ok(self.status())
    }

    /// Locks the vault (drops the in-memory key).
    pub fn lock(&mut self) -> VaultStatus {
        self.key = None;
        self.status()
    }

    /// The active key (locked -> error).
    pub fn key(&self) -> Result<[u8; 32], String> {
        self.key.ok_or_else(|| "vault is locked".to_string())
    }

    fn require_unlocked(&self) -> Result<[u8; 32], String> {
        self.key()
    }

    fn derive_master_key(&self, master_password: &str) -> Result<[u8; 32], String> {
        let blob = self
            .read_blob()
            .ok_or_else(|| "vault not initialized".to_string())?;
        let salt_b64 = blob
            .get("salt")
            .and_then(Value::as_str)
            .ok_or_else(|| "vault missing salt".to_string())?;
        let salt = unb64(salt_b64).ok_or_else(|| "vault salt invalid".to_string())?;
        let mut out = [0u8; 32];
        Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            argon2::Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32))
                .map_err(|e| e.to_string())?,
        )
        .hash_password_into(master_password.as_bytes(), &salt, &mut out)
        .map_err(|e| e.to_string())?;
        Ok(out)
    }

    /// Wraps `key` under a fresh Argon2id-derived key and writes the blob.
    fn write_master_blob(&mut self, master_password: &str, key: &[u8; 32]) -> Result<(), String> {
        let mut salt = [0u8; 16];
        getrandom(&mut salt).map_err(|e| e.to_string())?;
        let mut derived = [0u8; 32];
        Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            argon2::Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32))
                .map_err(|e| e.to_string())?,
        )
        .hash_password_into(master_password.as_bytes(), &salt, &mut derived)
        .map_err(|e| e.to_string())?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&derived));
        let mut nonce = [0u8; 12];
        getrandom(&mut nonce).map_err(|e| e.to_string())?;
        let encrypted = cipher
            .encrypt(Nonce::<Aes256Gcm>::from_slice(&nonce), key.as_slice())
            .map_err(|e| e.to_string())?;
        let blob = json!({
            "v": 1,
            "mode": "master",
            "kdf": "argon2id",
            "m_cost": ARGON2_M_COST,
            "t_cost": ARGON2_T_COST,
            "p_cost": ARGON2_P_COST,
            "salt": b64(&salt),
            "nonce": b64(&nonce),
            "cipher": b64(&encrypted),
        });
        self.write_blob(&blob).map_err(|e| e.to_string())
    }

    /// Wraps `key` under DPAPI and writes the blob.
    fn write_local_blob(&mut self, key: &[u8; 32]) -> Result<(), String> {
        let protected = dpapi_protect(key).map_err(|e| e.to_string())?;
        let blob = json!({
            "v": 1,
            "mode": "local",
            "protection": "dpapi",
            "cipher": b64(&protected),
        });
        self.write_blob(&blob).map_err(|e| e.to_string())
    }

    fn read_local_blob(&self) -> Result<[u8; 32], String> {
        let blob = self
            .read_blob()
            .ok_or_else(|| "vault not initialized".to_string())?;
        if blob.get("mode").and_then(Value::as_str) != Some("local") {
            return Err("vault is not in local mode".to_string());
        }
        let cipher_b64 = blob
            .get("cipher")
            .and_then(Value::as_str)
            .ok_or_else(|| "vault missing cipher".to_string())?;
        let protected = unb64(cipher_b64).ok_or_else(|| "vault cipher invalid".to_string())?;
        let key = dpapi_unprotect(&protected).map_err(|e| e.to_string())?;
        let mut out = [0u8; 32];
        if key.len() != 32 {
            return Err("vault key length mismatch".to_string());
        }
        out.copy_from_slice(&key);
        Ok(out)
    }
}

/// DPAPI protect (Windows). Uses a per-app entropy string to scope the blob.
fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>, String> {
    unsafe {
        use windows::Win32::Security::Cryptography::{
            CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        };
        let entropy = b"ssh-client-vault";
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let entropy_blob = CRYPT_INTEGER_BLOB {
            cbData: entropy.len() as u32,
            pbData: entropy.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptProtectData(
            &input,
            windows::core::w!("ssh-client vault key"),
            Some(&entropy_blob),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| format!("CryptProtectData failed: {e}"))?;
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            output.pbData as *mut core::ffi::c_void,
        )));
        Ok(result)
    }
}

/// DPAPI unprotect (Windows).
fn dpapi_unprotect(protected: &[u8]) -> Result<Vec<u8>, String> {
    unsafe {
        use windows::Win32::Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        };
        let entropy = b"ssh-client-vault";
        let input = CRYPT_INTEGER_BLOB {
            cbData: protected.len() as u32,
            pbData: protected.as_ptr() as *mut u8,
        };
        let entropy_blob = CRYPT_INTEGER_BLOB {
            cbData: entropy.len() as u32,
            pbData: entropy.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(
            &input,
            None,
            Some(&entropy_blob),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| format!("CryptUnprotectData failed: {e}"))?;
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            output.pbData as *mut core::ffi::c_void,
        )));
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault() -> (std::path::PathBuf, Vault) {
        let dir = std::env::temp_dir().join(format!(
            "ssh-vault-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&dir);
        let v = Vault::open(&dir);
        (dir, v)
    }

    #[test]
    fn master_password_enable_unlock_lock() {
        let (dir, mut v) = temp_vault();
        let s = v.enable_master_password("correct horse").expect("enable");
        assert!(s.initialized && s.unlocked);
        // wrong password must fail to unlock
        let mut v2 = Vault::open(&dir);
        assert!(v2.unlock("wrong").is_err());
        let s2 = v2.unlock("correct horse").expect("unlock");
        assert!(s2.initialized && s2.unlocked);
        let locked = v2.lock();
        assert!(!locked.unlocked);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_mode_roundtrip() {
        let (dir, mut v) = temp_vault();
        let s = v.unlock_local().expect("local unlock");
        assert!(s.initialized && s.unlocked);
        let mut v2 = Vault::open(&dir);
        let s2 = v2.unlock_local().expect("local unlock 2");
        assert!(s2.unlocked);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_password_rejected() {
        let (dir, mut v) = temp_vault();
        v.enable_master_password("pass").expect("enable");
        let mut v2 = Vault::open(&dir);
        assert!(v2.unlock("nope").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn vault_has_no_plaintext_key() {
        let (dir, mut v) = temp_vault();
        v.enable_master_password("pass").expect("enable");
        let text = std::fs::read_to_string(Vault::path_for(&dir)).expect("vault file");
        assert!(!text.contains("pass"));
        assert!(!text.contains("master_password"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
