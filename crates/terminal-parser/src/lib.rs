#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # terminal-parser
//!
//! Bounded byte-stream to terminal-parser pipeline (T062): fragmentation-safe
//! UTF-8 / CSI / OSC parsing with hard memory bounds for malicious input.
//! The event vocabulary (`ParseEvent`, `ParseBatch`, `ParserDiagnostic`,
//! `TerminalParser`) is owned by `terminal-state` (L1) and re-exported here.

pub mod stream;

pub use stream::{BoundedByteStreamParser, DEFAULT_MAX_SEQUENCE_LEN, MAX_UTF8_LEN};
pub use terminal_state::parser::{ParseBatch, ParseEvent, ParserDiagnostic, TerminalParser};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "terminal-parser";
