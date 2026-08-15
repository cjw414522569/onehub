//! Crash capture trigger + upload audit (T149).
//!
//! Deliberately panics with a message that carries a canary secret, a host
//! name, and a command; catches the panic, builds a sanitized dump, and
//! prints it. The contract audits the printed upload content: the canary and
//! content markers must never appear.

use telemetry::{CrashDump, CrashRetention, CrashSanitizer, CrashUploadPolicy};

/// The crash canary value (must never survive sanitization).
pub const CANARY: &str = "CRASH_CANARY_b6d4e08f";

fn main() {
    // Suppress the default panic hook so the raw (unsanitized) message never
    // reaches any output; only the sanitized dump is emitted.
    std::panic::set_hook(Box::new(|_| {}));

    let sanitizer = CrashSanitizer::with_denylist(&[
        "db.internal",
        "10.0.0.5",
        "ls -la",
        "c4145",
        "CRASH_CANARY_b6d4e08f",
    ]);
    let retention = CrashRetention::default();
    let upload = CrashUploadPolicy::default();

    // Upload gating: no upload without explicit consent.
    println!("crash:upload_consent={}", upload.can_upload() as u8);

    // Trigger a test crash whose message contains content markers.
    let result = std::panic::catch_unwind(|| {
        panic!(
            "ssh error connecting to db.internal (10.0.0.5) as c4145: {} while running `ls -la`",
            CANARY
        );
    });
    let raw_message = match result {
        Ok(()) => "no panic".to_owned(),
        Err(payload) => payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic payload".to_owned()),
    };

    // Capture and sanitize.
    let dump = CrashDump::capture(
        "0.1.0",
        1_700_000_000,
        "main",
        &raw_message,
        vec![
            "ssh_cli::run (main.rs:42)".to_owned(),
            "telemetry::crash::capture db.internal".to_owned(),
        ],
        &sanitizer,
    );

    // Retention: this dump is young and within the budget, so nothing is
    // pruned and the user can delete it.
    let mut dumps = vec![("dump-001".to_owned(), dump.captured_at_unix)];
    let pruned = retention.prune(&dumps, dump.captured_at_unix + 1);
    println!("crash:pruned={}", pruned.len());
    println!(
        "crash:deleted={}",
        retention.delete(&mut dumps, "dump-001") as u8
    );

    // The sanitized dump is the only thing that could be uploaded.
    let json = format!(
        "{{\"schema_version\":{},\"app_version\":\"{}\",\"thread\":\"{}\",\"message\":\"{}\"}}",
        dump.schema_version,
        dump.app_version,
        dump.thread_name,
        dump.panic_message.replace('"', "\\\"")
    );
    println!("crash:dump {json}");
}
