//! Credential provider and unlock interaction interface.
//!
//! Secret material is wrapped in zeroizing `secret` types and never enters
//! ordinary configuration.
//!
//! ```compile_fail
//! // CredentialValue must not be Debug-formattable (secrets cannot be logged).
//! let value = core_domain::CredentialValue::Password(
//!     secret::SecretString::from_string(String::from("hunter2")),
//! );
//! let _ = format!("{:?}", value);
//! ```

use secret::{SecretBytes, SecretString};

use crate::credential::{CredentialKind, CredentialRef};

/// A credential value returned by a provider.
///
/// Secret material is wrapped in zeroizing `secret` types (`SecretString` /
/// `SecretBytes`): it can never be formatted, cloned, serialized, or placed
/// into ordinary configuration. Agent and hardware keys are opaque handles
/// with no secret material. The enum deliberately does **not** implement
/// `Debug` or `Clone`.
pub enum CredentialValue {
    /// A password.
    Password(SecretString),
    /// A private key (PEM/OpenSSH bytes).
    PrivateKey(SecretBytes),
    /// A certificate (PEM/DER bytes).
    Certificate(SecretBytes),
    /// An SSH agent handle; no secret material is stored in the domain.
    Agent(AgentHandle),
    /// A hardware key handle; no secret material is stored in the domain.
    HardwareKey(HardwareKeyHandle),
}

impl CredentialValue {
    /// Returns the kind of this value.
    pub fn kind(&self) -> CredentialKind {
        match self {
            CredentialValue::Password(_) => CredentialKind::Password,
            CredentialValue::PrivateKey(_) => CredentialKind::PrivateKey,
            CredentialValue::Certificate(_) => CredentialKind::Certificate,
            CredentialValue::Agent(_) => CredentialKind::SshAgent,
            CredentialValue::HardwareKey(_) => CredentialKind::HardwareKey,
        }
    }

    /// Whether the value carries actual secret material (as opposed to a
    /// handle).
    pub fn is_secret_material(&self) -> bool {
        matches!(
            self,
            CredentialValue::Password(_)
                | CredentialValue::PrivateKey(_)
                | CredentialValue::Certificate(_)
        )
    }
}

/// Opaque SSH agent handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentHandle {
    /// Provider-local agent identifier.
    pub id: String,
}

/// Opaque hardware key handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareKeyHandle {
    /// Provider-local device identifier.
    pub device_id: String,
    /// Provider-local key identifier on the device.
    pub key_id: String,
}

/// How the provider asks the user to unlock credential material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnlockInteraction {
    /// No unlock interaction is required.
    None,
    /// The operating system shows its own prompt (keychain/credential
    /// manager).
    SystemPrompt,
    /// The app shows a localized prompt identified by a stable message key.
    InAppPrompt {
        /// Stable localization key, e.g. `credential.unlock.passphrase`.
        message_key: &'static str,
    },
    /// The provider uses platform biometrics.
    Biometric,
}

/// A provider-level error with no secret context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// The referenced credential does not exist.
    NotFound,
    /// The credential exists but is locked and requires an unlock
    /// interaction.
    Locked,
    /// The user or system rejected the unlock.
    Rejected,
    /// The provider does not support the requested kind.
    Unsupported,
    /// The provider failed without exposing secret context.
    Failed,
}

/// Contract for a credential provider (system keychain, secret store, agent,
/// hardware key). Implementations never place secret material into ordinary
/// configuration and never log it.
///
/// `async fn` in this trait is intentional: auto-trait bounds are deliberately
/// unspecified because the provider is a seam implemented by concrete
/// platform adapters (keychain, secure store, agent, hardware key).
#[allow(async_fn_in_trait)]
pub trait CredentialProvider {
    /// Retrieves a credential, performing an unlock interaction if needed.
    async fn retrieve(&self, reference: &CredentialRef) -> Result<CredentialValue, ProviderError>;

    /// Returns the unlock interaction the provider would use for `reference`.
    fn unlock_interaction(&self, reference: &CredentialRef) -> UnlockInteraction;

    /// Whether this provider supports the credential kind.
    fn supports(&self, kind: CredentialKind) -> bool;
}

