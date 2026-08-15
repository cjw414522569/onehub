#![forbid(unsafe_code)]

//! # telemetry
//!
//! Structured logging with trace-id correlation and dynamic level control
//! (T146). Default logs contain no sensitive fields; the canary example
//! (`examples/canary.rs`) is scanned by the contract to prove a secret can
//! never reach emitted log output.

pub mod log;

pub use log::{
    escape_value, LogContext, LogEntry, LogLevel, Logger, SensitiveFieldPolicy, TraceId,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "telemetry";
