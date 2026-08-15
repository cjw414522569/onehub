//! Port forwarding management and port-occupancy diagnosis (T113).
//!
//! [`ForwardManager`] creates local / remote / dynamic forwards, validates
//! the listen port against a known-occupied set (so a create on an occupied
//! port fails with an actionable message), and drives pause / resume /
//! reconnect / remove. Every forward has a copy-ready address label and a
//! [`risk warning`] (e.g. listening on all interfaces, privileged ports).

use std::collections::{BTreeSet, HashMap};

/// The forward kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardKind {
    /// Local forward (listen locally, target remote).
    Local,
    /// Remote forward (listen remote, target local).
    Remote,
    /// Dynamic SOCKS forward.
    Dynamic,
}

/// The forward state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardState {
    /// Running.
    Active,
    /// Paused by the user.
    Paused,
    /// Reconnecting after a drop.
    Reconnecting,
    /// A connection error occurred.
    Error,
}

/// A risk level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ForwardRisk {
    /// Low.
    Low,
    /// Medium.
    Medium,
    /// High.
    High,
}

/// A risk warning shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardRiskWarning {
    /// The risk level.
    pub level: ForwardRisk,
    /// The warning message.
    pub message: String,
}

/// A port forward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortForward {
    /// Stable id.
    pub id: u64,
    /// The kind.
    pub kind: ForwardKind,
    /// Listen host (e.g. "127.0.0.1" or "0.0.0.0").
    pub listen_host: String,
    /// Listen port.
    pub listen_port: u16,
    /// Target host ("" for dynamic).
    pub target_host: String,
    /// Target port (0 for dynamic).
    pub target_port: u16,
    /// The state.
    pub state: ForwardState,
}

impl PortForward {
    /// The copy-ready address label, e.g. "127.0.0.1:2222 -> 10.0.0.5:22".
    pub fn address_label(&self) -> String {
        let listen = format!("{}:{}", self.listen_host, self.listen_port);
        match self.kind {
            ForwardKind::Dynamic => format!("{listen} (dynamic)"),
            _ => format!("{listen} -> {}:{}", self.target_host, self.target_port),
        }
    }

    /// Risk warnings: listening on all interfaces, privileged ports.
    pub fn risk_warning(&self) -> Vec<ForwardRiskWarning> {
        let mut warnings = Vec::new();
        if self.listen_host == "0.0.0.0" || self.listen_host.is_empty() {
            warnings.push(ForwardRiskWarning {
                level: ForwardRisk::High,
                message: "Listening on all interfaces exposes the port to the network.".to_owned(),
            });
        }
        if self.listen_port < 1024 {
            warnings.push(ForwardRiskWarning {
                level: ForwardRisk::Medium,
                message: "Ports below 1024 require elevated privileges.".to_owned(),
            });
        }
        if warnings.is_empty() {
            warnings.push(ForwardRiskWarning {
                level: ForwardRisk::Low,
                message: "The forward listens on localhost only.".to_owned(),
            });
        }
        warnings
    }
}

/// Why a forward operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardError {
    /// The forward id is unknown.
    NotFound,
    /// The listen port is already occupied.
    Occupied(u16),
    /// The listen port is invalid (0 or reserved).
    InvalidPort,
}

/// The occupancy status of a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OccupancyStatus {
    /// The port is free.
    Free,
    /// The port is occupied.
    Occupied { port: u16 },
}

/// The forward manager.
#[derive(Debug, Clone, Default)]
pub struct ForwardManager {
    forwards: HashMap<u64, PortForward>,
    order: Vec<u64>,
    next_id: u64,
    occupied: BTreeSet<u16>,
}

impl ForwardManager {
    /// An empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records which ports are occupied on the host (diagnostics input).
    pub fn set_occupied_ports(&mut self, ports: &[u16]) {
        self.occupied = ports.iter().copied().collect();
    }