#[cfg(test)]
mod tests {
    use super::{
        AgentHandle, CredentialValue, HardwareKeyHandle, ProviderError, UnlockInteraction,
    };
    use crate::credential::{CredentialKind, CredentialRef};
    use crate::credential_provider::CredentialProvider;
    use secret::{SecretBytes, SecretString};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Backing store of the simulated provider (like an OS keychain: it holds
    /// the secret at rest, outside the domain).
    enum Stored {
        Password(String),
        PrivateKey(Vec<u8>),
        Certificate(Vec<u8>),
        Agent(AgentHandle),
        HardwareKey(HardwareKeyHandle),
    }

    /// Simulated provider that re-reads its backing store and builds fresh
    /// zeroizing values on every retrieve (mirrors a real keychain adapter).
    struct MockCredentialProvider {
        store: Mutex<HashMap<String, Stored>>,
    }

    impl MockCredentialProvider {
        fn new() -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
            }
        }
    }

    fn next_id() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        format!("mock-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    fn reference(kind: CredentialKind) -> CredentialRef {
        CredentialRef::new(next_id(), kind).expect("valid reference")
    }

    impl CredentialProvider for MockCredentialProvider {
        async fn retrieve(
            &self,
            reference: &CredentialRef,
        ) -> Result<CredentialValue, ProviderError> {
            let store = self.store.lock().expect("mock store lock");
            match store.get(reference.id()) {
                Some(Stored::Password(value)) => Ok(CredentialValue::Password(SecretString::from(
                    value.as_str(),
                ))),
                Some(Stored::PrivateKey(bytes)) => Ok(CredentialValue::PrivateKey(
                    SecretBytes::from_vec(bytes.clone()),
                )),
                Some(Stored::Certificate(bytes)) => Ok(CredentialValue::Certificate(
                    SecretBytes::from_vec(bytes.clone()),
                )),
                Some(Stored::Agent(handle)) => Ok(CredentialValue::Agent(handle.clone())),
                Some(Stored::HardwareKey(handle)) => {
                    Ok(CredentialValue::HardwareKey(handle.clone()))
                }
                None => Err(ProviderError::NotFound),
            }
        }

        fn unlock_interaction(&self, reference: &CredentialRef) -> UnlockInteraction {
            match reference.kind() {
                CredentialKind::HardwareKey => UnlockInteraction::Biometric,
                CredentialKind::SshAgent => UnlockInteraction::None,
                _ => UnlockInteraction::SystemPrompt,
            }
        }

        fn supports(&self, kind: CredentialKind) -> bool {
            matches!(
                kind,
                CredentialKind::Password
                    | CredentialKind::PrivateKey
                    | CredentialKind::Certificate
                    | CredentialKind::SshAgent
                    | CredentialKind::HardwareKey
            )
        }
    }

    #[tokio::test]
    async fn password_round_trip_never_enters_plain_config() {
        let provider = MockCredentialProvider::new();
        let cred = reference(CredentialKind::Password);
        provider.store.lock().expect("lock").insert(
            cred.id().to_owned(),
            Stored::Password("hunter2-correct-horse".to_owned()),
        );
        let value = provider.retrieve(&cred).await.expect("retrieve password");
        match &value {
            CredentialValue::Password(retrieved) => {
                assert_eq!(retrieved.expose_secret(), "hunter2-correct-horse");
            }
            _ => panic!("expected password value"),
        }
        assert_eq!(value.kind(), CredentialKind::Password);
        assert!(value.is_secret_material());
    }

    #[tokio::test]
    async fn private_key_and_certificate_are_secret_bytes() {
        let provider = MockCredentialProvider::new();
        let key_cred = reference(CredentialKind::PrivateKey);
        let cert_cred = reference(CredentialKind::Certificate);
        provider.store.lock().expect("lock").insert(
            key_cred.id().to_owned(),
            Stored::PrivateKey(b"-----BEGIN OPENSSH PRIVATE KEY-----".to_vec()),
        );
        provider.store.lock().expect("lock").insert(
            cert_cred.id().to_owned(),
            Stored::Certificate(b"-----BEGIN CERTIFICATE-----".to_vec()),
        );
        match provider.retrieve(&key_cred).await.expect("key") {
            CredentialValue::PrivateKey(bytes) => {
                assert_eq!(
                    bytes.expose_secret(),
                    b"-----BEGIN OPENSSH PRIVATE KEY-----"
                );
            }
            _ => panic!("expected private key value"),
        }
        match provider.retrieve(&cert_cred).await.expect("cert") {
            CredentialValue::Certificate(bytes) => {
                assert_eq!(bytes.expose_secret(), b"-----BEGIN CERTIFICATE-----");
            }
            _ => panic!("expected certificate value"),
        }
    }

    #[tokio::test]
    async fn agent_and_hardware_key_are_opaque_handles_without_secrets() {
        let provider = MockCredentialProvider::new();
        let agent_cred = reference(CredentialKind::SshAgent);
        let hw_cred = reference(CredentialKind::HardwareKey);
        provider.store.lock().expect("lock").insert(
            agent_cred.id().to_owned(),
            Stored::Agent(AgentHandle {
                id: "agent-1".to_owned(),
            }),
        );
        provider.store.lock().expect("lock").insert(
            hw_cred.id().to_owned(),
            Stored::HardwareKey(HardwareKeyHandle {
                device_id: "yubikey-5".to_owned(),
                key_id: "key-42".to_owned(),
            }),
        );
        let agent = provider.retrieve(&agent_cred).await.expect("agent");
        assert!(!agent.is_secret_material());
        assert!(matches!(agent, CredentialValue::Agent(AgentHandle { .. })));
        let hw = provider.retrieve(&hw_cred).await.expect("hardware key");
        assert!(!hw.is_secret_material());
        assert!(matches!(
            hw,
            CredentialValue::HardwareKey(HardwareKeyHandle { .. })
        ));
    }

    #[tokio::test]
    async fn missing_credential_returns_not_found_without_secret_context() {
        let provider = MockCredentialProvider::new();
        let cred = reference(CredentialKind::Password);
        let error = match provider.retrieve(&cred).await {
            Err(error) => error,
            Ok(_) => panic!("expected not-found error"),
        };
        assert_eq!(error, ProviderError::NotFound);
    }

    #[tokio::test]
    async fn unlock_interaction_matches_credential_kind() {
        let provider = MockCredentialProvider::new();
        assert_eq!(
            provider.unlock_interaction(&reference(CredentialKind::HardwareKey)),
            UnlockInteraction::Biometric
        );
        assert_eq!(
            provider.unlock_interaction(&reference(CredentialKind::SshAgent)),
            UnlockInteraction::None
        );
        assert_eq!(
            provider.unlock_interaction(&reference(CredentialKind::Password)),
            UnlockInteraction::SystemPrompt
        );
        assert!(provider.supports(CredentialKind::PrivateKey));
        assert!(provider.supports(CredentialKind::HardwareKey));
    }

    #[test]
    fn handles_debug_do_not_contain_secret_material() {
        let agent = AgentHandle {
            id: "agent-1".to_owned(),
        };
        let debug = format!("{agent:?}");
        assert!(debug.contains("agent-1"));
        assert!(!debug.contains("BEGIN"));
    }
}
