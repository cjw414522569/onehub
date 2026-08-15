#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # gateway
//!
//! Gateway service (T135 versioned session protocol; T136 target address
//! policy and SSRF protection; T137 authentication, short-lived tokens and
//! session isolation).

pub mod address_policy;
pub mod auth;
pub mod session_protocol;

pub use address_policy::{
    is_link_local, is_loopback, is_metadata, is_private, is_reserved, AddressPolicy,
    AddressPolicyError, ResolvedTarget, DEFAULT_ALLOWED_PORTS,
};
pub use auth::{AuthError, AuthToken, CredentialPolicy, SessionRegistry, TenantId, TokenIssuer};
pub use session_protocol::{
    CapabilitySet, GatewaySession, MessageFlags, MessageType, ProtocolError, SessionMessage,
    SessionPhase, SESSION_PROTOCOL_VERSION,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "gateway";
