use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

/// How a bounded channel treats messages when its buffer is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlowConsumerPolicy {
    /// Apply backpressure: `send` awaits capacity. Never drops data, but a
    /// producer can block (use where loss is unacceptable, e.g. input path).
    Block,
    /// Never block the producer: drop the newest message when full.
    DropNewest,
    /// Never block the producer: evict the oldest buffered message.
    DropOldest,
}

/// Running counters for a [`BoundedChannel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelStats {
    /// Maximum buffered items.
    pub capacity: usize,
    /// Current buffered items.
    pub len: usize,
    /// Items accepted into the buffer.
    pub total_sent: u64,
    /// Items delivered to consumers.
    pub total_received: u64,
    /// Items discarded at the door by the `DropNewest` policy (never counted
    /// in `total_sent`).
    pub dropped_newest: u64,
    /// Items evicted by the `DropOldest` policy (counted in `total_sent`).
    pub dropped_oldest: u64,
}

#[derive(Debug)]
struct State<T> {
    queue: VecDeque<T>,
    closed: bool,
    total_sent: u64,
    total_received: u64,
    dropped_newest: u64,
    dropped_oldest: u64,
}

#[derive(Debug)]
struct Inner<T> {
    capacity: usize,
    policy: SlowConsumerPolicy,
    state: Mutex<State<T>>,
    has_item: Notify,
    has_space: Notify,
}

/// A bounded, multi-producer multi-consumer message channel with close.
///
/// Memory is bounded by `capacity` regardless of producer rate. With the
/// `DropNewest`/`DropOldest` policies the producer never blocks, so a fast
/// network reader cannot be stalled by a slow UI consumer; overflow is
/// counted and observable through [`BoundedChannel::stats`]. `close` wakes
/// blocked receivers, which then drain remaining items and observe `None`.
#[derive(Clone, Debug)]
pub struct BoundedChannel<T> {
    inner: Arc<Inner<T>>,
}

impl<T> BoundedChannel<T> {
    /// Creates a channel with the given capacity and slow-consumer policy.
    ///
    /// `capacity` must be at least 1.
    pub fn new(capacity: usize, policy: SlowConsumerPolicy) -> Self {
        assert!(capacity >= 1, "bounded channel capacity must be at least 1");
        Self {
            inner: Arc::new(Inner {
                capacity,
                policy,
                state: Mutex::new(State {
                    queue: VecDeque::with_capacity(capacity),
                    closed: false,
                    total_sent: 0,
                    total_received: 0,
                    dropped_newest: 0,
                    dropped_oldest: 0,
                }),
                has_item: Notify::new(),
                has_space: Notify::new(),
            }),
        }
    }

    /// Returns the channel capacity.
    pub fn capacity(&self) -> usize {
        self.inner.capacity
    }

    /// Returns the number of buffered items.
    pub fn len(&self) -> usize {
        self.inner
            .state
            .lock()
            .expect("channel state lock")
            .queue
            .len()
    }

    /// Returns whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns whether the channel has been closed.
    pub fn is_closed(&self) -> bool {
        self.inner.state.lock().expect("channel state lock").closed
    }

    /// Closes the channel: sends are rejected and blocked receivers wake and
    /// observe `None` once the buffer is drained.
    pub fn close(&self) {
        let mut state = self.inner.state.lock().expect("channel state lock");
        state.closed = true;
        drop(state);
        self.inner.has_item.notify_waiters();
        self.inner.has_space.notify_waiters();
    }

    /// Returns running counters.
    pub fn stats(&self) -> ChannelStats {
        let state = self.inner.state.lock().expect("channel state lock");
        ChannelStats {
            capacity: self.inner.capacity,
            len: state.queue.len(),
            total_sent: state.total_sent,
            total_received: state.total_received,
            dropped_newest: state.dropped_newest,
            dropped_oldest: state.dropped_oldest,
        }
    }

    /// Attempts to enqueue without blocking or dropping.
    ///
    /// Returns `Err(item)` if the buffer is full or the channel is closed.
    pub fn try_send(&self, item: T) -> Result<(), T> {
        let mut state = self.inner.state.lock().expect("channel state lock");
        if state.closed || state.queue.len() >= self.inner.capacity {
            return Err(item);
        }
        state.total_sent += 1;
        state.queue.push_back(item);
        drop(state);
        self.inner.has_item.notify_one();
        Ok(())
    }

