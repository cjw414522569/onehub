use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// Kind of credential a [`CredentialRef`] refers to.
///
/// Secret material never lives in this crate; it stays in the secure store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CredentialKind {
    /// Interactive password.
    Password,
    /// Private key file or stored private key.
    PrivateKey,
    /// SSH agent (forwarded or local).
    SshAgent,
    /// Hardware-backed key (e.g. security key).
    HardwareKey,
    /// Certificate-based authentication.
    Certificate,
}

/// An immutable reference to a stored credential.
///
/// Contains no secret material, only an id, kind, and optional label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef {
    id: String,
    kind: CredentialKind,
    label: Option<String>,
}

impl CredentialRef {
    /// Creates a credential reference, rejecting an empty id.
    pub fn new(id: impl Into<String>, kind: CredentialKind) -> Result<Self, DomainError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(DomainError::EmptyId);
        }
        Ok(Self {
            id,
            kind,
            label: None,
        })
    }

    /// Sets the optional human-readable label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Returns the credential store id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the credential kind.
    pub fn kind(&self) -> CredentialKind {
        self.kind
    }

    /// Returns the optional label.
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use crate::credential::{CredentialKind, CredentialRef};
    use crate::error::DomainError;

    #[test]
    fn credential_ref_rejects_empty_id() {
        assert_eq!(
            CredentialRef::new("", CredentialKind::Password),
            Err(DomainError::EmptyId)
        );
    }

    #[test]
    fn credential_ref_builder_and_getters() {
        let reference = CredentialRef::new("cred-42", CredentialKind::PrivateKey)
            .expect("valid reference")
            .with_label("deploy key");
        assert_eq!(reference.id(), "cred-42");
        assert_eq!(reference.kind(), CredentialKind::PrivateKey);
        assert_eq!(reference.label(), Some("deploy key"));
    }

    #[test]
    fn credential_ref_serde_round_trip() {
        let reference = CredentialRef::new("cred-7", CredentialKind::HardwareKey)
            .expect("valid reference")
            .with_label("yubikey");
        let json = serde_json::to_string(&reference).expect("serialize");
        let decoded: CredentialRef = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, reference);
    }
}
