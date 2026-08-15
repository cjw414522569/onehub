//! Per-platform UI scheduler adapters (T099).
//!
//! The producer dispatches [`EventBatch`]es to the UI thread through a
//! non-blocking [`Scheduler`]: a full UI queue returns `false`
//! (backpressure), and the UI drains with [`Scheduler::poll`]. Windows-first:
//! [`WindowsUiScheduler`] is the implemented adapter (the deterministic
//! in-memory dispatch model; the real Win32 message-loop posting is a
//! blocked_environment binding on hosts without a native loop). Other
//! platforms keep interface-only boundaries.

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::event_stream::EventBatch;

/// A UI-thread scheduler contract: non-blocking dispatch, poll-based drain.
pub trait Scheduler: Send + Sync {
    /// The platform name.
    fn name(&self) -> &'static str;
    /// Dispatches a batch to the UI queue. Returns `false` (backpressure)
    /// when the UI is slow and the queue is full.
    fn dispatch(&self, batch: EventBatch) -> bool;
    /// The UI thread pulls the next batch.
    fn poll(&self) -> Option<EventBatch>;
    /// Queued batches awaiting the UI thread.
    fn queued(&self) -> usize;
}

/// A deterministic, memory-backed UI scheduler (test double / model).
#[derive(Debug)]
pub struct UiScheduler {
    name: &'static str,
    capacity: usize,
    queue: Mutex<VecDeque<EventBatch>>,
}

impl UiScheduler {
    /// A scheduler with a bounded UI queue.
    pub fn new(name: &'static str, capacity: usize) -> Self {
        Self {
            name,
            capacity: capacity.max(1),
            queue: Mutex::new(VecDeque::new()),
        }
    }
}

impl Scheduler for UiScheduler {
    fn name(&self) -> &'static str {
        self.name
    }

    fn dispatch(&self, batch: EventBatch) -> bool {
        let mut queue = self.queue.lock().expect("ui queue lock");
        if queue.len() >= self.capacity {
            return false;
        }
        queue.push_back(batch);
        true
    }

    fn poll(&self) -> Option<EventBatch> {
        self.queue.lock().expect("ui queue lock").pop_front()
    }

    fn queued(&self) -> usize {
        self.queue.lock().expect("ui queue lock").len()
    }
}

/// The Windows UI scheduler adapter (Windows-first). Dispatch is non-blocking;
/// on hosts without a native Win32 message loop the deterministic in-memory
/// model verifies the contract.
#[derive(Debug)]
pub struct WindowsUiScheduler {
    inner: UiScheduler,
}

impl WindowsUiScheduler {
    /// The Windows adapter with a bounded UI queue.
    pub fn new() -> Self {
        Self {
            inner: UiScheduler::new("windows", 8),
        }
    }
}

impl Default for WindowsUiScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler for WindowsUiScheduler {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn dispatch(&self, batch: EventBatch) -> bool {
        self.inner.dispatch(batch)
    }

    fn poll(&self) -> Option<EventBatch> {
        self.inner.poll()
    }

    fn queued(&self) -> usize {
        self.inner.queued()
    }
}

#[cfg(test)]
mod tests {
    use super::{Scheduler, UiScheduler, WindowsUiScheduler};
    use crate::event_stream::{BatchItem, EventBatch, EVENT_BATCH_VERSION};

    fn batch(sequence: u64) -> EventBatch {
        EventBatch {
            version: EVENT_BATCH_VERSION,
            sequence,
            items: vec![BatchItem::Event(vec![sequence as u8; 4])],
            total_bytes: 4,
            dropped: 0,
        }
    }

    #[test]
    fn scheduler_dispatch_and_poll_round_trip() {
        let scheduler = UiScheduler::new("memory", 4);
        assert!(scheduler.dispatch(batch(1)));
        assert!(scheduler.dispatch(batch(2)));
        assert_eq!(scheduler.queued(), 2);
        assert_eq!(scheduler.poll().unwrap().sequence, 1);
        assert_eq!(scheduler.poll().unwrap().sequence, 2);
        assert_eq!(scheduler.poll(), None);
        assert_eq!(scheduler.queued(), 0);
    }

    #[test]
    fn scheduler_backpressure_when_ui_is_full() {
        let scheduler = UiScheduler::new("memory", 2);
        assert!(scheduler.dispatch(batch(1)));
        assert!(scheduler.dispatch(batch(2)));
        // Full: the producer is told to back off (never blocks).
        assert!(!scheduler.dispatch(batch(3)));
        assert_eq!(scheduler.queued(), 2);
        // The UI drains and accepts again.
        scheduler.poll();
        assert!(scheduler.dispatch(batch(3)));
    }

    #[test]
    fn windows_scheduler_adapter_dispatches() {
        let scheduler = WindowsUiScheduler::new();
        assert_eq!(scheduler.name(), "windows");
        assert!(scheduler.dispatch(batch(1)));
        assert_eq!(scheduler.poll().unwrap().sequence, 1);
    }
}
