//! Multi-channel fair scheduling and flow-window management (T049).
//!
//! SSH multiplexes many channels over one connection (RFC 4254 §5). A large
//! file transfer (SFTP/SCP) or port-forward stream must never starve an
//! interactive terminal. This module provides:
//!
//! - [`TrafficClass`] classification: Control / Interactive / Bulk.
//! - [`FlowWindow`] per-channel send-window tracking (RFC 4254 §5.2).
//! - [`Scheduler`]: strict priority across classes plus deficit round robin
//!   (DRR) among bulk channels, with a bounded round budget and deterministic
//!   id-ordered iteration.
//!
//! The scheduler is pure (no I/O), so the fairness and window guarantees can be
//! verified deterministically and reused by any backend (russh/libssh/OpenSSH).

use std::collections::{BTreeMap, VecDeque};

/// Traffic class of a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrafficClass {
    /// Channel management / window control — always served first.
    Control,
    /// Interactive terminal or shell — latency sensitive, small volume.
    Interactive,
    /// Bulk data (SFTP, port forwarding, large upload/download).
    Bulk,
}

impl TrafficClass {
    /// Priority rank; lower is served first.
    pub const fn priority(self) -> u8 {
        match self {
            TrafficClass::Control => 0,
            TrafficClass::Interactive => 1,
            TrafficClass::Bulk => 2,
        }
    }
}

/// Per-channel SSH send window (RFC 4254 §5.2).
///
/// The receiver advertises how many bytes we may still send; every
/// `WINDOW_ADJUST` from the peer replenishes it. We never allow the window to
/// grow beyond a configured cap, so a misbehaving peer cannot inflate it
/// without bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowWindow {
    /// Bytes the peer still allows us to send.
    remaining: u64,
    /// Hard cap applied on every adjust (anti-inflation).
    max: u64,
    /// Maximum single packet we may emit.
    max_packet: usize,
    /// Whether the peer has ever replenished this window (diagnostics).
    adjusted: bool,
}

impl FlowWindow {
    /// Creates a window with `initial` remaining bytes, capped at `max`.
    pub fn new(initial: u32, max: u32, max_packet: usize) -> Self {
        Self {
            remaining: u64::from(initial),
            max: u64::from(max),
            max_packet,
            adjusted: false,
        }
    }

    /// Bytes still available to send.
    pub fn remaining(&self) -> u64 {
        self.remaining
    }

    /// Whether the peer has ever sent a window adjust.
    pub fn is_adjusted(&self) -> bool {
        self.adjusted
    }

    /// Whether `len` bytes fit in the window and the packet cap.
    pub fn can_send(&self, len: usize) -> bool {
        len > 0 && len <= self.max_packet && (len as u64) <= self.remaining
    }

    /// Consumes `len` bytes from the window after a send.
    pub fn consume(&mut self, len: usize) -> Result<(), QosError> {
        if !self.can_send(len) {
            return Err(QosError::WindowExceeded {
                requested: len,
                remaining: self.remaining,
            });
        }
        self.remaining -= len as u64;
        Ok(())
    }

    /// Replenishes the window by `delta`, saturating at the cap.
    pub fn adjust(&mut self, delta: u32) {
        self.adjusted = true;
        self.remaining = (self.remaining + u64::from(delta)).min(self.max);
    }
}

/// Scheduling and window error (no secret context).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QosError {
    /// The channel id is not registered.
    UnknownChannel(u32),
    /// The channel id is already registered.
    AlreadyRegistered(u32),
    /// The requested send exceeds the flow window.
    WindowExceeded { requested: usize, remaining: u64 },
    /// The scheduler configuration is invalid (zero budgets/windows).
    InvalidConfig,
}

impl core::fmt::Display for QosError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            QosError::UnknownChannel(id) => write!(formatter, "unknown channel {id}"),
            QosError::AlreadyRegistered(id) => write!(formatter, "channel {id} already registered"),
            QosError::WindowExceeded {
                requested,
                remaining,
            } => {
                write!(
                    formatter,
                    "send of {requested} bytes exceeds window {remaining}"
                )
            }
            QosError::InvalidConfig => write!(formatter, "invalid scheduler configuration"),
        }
    }
}

impl core::error::Error for QosError {}

