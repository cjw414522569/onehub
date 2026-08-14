#![forbid(unsafe_code)]

use sync_service::{ServiceConfig, SyncBackend, SystemClock};

fn main() {
    let config = ServiceConfig::default();
    let _backend = SyncBackend::new(config, std::sync::Arc::new(SystemClock));
    println!(
        "ssh-sync-service: minimal trusted sync backend ready (ciphertext-only envelopes; quota={} bytes/device; rate burst={}, refill={}/s; content-free audit)",
        config.quota_bytes, config.rate_capacity, config.rate_refill_per_sec
    );
}
