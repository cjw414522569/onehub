#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # terminal-state
//!
//! Shared terminal parser contract (L1 event vocabulary) and the primary /
//! alternate screen model with cursor, scroll region, DEC/ANSI modes
//! (T062/T063), plus the locked Unicode width policy and grapheme
//! segmentation used by the model (T064).

pub mod parser;
pub mod screen;
pub mod unicode;

pub use parser::{ParseBatch, ParseEvent, ParserDiagnostic, TerminalParser};
pub use screen::{Modes, ScreenBuffer, ScreenModel};
pub use unicode::{grapheme_clusters, WidthPolicy, UNICODE_VERSION};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "terminal-state";