/// Tunables for the scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerConfig {
    /// Bytes a bulk channel may emit per DRR round before yielding.
    pub bulk_quantum: usize,
    /// Max bytes one interactive channel may emit per round.
    pub interactive_quantum: usize,
    /// Max bytes a single `drain` call may authorize.
    pub round_budget: usize,
    /// Initial send window for newly registered channels.
    pub initial_window: u32,
    /// Hard cap applied on every window adjust.
    pub max_window: u32,
    /// Maximum single packet a channel may emit.
    pub max_packet: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            bulk_quantum: 16 * 1024,
            interactive_quantum: 4 * 1024,
            round_budget: 64 * 1024,
            initial_window: 2 * 1024 * 1024,
            max_window: 8 * 1024 * 1024,
            max_packet: 32 * 1024,
        }
    }
}

impl SchedulerConfig {
    /// Validates the configuration; zero budgets/windows are rejected.
    pub fn validate(&self) -> Result<(), QosError> {
        if self.bulk_quantum == 0
            || self.interactive_quantum == 0
            || self.round_budget == 0
            || self.max_packet == 0
            || self.initial_window == 0
            || self.max_window < self.initial_window
        {
            return Err(QosError::InvalidConfig);
        }
        Ok(())
    }
}

/// One authorized send produced by [`Scheduler::drain`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledSend {
    /// Channel id.
    pub channel: u32,
    /// Channel traffic class at scheduling time.
    pub class: TrafficClass,
    /// Bytes authorized for transmission.
    pub bytes: usize,
}

/// Per-channel visible state for diagnostics and QoS benchmarks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelSnapshot {
    /// Channel id.
    pub id: u32,
    /// Traffic class.
    pub class: TrafficClass,
    /// Send-window bytes still available.
    pub window_remaining: u64,
    /// Pending chunks in the queue.
    pub queued_chunks: usize,
    /// Pending bytes in the queue.
    pub queued_bytes: u64,
    /// Bytes authorized so far.
    pub bytes_sent: u64,
    /// Chunks authorized so far.
    pub chunks_sent: u64,
}

/// Aggregate scheduler state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchedulerSnapshot {
    /// Per-channel state in deterministic id order.
    pub channels: Vec<ChannelSnapshot>,
}

impl SchedulerSnapshot {
    /// Snapshot for one channel, if registered.
    pub fn channel(&self, id: u32) -> Option<&ChannelSnapshot> {
        self.channels.iter().find(|channel| channel.id == id)
    }
}

#[derive(Debug, Clone)]
struct ChannelState {
    class: TrafficClass,
    window: FlowWindow,
    deficit: u64,
    bytes_sent: u64,
    chunks_sent: u64,
}

/// Multi-channel fair scheduler with per-channel flow windows.
///
/// Deterministic: channels iterate in id order, DRR among bulk channels uses
/// per-channel deficit, and every `drain` re-runs the priority passes so a
/// newly enqueued interactive byte is always scheduled before bulk bytes.
#[derive(Debug, Clone)]
pub struct Scheduler {
    config: SchedulerConfig,
    channels: BTreeMap<u32, ChannelState>,
    queue: BTreeMap<u32, VecDeque<usize>>,
    queued_bytes: BTreeMap<u32, u64>,
}

impl Scheduler {
    /// Creates a scheduler with `config`.
    pub fn new(config: SchedulerConfig) -> Result<Self, QosError> {
        config.validate()?;
        Ok(Self {
            config,
            channels: BTreeMap::new(),
            queue: BTreeMap::new(),
            queued_bytes: BTreeMap::new(),
        })
    }

    /// Registers a channel with `class` and the default initial window.
    pub fn register(&mut self, id: u32, class: TrafficClass) -> Result<(), QosError> {
        if self.channels.contains_key(&id) {
            return Err(QosError::AlreadyRegistered(id));
        }
        self.channels.insert(
            id,
            ChannelState {
                class,
                window: FlowWindow::new(
                    self.config.initial_window,
                    self.config.max_window,
                    self.config.max_packet,
                ),
                deficit: 0,
                bytes_sent: 0,
                chunks_sent: 0,
            },
        );
        self.queue.insert(id, VecDeque::new());
        self.queued_bytes.insert(id, 0);
        Ok(())
    }

    /// Removes a channel; returns whether it was present.
    pub fn unregister(&mut self, id: u32) -> bool {
        let present = self.channels.remove(&id).is_some();
        self.queue.remove(&id);
        self.queued_bytes.remove(&id);
        present
    }

    /// Changes a channel's traffic class (e.g. SFTP completes -> idle).
    pub fn set_class(&mut self, id: u32, class: TrafficClass) -> Result<(), QosError> {
        let state = self
            .channels
            .get_mut(&id)
            .ok_or(QosError::UnknownChannel(id))?;
        state.class = class;
        Ok(())
    }

