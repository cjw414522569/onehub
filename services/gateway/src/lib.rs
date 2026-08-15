#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # gateway
//!
//! Gateway service (T135 adds the versioned session protocol; T136 adds the
//! target address policy and SSRF protection).

pub mod address_policy;
pub mod session_protocol;

pub use address_policy::{
    is_link_local, is_loopback, is_metadata, is_private, is_reserved, AddressPolicy,
    AddressPolicyError, ResolvedTarget, DEFAULT_ALLOWED_PORTS,
};
pub use session_protocol::{
    CapabilitySet, GatewaySession, MessageFlags, MessageType, ProtocolError, SessionMessage,
    SessionPhase, SESSION_PROTOCOL_VERSION,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "gateway";
