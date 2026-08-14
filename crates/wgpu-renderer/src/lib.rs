#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # wgpu-renderer
//!
//! Batched GPU terminal drawing on a single native wgpu surface (T077).

pub mod composite;
pub mod render;
pub mod throttle;

pub use composite::{
    selected_text, CompositeState, DecorationRect, FramePlan, FrameTimeline, Layer, LinkRect,
    SearchMatchRect,
};
pub use render::{
    build_plan, frame_stats, merge_to_budget, DrawBudget, DrawCall, FrameStats, RenderPlan,
    RenderSurface,
};
pub use throttle::{
    BoundedUpdateQueue, FrameCoalescer, RefreshThrottle, SessionPriority, SessionThrottler,
    ThrottleConfig,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "wgpu-renderer";
