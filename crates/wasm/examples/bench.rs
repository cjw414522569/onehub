//! Deterministic micro-benchmarks (T156): parse throughput, input-to-pixel
//! latency, and scrollback throughput against the real core models. Prints
//! a JSON summary consumed by `scripts/test-benchmarks.mjs` for the
//! statistics gate (P50/P95/P99/mean over N repeats).

use std::time::Instant;

use terminal_parser::BoundedByteStreamParser;
use terminal_state::{ScreenModel, TerminalParser};
use wasm::TerminalBridge;

/// Parse-corpus size (10 MB, matching the T003 measurement principle).
const CORPUS_MB: usize = 10;
/// Repeats for the statistics gate.
const REPEATS: usize = 30;

/// Builds a deterministic 10 MB VT corpus: 80-column text lines with a
/// single color sequence per line (representative of real TUI output).
fn corpus() -> Vec<u8> {
    let mut line = Vec::with_capacity(80);
    line.extend_from_slice(b"\x1b[32m");
    for ch in b'a'..=b'z' {
        line.push(ch);
    }
    line.extend_from_slice(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*()");
    line.truncate(78);
    line.extend_from_slice(b"\x1b[0m\n");
    let lines = CORPUS_MB * 1024 * 1024 / line.len();
    let mut bytes = Vec::with_capacity(CORPUS_MB * 1024 * 1024);
    for _ in 0..lines {
        bytes.extend_from_slice(&line);
    }
    bytes
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn percentile(values: &mut [f64], pct: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let index = ((values.len() - 1) as f64 * pct / 100.0).round() as usize;
    values[index.min(values.len() - 1)]
}

fn stats_ms(samples: &[f64]) -> serde_json::Value {
    let mut sorted = samples.to_vec();
    serde_json::json!({
        "count": samples.len(),
        "mean_ms": format!("{:.3}", mean(samples)),
        "p50_ms": format!("{:.3}", percentile(&mut sorted, 50.0)),
        "p95_ms": format!("{:.3}", percentile(&mut sorted, 95.0)),
        "p99_ms": format!("{:.3}", percentile(&mut sorted, 99.0)),
    })
}

fn stats_us(samples: &[f64]) -> serde_json::Value {
    let mut sorted = samples.to_vec();
    serde_json::json!({
        "count": samples.len(),
        "mean_us": format!("{:.3}", mean(samples)),
        "p50_us": format!("{:.3}", percentile(&mut sorted, 50.0)),
        "p95_us": format!("{:.3}", percentile(&mut sorted, 95.0)),
        "p99_us": format!("{:.3}", percentile(&mut sorted, 99.0)),
    })
}

fn main() {
    let data = corpus();

    // 1. Terminal parse throughput (MB/s) over REPEATS runs.
    let mut throughput = Vec::with_capacity(REPEATS);
    for _ in 0..REPEATS {
        let mut parser = BoundedByteStreamParser::new();
        let start = Instant::now();
        let _ = parser.feed(&data);
        let _ = parser.finish();
        let secs = start.elapsed().as_secs_f64();
        throughput.push(CORPUS_MB as f64 / secs);
    }
    let throughput_sorted = {
        let mut v = throughput.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v
    };
    let p50_mbps = throughput_sorted[throughput_sorted.len() / 2];
    let p95_mbps =
        throughput_sorted[((throughput_sorted.len() - 1) as f64 * 0.95).round() as usize];

    // 2. Input-to-pixel: bridge push + snapshot latency per small input.
    let mut latency = Vec::with_capacity(REPEATS);
    for _ in 0..REPEATS {
        let mut bridge = TerminalBridge::new(1, 24, 80);
        let start = Instant::now();
        bridge.push(b"hello \x1b[31mworld\r\n");
        let _ = bridge.snapshot();
        latency.push(start.elapsed().as_secs_f64() * 1_000_000.0); // us
    }

    // 2b. End-to-end input-to-pixel: encode a key event, push through the
    // bridge, snapshot the screen, and build the render plan (the full path
    // to pixels). Over REPEATS runs -> P50/P95/P99.
    let mut e2e_latency = Vec::with_capacity(REPEATS);
    for _ in 0..REPEATS {
        let mut bridge = TerminalBridge::new(1, 24, 80);
        let start = Instant::now();
        let event = terminal_state::input::KeyEvent {
            key: terminal_state::input::Key::Char('x'),
            modifiers: terminal_state::input::Modifiers::default(),
            repeat: 1,
        };
        let bytes = terminal_state::input::encode_key(
            &event,
            terminal_state::input::KeyboardProtocol::Xterm,
            false,
            false,
        );
        bridge.push(&bytes);
        let snapshot = bridge.snapshot();
        let _ = wgpu_renderer::render::build_plan(
            &snapshot,
            &wgpu_renderer::render::DrawBudget::default(),
        );
        e2e_latency.push(start.elapsed().as_secs_f64() * 1_000_000.0); // us
    }

    // 3. Scrollback: insert 10k lines, measure per-insert cost.
    let mut scroll = Vec::with_capacity(REPEATS);
    for _ in 0..REPEATS {
        let mut model = ScreenModel::new(1, 24, 80);
        model.set_scrollback_config(terminal_state::ScrollbackConfig::default());
        let start = Instant::now();
        for _ in 0..10_000 {
            let batch = BoundedByteStreamParser::new().feed(b"line\r\n");
            model.apply_batch(&batch);
        }
        scroll.push(start.elapsed().as_secs_f64() * 1_000.0); // ms per 10k lines
    }

    let summary = serde_json::json!({
        "task": "T156",
        "benchmarks": {
            "parse_throughput_mbps": { "p50": format!("{p50_mbps:.1}"), "p95": format!("{p95_mbps:.1}") },
            "input_to_pixel_us": stats_us(&latency),
            "e2e_input_to_pixel_us": stats_us(&e2e_latency),
            "scrollback_10k_lines_ms": stats_ms(&scroll),
        }
    });
    println!("BENCH_METRICS {summary}");
}
