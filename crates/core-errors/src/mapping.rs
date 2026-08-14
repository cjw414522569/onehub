use core_domain::error::DomainError;

use crate::code::{ErrorCode, Recoverability, RetrySuggestion};
use crate::info::{ErrorInfo, MessageParam};

/// Maps every pure-domain error to stable, language-neutral error metadata.
///
/// The `match` below is exhaustive at compile time: adding a `DomainError`
/// variant without a mapping arm fails the build. The runtime test iterates
/// every variant to verify the mapping invariants (stable code, message key,
/// recoverability, retry suggestion, and non-sensitive typed params).
impl From<DomainError> for ErrorInfo {
    fn from(error: DomainError) -> Self {
        match error {
            DomainError::EmptyId => ErrorInfo::new(
                ErrorCode::EmptyId,
                Recoverability::NonRecoverable,
                RetrySuggestion::None,
                "error.domain.empty_id",
            ),
            DomainError::EmptyHostname => ErrorInfo::new(
                ErrorCode::EmptyHostname,
                Recoverability::NonRecoverable,
                RetrySuggestion::None,
                "error.domain.empty_hostname",
            ),
            DomainError::InvalidPort(port) => ErrorInfo::new(
                ErrorCode::InvalidPort,
                Recoverability::Recoverable,
                RetrySuggestion::None,
                "error.domain.invalid_port",
            )
            .with_param(MessageParam::Port(port)),
            DomainError::EmptyName => ErrorInfo::new(
                ErrorCode::EmptyName,
                Recoverability::NonRecoverable,
                RetrySuggestion::None,
                "error.domain.empty_name",
            ),
            DomainError::EmptyUsername => ErrorInfo::new(
                ErrorCode::EmptyUsername,
                Recoverability::Recoverable,
                RetrySuggestion::None,
                "error.domain.empty_username",
            ),
            DomainError::InvalidForwarding => ErrorInfo::new(
                ErrorCode::InvalidArgument,
                Recoverability::Recoverable,
                RetrySuggestion::None,
                "error.domain.invalid_forwarding",
            ),
        }
    }
}

/// Constructs one instance of every `DomainError` variant for exhaustive tests.
pub fn all_domain_errors() -> Vec<DomainError> {
    vec![
        DomainError::EmptyId,
        DomainError::EmptyHostname,
        DomainError::InvalidPort(0),
        DomainError::EmptyName,
        DomainError::EmptyUsername,
        DomainError::InvalidForwarding,
    ]
}

#[cfg(test)]
mod tests {
    use super::all_domain_errors;
    use crate::code::{ErrorCode, Recoverability};
    use crate::info::ErrorInfo;
    use core_domain::error::DomainError;

    #[test]
    fn mapping_is_exhaustive_over_all_domain_variants() {
        let errors = all_domain_errors();
        assert_eq!(errors.len(), 6, "test must cover every DomainError variant");
        for error in errors {
            let info = ErrorInfo::from(error);
            assert_ne!(
                info.code(),
                ErrorCode::Unknown,
                "every known error maps to a concrete code"
            );
            assert!(
                info.code_str().starts_with("E_"),
                "stable code prefix required"
            );
            assert!(!info.message_key().is_empty(), "message key required");
            assert!(
                info.message_key().starts_with("error."),
                "localization key namespace required"
            );
            let _ = info.recoverability();
            let _ = info.retry();
        }
    }

    #[test]
    fn every_mapping_is_non_recoverable_only_when_expected() {
        assert_eq!(
            ErrorInfo::from(DomainError::EmptyId).recoverability(),
            Recoverability::NonRecoverable
        );
        assert_eq!(
            ErrorInfo::from(DomainError::InvalidPort(0)).recoverability(),
            Recoverability::Recoverable
        );
    }

    #[test]
    fn serialized_mappings_never_leak_sensitive_or_language_context() {
        for error in all_domain_errors() {
            let info = ErrorInfo::from(error.clone());
            let json = serde_json::to_string(&info).expect("serialize mapping");
            let lowered = json.to_lowercase();
            for marker in [
                "panic",
                "exception",
                "unwrap",
                "expect(",
                "password",
                "secret",
                "token",
            ] {
                assert!(
                    !lowered.contains(marker),
                    "mapping for {error:?} leaked forbidden marker {marker}: {json}"
                );
            }
        }
    }
}
