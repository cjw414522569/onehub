//! Gateway versioned session protocol (T135).
//!
//! [`GatewaySession`] is a state machine over a versioned, framed message
//! stream: handshake (version check) -> authentication -> capability
//! negotiation -> ready, with per-message [`MessageFlags`] for cancellation
//! and backpressure, and a resume token so a session reconnects after a
//! network failure without re-authenticating from scratch.

/// The session protocol version.
pub const SESSION_PROTOCOL_VERSION: u8 = 1;

/// The message type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Handshake hello.
    Hello,
    /// Authentication.
    Auth,
    /// Capability negotiation.
    Capabilities,
    /// Payload data.
    Data,
    /// Cancel a request mid-flight.
    Cancel,
    /// Backpressure control.
    Backpressure,
    /// Close the session.
    Close,
    /// Resume a previous session.
    Resume,
}

/// Per-message control flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MessageFlags {
    /// Whether the request is cancelled.
    pub cancel: bool,
    /// Whether the sender is backpressured.
    pub backpressure: bool,
}

/// A framed session message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMessage {
    /// The protocol version.
    pub version: u8,
    /// The type.
    pub kind: MessageType,
    /// A monotonic sequence.
    pub sequence: u64,
    /// Control flags.
    pub flags: MessageFlags,
    /// The payload.
    pub payload: Vec<u8>,
}

/// A capability set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySet {
    /// Capability names.
    pub items: Vec<String>,
}

impl CapabilitySet {
    /// The common capabilities of two sets (sorted intersection).
    pub fn negotiate(&self, other: &CapabilitySet) -> Vec<String> {
        let mut common: Vec<String> = self
            .items
            .iter()
            .filter(|item| other.items.contains(item))
            .cloned()
            .collect();
        common.sort();
        common
    }
}

/// The session phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    /// Handshake / authentication / negotiation.
    Handshake,
    /// Ready for data.
    Ready,
    /// Closed.
    Closed,
}

/// Why a protocol operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    /// The message version does not match the protocol.
    VersionMismatch,
    /// Data was sent before authentication.
    NotAuthenticated,
    /// The session is not ready.
    NotReady,
    /// The resume token is invalid.
    InvalidResume,
}

/// The gateway session state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewaySession {
    /// The negotiated protocol version.
    pub version: u8,
    /// The phase.
    pub phase: SessionPhase,
    /// Whether the client authenticated.
    pub authenticated: bool,
    /// Negotiated capabilities.
    pub negotiated: Vec<String>,
    /// Current backpressure state.
    pub backpressure: bool,
    /// The resume token (set after auth).
    pub resume_token: Option<u64>,
    /// The next sequence number.
    pub sequence: u64,
}

impl GatewaySession {
    /// A fresh session in the handshake phase.
    pub fn new() -> Self {
        Self {
            version: SESSION_PROTOCOL_VERSION,
            phase: SessionPhase::Handshake,
            authenticated: false,
            negotiated: Vec::new(),
            backpressure: false,
            resume_token: None,
            sequence: 0,
        }
    }

    /// Handles the handshake: the version must match.
    pub fn handle_hello(&mut self, message: &SessionMessage) -> Result<(), ProtocolError> {
        if message.version != SESSION_PROTOCOL_VERSION {
            return Err(ProtocolError::VersionMismatch);
        }
        self.sequence = message.sequence;
        Ok(())
    }

    /// Authenticates the client; a valid token sets the resume token.
    pub fn authenticate(&mut self, token: u64, valid: bool) -> Result<(), ProtocolError> {
        if !valid {
            return Err(ProtocolError::NotAuthenticated);
        }
        self.authenticated = true;
        self.resume_token = Some(token);
        Ok(())
    }

    /// Negotiates capabilities against the server's set and moves to Ready.
    pub fn negotiate_capabilities(&mut self, server: &CapabilitySet) -> Vec<String> {
        let client = CapabilitySet {
            items: vec![
                "shell".to_owned(),
                "sftp".to_owned(),
                "forward".to_owned(),
                "resume".to_owned(),
            ],
        };
        self.negotiated = client.negotiate(server);
        self.phase = SessionPhase::Ready;
        self.negotiated.clone()
    }