    /// Queues `bytes` of pending data for channel `id`.
    pub fn enqueue(&mut self, id: u32, bytes: usize) -> Result<(), QosError> {
        if !self.channels.contains_key(&id) {
            return Err(QosError::UnknownChannel(id));
        }
        if bytes == 0 {
            return Ok(());
        }
        self.queue
            .get_mut(&id)
            .expect("queue exists")
            .push_back(bytes);
        *self.queued_bytes.get_mut(&id).expect("queued bytes exists") += bytes as u64;
        Ok(())
    }

    /// Applies a peer `WINDOW_ADJUST` of `delta` bytes to channel `id`.
    pub fn window_adjust(&mut self, id: u32, delta: u32) -> Result<(), QosError> {
        let state = self
            .channels
            .get_mut(&id)
            .ok_or(QosError::UnknownChannel(id))?;
        state.window.adjust(delta);
        Ok(())
    }

    /// Authorizes up to `max_bytes` of sends (defaults to `round_budget`).
    ///
    /// Order: Control pass, Interactive pass (quantum-capped per channel), then
    /// Bulk DRR. Channels whose window cannot fit their head chunk are skipped
    /// and stay queued until a window adjust arrives.
    pub fn drain(&mut self, max_bytes: Option<usize>) -> Vec<ScheduledSend> {
        let budget = max_bytes
            .unwrap_or(self.config.round_budget)
            .min(self.config.round_budget);
        if budget == 0 || self.channels.is_empty() {
            return Vec::new();
        }
        let ids: Vec<u32> = self.channels.keys().copied().collect();
        let mut out = Vec::new();
        let mut spent = 0usize;

        // Pass 1 + 2: strict priority for Control and Interactive.
        for class in [TrafficClass::Control, TrafficClass::Interactive] {
            let per_channel_cap = match class {
                TrafficClass::Control => budget,
                TrafficClass::Interactive => self.config.interactive_quantum,
                _ => unreachable!("bulk handled separately"),
            };
            for &id in &ids {
                if spent >= budget {
                    return out;
                }
                if self.channels[&id].class != class {
                    continue;
                }
                loop {
                    if spent >= budget {
                        return out;
                    }
                    let Some(send) = self.try_send(id, per_channel_cap, budget - spent) else {
                        break;
                    };
                    spent += send.bytes;
                    out.push(send);
                }
            }
        }

        // Pass 3: bulk DRR. Replenish each bulk channel's deficit once per
        // drain, then let channels transmit up to their deficit in id order.
        let bulk_ids: Vec<u32> = ids
            .iter()
            .copied()
            .filter(|id| self.channels[id].class == TrafficClass::Bulk)
            .collect();
        if bulk_ids.is_empty() {
            return out;
        }
        let quantum = self.config.bulk_quantum as u64;
        for &id in &bulk_ids {
            let state = self.channels.get_mut(&id).expect("bulk channel exists");
            state.deficit = state.deficit.saturating_add(quantum);
        }
        loop {
            let mut progressed = false;
            for &id in &bulk_ids {
                if spent >= budget {
                    return out;
                }
                let Some(send) = self.try_send_bulk(id, budget - spent) else {
                    continue;
                };
                progressed = true;
                spent += send.bytes;
                out.push(send);
            }
            if !progressed || spent >= budget {
                break;
            }
        }
        out
    }

    /// Pending bytes for `id`, if registered.
    pub fn queued_bytes_for(&self, id: u32) -> Option<u64> {
        self.queued_bytes.get(&id).copied()
    }

    /// Window remaining for `id`, if registered.
    pub fn window_remaining(&self, id: u32) -> Option<u64> {
        self.channels.get(&id).map(|state| state.window.remaining())
    }

    /// Deterministic per-channel snapshot.
    pub fn snapshot(&self) -> SchedulerSnapshot {
        SchedulerSnapshot {
            channels: self
                .channels
                .iter()
                .map(|(id, state)| ChannelSnapshot {
                    id: *id,
                    class: state.class,
                    window_remaining: state.window.remaining(),
                    queued_chunks: self.queue[id].len(),
                    queued_bytes: self.queued_bytes[id],
                    bytes_sent: state.bytes_sent,
                    chunks_sent: state.chunks_sent,
                })
                .collect(),
        }
    }

