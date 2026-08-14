#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # wgpu-renderer
//!
//! Batched GPU terminal drawing on a single native wgpu surface (T077).

pub mod render;

pub use render::{
    build_plan, frame_stats, merge_to_budget, DrawBudget, DrawCall, FrameStats, RenderPlan,
    RenderSurface,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "wgpu-renderer";
