//! Transfer queue, background progress, failure retry, and safe
//! notifications (T115).
//!
//! [`TransferQueue`] manages transfers with background progress, transient /
//! permanent failure classification, auto-retry under a configurable policy,
//! and manual retry / cancel that **reuse the same entry id** (no duplicate
//! submission). System notifications are built from a safe label only - they
//! never include source/destination paths, so secrets cannot leak (verified
//! by a notification-leak test).

use std::collections::HashMap;

/// The queue entry state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueState {
    /// Waiting.
    Queued,
    /// Running in the background.
    Running,
    /// Finished.
    Done,
    /// Failed (retryable).
    Failed,
    /// Cancelled (retryable).
    Cancelled,
}

/// The failure kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// Transient (e.g. timeout): auto-retried under the policy.
    Transient,
    /// Permanent (e.g. auth rejected): no auto-retry.
    Permanent,
}

/// A failure record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The kind.
    pub kind: FailureKind,
    /// A human message (safe to show; no secrets).
    pub message: String,
}

/// The retry policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RetryPolicy {
    /// No auto-retry.
    #[default]
    None,
    /// Fixed attempts with a backoff.
    Fixed { attempts: u32, backoff_secs: u64 },
}

/// Transfer progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueProgress {
    /// Bytes transferred.
    pub bytes: u64,
    /// Total bytes.
    pub total: u64,
}

impl QueueProgress {
    /// The percent complete (0..=100).
    pub fn percent(&self) -> u8 {
        if self.total == 0 {
            return 100;
        }
        ((self.bytes as f64 / self.total as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8
    }
}

/// A queued transfer entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueEntry {
    /// Stable id (retry reuses it).
    pub id: u64,
    /// A safe, secret-free display label.
    pub label: String,
    /// The source path (may contain secrets; never shown in notifications).
    pub source: String,
    /// The destination path (may contain secrets).
    pub destination: String,
    /// The state.
    pub state: QueueState,
    /// Progress.
    pub progress: QueueProgress,
    /// Auto-retry attempts used.
    pub retries: u32,
    /// The last failure, if any.
    pub failure: Option<Failure>,
}

/// A system notification (built from the safe label only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferNotification {
    /// The title.
    pub title: String,
    /// The body.
    pub body: String,
}

impl TransferNotification {
    /// Whether the notification contains `needle` (leak check).
    pub fn contains(&self, needle: &str) -> bool {
        self.title.contains(needle) || self.body.contains(needle)
    }
}

/// Queue statistics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueueStats {
    /// Queued entries.
    pub queued: usize,
    /// Running entries.
    pub running: usize,
    /// Done entries.
    pub done: usize,
    /// Failed entries.
    pub failed: usize,
    /// Cancelled entries.
    pub cancelled: usize,
}

/// The transfer queue.
#[derive(Debug, Clone, Default)]
pub struct TransferQueue {
    entries: HashMap<u64, QueueEntry>,
    order: Vec<u64>,
    next_id: u64,
    retry_policy: RetryPolicy,
}

/// Why a queue operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    /// Unknown entry id.
    NotFound,
    /// The transition is not allowed in the current state.
    InvalidState,
}