    /// Diagnoses whether a port is free or occupied.
    pub fn diagnose(&self, listen_port: u16) -> OccupancyStatus {
        if self.occupied.contains(&listen_port) {
            OccupancyStatus::Occupied { port: listen_port }
        } else {
            OccupancyStatus::Free
        }
    }

    /// Creates a forward; fails with an actionable message on an occupied or
    /// invalid listen port.
    pub fn create(
        &mut self,
        kind: ForwardKind,
        listen_host: &str,
        listen_port: u16,
        target_host: &str,
        target_port: u16,
    ) -> Result<u64, ForwardError> {
        if listen_port == 0 {
            return Err(ForwardError::InvalidPort);
        }
        if self.occupied.contains(&listen_port) {
            return Err(ForwardError::Occupied(listen_port));
        }
        let id = self.next_id;
        self.next_id += 1;
        self.forwards.insert(
            id,
            PortForward {
                id,
                kind,
                listen_host: listen_host.to_owned(),
                listen_port,
                target_host: if kind == ForwardKind::Dynamic {
                    String::new()
                } else {
                    target_host.to_owned()
                },
                target_port: if kind == ForwardKind::Dynamic {
                    0
                } else {
                    target_port
                },
                state: ForwardState::Active,
            },
        );
        self.order.push(id);
        Ok(id)
    }

    /// Reads a forward.
    pub fn get(&self, id: u64) -> Option<&PortForward> {
        self.forwards.get(&id)
    }

    /// All forwards in creation order.
    pub fn list(&self) -> Vec<&PortForward> {
        self.order
            .iter()
            .filter_map(|id| self.forwards.get(id))
            .collect()
    }

    /// The address label for a forward (copy-ready).
    pub fn address_label(&self, id: u64) -> Option<String> {
        self.forwards.get(&id).map(PortForward::address_label)
    }

    /// Pauses an active forward.
    pub fn pause(&mut self, id: u64) -> Result<(), ForwardError> {
        let forward = self.forwards.get_mut(&id).ok_or(ForwardError::NotFound)?;
        if forward.state == ForwardState::Active {
            forward.state = ForwardState::Paused;
        }
        Ok(())
    }

    /// Resumes a paused forward.
    pub fn resume(&mut self, id: u64) -> Result<(), ForwardError> {
        let forward = self.forwards.get_mut(&id).ok_or(ForwardError::NotFound)?;
        if forward.state == ForwardState::Paused {
            forward.state = ForwardState::Active;
        }
        Ok(())
    }

    /// Starts a reconnect; the connection is re-established with
    /// `confirm_reconnected`.
    pub fn reconnect(&mut self, id: u64) -> Result<(), ForwardError> {
        let forward = self.forwards.get_mut(&id).ok_or(ForwardError::NotFound)?;
        forward.state = ForwardState::Reconnecting;
        Ok(())
    }

    /// Confirms a successful reconnect (back to active).
    pub fn confirm_reconnected(&mut self, id: u64) -> Result<(), ForwardError> {
        let forward = self.forwards.get_mut(&id).ok_or(ForwardError::NotFound)?;
        if forward.state == ForwardState::Reconnecting {
            forward.state = ForwardState::Active;
        }
        Ok(())
    }

    /// Removes a forward; returns whether it existed.
    pub fn remove(&mut self, id: u64) -> bool {
        let existed = self.forwards.remove(&id).is_some();
        self.order.retain(|existing| *existing != id);
        existed
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ForwardError, ForwardKind, ForwardManager, ForwardRisk, ForwardState, OccupancyStatus,
    };