    /// Authorizes one send from `id` up to `cap`, splitting the head chunk.
    ///
    /// The take is clamped by the flow window and the remaining round budget,
    /// so a single send never exceeds either.
    fn try_send(&mut self, id: u32, cap: usize, budget_left: usize) -> Option<ScheduledSend> {
        let head = *self.queue.get(&id)?.front()?;
        let take = head
            .min(cap)
            .min(self.config.max_packet)
            .min(budget_left)
            .min(self.channels[&id].window.remaining() as usize);
        if take == 0 {
            return None;
        }
        let class = self.channels[&id].class;
        {
            let state = self.channels.get_mut(&id)?;
            state.window.consume(take).ok()?;
            state.bytes_sent += take as u64;
            state.chunks_sent += 1;
        }
        self.pop_head(id, head, take);
        Some(ScheduledSend {
            channel: id,
            class,
            bytes: take,
        })
    }

    /// Authorizes one bulk send up to the channel's DRR deficit, clamped by
    /// the flow window and the remaining round budget.
    fn try_send_bulk(&mut self, id: u32, budget_left: usize) -> Option<ScheduledSend> {
        let state = self.channels.get(&id)?;
        if state.class != TrafficClass::Bulk || state.deficit == 0 {
            return None;
        }
        let head = *self.queue.get(&id)?.front()?;
        let take = (head as u64)
            .min(state.deficit)
            .min(self.config.max_packet as u64)
            .min(state.window.remaining())
            .min(budget_left as u64) as usize;
        if take == 0 {
            return None;
        }
        let class = state.class;
        {
            let state = self.channels.get_mut(&id)?;
            state.deficit -= take as u64;
            state.window.consume(take).ok()?;
            state.bytes_sent += take as u64;
            state.chunks_sent += 1;
        }
        self.pop_head(id, head, take);
        Some(ScheduledSend {
            channel: id,
            class,
            bytes: take,
        })
    }