impl TransferQueue {
    /// A queue with a retry policy.
    pub fn new(retry_policy: RetryPolicy) -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
            next_id: 0,
            retry_policy,
        }
    }

    /// Enqueues a transfer; returns its id.
    pub fn enqueue(&mut self, label: &str, source: &str, destination: &str, total: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.insert(
            id,
            QueueEntry {
                id,
                label: label.to_owned(),
                source: source.to_owned(),
                destination: destination.to_owned(),
                state: QueueState::Queued,
                progress: QueueProgress { bytes: 0, total },
                retries: 0,
                failure: None,
            },
        );
        self.order.push(id);
        id
    }

    /// Reads an entry.
    pub fn get(&self, id: u64) -> Option<&QueueEntry> {
        self.entries.get(&id)
    }

    /// All entries in creation order.
    pub fn list(&self) -> Vec<&QueueEntry> {
        self.order
            .iter()
            .filter_map(|id| self.entries.get(id))
            .collect()
    }

    /// The queue statistics.
    pub fn stats(&self) -> QueueStats {
        let mut stats = QueueStats::default();
        for entry in self.list() {
            match entry.state {
                QueueState::Queued => stats.queued += 1,
                QueueState::Running => stats.running += 1,
                QueueState::Done => stats.done += 1,
                QueueState::Failed => stats.failed += 1,
                QueueState::Cancelled => stats.cancelled += 1,
            }
        }
        stats
    }

    /// Starts a queued entry in the background.
    pub fn start(&mut self, id: u64) -> Result<(), QueueError> {
        let entry = self.entries.get_mut(&id).ok_or(QueueError::NotFound)?;
        if entry.state != QueueState::Queued {
            return Err(QueueError::InvalidState);
        }
        entry.state = QueueState::Running;
        Ok(())
    }

    /// Advances background progress.
    pub fn advance(&mut self, id: u64, bytes: u64) -> Result<(), QueueError> {
        let entry = self.entries.get_mut(&id).ok_or(QueueError::NotFound)?;
        if entry.state != QueueState::Running {
            return Err(QueueError::InvalidState);
        }
        entry.progress.bytes = entry
            .progress
            .bytes
            .saturating_add(bytes)
            .min(entry.progress.total);
        Ok(())
    }

    /// Marks an entry done.
    pub fn complete(&mut self, id: u64) -> Result<(), QueueError> {
        let entry = self.entries.get_mut(&id).ok_or(QueueError::NotFound)?;
        if entry.state != QueueState::Running {
            return Err(QueueError::InvalidState);
        }
        entry.state = QueueState::Done;
        entry.progress.bytes = entry.progress.total;
        entry.failure = None;
        Ok(())
    }

    /// Fails an entry. Transient failures auto-retry under the policy;
    /// permanent failures go straight to Failed.
    pub fn fail(&mut self, id: u64, kind: FailureKind, message: &str) -> Result<(), QueueError> {
        let entry = self.entries.get_mut(&id).ok_or(QueueError::NotFound)?;
        if entry.state != QueueState::Running {
            return Err(QueueError::InvalidState);
        }
        let attempts = match self.retry_policy {
            RetryPolicy::None => 0,
            RetryPolicy::Fixed { attempts, .. } => attempts,
        };
        if kind == FailureKind::Transient && entry.retries < attempts {
            entry.retries += 1;
            entry.failure = Some(Failure {
                kind,
                message: message.to_owned(),
            });
            entry.state = QueueState::Queued;
            entry.progress.bytes = 0;
            Ok(())
        } else {
            entry.failure = Some(Failure {
                kind,
                message: message.to_owned(),
            });
            entry.state = QueueState::Failed;
            Ok(())
        }
    }

    /// Manually retries a failed/cancelled entry: reuses the same id
    /// (no duplicate submission).
    pub fn retry(&mut self, id: u64) -> Result<(), QueueError> {
        let entry = self.entries.get_mut(&id).ok_or(QueueError::NotFound)?;
        if !matches!(entry.state, QueueState::Failed | QueueState::Cancelled) {
            return Err(QueueError::InvalidState);
        }
        entry.state = QueueState::Queued;
        entry.progress.bytes = 0;
        entry.failure = None;
        Ok(())
    }

    /// Cancels an entry (retryable).
    pub fn cancel(&mut self, id: u64) -> Result<(), QueueError> {
        let entry = self.entries.get_mut(&id).ok_or(QueueError::NotFound)?;
        if !matches!(entry.state, QueueState::Queued | QueueState::Running) {
            return Err(QueueError::InvalidState);
        }
        entry.state = QueueState::Cancelled;
        Ok(())
    }

    /// Builds a system notification from the **safe label only** (never the
    /// source/destination paths, so secrets cannot leak).
    pub fn notification_for(&self, id: u64) -> Option<TransferNotification> {
        let entry = self.entries.get(&id)?;
        let (title, body) = match entry.state {
            QueueState::Done => (
                "Transfer completed".to_owned(),
                format!("{} finished at 100%.", entry.label),
            ),
            QueueState::Failed => (
                "Transfer failed".to_owned(),
                format!(
                    "{} failed: {}",
                    entry.label,
                    entry
                        .failure
                        .as_ref()
                        .map(|failure| failure.message.as_str())
                        .unwrap_or("unknown error")
                ),
            ),
            QueueState::Running => (
                "Transfer in progress".to_owned(),
                format!("{} at {}%.", entry.label, entry.progress.percent()),
            ),
            _ => return None,
        };
        Some(TransferNotification { title, body })
    }
}

#[cfg(test)]
mod tests {
    use super::{FailureKind, QueueState, RetryPolicy, TransferQueue};

