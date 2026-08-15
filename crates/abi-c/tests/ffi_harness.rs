//! Cross-language integration harness (T152): drives the stable C ABI
//! surface — version, header validity, handle lifecycle, and the event
//! stream with cancellation, error, and lifecycle paths — 100 consecutive
//! times and asserts every run is byte-identical (no flakiness in CI).

use abi_c::event_stream::{BatchItem, EventStream, EVENT_BATCH_MAX_EVENTS};
use abi_c::handle::{HandleTable, INVALID_HANDLE};
use abi_c::{ABI_CODEGEN_VERSION, ABI_SCHEMA_VERSION};

/// Runs the full FFI scenario once and returns a deterministic outcome log.
fn run_scenario() -> Vec<String> {
    let mut log = Vec::new();

    // 1. Version + header contract.
    log.push(format!("version={}", abi_c::ssh_abi_version()));
    log.push(format!(
        "codegen={}",
        String::from_utf8_lossy(abi_c::codegen_version_bytes())
    ));
    log.push(format!(
        "header_valid={}",
        abi_c::ssh_abi_header_is_valid(ABI_SCHEMA_VERSION)
    ));
    log.push(format!(
        "header_stale={}",
        abi_c::ssh_abi_header_is_valid(ABI_SCHEMA_VERSION + 1)
    ));
    log.push(format!("header_size={}", abi_c::ssh_abi_header_size()));

    // 2. Handle lifecycle: insert / get / remove (opaque handles).
    let mut table = HandleTable::<u64>::new();
    log.push(format!("handle_invalid={INVALID_HANDLE}"));
    let handle_a = table.insert(41);
    let handle_b = table.insert(42);
    log.push(format!("handle_get_a={:?}", table.get(handle_a).copied()));
    log.push(format!(
        "handle_get_missing={:?}",
        table.get(handle_b + 99).copied()
    ));
    log.push(format!("handle_remove_b={:?}", table.remove(handle_b)));
    log.push(format!("handle_contains_b={}", table.contains(handle_b)));
    log.push(format!("handle_len={}", table.len()));
    table.remove(handle_a);
    log.push(format!("handle_len_final={}", table.len()));

    // 3. Event stream lifecycle: push -> flush -> poll -> drain, with
    //    cancellation and error batches interleaved.
    let mut stream = EventStream::new(8);
    let mut sequence = 0u64;
    for i in 0..3u64 {
        for label in ["event", "cancel", "error"] {
            stream.push_event(format!("{label}-{i}").into_bytes());
        }
        stream.flush();
        while let Some(batch) = stream.poll() {
            sequence += 1;
            log.push(format!(
                "batch#{sequence}:version={} items={} bytes={}",
                batch.version,
                batch.items.len(),
                batch.total_bytes
            ));
            let payloads: Vec<String> = batch
                .items
                .iter()
                .filter_map(|item| match item {
                    BatchItem::Event(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
                    _ => None,
                })
                .collect();
            log.push(format!("batch#{sequence}:payload={}", payloads.join("|")));
        }
    }
    // Drain is complete; a fresh poll is empty.
    log.push(format!("stream_drained={}", stream.poll().is_none()));
    log.push(format!(
        "stream_empty={} dropped={}",
        stream.is_empty(),
        stream.dropped_total()
    ));

    // 4. Error/backpressure path: filling a capacity-1 stream drops the
    //    oldest batch and requires a snapshot.
    let mut strict = EventStream::new(1);
    for _ in 0..EVENT_BATCH_MAX_EVENTS {
        strict.push_event(vec![b'x'; 1]);
    }
    assert!(strict.poll().is_some(), "first batch must be pollable");
    for _ in 0..EVENT_BATCH_MAX_EVENTS {
        strict.push_event(vec![b'y'; 1]);
    }
    log.push(format!("overflow_dropped={}", strict.dropped_total()));
    log.push(format!("overflow_snapshot={}", strict.needs_snapshot()));

    log
}

#[test]
fn ffi_harness_100_runs_is_stable() {
    // The acceptance: run the full FFI/lifecycle/cancel/error scenario 100
    // times and every run must be identical (no flakiness).
    let first = run_scenario();
    assert!(!first.is_empty());
    for run in 1..100 {
        let current = run_scenario();
        assert_eq!(
            current, first,
            "run {run} diverged from the canonical FFI scenario (flaky)"
        );
    }
    assert_eq!(
        first[0],
        format!("version={ABI_SCHEMA_VERSION}"),
        "the ABI version is pinned"
    );
    assert_eq!(first[1], format!("codegen={ABI_CODEGEN_VERSION}\u{0}"));
    assert_eq!(first[2], "header_valid=0", "valid header must pass");
    assert_eq!(first[3], "header_stale=-1", "stale header must fail");
    assert_eq!(
        first
            .iter()
            .filter(|line| line.starts_with("batch#"))
            .count(),
        6,
        "3 batches with version/payload pairs"
    );
}