    /// Receives a message; returns the payload for data messages.
    pub fn receive(&mut self, message: &SessionMessage) -> Result<Option<Vec<u8>>, ProtocolError> {
        if message.version != SESSION_PROTOCOL_VERSION {
            return Err(ProtocolError::VersionMismatch);
        }
        match message.kind {
            MessageType::Data => {
                if !self.authenticated {
                    return Err(ProtocolError::NotAuthenticated);
                }
                if self.phase != SessionPhase::Ready {
                    return Err(ProtocolError::NotReady);
                }
                self.sequence = self.sequence.max(message.sequence);
                Ok(Some(message.payload.clone()))
            }
            MessageType::Cancel => Ok(None),
            MessageType::Backpressure => {
                self.backpressure = message.flags.backpressure;
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Resumes a previous session after a network failure. The `expected`
    /// token is the server-held value for the previous session; the
    /// presented token must match it.
    pub fn resume(&mut self, presented: u64, expected: u64) -> Result<(), ProtocolError> {
        if presented != expected {
            return Err(ProtocolError::InvalidResume);
        }
        self.phase = SessionPhase::Ready;
        self.authenticated = true;
        self.resume_token = Some(expected);
        Ok(())
    }

    /// Closes the session.
    pub fn close(&mut self) {
        self.phase = SessionPhase::Closed;
    }
}

impl Default for GatewaySession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CapabilitySet, GatewaySession, MessageFlags, MessageType, ProtocolError, SessionMessage,
        SESSION_PROTOCOL_VERSION,
    };

    fn message(kind: MessageType, flags: MessageFlags) -> SessionMessage {
        SessionMessage {
            version: SESSION_PROTOCOL_VERSION,
            kind,
            sequence: 0,
            flags,
            payload: Vec::new(),
        }
    }

    #[test]
    fn handshake_auth_and_data_flow() {
        let mut session = GatewaySession::new();
        session
            .handle_hello(&message(MessageType::Hello, MessageFlags::default()))
            .unwrap();
        // Data before auth is refused.
        assert_eq!(
            session.receive(&message(MessageType::Data, MessageFlags::default())),
            Err(ProtocolError::NotAuthenticated)
        );
        session.authenticate(42, true).unwrap();
        session.negotiate_capabilities(&CapabilitySet {
            items: vec!["shell".to_owned(), "sftp".to_owned(), "forward".to_owned()],
        });
        assert_eq!(session.phase, super::SessionPhase::Ready);
        let data = SessionMessage {
            kind: MessageType::Data,
            payload: b"ping".to_vec(),
            ..message(MessageType::Data, MessageFlags::default())
        };
        assert_eq!(session.receive(&data).unwrap(), Some(b"ping".to_vec()));
        // Bad auth fails.
        let mut session = GatewaySession::new();
        session
            .handle_hello(&message(MessageType::Hello, MessageFlags::default()))
            .unwrap();
        assert_eq!(
            session.authenticate(1, false),
            Err(ProtocolError::NotAuthenticated)
        );
    }

    #[test]
    fn capability_negotiation_intersects() {
        let client = CapabilitySet {
            items: vec![
                "shell".to_owned(),
                "sftp".to_owned(),
                "forward".to_owned(),
                "resume".to_owned(),
            ],
        };
        let server = CapabilitySet {
            items: vec!["shell".to_owned(), "forward".to_owned()],
        };
        assert_eq!(client.negotiate(&server), vec!["forward", "shell"]);
    }

    #[test]
    fn backpressure_and_cancel_flags() {
        let mut session = GatewaySession::new();
        session
            .handle_hello(&message(MessageType::Hello, MessageFlags::default()))
            .unwrap();
        session.authenticate(1, true).unwrap();
        session.negotiate_capabilities(&CapabilitySet {
            items: vec!["shell".to_owned()],
        });
        let bp = SessionMessage {
            kind: MessageType::Backpressure,
            flags: MessageFlags {
                backpressure: true,
                cancel: false,
            },
            ..message(MessageType::Backpressure, MessageFlags::default())
        };
        session.receive(&bp).unwrap();
        assert!(session.backpressure);
        session
            .receive(&message(MessageType::Cancel, MessageFlags::default()))
            .unwrap();
    }

    #[test]
    fn resume_after_network_failure() {
        let mut session = GatewaySession::new();
        session
            .handle_hello(&message(MessageType::Hello, MessageFlags::default()))
            .unwrap();
        session.authenticate(7, true).unwrap();
        // Simulate a drop and reconnect: resume with the token.
        let mut resumed = GatewaySession::new();
        resumed
            .handle_hello(&message(MessageType::Hello, MessageFlags::default()))
            .unwrap();
        resumed.resume(7, 7).unwrap();
        assert_eq!(resumed.phase, super::SessionPhase::Ready);
        // Wrong token is refused.
        let mut bad = GatewaySession::new();
        bad.handle_hello(&message(MessageType::Hello, MessageFlags::default()))
            .unwrap();
        assert_eq!(bad.resume(99, 7), Err(ProtocolError::InvalidResume));
    }

    #[test]
    fn version_mismatch_is_rejected() {
        let mut session = GatewaySession::new();
        let old = SessionMessage {
            version: 0,
            ..message(MessageType::Hello, MessageFlags::default())
        };
        assert_eq!(
            session.handle_hello(&old),
            Err(ProtocolError::VersionMismatch)
        );
    }

    #[test]
    fn close_terminates() {
        let mut session = GatewaySession::new();
        session
            .handle_hello(&message(MessageType::Hello, MessageFlags::default()))
            .unwrap();
        session.authenticate(1, true).unwrap();
        session.close();
        assert_eq!(session.phase, super::SessionPhase::Closed);
    }
}
