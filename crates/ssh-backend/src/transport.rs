use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use core_domain::error::DomainError;
use core_domain::host::HostAddress;
use core_domain::host_key::HostKeyStatus;
use session_orchestrator::cancellation::{select_cancellation, CancelReason, CancellationToken};

/// A connection target: address plus username.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTarget {
    /// Network address.
    pub address: HostAddress,
    /// Username.
    pub username: String,
}

impl SessionTarget {
    /// Creates a target, rejecting an empty username.
    pub fn new(address: HostAddress, username: impl Into<String>) -> Result<Self, DomainError> {
        let username = username.into();
        if username.trim().is_empty() {
            return Err(DomainError::EmptyUsername);
        }
        Ok(Self { address, username })
    }
}

/// Transport-level error with a stable, language-neutral code.
///
/// Carries no secret context and no backend-library types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Underlying I/O failure.
    Io,
    /// The operation timed out.
    Timeout,
    /// The operation was cancelled.
    Cancelled,
    /// The host key was rejected under the active policy.
    HostKeyRejected { status: HostKeyStatus },
    /// Authentication failed.
    AuthenticationFailed,
    /// A protocol-level failure.
    ProtocolError,
    /// The requested feature is unsupported.
    Unsupported,
    /// The session is already closed.
    Closed,
}

impl TransportError {
    /// Stable string code (never renumbered).
    pub const fn stable_code(&self) -> &'static str {
        match self {
            TransportError::Io => "E_TRANSPORT_IO",
            TransportError::Timeout => "E_TRANSPORT_TIMEOUT",
            TransportError::Cancelled => "E_TRANSPORT_CANCELLED",
            TransportError::HostKeyRejected { .. } => "E_TRANSPORT_HOST_KEY_REJECTED",
            TransportError::AuthenticationFailed => "E_TRANSPORT_AUTHENTICATION_FAILED",
            TransportError::ProtocolError => "E_TRANSPORT_PROTOCOL",
            TransportError::Unsupported => "E_TRANSPORT_UNSUPPORTED",
            TransportError::Closed => "E_TRANSPORT_CLOSED",
        }
    }
}

impl From<CancelReason> for TransportError {
    fn from(reason: CancelReason) -> Self {
        match reason {
            CancelReason::Cancelled => TransportError::Cancelled,
            CancelReason::DeadlineExpired => TransportError::Timeout,
        }
    }
}

/// Opaque connected-session handle.
///
/// The concrete backend (e.g. russh adapter) wraps its own session inside this
/// handle; the domain and orchestration layers only see the opaque id.
#[derive(Clone, Debug)]
pub struct SessionHandle {
    id: u64,
    closed: Arc<AtomicBool>,
}

impl SessionHandle {
    fn new(id: u64) -> Self {
        Self {
            id,
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns the opaque session id.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Whether the session has been closed.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Closes the session gracefully.
    pub async fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

/// The injectable SSH transport contract.
///
/// The trait deliberately uses only domain types ([`SessionTarget`],
/// [`TransportError`], [`SessionHandle`]) and never exposes backend-library
/// types, so the orchestration layer can be tested with a fake transport.
#[allow(async_fn_in_trait)]
pub trait SshTransport: Send + Sync {
    /// Connects to `target`, honouring the cancellation token.
    async fn connect(
        &self,
        target: &SessionTarget,
        token: &CancellationToken,
    ) -> Result<SessionHandle, TransportError>;

    /// Stable transport name (e.g. `fake`, `russh-adapter`).
    fn name(&self) -> &'static str;
}

/// Behaviour of the [`FakeTransport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeTransportMode {
    /// Connect succeeds immediately.
    Connect,
    /// Connect fails with an I/O error.
    FailIo,
    /// Connect fails with an authentication error.
    FailAuthentication,
    /// Connect waits until cancelled.
    Slow,
}

/// A deterministic fake transport for contract and orchestration tests.
#[derive(Debug, Clone, Copy)]
pub struct FakeTransport {
    /// Behaviour.
    pub mode: FakeTransportMode,
}

impl FakeTransport {
    /// Creates a fake transport with the given behaviour.
    pub const fn new(mode: FakeTransportMode) -> Self {
        Self { mode }
    }
}

static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

impl SshTransport for FakeTransport {
    async fn connect(
        &self,
        _target: &SessionTarget,
        token: &CancellationToken,
    ) -> Result<SessionHandle, TransportError> {
        match self.mode {
            FakeTransportMode::Connect => {
                let _ = select_cancellation(token, async {}).await;
                Ok(SessionHandle::new(
                    NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst),
                ))
            }
            FakeTransportMode::FailIo => Err(TransportError::Io),
            FakeTransportMode::FailAuthentication => Err(TransportError::AuthenticationFailed),
            FakeTransportMode::Slow => {
                let result = select_cancellation(token, std::future::pending::<()>()).await;
                match result {
                    Ok(()) => unreachable!("pending never completes"),
                    Err(reason) => Err(reason.into()),
                }
            }
        }
    }

    fn name(&self) -> &'static str {
        "fake"
    }
}

