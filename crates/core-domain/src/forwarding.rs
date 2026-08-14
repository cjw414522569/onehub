use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// Kind of port forwarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ForwardingKind {
    /// Local forwarding: listen locally, forward to a remote target.
    Local,
    /// Remote forwarding: listen on the remote side, forward back to a local
    /// target.
    Remote,
    /// Dynamic SOCKS5 forwarding on the local side.
    Dynamic,
}

/// Address family binding semantics for a forwarding endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ForwardingFamily {
    /// Bind IPv4 only.
    Ipv4,
    /// Bind IPv6 only.
    Ipv6,
    /// Bind any available family.
    Any,
}

/// A network endpoint used by forwarding (host + port).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ForwardingEndpoint {
    /// Host name or IP literal; empty means the wildcard bind address.
    pub host: String,
    /// TCP port (1..=65535).
    pub port: u16,
}

impl ForwardingEndpoint {
    /// Creates an endpoint, rejecting an invalid port.
    pub fn new(host: impl Into<String>, port: u16) -> Result<Self, DomainError> {
        if port == 0 {
            return Err(DomainError::InvalidPort(port));
        }
        Ok(Self {
            host: host.into(),
            port,
        })
    }
}

/// A complete forwarding specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardingSpec {
    /// Forwarding kind.
    pub kind: ForwardingKind,
    /// Local listen endpoint (for local/dynamic) or remote listen endpoint
    /// (for remote).
    pub listen: ForwardingEndpoint,
    /// Target endpoint (absent for dynamic SOCKS).
    pub target: Option<ForwardingEndpoint>,
    /// Address family binding preference.
    pub family: ForwardingFamily,
}

impl ForwardingSpec {
    /// Creates a forwarding spec.
    pub fn new(
        kind: ForwardingKind,
        listen: ForwardingEndpoint,
        target: Option<ForwardingEndpoint>,
        family: ForwardingFamily,
    ) -> Result<Self, DomainError> {
        if kind == ForwardingKind::Dynamic && target.is_some() {
            return Err(DomainError::InvalidForwarding);
        }
        Ok(Self {
            kind,
            listen,
            target,
            family,
        })
    }

    /// The stable identity of the listen endpoint (host:port), used for
    /// conflict detection.
    pub fn listen_key(&self) -> String {
        format!("{}:{}", self.listen.host, self.listen.port)
    }
}

/// Result of adding a forwarding to the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardingAddResult {
    /// Added successfully.
    Added,
    /// The listen endpoint is already in use by another forwarding.
    Conflict { key: String },
}

/// A table of active forwardings with conflict detection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardingTable {
    entries: Vec<ForwardingSpec>,
}

