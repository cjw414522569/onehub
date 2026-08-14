use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use session_orchestrator::cancellation::{
    select_cancellation, select_deadline, CancellationToken, Deadline,
};

/// Keepalive configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepaliveConfig {
    /// Interval between keepalive probes.
    pub interval: Duration,
    /// Timeout for a probe response before the connection is considered dead.
    pub timeout: Duration,
}

impl Default for KeepaliveConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(10),
        }
    }
}

/// Why a liveness probe failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeError {
    /// The connection is dead (no response / transport error).
    Dead,
    /// The probe did not respond within the timeout.
    Timeout,
}

/// A liveness probe (keepalive ping) against the SSH connection.
#[allow(async_fn_in_trait)]
pub trait LivenessProbe: Send + Sync {
    /// Returns `Ok(())` if the connection is alive.
    fn ping(&self) -> impl std::future::Future<Output = Result<(), ProbeError>> + Send;
}

/// Exponential backoff for reconnects, capped at a maximum delay so a
/// flapping network never produces a reconnect storm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectBackoff {
    /// Delay for attempt 0.
    pub base_delay: Duration,
    /// Maximum delay.
    pub max_delay: Duration,
    /// Multiplicative factor per attempt.
    pub factor: u32,
}

impl ReconnectBackoff {
    /// Creates a backoff policy.
    pub fn new(
        base_delay: Duration,
        max_delay: Duration,
        factor: u32,
    ) -> Result<Self, &'static str> {
        if base_delay.is_zero() || max_delay < base_delay || factor < 2 {
            return Err("invalid backoff: need 0 < base <= max and factor >= 2");
        }
        Ok(Self {
            base_delay,
            max_delay,
            factor,
        })
    }

    /// The delay to wait before reconnect attempt `attempt` (0-based),
    /// capped at `max_delay`.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let mut delay = self.base_delay.as_millis() as u64;
        for _ in 0..attempt {
            delay = delay.saturating_mul(self.factor as u64);
            if delay >= self.max_delay.as_millis() as u64 {
                return self.max_delay;
            }
        }
        Duration::from_millis(delay.min(self.max_delay.as_millis() as u64))
    }

    /// Whether, after `attempt` failures, a new attempt would exceed the
    /// storm budget of `max_attempts_per_window` within `window`.
    pub fn would_storm(
        &self,
        attempt: u32,
        max_attempts_per_window: u32,
        window: Duration,
    ) -> bool {
        // The time consumed by attempts 0..=attempt must stay under the
        // window; otherwise the schedule would re-connect too aggressively.
        let mut total_ms = 0u64;
        for index in 0..=attempt {
            total_ms = total_ms.saturating_add(self.delay_for_attempt(index).as_millis() as u64);
            if total_ms > window.as_millis() as u64 {
                return true;
            }
        }
        attempt.saturating_add(1) > max_attempts_per_window
    }
}

/// Observable monitor state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorState {
    /// Whether the last probe considered the connection alive.
    pub alive: bool,
    /// Current reconnect attempt counter.
    pub reconnect_attempt: u32,
    /// Delay (ms) applied before the next reconnect.
    pub next_backoff_ms: u64,
}

/// Shared handle to the monitor state (status is visible to the UI).
#[derive(Debug, Clone)]
pub struct MonitorStateHandle {
    alive: Arc<AtomicBool>,
    attempt: Arc<AtomicU32>,
    backoff_ms: Arc<AtomicU64>,
}

impl MonitorStateHandle {
    fn new() -> Self {
        Self {
            alive: Arc::new(AtomicBool::new(true)),
            attempt: Arc::new(AtomicU32::new(0)),
            backoff_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Snapshot of the current state.
    pub fn snapshot(&self) -> MonitorState {
        MonitorState {
            alive: self.alive.load(Ordering::SeqCst),
            reconnect_attempt: self.attempt.load(Ordering::SeqCst),
            next_backoff_ms: self.backoff_ms.load(Ordering::SeqCst),
        }
    }
}

/// Probes the connection with a per-probe timeout.
pub async fn probe_with_timeout<P: LivenessProbe + ?Sized>(
    probe: &P,
    timeout: Duration,
) -> Result<(), ProbeError> {
    let deadline = Deadline::after(timeout);
    match select_deadline(deadline, probe.ping()).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(ProbeError::Dead)) => Err(ProbeError::Dead),
        Ok(Err(ProbeError::Timeout)) => Err(ProbeError::Timeout),
        Err(_) => Err(ProbeError::Timeout),
    }
}

