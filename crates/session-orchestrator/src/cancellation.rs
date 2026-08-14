use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

use tokio::sync::watch;

/// Why an operation stopped before producing a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    /// The operation was cooperatively cancelled.
    Cancelled,
    /// The operation exceeded its deadline.
    DeadlineExpired,
}

impl CancelReason {
    /// Stable, human-readable reason string.
    pub const fn as_str(self) -> &'static str {
        match self {
            CancelReason::Cancelled => "cancelled",
            CancelReason::DeadlineExpired => "deadline_expired",
        }
    }
}

/// A shareable, cooperative cancellation signal.
///
/// Derived from `tokio::sync::watch` so there is no lost-wakeup race:
/// a waiter that subscribes after `cancel()` observes the current value and
/// returns immediately; a waiter that subscribed earlier is woken by the send.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    sender: watch::Sender<bool>,
    // Kept alive so `send` always succeeds even before any waiter subscribes.
    _receiver: watch::Receiver<bool>,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    /// Creates an uncancelled token.
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(false);
        Self {
            inner: Arc::new(Inner {
                sender,
                _receiver: receiver,
            }),
        }
    }

    /// Signals cancellation to every current and future waiter.
    pub fn cancel(&self) {
        let _ = self.inner.sender.send(true);
    }

    /// Returns whether cancellation has been signalled.
    pub fn is_cancelled(&self) -> bool {
        *self.inner.sender.borrow()
    }

    /// Waits until the token is cancelled.
    ///
    /// Returns immediately if cancellation already happened.
    pub async fn cancelled(&self) {
        let mut receiver = self.inner.sender.subscribe();
        if *receiver.borrow_and_update() {
            return;
        }
        while receiver.changed().await.is_ok() {
            if *receiver.borrow_and_update() {
                return;
            }
        }
    }
}

/// An absolute instant before which an operation should finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline {
    at: Instant,
}

impl Deadline {
    /// Creates a deadline `duration` from now.
    pub fn after(duration: Duration) -> Self {
        Self {
            at: Instant::now() + duration,
        }
    }

    /// Creates a deadline at a specific instant.
    pub fn at(at: Instant) -> Self {
        Self { at }
    }

    /// Returns the time remaining, or `None` if already expired.
    pub fn remaining(&self) -> Option<Duration> {
        self.at.checked_duration_since(Instant::now())
    }

    /// Returns whether the deadline has passed.
    pub fn is_expired(&self) -> bool {
        self.remaining().is_none_or(|remaining| remaining.is_zero())
    }
}

/// Runs `future`, returning its result unless the token is cancelled first.
///
/// `biased` makes a completed operation win over simultaneous cancellation,
/// which gives callers deterministic, structured-concurrency semantics.
pub async fn select_cancellation<T>(
    token: &CancellationToken,
    future: impl Future<Output = T>,
) -> Result<T, CancelReason> {
    tokio::select! {
        biased;
        result = future => Ok(result),
        _ = token.cancelled() => Err(CancelReason::Cancelled),
    }
}

/// Runs `future`, returning its result unless the deadline expires first.
pub async fn select_deadline<T>(
    deadline: Deadline,
    future: impl Future<Output = T>,
) -> Result<T, CancelReason> {
    let Some(remaining) = deadline.remaining() else {
        return Err(CancelReason::DeadlineExpired);
    };
    tokio::select! {
        biased;
        result = future => Ok(result),
        _ = tokio::time::sleep(remaining) => Err(CancelReason::DeadlineExpired),
    }
}

/// Runs `future` under both a cancellation token and a deadline.
///
/// Either signal stops the operation; the operation's own completion always
/// wins when it is ready at the same time.
pub async fn select_guarded<T>(
    token: &CancellationToken,
    deadline: Deadline,
    future: impl Future<Output = T>,
) -> Result<T, CancelReason> {
    tokio::select! {
        biased;
        result = future => Ok(result),
        _ = token.cancelled() => Err(CancelReason::Cancelled),
        _ = sleep_until_deadline(deadline) => Err(CancelReason::DeadlineExpired),
    }
}