    #[test]
    fn queue_lifecycle_and_stats() {
        let mut queue = TransferQueue::new(RetryPolicy::None);
        let a = queue.enqueue("A", "/s/a", "/d/a", 100);
        let b = queue.enqueue("B", "/s/b", "/d/b", 200);
        let c = queue.enqueue("C", "/s/c", "/d/c", 300);
        queue.start(a).unwrap();
        queue.start(b).unwrap();
        queue.complete(a).unwrap();
        queue.start(c).unwrap();
        queue
            .fail(c, FailureKind::Permanent, "auth rejected")
            .unwrap();
        let stats = queue.stats();
        assert_eq!(stats.done, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.running, 1);
        assert_eq!(queue.get(b).unwrap().state, QueueState::Running);
    }

    #[test]
    fn cancel_retry_do_not_duplicate() {
        let mut queue = TransferQueue::new(RetryPolicy::None);
        let id = queue.enqueue("A", "/s/a", "/d/a", 50);
        queue.start(id).unwrap();
        queue.cancel(id).unwrap();
        assert_eq!(queue.get(id).unwrap().state, QueueState::Cancelled);
        queue.retry(id).unwrap();
        assert_eq!(queue.list().len(), 1, "retry reuses the same id");
        queue.start(id).unwrap();
        queue.fail(id, FailureKind::Permanent, "boom").unwrap();
        queue.retry(id).unwrap();
        assert_eq!(queue.list().len(), 1, "no duplicate submission on retry");
    }

    #[test]
    fn transient_failures_auto_retry_under_policy() {
        let mut queue = TransferQueue::new(RetryPolicy::Fixed {
            attempts: 2,
            backoff_secs: 5,
        });
        let id = queue.enqueue("A", "/s/a", "/d/a", 10);
        queue.start(id).unwrap();
        // First transient failure: auto-requeued (retries=1).
        queue.fail(id, FailureKind::Transient, "timeout").unwrap();
        assert_eq!(queue.get(id).unwrap().state, QueueState::Queued);
        assert_eq!(queue.get(id).unwrap().retries, 1);
        queue.start(id).unwrap();
        // Second transient failure: still within attempts (retries=2).
        queue.fail(id, FailureKind::Transient, "timeout").unwrap();
        assert_eq!(queue.get(id).unwrap().state, QueueState::Queued);
        assert_eq!(queue.get(id).unwrap().retries, 2);
        queue.start(id).unwrap();
        // Third failure exceeds attempts: goes to Failed.
        queue.fail(id, FailureKind::Transient, "timeout").unwrap();
        assert_eq!(queue.get(id).unwrap().state, QueueState::Failed);
        assert_eq!(queue.get(id).unwrap().retries, 2);
    }

    #[test]
    fn permanent_failure_does_not_auto_retry() {
        let mut queue = TransferQueue::new(RetryPolicy::Fixed {
            attempts: 3,
            backoff_secs: 1,
        });
        let id = queue.enqueue("A", "/s/a", "/d/a", 10);
        queue.start(id).unwrap();
        queue
            .fail(id, FailureKind::Permanent, "host key rejected")
            .unwrap();
        assert_eq!(queue.get(id).unwrap().state, QueueState::Failed);
        assert_eq!(
            queue.get(id).unwrap().retries,
            0,
            "permanent failures never auto-retry"
        );
    }

    #[test]
    fn notifications_do_not_leak_secrets() {
        let mut queue = TransferQueue::new(RetryPolicy::None);
        // The source path carries a secret; the notification must not.
        let secret = "TOKEN_XYZ_123";
        let id = queue.enqueue(
            "backup transfer",
            &format!("/secrets/{secret}/backup.tar"),
            "/local/backup.tar",
            500,
        );
        queue.start(id).unwrap();
        queue.advance(id, 250).unwrap();
        let notification = queue.notification_for(id).unwrap();
        assert!(
            !notification.contains(secret),
            "notification must not leak the source path secret"
        );
        assert!(notification.body.contains("50%"));
        queue.complete(id).unwrap();
        let done = queue.notification_for(id).unwrap();
        assert!(!done.contains(secret));
        assert!(done.title.contains("completed"));
        // Queued entries produce no notification.
        let queued = queue.enqueue("idle", "/s/x", "/d/x", 1);
        assert!(queue.notification_for(queued).is_none());
    }

    #[test]
    fn background_progress_advances() {
        let mut queue = TransferQueue::new(RetryPolicy::None);
        let id = queue.enqueue("A", "/s/a", "/d/a", 100);
        queue.start(id).unwrap();
        queue.advance(id, 25).unwrap();
        assert_eq!(queue.get(id).unwrap().progress.percent(), 25);
        queue.advance(id, 200).unwrap();
        assert_eq!(queue.get(id).unwrap().progress.percent(), 100);
    }
}