/// Runs the keepalive + reconnect loop.
///
/// - Probes every `interval` with `timeout`.
/// - On a dead connection, waits `backoff.delay_for_attempt(attempt)` then
///   retries; `attempt` resets on the first successful probe.
/// - Observes the cancellation token and exposes `MonitorStateHandle`.
/// - A flapping network cannot storm: backoff grows exponentially and is
///   capped at `max_delay`.
pub async fn run_reconnect_loop<P: LivenessProbe + ?Sized>(
    probe: &P,
    keepalive: KeepaliveConfig,
    backoff: ReconnectBackoff,
    token: &CancellationToken,
) -> MonitorStateHandle {
    let state = MonitorStateHandle::new();
    let mut attempt = 0u32;

    loop {
        // Probe (or wait for the next interval).
        let wait = async {
            if attempt == 0 {
                tokio::time::sleep(keepalive.interval).await;
            }
        };
        if select_cancellation(token, wait).await.is_err() {
            break;
        }

        let alive = matches!(probe_with_timeout(probe, keepalive.timeout).await, Ok(()));
        state.alive.store(alive, Ordering::SeqCst);

        if alive {
            attempt = 0;
            state.attempt.store(0, Ordering::SeqCst);
            state.backoff_ms.store(0, Ordering::SeqCst);
        } else {
            let delay = backoff.delay_for_attempt(attempt);
            attempt = attempt.saturating_add(1);
            state.attempt.store(attempt, Ordering::SeqCst);
            state
                .backoff_ms
                .store(delay.as_millis() as u64, Ordering::SeqCst);
            let wait = async {
                tokio::time::sleep(delay).await;
            };
            if select_cancellation(token, wait).await.is_err() {
                break;
            }
        }
    }

    state
}

#[cfg(test)]
mod tests {
    use super::{
        probe_with_timeout, run_reconnect_loop, KeepaliveConfig, LivenessProbe, ProbeError,
        ReconnectBackoff,
    };
    use session_orchestrator::cancellation::CancellationToken;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    /// A scripted probe: fails for the first `failures` pings, then succeeds.
    struct FlakyProbe {
        failures: AtomicU32,
    }

