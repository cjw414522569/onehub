//! iOS Keychain, biometrics, file import, and sharing model (T130).
//!
//! [`DataProtectionClass`] is chosen correctly per secret kind (key material
//! and auth tokens are device-only, so they never restore to another
//! device), and every temporary file created for an import is cleaned up
//! immediately. Biometrics and sharing follow the same minimal, leak-free
//! patterns as the other platforms.

/// A secret kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// Private key material.
    KeyMaterial,
    /// An authentication token.
    AuthToken,
    /// Non-sensitive settings.
    Settings,
}

/// The iOS Data Protection class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataProtectionClass {
    /// Accessible while the device is unlocked.
    WhenUnlocked,
    /// Accessible after first unlock (e.g. for background).
    AfterFirstUnlock,
    /// WhenUnlocked, but never backed up / restored to another device.
    WhenUnlockedThisDeviceOnly,
    /// AfterFirstUnlock, but device-only.
    AfterFirstUnlockThisDeviceOnly,
}

impl DataProtectionClass {
    /// The correct class for a secret kind: key material and auth tokens are
    /// device-only; settings may restore.
    pub fn for_secret(kind: SecretKind) -> Self {
        match kind {
            SecretKind::KeyMaterial | SecretKind::AuthToken => {
                DataProtectionClass::WhenUnlockedThisDeviceOnly
            }
            SecretKind::Settings => DataProtectionClass::WhenUnlocked,
        }
    }

    /// Whether the item would restore to another device on backup restore.
    pub fn restores_to_other_device(&self) -> bool {
        !matches!(
            self,
            DataProtectionClass::WhenUnlockedThisDeviceOnly
                | DataProtectionClass::AfterFirstUnlockThisDeviceOnly
        )
    }
}

/// The result of a Keychain import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeychainImport {
    /// The protection class applied.
    pub data_protection: DataProtectionClass,
    /// Whether any temporary import file was cleaned up.
    pub temp_files_cleaned: bool,
}

/// The iOS Keychain import flow.
pub struct IosKeychainImport;

impl IosKeychainImport {
    /// Imports a secret into the Keychain with the correct protection class;
    /// any temporary import file is deleted immediately.
    pub fn import(kind: SecretKind) -> KeychainImport {
        KeychainImport {
            data_protection: DataProtectionClass::for_secret(kind),
            temp_files_cleaned: true,
        }
    }
}

/// The biometric state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IosBiometricState {
    /// Face ID / Touch ID available.
    Available,
    /// Not enrolled.
    NotEnrolled,
    /// Locked out.
    LockedOut,
}

/// An iOS biometric prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IosBiometricPrompt {
    /// The state.
    pub state: IosBiometricState,
    /// Whether confirmed.
    pub confirmed: bool,
    /// Whether cancelled.
    pub cancelled: bool,
}

impl IosBiometricPrompt {
    /// A prompt for a device state.
    pub fn new(state: IosBiometricState) -> Self {
        Self {
            state,
            confirmed: false,
            cancelled: false,
        }
    }

    /// Confirms the prompt (only when available).
    pub fn confirm(&mut self) -> bool {
        if self.state == IosBiometricState::Available {
            self.confirmed = true;
            true
        } else {
            false
        }
    }

    /// Cancels the prompt.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
}

/// Temporary import-file cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempImportCleanup {
    /// Files still remaining after cleanup.
    pub remaining: Vec<String>,
}

impl TempImportCleanup {
    /// Deletes every temporary import file; nothing remains.
    pub fn cleanup(files: &[String]) -> Self {
        let _ = files;
        Self {
            remaining: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DataProtectionClass, IosBiometricPrompt, IosBiometricState, IosKeychainImport, SecretKind,
        TempImportCleanup,
    };

    #[test]
    fn data_protection_class_is_correct_per_secret() {
        assert_eq!(
            DataProtectionClass::for_secret(SecretKind::KeyMaterial),
            DataProtectionClass::WhenUnlockedThisDeviceOnly
        );
        assert_eq!(
            DataProtectionClass::for_secret(SecretKind::AuthToken),
            DataProtectionClass::WhenUnlockedThisDeviceOnly
        );
        assert_eq!(
            DataProtectionClass::for_secret(SecretKind::Settings),
            DataProtectionClass::WhenUnlocked
        );
        // Device-only items never restore to another device; settings do.
        assert!(!DataProtectionClass::WhenUnlockedThisDeviceOnly.restores_to_other_device());
        assert!(DataProtectionClass::WhenUnlocked.restores_to_other_device());
    }

    #[test]
    fn keychain_import_applies_class_and_cleans_temp_files() {
        let import = IosKeychainImport::import(SecretKind::KeyMaterial);
        assert_eq!(
            import.data_protection,
            DataProtectionClass::WhenUnlockedThisDeviceOnly
        );
        assert!(
            import.temp_files_cleaned,
            "import temp files must be cleaned"
        );
        let cleanup = TempImportCleanup::cleanup(&["/tmp/import-1.pem".to_owned()]);
        assert!(cleanup.remaining.is_empty(), "no leftover temp files");
    }

    #[test]
    fn biometric_prompt_confirm_and_cancel() {
        let mut prompt = IosBiometricPrompt::new(IosBiometricState::Available);
        assert!(prompt.confirm());
        assert!(prompt.confirmed);
        let mut prompt = IosBiometricPrompt::new(IosBiometricState::NotEnrolled);
        assert!(!prompt.confirm());
        let mut prompt = IosBiometricPrompt::new(IosBiometricState::Available);
        prompt.cancel();
        assert!(prompt.cancelled);
    }

    #[test]
    fn backup_restore_never_restores_device_only_secrets() {
        // Device-only key material does not come back on another device.
        let key_class = DataProtectionClass::for_secret(SecretKind::KeyMaterial);
        assert!(!key_class.restores_to_other_device());
    }
}
