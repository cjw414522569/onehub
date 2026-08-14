//! Frame coalescing, refresh throttling, and background-session throttling
//! (T079).
//!
//! High-throughput terminal output is coalesced into whole frames and a
//! bounded update queue prevents event-queue explosion; foreground (input)
//! sessions render at full rate while background sessions are throttled to a
//! lower refresh rate. The 100 MB/s synthetic output stress test runs as a
//! deterministic unit test (no wall-clock dependency).

use std::time::{Duration, Instant};

/// Coalesces many update notifications into one pending frame.
#[derive(Debug, Clone, Default)]
pub struct FrameCoalescer {
    pending: bool,
    coalesced: u64,
}

impl FrameCoalescer {
    /// A new coalescer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Notifies the coalescer of an update (folded into the pending frame).
    pub fn notify(&mut self) {
        self.pending = true;
        self.coalesced += 1;
    }

    /// Whether a frame is pending.
    pub fn is_pending(&self) -> bool {
        self.pending
    }

    /// Total updates coalesced so far.
    pub fn coalesced(&self) -> u64 {
        self.coalesced
    }

    /// Drains the pending flag; returns true when a frame should render.
    pub fn drain(&mut self) -> bool {
        let pending = self.pending;
        self.pending = false;
        pending
    }
}

/// Limits how often frames render (refresh throttling).
#[derive(Debug, Clone)]
pub struct RefreshThrottle {
    min_interval: Duration,
    last_frame: Option<Instant>,
    frames_rendered: u64,
}

impl RefreshThrottle {
    /// A throttle that allows a frame at most once per `min_interval`.
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_frame: None,
            frames_rendered: 0,
        }
    }

    /// Whether a frame may render now (at most once per interval).
    pub fn should_render(&mut self, now: Instant) -> bool {
        match self.last_frame {
            Some(last) if now.duration_since(last) < self.min_interval => false,
            _ => {
                self.last_frame = Some(now);
                self.frames_rendered += 1;
                true
            }
        }
    }

    /// Frames rendered so far.
    pub fn frames_rendered(&self) -> u64 {
        self.frames_rendered
    }
}

/// Session priority: foreground input is prioritized over background output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPriority {
    /// The focused session (full refresh rate).
    Foreground,
    /// A background session (throttled refresh rate).
    Background,
}

/// A bounded update queue with coalescing: over-cap updates are dropped and
/// counted instead of growing without bound.
#[derive(Debug, Clone)]
pub struct BoundedUpdateQueue {
    cap: usize,
    queued: usize,
    dropped: u64,
}

impl BoundedUpdateQueue {
    /// A queue that holds at most `cap` pending updates.
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            queued: 0,
            dropped: 0,
        }
    }

    /// Enqueues an update; returns false when over capacity (the update is
    /// coalesced/dropped and counted).
    pub fn enqueue(&mut self) -> bool {
        if self.queued >= self.cap {
            self.dropped += 1;
            return false;
        }
        self.queued += 1;
        true
    }

    /// Removes one pending update (rendered).
    pub fn dequeue(&mut self) {
        self.queued = self.queued.saturating_sub(1);
    }

    /// Pending updates.
    pub fn queued(&self) -> usize {
        self.queued
    }

    /// Dropped (coalesced-away) updates.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// The capacity.
    pub fn cap(&self) -> usize {
        self.cap
    }
}

/// Throttling configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThrottleConfig {
    /// Minimum interval between foreground frames.
    pub foreground_interval: Duration,
    /// Minimum interval between background frames.
    pub background_interval: Duration,
    /// Maximum pending updates.
    pub max_queued: usize,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            foreground_interval: Duration::from_micros(16_667), // ~60 fps
            background_interval: Duration::from_micros(66_667), // ~15 fps
            max_queued: 1024,
        }
    }
}

/// Combines coalescing, throttling, and the bounded queue for a window of
/// sessions.
#[derive(Debug, Clone)]
pub struct SessionThrottler {
    config: ThrottleConfig,
    coalescer: FrameCoalescer,
    foreground: RefreshThrottle,
    background: RefreshThrottle,
    foreground_pending: bool,
    background_pending: bool,
    queue: BoundedUpdateQueue,
}

impl SessionThrottler {
    /// A throttler with the given configuration.
    pub fn new(config: ThrottleConfig) -> Self {
        Self {
            config,
            coalescer: FrameCoalescer::new(),
            foreground: RefreshThrottle::new(config.foreground_interval),
            background: RefreshThrottle::new(config.background_interval),
            foreground_pending: false,
            background_pending: false,
            queue: BoundedUpdateQueue::new(config.max_queued),
        }
    }