#[cfg(test)]
mod tests {
    use super::{FakeTransport, FakeTransportMode, SessionTarget, SshTransport, TransportError};
    use core_domain::host::HostAddress;
    use session_orchestrator::cancellation::CancellationToken;

    fn target() -> SessionTarget {
        SessionTarget::new(
            HostAddress::with_default_port("example.com").expect("address"),
            "alice",
        )
        .expect("target")
    }

    #[tokio::test]
    async fn fake_connect_returns_opaque_handle() {
        let transport = FakeTransport::new(FakeTransportMode::Connect);
        let token = CancellationToken::new();
        let handle = transport.connect(&target(), &token).await.expect("connect");
        assert_eq!(transport.name(), "fake");
        assert!(!handle.is_closed());
        assert!(handle.id() >= 1);
        handle.close().await;
        assert!(handle.is_closed());
    }

    #[tokio::test]
    async fn fake_fail_modes_map_to_stable_errors() {
        let token = CancellationToken::new();
        let io = FakeTransport::new(FakeTransportMode::FailIo);
        assert!(matches!(
            io.connect(&target(), &token).await,
            Err(TransportError::Io)
        ));
        let auth = FakeTransport::new(FakeTransportMode::FailAuthentication);
        assert!(matches!(
            auth.connect(&target(), &token).await,
            Err(TransportError::AuthenticationFailed)
        ));
    }

    #[tokio::test]
    async fn fake_slow_connect_honours_cancellation() {
        let transport = FakeTransport::new(FakeTransportMode::Slow);
        let token = CancellationToken::new();
        let handle = tokio::spawn({
            let target = target();
            let token = token.clone();
            async move { transport.connect(&target, &token).await }
        });
        tokio::task::yield_now().await;
        token.cancel();
        assert!(matches!(
            handle.await.expect("joined"),
            Err(TransportError::Cancelled)
        ));
    }

    #[test]
    fn transport_error_codes_are_unique_and_stable() {
        use super::TransportError::*;
        let codes = [
            Io.stable_code(),
            Timeout.stable_code(),
            Cancelled.stable_code(),
            TransportError::HostKeyRejected {
                status: core_domain::host_key::HostKeyStatus::Changed,
            }
            .stable_code(),
            AuthenticationFailed.stable_code(),
            ProtocolError.stable_code(),
            Unsupported.stable_code(),
            Closed.stable_code(),
        ];
        let mut seen = std::collections::HashSet::new();
        for code in codes {
            assert!(code.starts_with("E_TRANSPORT_"), "prefix required: {code}");
            assert!(seen.insert(code), "duplicate stable code: {code}");
        }
        assert_eq!(codes.len(), 8);
    }

    #[test]
    fn session_target_rejects_empty_username() {
        let address = HostAddress::with_default_port("example.com").expect("address");
        assert!(SessionTarget::new(address, "  ").is_err());
    }

    #[test]
    fn cancel_reason_maps_to_transport_error() {
        use session_orchestrator::cancellation::CancelReason;
        assert_eq!(
            TransportError::from(CancelReason::Cancelled),
            TransportError::Cancelled
        );
        assert_eq!(
            TransportError::from(CancelReason::DeadlineExpired),
            TransportError::Timeout
        );
    }
}