async fn sleep_until_deadline(deadline: Deadline) {
    if let Some(remaining) = deadline.remaining() {
        tokio::time::sleep(remaining).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        select_cancellation, select_deadline, select_guarded, CancelReason, CancellationToken,
        Deadline,
    };
    use std::time::Duration;

    #[tokio::test(start_paused = true)]
    async fn token_starts_uncancelled_and_cancel_wakes_waiters() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        let waiter = tokio::spawn({
            let token = token.clone();
            async move {
                token.cancelled().await;
                true
            }
        });
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
        assert!(
            waiter.await.expect("waiter task joined"),
            "cancelled() must resolve after cancel()"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_returns_immediately_when_already_cancelled() {
        let token = CancellationToken::new();
        token.cancel();
        token.cancelled().await;
        assert!(token.is_cancelled());
    }

    #[tokio::test(start_paused = true)]
    async fn select_cancellation_wins_the_race() {
        let token = CancellationToken::new();
        let handle = tokio::spawn({
            let token = token.clone();
            async move { select_cancellation(&token, std::future::pending::<&str>()).await }
        });
        tokio::task::yield_now().await;
        token.cancel();
        assert_eq!(
            handle.await.expect("task joined"),
            Err(CancelReason::Cancelled)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn select_cancellation_returns_result_when_operation_finishes_first() {
        let token = CancellationToken::new();
        let result = select_cancellation(&token, async { 42 }).await;
        assert_eq!(result, Ok(42));
    }

    #[tokio::test(start_paused = true)]
    async fn select_cancellation_after_many_spawns_has_no_races() {
        let token = CancellationToken::new();
        let mut handles = Vec::new();
        for _ in 0..32 {
            let token = token.clone();
            handles.push(tokio::spawn(async move {
                select_cancellation(&token, std::future::pending::<u32>()).await
            }));
        }
        tokio::task::yield_now().await;
        token.cancel();
        for handle in handles {
            assert_eq!(
                handle.await.expect("task joined"),
                Err(CancelReason::Cancelled)
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_expires_and_times_out_pending_operation() {
        let deadline = Deadline::after(Duration::from_secs(1));
        assert!(!deadline.is_expired());
        let result = select_deadline(deadline, std::future::pending::<&str>()).await;
        assert_eq!(result, Err(CancelReason::DeadlineExpired));
        assert!(
            deadline.is_expired(),
            "deadline must be expired after timeout"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_allows_fast_completion() {
        let deadline = Deadline::after(Duration::from_secs(60));
        let result = select_deadline(deadline, async { "done" }).await;
        assert_eq!(result, Ok("done"));
    }

    #[tokio::test(start_paused = true)]
    async fn already_expired_deadline_fails_immediately() {
        let deadline = Deadline::after(Duration::from_millis(1));
        tokio::time::advance(Duration::from_millis(2)).await;
        assert!(deadline.is_expired());
        let result = select_deadline(deadline, async { "late" }).await;
        assert_eq!(result, Err(CancelReason::DeadlineExpired));
    }

    // Structured concurrency acceptance: connect, auth, transfer, and close
    // are each reliably cancellable and deadline-bounded.

    async fn simulated_connect(
        token: &CancellationToken,
        deadline: Deadline,
    ) -> Result<&'static str, CancelReason> {
        select_guarded(token, deadline, async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            "connected"
        })
        .await
    }

    async fn simulated_auth(
        token: &CancellationToken,
        deadline: Deadline,
    ) -> Result<&'static str, CancelReason> {
        select_guarded(token, deadline, async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            "authenticated"
        })
        .await
    }

    async fn simulated_transfer(
        token: &CancellationToken,
        deadline: Deadline,
    ) -> Result<&'static str, CancelReason> {
        select_guarded(token, deadline, async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            "transferred"
        })
        .await
    }

    async fn simulated_close(
        token: &CancellationToken,
        deadline: Deadline,
    ) -> Result<&'static str, CancelReason> {
        select_guarded(token, deadline, async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            "closed"
        })
        .await
    }

    #[tokio::test(start_paused = true)]
    async fn connect_auth_transfer_close_all_cancel_reliably() {
        let deadline = Deadline::after(Duration::from_secs(30));
        for phase in ["connect", "auth", "transfer", "close"] {
            let token = CancellationToken::new();
            let handle = tokio::spawn({
                let token = token.clone();
                async move {
                    match phase {
                        "connect" => simulated_connect(&token, deadline).await,
                        "auth" => simulated_auth(&token, deadline).await,
                        "transfer" => simulated_transfer(&token, deadline).await,
                        _ => simulated_close(&token, deadline).await,
                    }
                }
            });
            tokio::task::yield_now().await;
            token.cancel();
            let result = handle.await.expect("task joined");
            assert_eq!(
                result,
                Err(CancelReason::Cancelled),
                "{phase} must cancel reliably"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn connect_auth_transfer_close_all_timeout_reliably() {
        let phases = ["connect", "auth", "transfer", "close"];
        for phase in phases {
            let token = CancellationToken::new();
            let deadline = Deadline::after(Duration::from_millis(5));
            let handle = tokio::spawn(async move {
                match phase {
                    "connect" => simulated_connect(&token, deadline).await,
                    "auth" => simulated_auth(&token, deadline).await,
                    "transfer" => simulated_transfer(&token, deadline).await,
                    _ => simulated_close(&token, deadline).await,
                }
            });
            let result = handle.await.expect("task joined");
            assert_eq!(
                result,
                Err(CancelReason::DeadlineExpired),
                "{phase} must time out reliably"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn completion_wins_over_simultaneous_cancellation() {
        let token = CancellationToken::new();
        let result = select_guarded(&token, Deadline::after(Duration::from_secs(30)), async {
            token.cancel();
            "completed"
        })
        .await;
        assert_eq!(
            result,
            Ok("completed"),
            "biased select must prefer operation completion"
        );
    }
}
