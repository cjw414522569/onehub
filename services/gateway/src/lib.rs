#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # gateway
//!
//! Gateway service (T135 adds the versioned session protocol).

pub mod session_protocol;

pub use session_protocol::{
    CapabilitySet, GatewaySession, MessageFlags, MessageType, ProtocolError, SessionMessage,
    SessionPhase, SESSION_PROTOCOL_VERSION,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "gateway";
