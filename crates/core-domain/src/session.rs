use serde::{Deserialize, Serialize};

use crate::credential::CredentialRef;
use crate::error::DomainError;
use crate::host::HostId;

/// An immutable session profile binding a host, an optional username,
/// and a credential reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionProfile {
    id: String,
    name: String,
    host_id: HostId,
    credential_ref: CredentialRef,
    username: Option<String>,
}

impl SessionProfile {
    /// Creates a session profile, rejecting empty id or name.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        host_id: HostId,
        credential_ref: CredentialRef,
    ) -> Result<Self, DomainError> {
        let id = id.into();
        let name = name.into();
        if id.trim().is_empty() {
            return Err(DomainError::EmptyId);
        }
        if name.trim().is_empty() {
            return Err(DomainError::EmptyName);
        }
        Ok(Self {
            id,
            name,
            host_id,
            credential_ref,
            username: None,
        })
    }

    /// Sets the optional default username.
    pub fn with_username(mut self, username: impl Into<String>) -> Result<Self, DomainError> {
        let username = username.into();
        if username.trim().is_empty() {
            return Err(DomainError::EmptyUsername);
        }
        self.username = Some(username);
        Ok(self)
    }

    /// Returns the profile id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the profile name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the host id.
    pub fn host_id(&self) -> &HostId {
        &self.host_id
    }

    /// Returns the credential reference.
    pub fn credential_ref(&self) -> &CredentialRef {
        &self.credential_ref
    }

    /// Returns the optional default username.
    pub fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use crate::credential::{CredentialKind, CredentialRef};
    use crate::error::DomainError;
    use crate::host::HostId;
    use crate::session::SessionProfile;

    #[test]
    fn session_profile_rejects_empty_fields() {
        let host_id = HostId::new("host-1").expect("valid host id");
        let credential =
            CredentialRef::new("cred-1", CredentialKind::Password).expect("valid credential");
        assert_eq!(
            SessionProfile::new("", "prod", host_id.clone(), credential.clone()),
            Err(DomainError::EmptyId)
        );
        assert_eq!(
            SessionProfile::new("profile-1", "  ", host_id.clone(), credential.clone()),
            Err(DomainError::EmptyName)
        );
    }

    #[test]
    fn session_profile_builder_and_getters() {
        let host_id = HostId::new("host-2").expect("valid host id");
        let credential =
            CredentialRef::new("cred-2", CredentialKind::SshAgent).expect("valid credential");
        let profile =
            SessionProfile::new("profile-2", "staging", host_id.clone(), credential.clone())
                .expect("valid profile")
                .with_username("deploy")
                .expect("valid username");
        assert_eq!(profile.id(), "profile-2");
        assert_eq!(profile.name(), "staging");
        assert_eq!(profile.host_id(), &host_id);
        assert_eq!(profile.credential_ref(), &credential);
        assert_eq!(profile.username(), Some("deploy"));
    }

    #[test]
    fn session_profile_rejects_empty_username() {
        let host_id = HostId::new("host-3").expect("valid host id");
        let credential =
            CredentialRef::new("cred-3", CredentialKind::Password).expect("valid credential");
        let profile =
            SessionProfile::new("profile-3", "prod", host_id, credential).expect("valid profile");
        assert_eq!(profile.with_username("  "), Err(DomainError::EmptyUsername));
    }

    #[test]
    fn session_profile_serde_round_trip() {
        let profile = SessionProfile::new(
            "profile-4",
            "bastion",
            HostId::new("host-4").expect("valid host id"),
            CredentialRef::new("cred-4", CredentialKind::Certificate).expect("valid credential"),
        )
        .expect("valid profile")
        .with_username("ops")
        .expect("valid username");
        let json = serde_json::to_string(&profile).expect("serialize");
        let decoded: SessionProfile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, profile);
    }
}
