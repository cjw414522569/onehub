//! Canary secret log scan (T146).
//!
//! Emits structured logs while attempting to log a canary secret under
//! sensitive field names. The contract scans this example's stdout for the
//! canary value: if the logger ever leaks it, the scan fails.

use telemetry::{LogLevel, Logger, TraceId};

/// The canary secret. The contract must never find this string in emitted
/// log output.
pub const CANARY: &str = "CANARY_SECRET_5e8c2f91a4b7";

fn main() {
    let logger = Logger::new("gateway.canary");
    let trace = TraceId::from_u64(0xca7cafe);

    // Attempt to log the canary under sensitive field names (dropped).
    for (name, level) in [
        ("auth_token", LogLevel::Info),
        ("password", LogLevel::Warn),
        ("private_key", LogLevel::Info),
        ("secret", LogLevel::Debug),
        ("credential", LogLevel::Info),
    ] {
        if let Some(entry) = logger.log(level, trace, "operation", &[(name, CANARY)]) {
            println!("{}", logger.render(&entry));
        }
    }

    // Emit a benign line for completeness.
    if let Some(entry) = logger.log(
        LogLevel::Info,
        trace,
        "canary scan complete",
        &[("status", "ok")],
    ) {
        println!("{}", logger.render(&entry));
    }
}