    /// Delivers `item` according to the slow-consumer policy.
    ///
    /// - `Block`: awaits capacity (backpressure); returns `false` if closed.
    /// - `DropNewest`: never blocks; discards `item` when full.
    /// - `DropOldest`: never blocks; evicts the oldest buffered item.
    ///
    /// Returns `false` when the channel is closed and the item was rejected.
    pub async fn send(&self, item: T) -> bool {
        match self.inner.policy {
            SlowConsumerPolicy::Block => {
                let mut item = item;
                loop {
                    match self.try_send(item) {
                        Ok(()) => return true,
                        Err(returned) => {
                            if self.is_closed() {
                                return false;
                            }
                            item = returned;
                            self.wait_for_space().await;
                        }
                    }
                }
            }
            SlowConsumerPolicy::DropNewest => {
                let mut state = self.inner.state.lock().expect("channel state lock");
                if state.closed {
                    return false;
                }
                if state.queue.len() >= self.inner.capacity {
                    state.dropped_newest += 1;
                    return true;
                }
                state.total_sent += 1;
                state.queue.push_back(item);
                drop(state);
                self.inner.has_item.notify_one();
                true
            }
            SlowConsumerPolicy::DropOldest => {
                let mut state = self.inner.state.lock().expect("channel state lock");
                if state.closed {
                    return false;
                }
                if state.queue.len() >= self.inner.capacity {
                    state.queue.pop_front();
                    state.dropped_oldest += 1;
                }
                state.total_sent += 1;
                state.queue.push_back(item);
                drop(state);
                self.inner.has_item.notify_one();
                true
            }
        }
    }

    /// Receives the next item, waiting until one is available.
    ///
    /// Returns `None` after the channel is closed and the buffer is drained.
    pub async fn recv(&self) -> Option<T> {
        let mut notified = std::pin::pin!(self.inner.has_item.notified());
        loop {
            {
                let mut state = self.inner.state.lock().expect("channel state lock");
                if let Some(item) = state.queue.pop_front() {
                    state.total_received += 1;
                    drop(state);
                    self.inner.has_space.notify_waiters();
                    return Some(item);
                }
                if state.closed {
                    return None;
                }
            }
            notified.as_mut().await;
            notified.set(self.inner.has_item.notified());
        }
    }

    /// Attempts to receive without blocking.
    pub fn try_recv(&self) -> Option<T> {
        let mut state = self.inner.state.lock().expect("channel state lock");
        let item = state.queue.pop_front()?;
        state.total_received += 1;
        drop(state);
        self.inner.has_space.notify_waiters();
        Some(item)
    }

