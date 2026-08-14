#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # session-orchestrator
//!
//! Session lifecycle orchestration with structured concurrency conventions:
//! cooperative cancellation tokens, deadlines, bounded message channels,
//! bounded task groups, and an event-sourced session state machine.

pub mod bounded_channel;
pub mod cancellation;
pub mod session_state;

pub use bounded_channel::{BoundedChannel, ChannelStats, SlowConsumerPolicy};
pub use cancellation::{
    select_cancellation, select_deadline, select_guarded, CancelReason, CancellationToken, Deadline,
};
pub use session_state::{
    apply, replay, replay_from, SessionEffect, SessionEvent, SessionSnapshot, SessionState,
    SessionTransition, SessionTransitionResult,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "session-orchestrator";
