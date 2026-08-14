#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # core-protocol
//!
//! Versioned protocol domain model and capability negotiation, aligned with
//! `protocol/schema/domain-v1.json` and `protocol/terminal/terminal-contract-v1.json`.

pub mod capabilities;
pub mod terminal;

pub use capabilities::{
    negotiate, negotiate_with_platform, Capability, CapabilitySet, NegotiationResult, PlatformId,
    PlatformProfile, ALL_CAPABILITIES,
};
pub use terminal::{
    CursorState, DeltaOp, Extension, Hyperlink, ImagePlaceholder, TerminalBatch, TerminalCell,
    TerminalColor, TerminalDelta, TerminalMessage, TerminalProtocolVersion, TerminalRow,
    TerminalSnapshot, TerminalStyle, UnderlineStyle,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "core-protocol";
