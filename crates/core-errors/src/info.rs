use serde::{Deserialize, Serialize};

use crate::code::{ErrorCode, Recoverability, RetrySuggestion};

/// A typed, non-sensitive localization parameter.
///
/// Message parameters are deliberately typed and carry no secret material:
/// credentials, keys, tokens, and terminal text must never appear here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageParam {
    /// Short, non-sensitive text such as a host label.
    Text(String),
    /// A count.
    Count(u64),
    /// A TCP port number.
    Port(u16),
    /// A stable identifier that contains no secrets.
    Identifier(String),
}

/// Stable, language-neutral error description that can cross FFI.
///
/// Carries a stable code, recoverability, a retry suggestion, a localization
/// message key, and typed non-sensitive parameters. It never carries a
/// language exception, raw panic text, or sensitive context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorInfo {
    code: ErrorCode,
    recoverability: Recoverability,
    retry: RetrySuggestion,
    message_key: String,
    params: Vec<MessageParam>,
}

impl ErrorInfo {
    /// Creates error metadata without parameters.
    pub fn new(
        code: ErrorCode,
        recoverability: Recoverability,
        retry: RetrySuggestion,
        message_key: impl Into<String>,
    ) -> Self {
        Self {
            code,
            recoverability,
            retry,
            message_key: message_key.into(),
            params: Vec::new(),
        }
    }

    /// Appends a typed, non-sensitive message parameter.
    pub fn with_param(mut self, param: MessageParam) -> Self {
        self.params.push(param);
        self
    }

    /// Returns the stable error code.
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// Returns the stable string code.
    pub fn code_str(&self) -> &'static str {
        self.code.as_str()
    }

    /// Returns whether the caller can recover.
    pub fn recoverability(&self) -> Recoverability {
        self.recoverability
    }

    /// Returns the retry suggestion.
    pub fn retry(&self) -> RetrySuggestion {
        self.retry
    }

    /// Returns the localization message key.
    pub fn message_key(&self) -> &str {
        &self.message_key
    }

    /// Returns the typed, non-sensitive parameters.
    pub fn params(&self) -> &[MessageParam] {
        &self.params
    }
}

#[cfg(test)]
mod tests {
    use super::{ErrorInfo, MessageParam};
    use crate::code::{ErrorCode, Recoverability, RetrySuggestion};

    #[test]
    fn error_info_getters_and_builder() {
        let info = ErrorInfo::new(
            ErrorCode::InvalidPort,
            Recoverability::Recoverable,
            RetrySuggestion::None,
            "error.domain.invalid_port",
        )
        .with_param(MessageParam::Port(0));
        assert_eq!(info.code(), ErrorCode::InvalidPort);
        assert_eq!(info.code_str(), "E_INVALID_PORT");
        assert_eq!(info.recoverability(), Recoverability::Recoverable);
        assert_eq!(info.retry(), RetrySuggestion::None);
        assert_eq!(info.message_key(), "error.domain.invalid_port");
        assert_eq!(info.params(), &[MessageParam::Port(0)]);
    }

    #[test]
    fn error_info_serde_round_trip() {
        let info = ErrorInfo::new(
            ErrorCode::Timeout,
            Recoverability::Recoverable,
            RetrySuggestion::WithBackoff {
                max_attempts: 3,
                base_delay_ms: 250,
            },
            "error.network.timeout",
        )
        .with_param(MessageParam::Count(3));
        let json = serde_json::to_string(&info).expect("serialize");
        let decoded: ErrorInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, info);
    }

    #[test]
    fn serialized_error_info_contains_no_language_exception_markers() {
        let info = ErrorInfo::new(
            ErrorCode::Internal,
            Recoverability::NonRecoverable,
            RetrySuggestion::None,
            "error.internal",
        );
        let json = serde_json::to_string(&info).expect("serialize");
        for marker in ["panic", "exception", "unwrap", "expect", "std::"] {
            assert!(
                !json.to_lowercase().contains(marker),
                "serialized error must not leak language exception marker {marker}: {json}"
            );
        }
    }
}
