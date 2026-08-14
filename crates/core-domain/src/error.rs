use serde::{Deserialize, Serialize};

/// Validation failure for immutable domain values.
///
/// These are pure-domain errors without UI, database, or SSH context.
/// Cross-boundary stable error codes are owned by `core-errors`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DomainError {
    /// An identifier was empty or whitespace-only.
    EmptyId,
    /// A hostname was empty or whitespace-only.
    EmptyHostname,
    /// A port was out of the valid 1..=65535 range.
    InvalidPort(u16),
    /// A session profile name was empty or whitespace-only.
    EmptyName,
    /// A username was empty or whitespace-only.
    EmptyUsername,
    /// A domain combination was invalid (e.g. dynamic forwarding with a
    /// target).
    InvalidForwarding,
}

impl core::fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DomainError::EmptyId => write!(formatter, "identifier must not be empty"),
            DomainError::EmptyHostname => write!(formatter, "hostname must not be empty"),
            DomainError::InvalidPort(port) => {
                write!(formatter, "port {port} is out of the valid 1..=65535 range")
            }
            DomainError::EmptyName => write!(formatter, "name must not be empty"),
            DomainError::EmptyUsername => write!(formatter, "username must not be empty"),
            DomainError::InvalidForwarding => write!(formatter, "invalid forwarding combination"),
        }
    }
}

impl core::error::Error for DomainError {}

#[cfg(test)]
mod tests {
    use super::DomainError;

    #[test]
    fn display_and_error_trait_work() {
        let message = DomainError::EmptyId.to_string();
        assert!(!message.is_empty());
        let _: &dyn std::error::Error = &DomainError::InvalidPort(0);
        assert!(message.contains("empty"));
    }
}
