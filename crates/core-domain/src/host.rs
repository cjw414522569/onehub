use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// Default TCP port used for SSH when a host does not specify one.
pub const DEFAULT_SSH_PORT: u16 = 22;

/// Stable identifier of a host entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HostId(String);

impl HostId {
    /// Builds a host id, rejecting empty or whitespace-only values.
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DomainError::EmptyId);
        }
        Ok(Self(value))
    }

    /// Returns the underlying id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Network location of a host: a hostname and a TCP port.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HostAddress {
    hostname: String,
    port: u16,
}

impl HostAddress {
    /// Builds an address with an explicit port, validating both fields.
    pub fn new(hostname: impl Into<String>, port: u16) -> Result<Self, DomainError> {
        let hostname = hostname.into();
        if hostname.trim().is_empty() {
            return Err(DomainError::EmptyHostname);
        }
        if port == 0 {
            return Err(DomainError::InvalidPort(port));
        }
        Ok(Self { hostname, port })
    }

    /// Builds an address using [`DEFAULT_SSH_PORT`].
    pub fn with_default_port(hostname: impl Into<String>) -> Result<Self, DomainError> {
        Self::new(hostname, DEFAULT_SSH_PORT)
    }

    /// Returns the hostname.
    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    /// Returns the TCP port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Formats the address as `hostname:port` for display.
    pub fn host_port_string(&self) -> String {
        format!("{}:{}", self.hostname, self.port)
    }
}

/// An immutable host entry.
///
/// Contains only pure domain data; no UI, database, or SSH implementation types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    id: HostId,
    address: HostAddress,
    display_name: Option<String>,
    tags: Vec<String>,
}

impl Host {
    /// Creates a host with no display name and no tags.
    pub fn new(id: HostId, address: HostAddress) -> Self {
        Self {
            id,
            address,
            display_name: None,
            tags: Vec::new(),
        }
    }

    /// Sets the optional display name.
    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }

    /// Appends a tag.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Returns the host id.
    pub fn id(&self) -> &HostId {
        &self.id
    }

    /// Returns the network address.
    pub fn address(&self) -> &HostAddress {
        &self.address
    }

    /// Returns the optional display name.
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    /// Returns the tags.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }
}

#[cfg(test)]
mod tests {
    use crate::error::DomainError;
    use crate::host::{Host, HostAddress, HostId, DEFAULT_SSH_PORT};

    #[test]
    fn host_id_rejects_empty_and_accepts_valid() {
        assert_eq!(HostId::new("   "), Err(DomainError::EmptyId));
        let id = HostId::new("production-01").expect("valid id");
        assert_eq!(id.as_str(), "production-01");
    }

    #[test]
    fn address_validates_hostname_and_port() {
        assert_eq!(HostAddress::new("", 22), Err(DomainError::EmptyHostname));
        assert_eq!(
            HostAddress::new("db.internal", 0),
            Err(DomainError::InvalidPort(0))
        );
        let address = HostAddress::new("db.internal", 2222).expect("valid address");
        assert_eq!(address.hostname(), "db.internal");
        assert_eq!(address.port(), 2222);
        assert_eq!(address.host_port_string(), "db.internal:2222");
        let defaulted =
            HostAddress::with_default_port("db.internal").expect("valid default address");
        assert_eq!(defaulted.port(), DEFAULT_SSH_PORT);
    }

    #[test]
    fn host_builder_is_immutable_and_composable() {
        let id = HostId::new("web-1").expect("valid id");
        let address = HostAddress::with_default_port("web.example.com").expect("valid address");
        let host = Host::new(id.clone(), address)
            .with_display_name("Web One")
            .with_tag("prod")
            .with_tag("tier1");
        assert_eq!(host.id(), &id);
        assert_eq!(host.display_name(), Some("Web One"));
        assert_eq!(host.tags(), &["prod".to_string(), "tier1".to_string()]);
    }

    #[test]
    fn host_defaults_are_absent() {
        let id = HostId::new("defaults").expect("valid id");
        let address =
            HostAddress::with_default_port("defaults.example.com").expect("valid address");
        let host = Host::new(id, address);
        assert_eq!(host.display_name(), None);
        assert!(host.tags().is_empty());
    }

    #[test]
    fn host_serde_round_trip_preserves_data() {
        let host = Host::new(
            HostId::new("round-trip").expect("valid id"),
            HostAddress::new("rt.example.com", 2200).expect("valid address"),
        )
        .with_display_name("Round Trip")
        .with_tag("test");
        let json = serde_json::to_string(&host).expect("serialize");
        let decoded: Host = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, host);
    }

    #[test]
    fn host_id_serde_transparent() {
        let id = HostId::new("transparent").expect("valid id");
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, "\"transparent\"");
        let decoded: HostId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, id);
    }
}
