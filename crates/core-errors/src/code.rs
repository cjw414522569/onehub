use serde::{Deserialize, Serialize};

/// Stable, language-neutral error code.
///
/// String codes are frozen once shipped; new codes are appended, never renumbered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorCode {
    /// A generic invalid argument.
    InvalidArgument,
    /// An identifier was empty.
    EmptyId,
    /// A hostname was empty.
    EmptyHostname,
    /// A port was out of range.
    InvalidPort,
    /// A name was empty.
    EmptyName,
    /// A username was empty.
    EmptyUsername,
    /// The requested entity does not exist.
    NotFound,
    /// The entity already exists.
    AlreadyExists,
    /// Permission was denied.
    PermissionDenied,
    /// The service or resource is currently unavailable.
    Unavailable,
    /// The operation timed out.
    Timeout,
    /// The operation was cancelled.
    Cancelled,
    /// Authentication failed.
    AuthenticationFailed,
    /// A network-level failure occurred.
    NetworkError,
    /// A protocol-level failure occurred.
    ProtocolError,
    /// The operation is not valid in the current state.
    InvalidState,
    /// An internal invariant was violated.
    Internal,
    /// The error could not be classified.
    Unknown,
}

impl ErrorCode {
    /// Returns the frozen stable string code (never renumbered).
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidArgument => "E_INVALID_ARGUMENT",
            ErrorCode::EmptyId => "E_EMPTY_ID",
            ErrorCode::EmptyHostname => "E_EMPTY_HOSTNAME",
            ErrorCode::InvalidPort => "E_INVALID_PORT",
            ErrorCode::EmptyName => "E_EMPTY_NAME",
            ErrorCode::EmptyUsername => "E_EMPTY_USERNAME",
            ErrorCode::NotFound => "E_NOT_FOUND",
            ErrorCode::AlreadyExists => "E_ALREADY_EXISTS",
            ErrorCode::PermissionDenied => "E_PERMISSION_DENIED",
            ErrorCode::Unavailable => "E_UNAVAILABLE",
            ErrorCode::Timeout => "E_TIMEOUT",
            ErrorCode::Cancelled => "E_CANCELLED",
            ErrorCode::AuthenticationFailed => "E_AUTHENTICATION_FAILED",
            ErrorCode::NetworkError => "E_NETWORK_ERROR",
            ErrorCode::ProtocolError => "E_PROTOCOL_ERROR",
            ErrorCode::InvalidState => "E_INVALID_STATE",
            ErrorCode::Internal => "E_INTERNAL",
            ErrorCode::Unknown => "E_UNKNOWN",
        }
    }
}

impl core::fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Whether the caller may recover by correcting input or retrying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recoverability {
    /// The caller can recover (for example, fix input or retry later).
    Recoverable,
    /// The caller must not retry without a state change.
    NonRecoverable,
}

/// Language-neutral retry suggestion (never an instruction to leak secrets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetrySuggestion {
    /// Do not retry.
    None,
    /// Retry exactly once.
    Once,
    /// Retry with bounded backoff.
    WithBackoff {
        /// Maximum number of attempts including the first.
        max_attempts: u32,
        /// Base delay in milliseconds.
        base_delay_ms: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::{ErrorCode, Recoverability, RetrySuggestion};

    #[test]
    fn stable_string_codes_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for code in [
            ErrorCode::InvalidArgument,
            ErrorCode::EmptyId,
            ErrorCode::EmptyHostname,
            ErrorCode::InvalidPort,
            ErrorCode::EmptyName,
            ErrorCode::EmptyUsername,
            ErrorCode::NotFound,
            ErrorCode::AlreadyExists,
            ErrorCode::PermissionDenied,
            ErrorCode::Unavailable,
            ErrorCode::Timeout,
            ErrorCode::Cancelled,
            ErrorCode::AuthenticationFailed,
            ErrorCode::NetworkError,
            ErrorCode::ProtocolError,
            ErrorCode::InvalidState,
            ErrorCode::Internal,
            ErrorCode::Unknown,
        ] {
            let value = code.as_str();
            assert!(value.starts_with("E_"), "code must use E_ prefix: {value}");
            assert!(seen.insert(value), "duplicate stable code: {value}");
        }
    }

    #[test]
    fn serde_round_trip_for_code_enums() {
        let json = serde_json::to_string(&ErrorCode::Timeout).expect("serialize code");
        assert_eq!(json, "\"Timeout\"");
        let decoded: ErrorCode = serde_json::from_str(&json).expect("deserialize code");
        assert_eq!(decoded, ErrorCode::Timeout);

        let retry = RetrySuggestion::WithBackoff {
            max_attempts: 3,
            base_delay_ms: 500,
        };
        let round: RetrySuggestion =
            serde_json::from_str(&serde_json::to_string(&retry).expect("serialize retry"))
                .expect("deserialize retry");
        assert_eq!(round, retry);
        assert_ne!(round, RetrySuggestion::None);

        let recoverable = Recoverability::Recoverable;
        let round2: Recoverability =
            serde_json::from_str(&serde_json::to_string(&recoverable).expect("serialize"))
                .expect("deserialize");
        assert_eq!(round2, recoverable);
    }
}
