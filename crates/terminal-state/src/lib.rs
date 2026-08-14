#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # terminal-state
//!
//! Shared terminal parser contract (L1 event vocabulary) and the primary /
//! alternate screen model with cursor, scroll region, and DEC/ANSI modes
//! (T062/T063).

pub mod parser;
pub mod screen;

pub use parser::{ParseBatch, ParseEvent, ParserDiagnostic, TerminalParser};
pub use screen::{Modes, ScreenBuffer, ScreenModel};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "terminal-state";