    impl LivenessProbe for FlakyProbe {
        async fn ping(&self) -> Result<(), ProbeError> {
            let remaining =
                self.failures
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                        if value == 0 {
                            None
                        } else {
                            Some(value - 1)
                        }
                    });
            if remaining.is_ok() {
                Err(ProbeError::Dead)
            } else {
                Ok(())
            }
        }
    }

    struct HangingProbe;
    impl LivenessProbe for HangingProbe {
        async fn ping(&self) -> Result<(), ProbeError> {
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    #[test]
    fn backoff_grows_exponentially_and_caps() {
        let backoff =
            ReconnectBackoff::new(Duration::from_millis(100), Duration::from_millis(1600), 2)
                .expect("backoff");
        assert_eq!(backoff.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(backoff.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(backoff.delay_for_attempt(2), Duration::from_millis(400));
        assert_eq!(backoff.delay_for_attempt(3), Duration::from_millis(800));
        assert_eq!(backoff.delay_for_attempt(4), Duration::from_millis(1600));
        // Capped.
        assert_eq!(backoff.delay_for_attempt(10), Duration::from_millis(1600));
    }

    #[test]
    fn backoff_rejects_invalid_policies() {
        assert!(ReconnectBackoff::new(Duration::ZERO, Duration::from_secs(1), 2).is_err());
        assert!(ReconnectBackoff::new(Duration::from_secs(5), Duration::from_secs(1), 2).is_err());
        assert!(
            ReconnectBackoff::new(Duration::from_millis(100), Duration::from_secs(1), 1).is_err()
        );
    }

    #[test]
    fn backoff_prevents_reconnect_storms() {
        let backoff =
            ReconnectBackoff::new(Duration::from_millis(100), Duration::from_millis(1600), 2)
                .expect("backoff");
        let window = Duration::from_secs(1);
        // Attempts 0..=3 consume 100+200+400+800 = 1500ms > 1s window.
        assert!(!backoff.would_storm(2, 10, window));
        assert!(backoff.would_storm(3, 10, window));
        // Even with a huge window, the per-window cap applies.
        assert!(backoff.would_storm(10, 5, Duration::from_secs(60)));
    }

    #[tokio::test(start_paused = true)]
    async fn hanging_probe_times_out_and_reports_dead() {
        let probe = HangingProbe;
        // With a paused clock, probe_with_timeout must still report a timeout
        // after the (virtual) deadline expires.
        assert_eq!(
            probe_with_timeout(&probe, Duration::from_millis(50)).await,
            Err(ProbeError::Timeout)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn flaky_probe_triggers_backoff_and_recovers() {
        let probe = FlakyProbe {
            failures: AtomicU32::new(2),
        };
        let token = CancellationToken::new();
        let handle = tokio::spawn({
            let token = token.clone();
            async move {
                run_reconnect_loop(
                    &probe,
                    KeepaliveConfig {
                        interval: Duration::from_millis(10),
                        timeout: Duration::from_millis(50),
                    },
                    ReconnectBackoff::new(Duration::from_millis(10), Duration::from_millis(80), 2)
                        .expect("backoff"),
                    &token,
                )
                .await
            }
        });
        tokio::task::yield_now().await;

        // Step virtual time so the loop can run through the two failures and
        // recover (single `advance` does not fire timers registered mid-step).
        for _ in 0..40 {
            tokio::time::advance(Duration::from_millis(5)).await;
            tokio::task::yield_now().await;
        }
        token.cancel();
        let state = handle.await.expect("loop joins on cancel");
        let snapshot = state.snapshot();
        assert!(snapshot.alive, "connection must recover after flapping");
        assert_eq!(
            snapshot.reconnect_attempt, 0,
            "attempt resets after recovery"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn dead_connection_attempts_grow_without_storm() {
        struct AlwaysDead;
        impl LivenessProbe for AlwaysDead {
            async fn ping(&self) -> Result<(), ProbeError> {
                Err(ProbeError::Dead)
            }
        }
        let probe = AlwaysDead;
        let token = CancellationToken::new();
        let handle = tokio::spawn({
            let token = token.clone();
            async move {
                run_reconnect_loop(
                    &probe,
                    KeepaliveConfig {
                        interval: Duration::from_millis(10),
                        timeout: Duration::from_millis(50),
                    },
                    ReconnectBackoff::new(Duration::from_millis(10), Duration::from_millis(80), 2)
                        .expect("backoff"),
                    &token,
                )
                .await
            }
        });
        tokio::task::yield_now().await;

        // Step 1s of virtual time; the exponential cap must keep attempts
        // well below a naive storm (e.g. 100 per second) while still growing.
        for _ in 0..200 {
            tokio::time::advance(Duration::from_millis(5)).await;
            tokio::task::yield_now().await;
        }
        token.cancel();
        let state = handle.await.expect("loop joins on cancel");
        let snapshot = state.snapshot();
        assert!(!snapshot.alive);
        // The loop must have actually reconnected several times...
        assert!(
            snapshot.reconnect_attempt >= 3,
            "expected several reconnect attempts, got {}",
            snapshot.reconnect_attempt
        );
        // ...but the exponential cap keeps it far below a naive storm
        // (100 per second would be ~100 attempts in 1s).
        assert!(
            snapshot.reconnect_attempt < 20,
            "no reconnect storm: {}",
            snapshot.reconnect_attempt
        );
        assert!(snapshot.next_backoff_ms >= 10);
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_stops_the_loop() {
        let probe = FlakyProbe {
            failures: AtomicU32::new(0),
        };
        let token = CancellationToken::new();
        let handle = tokio::spawn({
            let token = token.clone();
            async move {
                run_reconnect_loop(
                    &probe,
                    KeepaliveConfig {
                        interval: Duration::from_secs(1),
                        timeout: Duration::from_millis(50),
                    },
                    ReconnectBackoff::new(Duration::from_millis(10), Duration::from_millis(80), 2)
                        .expect("backoff"),
                    &token,
                )
                .await
            }
        });
        tokio::task::yield_now().await;
        token.cancel();
        let state = handle.await.expect("loop exits on cancel");
        let _ = state.snapshot();
    }
}