    /// Splits the head chunk: pops it if fully taken, otherwise shrinks it.
    fn pop_head(&mut self, id: u32, head: usize, take: usize) {
        let queue = self.queue.get_mut(&id).expect("queue exists");
        if take == head {
            queue.pop_front();
        } else {
            *queue.front_mut().expect("head exists") = head - take;
        }
        let pending = self.queued_bytes.get_mut(&id).expect("pending exists");
        *pending -= take as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::{FlowWindow, QosError, ScheduledSend, Scheduler, SchedulerConfig, TrafficClass};

    fn default_config() -> SchedulerConfig {
        SchedulerConfig::default()
    }

    #[test]
    fn interactive_is_scheduled_before_bulk() {
        let mut scheduler = Scheduler::new(default_config()).expect("scheduler");
        scheduler.register(1, TrafficClass::Bulk).expect("bulk");
        scheduler
            .register(2, TrafficClass::Interactive)
            .expect("interactive");
        scheduler.enqueue(1, 1_000_000).expect("bulk data");
        scheduler.enqueue(2, 1).expect("keystroke");
        let sends = scheduler.drain(None);
        assert_eq!(sends[0].channel, 2, "interactive byte must come first");
        assert_eq!(sends[0].bytes, 1);
        assert!(
            sends.iter().any(|send| send.channel == 1),
            "bulk still progresses"
        );
    }

    #[test]
    fn bulk_channels_share_fairly_and_none_starves() {
        let mut scheduler = Scheduler::new(default_config()).expect("scheduler");
        scheduler.register(1, TrafficClass::Bulk).expect("sfpt");
        scheduler.register(2, TrafficClass::Bulk).expect("forward");
        let each = 1_000_000usize;
        scheduler.enqueue(1, each).expect("data a");
        scheduler.enqueue(2, each).expect("data b");
        loop {
            let sends = scheduler.drain(None);
            if sends.is_empty() {
                break;
            }
        }
        let snapshot = scheduler.snapshot();
        let a = snapshot.channel(1).expect("channel 1");
        let b = snapshot.channel(2).expect("channel 2");
        assert_eq!(a.bytes_sent, each as u64);
        assert_eq!(b.bytes_sent, each as u64);
        let ratio = (a.bytes_sent as f64) / (b.bytes_sent as f64).max(1.0);
        assert!(
            ratio > 0.4 && ratio < 2.5,
            "DRR share out of bounds: {ratio}"
        );
    }

    #[test]
    fn flow_window_blocks_and_resumes_on_adjust() {
        let config = SchedulerConfig {
            initial_window: 100,
            max_window: 1_000,
            max_packet: 1024,
            ..SchedulerConfig::default()
        };
        let mut scheduler = Scheduler::new(config).expect("scheduler");
        scheduler.register(1, TrafficClass::Bulk).expect("bulk");
        scheduler.enqueue(1, 200).expect("head chunk");
        let sends = scheduler.drain(None);
        assert_eq!(sends[0].bytes, 100, "only window-sized part authorized");
        assert_eq!(sends.len(), 1);
        // Window is now empty: the remainder stays queued and cannot send.
        assert!(scheduler.drain(None).is_empty());
        assert_eq!(scheduler.queued_bytes_for(1), Some(100));
        // Peer replenishes; the remainder flows.
        scheduler.window_adjust(1, 200).expect("adjust");
        let sends = scheduler.drain(None);
        assert_eq!(sends[0].bytes, 100);
        assert!(scheduler.drain(None).is_empty());
        assert_eq!(scheduler.queued_bytes_for(1), Some(0));
    }

    #[test]
    fn window_adjust_is_capped_at_max() {
        let config = SchedulerConfig {
            initial_window: 100,
            max_window: 150,
            max_packet: 1024,
            ..SchedulerConfig::default()
        };
        let mut scheduler = Scheduler::new(config).expect("scheduler");
        scheduler
            .register(1, TrafficClass::Interactive)
            .expect("interactive");
        scheduler.window_adjust(1, 10_000).expect("huge adjust");
        assert_eq!(
            scheduler.window_remaining(1),
            Some(150),
            "capped at max_window"
        );
    }

    #[test]
    fn round_budget_limits_each_drain() {
        let config = SchedulerConfig {
            round_budget: 1_000,
            ..SchedulerConfig::default()
        };
        let mut scheduler = Scheduler::new(config).expect("scheduler");
        scheduler.register(1, TrafficClass::Bulk).expect("bulk");
        scheduler.enqueue(1, 10_000).expect("data");
        let first = scheduler.drain(None);
        let spent: usize = first.iter().map(|send| send.bytes).sum();
        assert!(spent <= 1_000, "budget exceeded: {spent}");
        // More drains drain the rest.
        let mut total = spent;
        loop {
            let sends = scheduler.drain(None);
            if sends.is_empty() {
                break;
            }
            total += sends.iter().map(|send| send.bytes).sum::<usize>();
        }
        assert_eq!(total, 10_000);
    }

    #[test]
    fn class_change_moves_channel_to_priority() {
        let mut scheduler = Scheduler::new(default_config()).expect("scheduler");
        scheduler.register(1, TrafficClass::Bulk).expect("bulk");
        scheduler.register(2, TrafficClass::Bulk).expect("bulk2");
        scheduler.enqueue(1, 1_000_000).expect("data a");
        scheduler
            .set_class(2, TrafficClass::Interactive)
            .expect("promote");
        scheduler.enqueue(2, 1).expect("interactive");
        let sends = scheduler.drain(None);
        assert_eq!(sends[0].channel, 2);
    }

    #[test]
    fn unknown_and_duplicate_channels_are_rejected() {
        let mut scheduler = Scheduler::new(default_config()).expect("scheduler");
        assert_eq!(scheduler.enqueue(99, 1), Err(QosError::UnknownChannel(99)));
        assert_eq!(
            scheduler.window_adjust(99, 1),
            Err(QosError::UnknownChannel(99))
        );
        scheduler
            .register(1, TrafficClass::Interactive)
            .expect("register");
        assert_eq!(
            scheduler.register(1, TrafficClass::Bulk),
            Err(QosError::AlreadyRegistered(1))
        );
        assert!(!scheduler.unregister(99));
        assert!(scheduler.unregister(1));
        assert_eq!(scheduler.queued_bytes_for(1), None);
    }

    #[test]
    fn invalid_config_is_rejected() {
        let bad = SchedulerConfig {
            bulk_quantum: 0,
            ..SchedulerConfig::default()
        };
        assert_eq!(Scheduler::new(bad).unwrap_err(), QosError::InvalidConfig);
        let bad_max = SchedulerConfig {
            initial_window: 1_000,
            max_window: 500,
            ..SchedulerConfig::default()
        };
        assert_eq!(
            Scheduler::new(bad_max).unwrap_err(),
            QosError::InvalidConfig
        );
    }

    #[test]
    fn window_exceeded_is_reported() {
        let mut window = FlowWindow::new(10, 100, 1024);
        assert!(window.can_send(10));
        assert!(!window.can_send(11));
        assert_eq!(
            window.consume(11),
            Err(QosError::WindowExceeded {
                requested: 11,
                remaining: 10
            })
        );
        window.consume(10).expect("consume");
        assert_eq!(window.remaining(), 0);
        window.adjust(50);
        assert!(window.is_adjusted());
        assert_eq!(window.remaining(), 50);
    }

    #[test]
    fn qos_benchmark_concurrent_terminal_sftp_forwarding() {
        // Acceptance benchmark: one interactive terminal + two SFTP transfers
        // + two port-forward streams. The terminal must never wait behind bulk
        // data and all bulk must eventually complete.
        let config = SchedulerConfig {
            round_budget: 64 * 1024,
            initial_window: 64 * 1024,
            max_window: 1024 * 1024,
            ..SchedulerConfig::default()
        };
        let mut scheduler = Scheduler::new(config).expect("scheduler");
        scheduler
            .register(1, TrafficClass::Interactive)
            .expect("terminal");
        scheduler.register(2, TrafficClass::Bulk).expect("sftp-a");
        scheduler.register(3, TrafficClass::Bulk).expect("sftp-b");
        scheduler
            .register(4, TrafficClass::Bulk)
            .expect("forward-a");
        scheduler
            .register(5, TrafficClass::Bulk)
            .expect("forward-b");

        let bulk_total = 8 * 1024 * 1024usize; // 8 MiB per bulk channel
        for id in [2u32, 3, 4, 5] {
            scheduler.enqueue(id, bulk_total).expect("bulk data");
        }

        let mut rounds = 0u64;
        let mut interactive_served_total = 0u64;
        let mut interactive_served_in_round = 0u64;
        let mut bulk_completed = false;
        let mut per_round_bulk_progress_nonzero = true;
        while !bulk_completed && rounds < 10_000 {
            // Peer window adjusts keep flow control active but never let the
            // bulk windows fully close.
            for id in [2u32, 3, 4, 5] {
                scheduler.window_adjust(id, 16 * 1024).expect("adjust");
            }
            // Each round the terminal produces one small interactive event.
            scheduler.enqueue(1, 1).expect("keystroke");
            let sends = scheduler.drain(None);
            let interactive_served = sends
                .iter()
                .any(|send| send.channel == 1 && send.class == TrafficClass::Interactive);
            if interactive_served {
                interactive_served_total += 1;
                interactive_served_in_round += 1;
            }
            let bulk_in_round: usize = sends
                .iter()
                .filter(|send| send.class == TrafficClass::Bulk)
                .map(|send| send.bytes)
                .sum();
            let remaining: u64 = [2u32, 3, 4, 5]
                .iter()
                .map(|id| scheduler.queued_bytes_for(*id).unwrap_or(0))
                .sum();
            bulk_completed = remaining == 0;
            if !bulk_completed && bulk_in_round == 0 {
                per_round_bulk_progress_nonzero = false;
            }
            rounds += 1;
        }

        // The interactive byte is served in the very drain it is enqueued:
        // zero waiting rounds behind bulk data.
        assert_eq!(
            interactive_served_total, rounds,
            "interactive byte must be served every round"
        );
        assert_eq!(
            interactive_served_in_round, rounds,
            "interactive latency must be 0 rounds"
        );
        assert!(
            per_round_bulk_progress_nonzero,
            "bulk made no progress in some round before completion (starvation)"
        );
        assert!(bulk_completed, "bulk transfers never completed");
        assert!(
            rounds < 10_000,
            "benchmark did not terminate within round bound"
        );
        let snapshot = scheduler.snapshot();
        for id in [2u32, 3, 4, 5] {
            let channel = snapshot.channel(id).expect("bulk channel");
            assert_eq!(channel.bytes_sent, bulk_total as u64);
        }
        // Flow control actually engaged: at no point did any bulk window hold
        // the full 8 MiB transfer.
        assert!(
            rounds > 500,
            "benchmark too fast to exercise flow control: {rounds} rounds"
        );
    }

    #[test]
    fn scheduled_send_carries_class() {
        let mut scheduler = Scheduler::new(default_config()).expect("scheduler");
        scheduler
            .register(7, TrafficClass::Control)
            .expect("control");
        scheduler.enqueue(7, 5).expect("control data");
        let sends = scheduler.drain(None);
        assert_eq!(
            sends[0],
            ScheduledSend {
                channel: 7,
                class: TrafficClass::Control,
                bytes: 5
            }
        );
    }
}