impl ForwardingTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attempts to add a forwarding. Fails with a conflict if the listen
    /// endpoint is already bound.
    pub fn add(&mut self, spec: ForwardingSpec) -> ForwardingAddResult {
        let key = spec.listen_key();
        if self
            .entries
            .iter()
            .any(|existing| existing.listen_key() == key)
        {
            return ForwardingAddResult::Conflict { key };
        }
        self.entries.push(spec);
        ForwardingAddResult::Added
    }

    /// Removes a forwarding by listen key; returns whether it was present.
    pub fn remove(&mut self, listen_key: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|spec| spec.listen_key() != listen_key);
        self.entries.len() != before
    }

    /// Returns the active forwardings.
    pub fn entries(&self) -> &[ForwardingSpec] {
        &self.entries
    }

    /// Returns whether a listen endpoint is already bound.
    pub fn contains_listen(&self, listen_key: &str) -> bool {
        self.entries
            .iter()
            .any(|spec| spec.listen_key() == listen_key)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ForwardingAddResult, ForwardingEndpoint, ForwardingFamily, ForwardingKind, ForwardingSpec,
        ForwardingTable,
    };
    use crate::error::DomainError;

    fn endpoint(host: &str, port: u16) -> ForwardingEndpoint {
        ForwardingEndpoint::new(host, port).expect("valid endpoint")
    }

    fn local(host: &str, port: u16, target_host: &str, target_port: u16) -> ForwardingSpec {
        ForwardingSpec::new(
            ForwardingKind::Local,
            endpoint(host, port),
            Some(endpoint(target_host, target_port)),
            ForwardingFamily::Any,
        )
        .expect("valid spec")
    }

    #[test]
    fn endpoint_rejects_zero_port() {
        assert_eq!(
            ForwardingEndpoint::new("127.0.0.1", 0),
            Err(DomainError::InvalidPort(0))
        );
    }

    #[test]
    fn dynamic_forwarding_has_no_target() {
        let spec = ForwardingSpec::new(
            ForwardingKind::Dynamic,
            endpoint("127.0.0.1", 1080),
            None,
            ForwardingFamily::Any,
        )
        .expect("dynamic spec");
        assert_eq!(spec.kind, ForwardingKind::Dynamic);
        assert!(spec.target.is_none());
    }

    #[test]
    fn dynamic_forwarding_rejects_target() {
        let result = ForwardingSpec::new(
            ForwardingKind::Dynamic,
            endpoint("127.0.0.1", 1080),
            Some(endpoint("db.internal", 5432)),
            ForwardingFamily::Any,
        );
        assert!(result.is_err());
    }

    #[test]
    fn local_and_remote_forwarding_round_trip() {
        let local = local("127.0.0.1", 5432, "db.internal", 5432);
        let json = serde_json::to_string(&local).expect("serialize");
        let decoded: ForwardingSpec = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, local);

        let remote = ForwardingSpec::new(
            ForwardingKind::Remote,
            endpoint("0.0.0.0", 8080),
            Some(endpoint("127.0.0.1", 80)),
            ForwardingFamily::Ipv4,
        )
        .expect("remote spec");
        let json = serde_json::to_string(&remote).expect("serialize remote");
        let decoded: ForwardingSpec = serde_json::from_str(&json).expect("deserialize remote");
        assert_eq!(decoded, remote);
        assert_eq!(decoded.family, ForwardingFamily::Ipv4);
    }

    #[test]
    fn port_conflict_is_detected_on_same_listen_endpoint() {
        let mut table = ForwardingTable::new();
        assert_eq!(
            table.add(local("127.0.0.1", 5432, "a.internal", 5432)),
            ForwardingAddResult::Added
        );
        // Same host:port -> conflict.
        assert_eq!(
            table.add(local("127.0.0.1", 5432, "b.internal", 5432)),
            ForwardingAddResult::Conflict {
                key: "127.0.0.1:5432".to_owned()
            }
        );
        // Different port -> allowed.
        assert_eq!(
            table.add(local("127.0.0.1", 5433, "b.internal", 5432)),
            ForwardingAddResult::Added
        );
        assert_eq!(table.entries().len(), 2);
        assert!(table.contains_listen("127.0.0.1:5432"));
    }

    #[test]
    fn ipv4_ipv6_and_wildcard_bindings_are_distinct_endpoints() {
        let mut table = ForwardingTable::new();
        table.add(local("127.0.0.1", 8080, "web.internal", 80));
        table.add(local("::1", 8080, "web.internal", 80));
        table.add(local("0.0.0.0", 8080, "web.internal", 80));
        // All three listen endpoints differ, so no conflict.
        assert_eq!(table.entries().len(), 3);
    }

    #[test]
    fn remove_releases_the_listen_endpoint() {
        let mut table = ForwardingTable::new();
        table.add(local("127.0.0.1", 9999, "svc.internal", 9999));
        assert!(table.remove("127.0.0.1:9999"));
        assert!(!table.remove("127.0.0.1:9999"));
        assert!(!table.contains_listen("127.0.0.1:9999"));
        // The freed endpoint can be reused.
        assert_eq!(
            table.add(local("127.0.0.1", 9999, "other.internal", 9999)),
            ForwardingAddResult::Added
        );
    }

    #[test]
    fn serialization_round_trip_for_table() {
        let mut table = ForwardingTable::new();
        table.add(local("127.0.0.1", 5432, "db.internal", 5432));
        let json = serde_json::to_string(&table).expect("serialize table");
        let decoded: ForwardingTable = serde_json::from_str(&json).expect("deserialize table");
        assert_eq!(decoded, table);
        assert_eq!(decoded.entries().len(), 1);
    }
}
