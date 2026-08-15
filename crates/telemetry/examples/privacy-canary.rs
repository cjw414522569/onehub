//! Privacy canary (T147): under explicit consent, attempts to capture data
//! classes that must never leave the machine (terminal content, commands,
//! identity, host data) plus a fixed canary value. The contract scans the
//! outbound capture and fails if any forbidden data leaks.

use telemetry::{TelemetryCollector, TelemetryConsent};

/// The privacy canary value (must never appear in outbound telemetry).
pub const CANARY: &str = "PRIVACY_CANARY_3d19f7aa8c";

fn main() {
    let mut off = TelemetryCollector::default();
    let on = TelemetryCollector::new(TelemetryConsent::ExplicitConsent);

    // 1. Default-off capture: nothing leaves the machine.
    let off_capture = off.collect(
        "app_start",
        &[("platform", "windows"), ("app_version", "0.1.0")],
    );
    println!(
        "capture:consent=off outbound={}",
        off_capture.is_some() as u8
    );
    off.set_consent(TelemetryConsent::ExplicitConsent);

    // 2. Explicit-consent, allowlisted event: allowed fields only.
    if let Some(event) = on.collect(
        "app_start",
        &[
            ("platform", "windows"),
            ("app_version", "0.1.0"),
            ("build_channel", "stable"),
        ],
    ) {
        println!("{}", on.render(&event));
    }

    // 3. Forbidden data classes under explicit consent: each must be
    //    rejected (return None), so nothing prints.
    let attempts: &[(&str, &str)] = &[
        ("terminal_text", "ls -la"),
        ("command", "rm -rf /"),
        ("command_history", "cat secret.txt"),
        ("user_identity", "c4145"),
        ("host_name", "db.internal"),
        ("host_address", "10.0.0.5"),
        ("private_key", CANARY),
        ("auth_token", CANARY),
        ("payload", CANARY),
    ];
    let mut rejected = 0usize;
    for (field, value) in attempts {
        if on.collect("feature_used", &[(field, value)]).is_none() {
            rejected += 1;
        }
    }
    println!("capture:rejected_forbidden={rejected}/{}", attempts.len());

    // 4. The benign allowlisted field must still work.
    if let Some(event) = on.collect("feature_used", &[("feature", "port_forward")]) {
        println!("{}", on.render(&event));
    }
}
