//! Batch event streams with backpressure and snapshot recovery (T099).
//!
//! Events never cross the ABI one character at a time: they accumulate into
//! versioned [`EventBatch`]es (a batch is the ABI transfer unit) and are
//! flushed when a size or count threshold is reached. A slow consumer (full
//! queue) applies **backpressure** without blocking the producer: the oldest
//! queued batch is dropped, `dropped` is counted, and the next flushed batch
//! carries a [`BatchItem::SnapshotRequired`] marker. The consumer then asks
//! for [`BatchItem::Snapshot`] via [`EventStream::produce_snapshot`] and
//! rebuilds the full state — a stalled UI recovers from a snapshot instead of
//! missing updates forever.

use std::collections::VecDeque;

/// The batch format version.
pub const EVENT_BATCH_VERSION: u8 = 1;
/// Max events per batch before an automatic flush.
pub const EVENT_BATCH_MAX_EVENTS: usize = 64;
/// Max payload bytes per batch before an automatic flush.
pub const EVENT_BATCH_MAX_BYTES: usize = 4096;

/// One item inside a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchItem {
    /// An opaque encoded event (the smallest unit that ever crosses the ABI
    /// is a batch, never a single character).
    Event(Vec<u8>),
    /// Backpressure dropped events; the consumer must request a snapshot.
    SnapshotRequired,
    /// A full-state snapshot payload for recovery after a stall.
    Snapshot(Vec<u8>),
}

/// A versioned batch — the unit that crosses the ABI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventBatch {
    /// Batch format version.
    pub version: u8,
    /// Monotonic batch sequence.
    pub sequence: u64,
    /// The items in this batch.
    pub items: Vec<BatchItem>,
    /// Sum of payload bytes in this batch.
    pub total_bytes: u64,
    /// Events dropped by backpressure up to this batch.
    pub dropped: u64,
}

/// The outcome of pushing one event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PushResult {
    /// Whether the pending events were flushed into a batch.
    pub flushed: bool,
    /// Total events dropped by backpressure so far.
    pub dropped: u64,
    /// Whether the consumer must recover via snapshot.
    pub snapshot_required: bool,
}

/// A bounded, non-blocking batch event stream with snapshot recovery.
#[derive(Debug, Clone)]
pub struct EventStream {
    capacity: usize,
    queue: VecDeque<EventBatch>,
    pending: Vec<BatchItem>,
    pending_bytes: usize,
    sequence: u64,
    dropped: u64,
    needs_snapshot: bool,
}

