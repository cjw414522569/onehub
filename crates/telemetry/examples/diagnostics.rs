//! Diagnostics export example (T148): samples the network / parse / render /
//! memory paths, exports the versioned report, and prints it. The contract
//! scans the exported report for content markers — it must contain only
//! numeric aggregates and fixed labels.

use telemetry::DiagnosticsSampler;

fn main() {
    let mut sampler = DiagnosticsSampler::new();

    // Network: RTT samples (ms).
    for ms in [40u64, 42, 45, 41, 80, 44, 43, 46, 45, 44] {
        sampler.record_network_latency(ms);
    }
    // Parse: throughput samples (MB/s).
    for mbps in [180u64, 210, 195, 220, 205] {
        sampler.record_parse_throughput(mbps);
    }
    // Render: frame times (ms).
    for ms in [8u64, 9, 8, 12, 10, 9, 11, 10] {
        sampler.record_render_frame(ms);
    }
    // Memory: steady-state (KB).
    for kb in [148_000u64, 151_000, 149_000, 150_500] {
        sampler.record_memory(kb);
    }

    println!(
        "{}",
        telemetry::DiagnosticReport::export(&sampler).to_json()
    );
}
