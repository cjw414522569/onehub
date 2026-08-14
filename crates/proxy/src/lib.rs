#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # proxy
//!
//! SOCKS5 (RFC 1928/1929) and HTTP CONNECT proxy adapters. Both protocols are
//! implemented in-house so the compatibility matrix (authentication, DNS
//! policy, IPv6, timeouts) runs over real loopback sockets and the wire format
//! stays auditable.

pub mod http_connect;
pub mod socks5;

pub use http_connect::{
    build_connect_request, http_connect, parse_status_code, HttpConnectConfig, MAX_HEADER_BYTES,
};
pub use socks5::{
    decode_auth_request, decode_connect_request, decode_method_selection, decode_reply,
    encode_auth_request, encode_connect_request, encode_greeting, encode_reply, socks5_connect,
    DnsPolicy, ProxyTarget, Socks5Config, ATYP_DOMAIN, ATYP_IPV4, ATYP_IPV6, CMD_CONNECT,
    METHOD_NO_ACCEPTABLE, METHOD_NO_AUTH, METHOD_USER_PASS, REP_CONNECTION_REFUSED, REP_SUCCESS,
    SOCKS5_VERSION,
};

/// Stable proxy failure kind (no secret context).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyErrorKind {
    /// Underlying I/O failure.
    Io,
    /// The handshake timed out.
    Timeout,
    /// A protocol-level violation.
    Protocol,
    /// The proxy rejected every offered authentication method.
    NoAcceptableMethod,
    /// The proxy rejected the credentials.
    AuthenticationRejected,
    /// The proxy refused the CONNECT (SOCKS5 reply code).
    ConnectRejected { code: u8 },
    /// The HTTP CONNECT proxy returned a non-2xx status.
    HttpStatus { code: u16 },
    /// The peer requested/sent something unsupported.
    Unsupported,
}

impl ProxyErrorKind {
    /// Stable string code (never renumbered).
    pub const fn stable_code(self) -> &'static str {
        match self {
            ProxyErrorKind::Io => "E_PROXY_IO",
            ProxyErrorKind::Timeout => "E_PROXY_TIMEOUT",
            ProxyErrorKind::Protocol => "E_PROXY_PROTOCOL",
            ProxyErrorKind::NoAcceptableMethod => "E_PROXY_NO_METHOD",
            ProxyErrorKind::AuthenticationRejected => "E_PROXY_AUTH_REJECTED",
            ProxyErrorKind::ConnectRejected { .. } => "E_PROXY_CONNECT_REJECTED",
            ProxyErrorKind::HttpStatus { .. } => "E_PROXY_HTTP_STATUS",
            ProxyErrorKind::Unsupported => "E_PROXY_UNSUPPORTED",
        }
    }
}

/// A proxy error with a stable kind and a human-readable detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyError {
    /// Failure kind.
    pub kind: ProxyErrorKind,
    /// Detail (never contains secrets).
    pub detail: String,
}

impl ProxyError {
    /// Builds an error.
    pub fn new(kind: ProxyErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// A protocol violation.
    pub fn protocol(detail: impl Into<String>) -> Self {
        Self::new(ProxyErrorKind::Protocol, detail)
    }

    /// The stable code for this error.
    pub fn stable_code(&self) -> &'static str {
        self.kind.stable_code()
    }
}

impl core::fmt::Display for ProxyError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{}: {}", self.stable_code(), self.detail)
    }
}

impl core::error::Error for ProxyError {}

impl From<std::io::Error> for ProxyError {
    fn from(error: std::io::Error) -> Self {
        Self::new(ProxyErrorKind::Io, error.to_string())
    }
}

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "proxy";
