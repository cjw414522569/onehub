#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # wasm
//!
//! WASM/WebGPU compile of the terminal and domain core with a versioned JS
//! interop boundary (T138).

pub mod bridge;
pub mod ffi;

pub use bridge::{BridgeOutput, TerminalBridge, BRIDGE_VERSION};
pub use ffi::{boundary_version, JsOutput, JsPlanStats, JsTerminal, WASM_BOUNDARY_VERSION};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "wasm";