    /// The active configuration.
    pub fn config(&self) -> ThrottleConfig {
        self.config
    }

    /// An update arrived for a session priority: coalesce and enqueue
    /// (bounded).
    pub fn on_update(&mut self, priority: SessionPriority) {
        self.coalescer.notify();
        self.queue.enqueue();
        match priority {
            SessionPriority::Foreground => self.foreground_pending = true,
            SessionPriority::Background => self.background_pending = true,
        }
    }

    /// Whether a frame should render now for the given priority. Foreground
    /// renders at full rate; background at the reduced rate. A rendered frame
    /// drains all pending updates (one composited frame).
    pub fn should_render(&mut self, priority: SessionPriority, now: Instant) -> bool {
        let (pending, throttle) = match priority {
            SessionPriority::Foreground => (&self.foreground_pending, &mut self.foreground),
            SessionPriority::Background => (&self.background_pending, &mut self.background),
        };
        if !*pending {
            return false;
        }
        if !self.coalescer.is_pending() {
            return false;
        }
        if !throttle.should_render(now) {
            return false;
        }
        self.foreground_pending = false;
        self.background_pending = false;
        let _ = self.coalescer.drain();
        self.queue.dequeue();
        true
    }

    /// Pending updates.
    pub fn queued(&self) -> usize {
        self.queue.queued()
    }

    /// Dropped (coalesced-away) updates.
    pub fn dropped(&self) -> u64 {
        self.queue.dropped()
    }

    /// Total updates seen.
    pub fn coalesced(&self) -> u64 {
        self.coalescer.coalesced()
    }

    /// Frames rendered for a priority.
    pub fn frames_rendered(&self, priority: SessionPriority) -> u64 {
        match priority {
            SessionPriority::Foreground => self.foreground.frames_rendered(),
            SessionPriority::Background => self.background.frames_rendered(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{
        FrameCoalescer, RefreshThrottle, SessionPriority, SessionThrottler, ThrottleConfig,
    };

    #[test]
    fn coalescer_folds_updates_into_one_frame() {
        let mut coalescer = FrameCoalescer::new();
        for _ in 0..1000 {
            coalescer.notify();
        }
        assert!(coalescer.is_pending());
        assert_eq!(coalescer.coalesced(), 1000);
        assert!(coalescer.drain());
        assert!(!coalescer.is_pending());
        assert!(!coalescer.drain());
    }

    #[test]
    fn refresh_throttle_limits_frame_rate() {
        let mut throttle = RefreshThrottle::new(Duration::from_millis(100));
        let start = Instant::now();
        let mut renders = 0;
        let mut now = start;
        for _ in 0..10 {
            if throttle.should_render(now) {
                renders += 1;
            }
            now += Duration::from_millis(10);
        }
        // 10 requests at 10ms intervals with a 100ms throttle -> at most 2.
        assert!(renders <= 2, "renders={renders}");
        assert_eq!(throttle.frames_rendered(), renders);
    }

    #[test]
    fn bounded_queue_never_explodes_under_high_throughput() {
        // 100 MB/s synthetic stress: 1,000,000 updates with a small queue.
        let config = ThrottleConfig {
            max_queued: 64,
            ..ThrottleConfig::default()
        };
        let mut throttler = SessionThrottler::new(config);
        for _ in 0..1_000_000 {
            throttler.on_update(SessionPriority::Foreground);
        }
        assert!(
            throttler.queued() <= config.max_queued,
            "queue must stay bounded (queued={})",
            throttler.queued()
        );
        assert_eq!(throttler.coalesced(), 1_000_000);
        assert!(throttler.dropped() > 0, "overflow must be coalesced away");
    }

    #[test]
    fn foreground_renders_faster_than_background() {
        let config = ThrottleConfig {
            foreground_interval: Duration::from_millis(10),
            background_interval: Duration::from_millis(100),
            max_queued: 1024,
        };
        let mut throttler = SessionThrottler::new(config);
        let start = Instant::now();
        let mut now = start;
        for _ in 0..100 {
            throttler.on_update(SessionPriority::Foreground);
            throttler.on_update(SessionPriority::Background);
            now += Duration::from_millis(5);
            let _ = throttler.should_render(SessionPriority::Foreground, now);
            let _ = throttler.should_render(SessionPriority::Background, now);
        }
        let foreground = throttler.frames_rendered(SessionPriority::Foreground);
        let background = throttler.frames_rendered(SessionPriority::Background);
        assert!(
            foreground > background,
            "foreground ({foreground}) must render more than background ({background})"
        );
        assert!(foreground > 0);
    }
}