impl EventStream {
    /// A stream that queues at most `capacity` batches before dropping the
    /// oldest (backpressure) and requiring a snapshot.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            queue: VecDeque::new(),
            pending: Vec::new(),
            pending_bytes: 0,
            sequence: 0,
            dropped: 0,
            needs_snapshot: false,
        }
    }

    /// Pushes one event. Never blocks: when the queue is full, the oldest
    /// batch is dropped and the consumer is told to recover via snapshot.
    pub fn push_event(&mut self, event: Vec<u8>) -> PushResult {
        self.pending_bytes += event.len();
        self.pending.push(BatchItem::Event(event));
        let mut result = PushResult::default();
        if self.pending.len() >= EVENT_BATCH_MAX_EVENTS
            || self.pending_bytes >= EVENT_BATCH_MAX_BYTES
        {
            result.flushed = self.flush();
        }
        result.dropped = self.dropped;
        result.snapshot_required = self.needs_snapshot;
        result
    }

    /// Flushes the pending events into a batch. When the queue is full the
    /// oldest batch is dropped (bounded memory) and a snapshot is required.
    /// Returns whether anything was flushed.
    pub fn flush(&mut self) -> bool {
        if self.pending.is_empty() && !self.needs_snapshot {
            return false;
        }
        let mut items = std::mem::take(&mut self.pending);
        self.pending_bytes = 0;
        if self.needs_snapshot {
            items.insert(0, BatchItem::SnapshotRequired);
        }
        let total_bytes = items
            .iter()
            .map(|item| match item {
                BatchItem::Event(bytes) | BatchItem::Snapshot(bytes) => bytes.len(),
                BatchItem::SnapshotRequired => 0,
            })
            .sum::<usize>();
        if self.queue.len() >= self.capacity {
            if let Some(oldest) = self.queue.pop_front() {
                self.dropped += oldest
                    .items
                    .iter()
                    .filter(|item| matches!(item, BatchItem::Event(_)))
                    .count() as u64;
            }
            self.needs_snapshot = true;
        }
        self.queue.push_back(EventBatch {
            version: EVENT_BATCH_VERSION,
            sequence: self.sequence,
            items,
            total_bytes: total_bytes as u64,
            dropped: self.dropped,
        });
        self.sequence = self.sequence.saturating_add(1);
        true
    }

    /// The consumer pulls the next batch (`None` when empty).
    pub fn poll(&mut self) -> Option<EventBatch> {
        self.queue.pop_front()
    }

    /// The consumer requests a full-state snapshot (recovery after a stall).
    /// Resets the dropped counter; returns whether it was enqueued.
    pub fn produce_snapshot(&mut self, snapshot: Vec<u8>) -> bool {
        let mut batch = EventBatch {
            version: EVENT_BATCH_VERSION,
            sequence: self.sequence,
            items: vec![BatchItem::Snapshot(snapshot)],
            total_bytes: 0,
            dropped: 0,
        };
        batch.total_bytes = batch
            .items
            .iter()
            .map(|item| match item {
                BatchItem::Event(bytes) | BatchItem::Snapshot(bytes) => bytes.len(),
                BatchItem::SnapshotRequired => 0,
            })
            .sum::<usize>() as u64;
        if self.queue.len() >= self.capacity {
            self.queue.pop_front();
        }
        self.queue.push_back(batch);
        self.sequence = self.sequence.saturating_add(1);
        self.needs_snapshot = false;
        self.dropped = 0;
        true
    }

    /// Queued (flushed but not yet polled) batches.
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// Unflushed pending events.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Total events dropped by backpressure (reset by a snapshot).
    pub fn dropped_total(&self) -> u64 {
        self.dropped
    }

    /// Whether the consumer must recover via snapshot.
    pub fn needs_snapshot(&self) -> bool {
        self.needs_snapshot
    }

    /// The configured queue capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Whether nothing is queued or pending.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty() && self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{BatchItem, EventBatch, EventStream, EVENT_BATCH_MAX_EVENTS};

    #[test]
    fn events_are_batched_never_per_character() {
        let mut stream = EventStream::new(32);
        // 1000 single-character events: batching must collapse them into
        // ~ceil(1000/64) batches, never 1000 transfers.
        for value in 1..=1000u64 {
            stream.push_event(value.to_le_bytes().to_vec());
        }
        stream.flush();
        let batches: Vec<EventBatch> = {
            let mut out = Vec::new();
            while let Some(batch) = stream.poll() {
                out.push(batch);
            }
            out
        };
        assert!(
            batches.len() < 1000 / 16,
            "batches must be far fewer than events: {} vs 1000",
            batches.len()
        );
        assert_eq!(batches.len(), 1000_usize.div_ceil(EVENT_BATCH_MAX_EVENTS));
        for batch in &batches {
            assert_eq!(batch.version, super::EVENT_BATCH_VERSION);
            assert!(batch.total_bytes >= 8 * batch.items.len() as u64);
        }
        let values: Vec<u64> = batches
            .iter()
            .flat_map(|batch| batch.items.iter())
            .filter_map(|item| match item {
                BatchItem::Event(bytes) => {
                    Some(u64::from_le_bytes(bytes.as_slice().try_into().unwrap()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(values.len(), 1000, "no events lost when the UI keeps up");
        assert_eq!(values.last(), Some(&1000));
    }

    #[test]
    fn backpressure_drops_and_requests_snapshot() {
        let mut stream = EventStream::new(2);
        let mut latest = 0u64;
        let mut saw_snapshot_required = false;
        for _ in 0..1000 {
            latest += 1;
            stream.push_event(latest.to_le_bytes().to_vec());
        }
        assert!(
            stream.dropped_total() > 0,
            "a flood past capacity must drop events"
        );
        while let Some(batch) = stream.poll() {
            if batch.items.contains(&BatchItem::SnapshotRequired) {
                saw_snapshot_required = true;
            }
        }
        assert!(
            saw_snapshot_required,
            "dropped events must request a snapshot"
        );
    }

    #[test]
    fn slow_ui_recovers_via_snapshot() {
        let mut stream = EventStream::new(2);
        let mut latest = 0u64;
        let mut applied = 0u64;
        for _ in 0..1000 {
            latest += 1;
            stream.push_event(latest.to_le_bytes().to_vec());
        }
        let mut saw_snapshot_required = false;
        // Drain with snapshot recovery: on SnapshotRequired, request a
        // snapshot of the latest state; a Snapshot overwrites the applied
        // state, so the UI converges even after drops.
        while let Some(batch) = stream.poll() {
            for item in batch.items {
                match item {
                    BatchItem::Event(bytes) => {
                        applied = u64::from_le_bytes(bytes.try_into().unwrap());
                    }
                    BatchItem::SnapshotRequired => {
                        saw_snapshot_required = true;
                        stream.produce_snapshot(latest.to_le_bytes().to_vec());
                    }
                    BatchItem::Snapshot(bytes) => {
                        applied = u64::from_le_bytes(bytes.try_into().unwrap());
                    }
                }
            }
        }
        assert!(saw_snapshot_required);
        assert_eq!(applied, latest, "snapshot recovery must converge to latest");
        assert_eq!(
            stream.dropped_total(),
            0,
            "snapshot resets the drop counter"
        );
    }

    #[test]
    fn producer_never_blocks_on_stalled_ui() {
        let mut stream = EventStream::new(2);
        // The consumer never polls; pushes must still return immediately
        // (non-blocking) and the queue stays bounded.
        let mut latest = 0u64;
        for _ in 0..10_000 {
            latest += 1;
            let result = stream.push_event(latest.to_le_bytes().to_vec());
            assert!(stream.queue_len() <= stream.capacity());
            let _ = result;
        }
        assert!(
            stream.dropped_total() > 0,
            "a stalled UI must trigger backpressure drops"
        );
        assert!(
            stream.needs_snapshot(),
            "a stalled UI must require snapshot recovery"
        );
        // Once the UI resumes, it recovers via snapshot.
        let mut applied = 0u64;
        let mut saw_snapshot_required = false;
        while let Some(batch) = stream.poll() {
            for item in batch.items {
                match item {
                    BatchItem::Event(bytes) => {
                        applied = u64::from_le_bytes(bytes.try_into().unwrap());
                    }
                    BatchItem::SnapshotRequired => {
                        saw_snapshot_required = true;
                        stream.produce_snapshot(latest.to_le_bytes().to_vec());
                    }
                    BatchItem::Snapshot(bytes) => {
                        applied = u64::from_le_bytes(bytes.try_into().unwrap());
                    }
                }
            }
        }
        assert!(saw_snapshot_required);
        assert_eq!(applied, latest, "recovered UI matches the latest state");
    }

    #[test]
    fn flush_requires_thresholds_and_pending_is_bounded() {
        let mut stream = EventStream::new(4);
        for value in 1..=10u64 {
            stream.push_event(value.to_le_bytes().to_vec());
        }
        // 10 events: below the 64-event threshold, so nothing flushed yet.
        assert_eq!(stream.pending_len(), 10);
        assert_eq!(stream.queue_len(), 0);
        stream.flush();
        assert_eq!(stream.queue_len(), 1);
        assert_eq!(stream.pending_len(), 0);
    }
}