    async fn wait_for_space(&self) {
        let mut notified = std::pin::pin!(self.inner.has_space.notified());
        loop {
            {
                let state = self.inner.state.lock().expect("channel state lock");
                let has_space = state.queue.len() < self.inner.capacity;
                drop(state);
                if has_space {
                    return;
                }
            }
            notified.as_mut().await;
            notified.set(self.inner.has_space.notified());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BoundedChannel, ChannelStats, SlowConsumerPolicy};

    const MESSAGE_BYTES: usize = 1024;
    const GIB_MESSAGES: u64 = 1_048_576; // 1024 bytes * 1,048,576 = 1 GiB

    #[test]
    fn try_send_respects_capacity_without_blocking() {
        let channel = BoundedChannel::new(2, SlowConsumerPolicy::DropNewest);
        assert_eq!(channel.try_send(1), Ok(()));
        assert_eq!(channel.try_send(2), Ok(()));
        assert_eq!(channel.try_send(3), Err(3));
        assert_eq!(channel.len(), 2);
        assert_eq!(channel.try_recv(), Some(1));
        assert_eq!(channel.try_recv(), Some(2));
        assert_eq!(channel.try_recv(), None);
        let stats = channel.stats();
        assert_eq!(stats.total_sent, 2);
        assert_eq!(stats.total_received, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn block_policy_applies_backpressure() {
        let channel = BoundedChannel::new(2, SlowConsumerPolicy::Block);
        assert!(channel.send(1).await);
        assert!(channel.send(2).await);
        let sender = tokio::spawn({
            let channel = channel.clone();
            async move {
                channel.send(3).await;
            }
        });
        tokio::task::yield_now().await;
        assert_eq!(
            channel.len(),
            2,
            "third send must be blocked by backpressure"
        );
        assert_eq!(channel.recv().await, Some(1));
        sender
            .await
            .expect("blocked sender completes after space frees");
        assert_eq!(channel.len(), 2);
        assert_eq!(channel.recv().await, Some(2));
        assert_eq!(channel.recv().await, Some(3));
        let stats = channel.stats();
        assert_eq!(stats.total_sent, 3);
        assert_eq!(stats.total_received, 3);
        assert_eq!(stats.dropped_newest, 0);
        assert_eq!(stats.dropped_oldest, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn drop_newest_never_blocks_and_counts_drops() {
        let channel = BoundedChannel::new(2, SlowConsumerPolicy::DropNewest);
        assert!(channel.send(1).await);
        assert!(channel.send(2).await);
        assert!(channel.send(3).await);
        assert!(channel.send(4).await);
        assert_eq!(channel.recv().await, Some(1));
        assert_eq!(channel.recv().await, Some(2));
        let stats = channel.stats();
        assert_eq!(stats.total_sent, 2);
        assert_eq!(stats.total_received, 2);
        assert_eq!(stats.dropped_newest, 2);
        assert_eq!(stats.dropped_oldest, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn drop_oldest_evicts_oldest_and_counts_evictions() {
        let channel = BoundedChannel::new(2, SlowConsumerPolicy::DropOldest);
        assert!(channel.send(1).await);
        assert!(channel.send(2).await);
        assert!(channel.send(3).await);
        assert!(channel.send(4).await);
        assert_eq!(channel.recv().await, Some(3), "1 and 2 must be evicted");
        assert_eq!(channel.recv().await, Some(4));
        let stats = channel.stats();
        assert_eq!(stats.total_sent, 4);
        assert_eq!(stats.total_received, 2);
        assert_eq!(stats.dropped_oldest, 2);
        assert_eq!(stats.dropped_newest, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn close_wakes_blocked_receivers_and_send_fails() {
        let channel = BoundedChannel::new(1, SlowConsumerPolicy::Block);
        let waiter = tokio::spawn({
            let channel = channel.clone();
            async move { channel.recv().await }
        });
        tokio::task::yield_now().await;
        channel.close();
        assert_eq!(waiter.await.expect("receiver wakes on close"), None);
        assert!(channel.is_closed());
        assert_eq!(channel.try_send(1), Err(1));
        assert!(!channel.send(1).await);
    }

    #[tokio::test(start_paused = true)]
    async fn close_drains_remaining_items_before_none() {
        let channel = BoundedChannel::new(4, SlowConsumerPolicy::Block);
        channel.send(1).await;
        channel.send(2).await;
        channel.close();
        assert_eq!(channel.recv().await, Some(1));
        assert_eq!(channel.recv().await, Some(2));
        assert_eq!(channel.recv().await, None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn one_gib_synthetic_output_stays_bounded_and_non_blocking() {
        let channel = BoundedChannel::new(1024, SlowConsumerPolicy::DropNewest);
        let producer = tokio::spawn({
            let channel = channel.clone();
            async move {
                let item = vec![0u8; MESSAGE_BYTES];
                for _ in 0..GIB_MESSAGES {
                    // Non-blocking: DropNewest discards overflow immediately.
                    let _accepted = channel.send(item.clone()).await;
                }
            }
        });
        let consumer = tokio::spawn({
            let channel = channel.clone();
            async move {
                let mut count = 0u64;
                while count < 2048 {
                    match channel.recv().await {
                        Some(_item) => count += 1,
                        None => break,
                    }
                    // Deliberately slow consumer.
                    tokio::task::yield_now().await;
                }
                count
            }
        });
        producer.await.expect("producer completes without blocking");
        channel.close();
        let consumed = consumer.await.expect("consumer completes");
        let stats: ChannelStats = channel.stats();
        assert!(
            stats.dropped_newest > 0,
            "slow consumer must force overflow drops (dropped_newest={})",
            stats.dropped_newest
        );
        assert!(
            stats.len <= stats.capacity,
            "buffer must stay bounded (len={}, capacity={})",
            stats.len,
            stats.capacity
        );
        assert_eq!(
            stats.total_sent + stats.dropped_newest,
            GIB_MESSAGES,
            "every produced message is either accepted or dropped at the door"
        );
        assert_eq!(stats.total_received, consumed);
        assert_eq!(
            stats.total_sent,
            stats.total_received + stats.dropped_oldest + stats.len as u64,
            "accepted messages are delivered, evicted, or still buffered"
        );
    }

    #[test]
    fn stats_are_consistent_for_mixed_operations() {
        let channel = BoundedChannel::new(3, SlowConsumerPolicy::DropOldest);
        assert_eq!(channel.capacity(), 3);
        channel.try_send("a").unwrap();
        channel.try_send("b").unwrap();
        channel.try_send("c").unwrap();
        assert_eq!(channel.try_send("d"), Err("d"));
        assert_eq!(channel.try_recv(), Some("a"));
        let stats = channel.stats();
        assert_eq!(stats.len, 2);
        assert_eq!(stats.total_received, 1);
        assert_eq!(stats.total_sent, 3);
    }

    #[test]
    fn capacity_one_is_supported() {
        let channel = BoundedChannel::new(1, SlowConsumerPolicy::DropNewest);
        assert!(channel.try_send(1).is_ok());
        assert_eq!(channel.len(), 1);
        assert_eq!(channel.try_send(2), Err(2));
    }
}