    #[test]
    fn create_local_remote_dynamic_forwards() {
        let mut manager = ForwardManager::new();
        let local = manager
            .create(ForwardKind::Local, "127.0.0.1", 2222, "10.0.0.5", 22)
            .unwrap();
        let remote = manager
            .create(ForwardKind::Remote, "0.0.0.0", 8080, "localhost", 80)
            .unwrap();
        let dynamic = manager
            .create(ForwardKind::Dynamic, "127.0.0.1", 1080, "ignored", 0)
            .unwrap();
        assert_eq!(manager.list().len(), 3);
        assert_eq!(manager.get(local).unwrap().state, ForwardState::Active);
        assert_eq!(manager.get(remote).unwrap().kind, ForwardKind::Remote);
        // Dynamic forwards carry no target.
        assert_eq!(manager.get(dynamic).unwrap().target_host, "");
        assert_eq!(manager.get(dynamic).unwrap().target_port, 0);
    }

    #[test]
    fn occupied_port_create_fails_with_message() {
        let mut manager = ForwardManager::new();
        manager.set_occupied_ports(&[2222]);
        assert_eq!(
            manager.diagnose(2222),
            OccupancyStatus::Occupied { port: 2222 }
        );
        assert_eq!(manager.diagnose(3333), OccupancyStatus::Free);
        assert_eq!(
            manager.create(ForwardKind::Local, "127.0.0.1", 2222, "10.0.0.5", 22),
            Err(ForwardError::Occupied(2222))
        );
        assert_eq!(
            manager.create(ForwardKind::Local, "127.0.0.1", 0, "h", 22),
            Err(ForwardError::InvalidPort)
        );
    }

    #[test]
    fn pause_resume_reconnect_cycle() {
        let mut manager = ForwardManager::new();
        let id = manager
            .create(ForwardKind::Local, "127.0.0.1", 2222, "10.0.0.5", 22)
            .unwrap();
        manager.pause(id).unwrap();
        assert_eq!(manager.get(id).unwrap().state, ForwardState::Paused);
        manager.resume(id).unwrap();
        assert_eq!(manager.get(id).unwrap().state, ForwardState::Active);
        manager.reconnect(id).unwrap();
        assert_eq!(manager.get(id).unwrap().state, ForwardState::Reconnecting);
        manager.confirm_reconnected(id).unwrap();
        assert_eq!(manager.get(id).unwrap().state, ForwardState::Active);
        assert_eq!(manager.pause(999), Err(ForwardError::NotFound));
    }

    #[test]
    fn risk_warnings_wildcard_and_privileged() {
        let mut manager = ForwardManager::new();
        let id = manager
            .create(ForwardKind::Local, "0.0.0.0", 22, "10.0.0.5", 22)
            .unwrap();
        let warnings = manager.get(id).unwrap().risk_warning();
        assert!(
            warnings.iter().any(|w| w.level == ForwardRisk::High),
            "wildcard listen must be high risk"
        );
        assert!(
            warnings.iter().any(|w| w.level == ForwardRisk::Medium),
            "privileged port must warn"
        );
        // Localhost-only is low risk.
        let id = manager
            .create(ForwardKind::Local, "127.0.0.1", 2222, "10.0.0.5", 22)
            .unwrap();
        let warnings = manager.get(id).unwrap().risk_warning();
        assert_eq!(warnings[0].level, ForwardRisk::Low);
    }

    #[test]
    fn address_label_is_copy_ready() {
        let mut manager = ForwardManager::new();
        let id = manager
            .create(ForwardKind::Local, "127.0.0.1", 2222, "10.0.0.5", 22)
            .unwrap();
        assert_eq!(
            manager.address_label(id).as_deref(),
            Some("127.0.0.1:2222 -> 10.0.0.5:22")
        );
        let dynamic = manager
            .create(ForwardKind::Dynamic, "127.0.0.1", 1080, "", 0)
            .unwrap();
        assert_eq!(
            manager.address_label(dynamic).as_deref(),
            Some("127.0.0.1:1080 (dynamic)")
        );
    }

    #[test]
    fn remove_frees_forward() {
        let mut manager = ForwardManager::new();
        let id = manager
            .create(ForwardKind::Local, "127.0.0.1", 2222, "10.0.0.5", 22)
            .unwrap();
        assert!(manager.remove(id));
        assert!(!manager.remove(id));
        assert!(manager.list().is_empty());
    }
}
