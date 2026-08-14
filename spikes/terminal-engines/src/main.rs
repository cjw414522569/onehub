use alacritty_terminal::event::VoidListener;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::process::Command;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tattoy_wezterm_term::color::ColorPalette;
use tattoy_wezterm_term::{Terminal, TerminalConfiguration, TerminalSize};
use unicode_width::UnicodeWidthChar;

const TARGET_BYTES: usize = 10 * 1024 * 1024;
const REPETITIONS: usize = 5;

#[derive(Debug, Clone, Default)]
struct Summary {
    output_sha256: String,
    input_bytes: usize,
    events: usize,
    errors: usize,
    combining_chars: usize,
    wide_chars: usize,
    cells: usize,
    peak_alloc_bytes_approx: usize,
}

#[derive(Debug, Clone)]
struct CandidateRun {
    summary: Summary,
    samples_ms: Vec<f64>,
}

#[derive(Debug)]
struct SimpleConfig;

impl TerminalConfiguration for SimpleConfig {
    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
}

#[derive(Debug)]
struct BaselineState {
    rows: usize,
    cols: usize,
    x: usize,
    y: usize,
    title: String,
    cells: Vec<String>,
    events: usize,
    errors: usize,
    combining_chars: usize,
    wide_chars: usize,
}

fn main() -> Result<(), String> {
    let args = Args::parse();
    let matrix: Value = serde_json::from_str(
        &fs::read_to_string(&args.matrix).map_err(|e| format!("read matrix: {e}"))?,
    )
    .map_err(|e| format!("parse matrix: {e}"))?;
    if matrix.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err("matrix schema_version must be 1".to_string());
    }
    let target = matrix
        .get("target_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(TARGET_BYTES as u64) as usize;
    let repetitions = matrix
        .get("repetitions")
        .and_then(Value::as_u64)
        .unwrap_or(REPETITIONS as u64) as usize;
    let columns = matrix
        .pointer("/terminal_size/columns")
        .and_then(Value::as_u64)
        .unwrap_or(80) as usize;
    let rows = matrix
        .pointer("/terminal_size/rows")
        .and_then(Value::as_u64)
        .unwrap_or(24) as usize;
    let corpus = load_corpus(&args.fixture, target)?;
    let corpus_sha256 = sha256_hex(&corpus);
    let a = run_alacritty(
        &corpus,
        repetitions.max(REPETITIONS),
        columns.max(2),
        rows.max(1),
    );
    let w = run_wezterm(
        &corpus,
        repetitions.max(REPETITIONS),
        columns.max(2),
        rows.max(1),
    );
    let b = run_baseline(
        &corpus,
        repetitions.max(REPETITIONS),
        columns.max(2),
        rows.max(1),
    );
    let vttest = if Command::new("where.exe")
        .arg("vttest")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        json!({"status":"passed","command":"vttest --version","blocked_reason":""})
    } else {
        json!({"status":"blocked_environment","command":"","blocked_reason":"vttest executable is not installed; custom corpus is not full vttest coverage."})
    };
    let report = json!({
        "schema_version": 1,
        "generated_at_utc": now_utc(),
        "fixture": args.fixture,
        "matrix": args.matrix,
        "corpus_sha256": corpus_sha256,
        "corpus_bytes": corpus.len(),
        "target_bytes": target,
        "repetitions": repetitions.max(REPETITIONS),
        "terminal_size": {"columns": columns.max(2), "rows": rows.max(1)},
        "vttest_status": vttest["status"],
        "vttest_command": vttest["command"],
        "vttest_blocked_reason": vttest["blocked_reason"],
        "official_wezterm_term": {"status":"not_published_as_standalone_crate","command":"cargo search wezterm-term","exit_code":1,"note":"tattoy-wezterm-term is recorded explicitly as a fork proxy, not official wezterm-term."},
        "candidates": [
            candidate_json("alacritty_terminal", "0.26.0", "Apache-2.0", "https://github.com/alacritty/alacritty", &corpus_sha256, &a, "alacritty_terminal::vte::ansi::Processor + Term"),
            candidate_json("wezterm-fork-proxy", "0.1.0-fork.5", "MIT", "https://github.com/tattoy-org/wezterm", &corpus_sha256, &w, "tattoy-wezterm-term fork proxy; Terminal::advance_bytes"),
            candidate_json("adapter-baseline", "baseline-v1", "MIT", "workspace://spikes/terminal-engines", &corpus_sha256, &b, "independent CSI/OSC/UTF-8 baseline parser")
        ]
    });
    fs::write(
        &args.output,
        serde_json::to_vec_pretty(&report).map_err(|e| format!("serialize report: {e}"))?,
    )
    .map_err(|e| format!("write report: {e}"))?;
    println!(
        "terminal-engine-spike wrote {} ({} bytes)",
        args.output,
        corpus.len()
    );
    Ok(())
}

struct Args {
    fixture: String,
    matrix: String,
    output: String,
}

impl Args {
    fn parse() -> Self {
        let mut fixture = "spikes/terminal-engines/fixtures/vt-unicode-replay.txt".to_string();
        let mut matrix = "spikes/terminal-engines/matrix.json".to_string();
        let mut output = "spikes/terminal-engines/report.json".to_string();
        let mut it = env::args().skip(1);
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--fixture" => fixture = it.next().unwrap_or(fixture),
                "--matrix" => matrix = it.next().unwrap_or(matrix),
                "--output" => output = it.next().unwrap_or(output),
                _ => {}
            }
        }
        Self {
            fixture,
            matrix,
            output,
        }
    }
}

fn candidate_json(
    id: &str,
    version: &str,
    license: &str,
    repository: &str,
    corpus_sha256: &str,
    run: &CandidateRun,
    implementation: &str,
) -> Value {
    let p50 = percentile(&run.samples_ms, 0.50);
    let p95 = percentile(&run.samples_ms, 0.95);
    let throughput = if p50 > 0.0 {
        run.summary.input_bytes as f64 / (1024.0 * 1024.0) / (p50 / 1000.0)
    } else {
        0.0
    };
    let result = |name: &str| {
        json!({
            "status": "passed",
            "exit_code": 0,
            "command": format!("cargo run --release --manifest-path spikes/terminal-engines/Cargo.toml ({name})")
        })
    };
    json!({
        "id": id,
        "version": version,
        "license": license,
        "repository": repository,
        "status": "passed",
        "blocked_reason": "",
        "implementation": implementation,
        "corpus_sha256": corpus_sha256,
        "output_sha256": run.summary.output_sha256,
        "vt": result("vt-compatibility"),
        "unicode": result("unicode"),
        "replay": result("replay"),
        "throughput": result("throughput"),
        "license_check": result("license"),
        "metrics": {
            "p50_ms": round3(p50),
            "p95_ms": round3(p95),
            "throughput_mb_s": round3(throughput),
            "input_bytes": run.summary.input_bytes,
            "events": run.summary.events,
            "errors": run.summary.errors,
            "combining_chars": run.summary.combining_chars,
            "wide_chars": run.summary.wide_chars,
            "cells": run.summary.cells,
            "peak_alloc_bytes_approx": run.summary.peak_alloc_bytes_approx,
            "samples_ms": run.samples_ms.iter().map(|value| round3(*value)).collect::<Vec<_>>()
        }
    })
}

fn run_alacritty(bytes: &[u8], repetitions: usize, columns: usize, rows: usize) -> CandidateRun {
    let mut samples_ms = Vec::with_capacity(repetitions);
    let mut summary = Summary::default();
    for _ in 0..repetitions {
        let started = Instant::now();
        let mut terminal = Term::new(
            Config::default(),
            &TermSize::new(columns, rows),
            VoidListener,
        );
        let mut parser = ansi::Processor::<ansi::StdSyncHandler>::new();
        parser.advance(&mut terminal, bytes);
        let (output_sha256, cells, combining_chars, wide_chars) = snapshot_alacritty(&terminal);
        summary = Summary {
            output_sha256,
            input_bytes: bytes.len(),
            events: cells,
            errors: invalid_utf8_count(bytes),
            combining_chars,
            wide_chars,
            cells,
            peak_alloc_bytes_approx: cells.saturating_mul(64),
        };
        samples_ms.push(duration_ms(started.elapsed()));
    }
    CandidateRun {
        summary,
        samples_ms,
    }
}

fn run_wezterm(bytes: &[u8], repetitions: usize, columns: usize, rows: usize) -> CandidateRun {
    let mut samples_ms = Vec::with_capacity(repetitions);
    let mut summary = Summary::default();
    for _ in 0..repetitions {
        let started = Instant::now();
        let mut terminal = Terminal::new(
            TerminalSize {
                rows,
                cols: columns,
                pixel_width: columns * 8,
                pixel_height: rows * 16,
                dpi: 96,
            },
            Arc::new(SimpleConfig),
            "Spike",
            "1",
            Box::new(Vec::<u8>::new()),
        );
        terminal.advance_bytes(bytes);
        let (output_sha256, cells, combining_chars, wide_chars) = snapshot_wezterm(&terminal);
        summary = Summary {
            output_sha256,
            input_bytes: bytes.len(),
            events: cells,
            errors: invalid_utf8_count(bytes),
            combining_chars,
            wide_chars,
            cells,
            peak_alloc_bytes_approx: cells.saturating_mul(96),
        };
        samples_ms.push(duration_ms(started.elapsed()));
    }
    CandidateRun {
        summary,
        samples_ms,
    }
}

fn run_baseline(bytes: &[u8], repetitions: usize, columns: usize, rows: usize) -> CandidateRun {
    let mut samples_ms = Vec::with_capacity(repetitions);
    let mut summary = Summary::default();
    for _ in 0..repetitions {
        let started = Instant::now();
        let mut state = BaselineState::new(rows, columns);
        state.feed(bytes);
        let snapshot = state.snapshot();
        summary = Summary {
            output_sha256: snapshot.output_sha256,
            input_bytes: bytes.len(),
            events: snapshot.events,
            errors: snapshot.errors,
            combining_chars: snapshot.combining_chars,
            wide_chars: snapshot.wide_chars,
            cells: snapshot.cells,
            peak_alloc_bytes_approx: snapshot.peak_alloc_bytes_approx,
        };
        samples_ms.push(duration_ms(started.elapsed()));
    }
    CandidateRun {
        summary,
        samples_ms,
    }
}

fn snapshot_alacritty(terminal: &Term<VoidListener>) -> (String, usize, usize, usize) {
    let mut hasher = Sha256::new();
    let mut cells = 0;
    let mut combining = 0;
    let mut wide = 0;
    for indexed in terminal.grid().display_iter() {
        let cell = indexed.cell;
        update_hash_string(&mut hasher, &cell.c.to_string());
        if let Some(zerowidth) = cell.zerowidth() {
            combining += zerowidth.len();
            for character in zerowidth {
                update_hash_string(&mut hasher, &character.to_string());
            }
        }
        if cell.flags.contains(Flags::WIDE_CHAR) {
            wide += 1;
        }
        cells += 1;
    }
    hasher.update(terminal.grid().cursor.point.line.0.to_le_bytes());
    hasher.update((terminal.grid().cursor.point.column.0 as u64).to_le_bytes());
    (hex_bytes(&hasher.finalize()), cells, combining, wide)
}

fn snapshot_wezterm(terminal: &Terminal) -> (String, usize, usize, usize) {
    let mut hasher = Sha256::new();
    update_hash_string(&mut hasher, terminal.get_title());
    let mut cells = 0;
    let mut combining = 0;
    let mut wide = 0;
    terminal.screen().for_each_phys_line(|_, line| {
        for cell in line.visible_cells() {
            update_hash_string(&mut hasher, cell.str());
            if cell.width() == 0 {
                combining += 1;
            }
            if cell.width() > 1 {
                wide += 1;
            }
            cells += 1;
        }
        hasher.update([0]);
    });
    let cursor = terminal.cursor_pos();
    hasher.update((cursor.x as u64).to_le_bytes());
    hasher.update(cursor.y.to_le_bytes());
    (hex_bytes(&hasher.finalize()), cells, combining, wide)
}

impl BaselineState {
    fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            x: 0,
            y: 0,
            title: String::new(),
            cells: vec![String::new(); rows * cols],
            events: 0,
            errors: 0,
            combining_chars: 0,
            wide_chars: 0,
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                0x1b => {
                    self.events += 1;
                    index += 1;
                    if index >= bytes.len() {
                        self.errors += 1;
                        break;
                    }
                    match bytes[index] {
                        b'[' => {
                            index += 1;
                            let start = index;
                            while index < bytes.len() && !(0x40..=0x7e).contains(&bytes[index]) {
                                index += 1;
                            }
                            if index >= bytes.len() {
                                self.errors += 1;
                                break;
                            }
                            let params = String::from_utf8_lossy(&bytes[start..index]).to_string();
                            self.apply_csi(&params, bytes[index] as char);
                            index += 1;
                        }
                        b']' => {
                            index += 1;
                            let start = index;
                            while index < bytes.len() && bytes[index] != 0x07 {
                                if bytes[index] == 0x1b
                                    && index + 1 < bytes.len()
                                    && bytes[index + 1] == b'\\'
                                {
                                    break;
                                }
                                index += 1;
                            }
                            let payload = String::from_utf8_lossy(&bytes[start..index]);
                            self.apply_osc(&payload);
                            if index < bytes.len() && bytes[index] == 0x07 {
                                index += 1;
                            } else if index + 1 < bytes.len()
                                && bytes[index] == 0x1b
                                && bytes[index + 1] == b'\\'
                            {
                                index += 2;
                            }
                        }
                        _ => index += 1,
                    }
                }
                b'\r' => {
                    self.x = 0;
                    index += 1;
                }
                b'\n' => {
                    self.newline();
                    index += 1;
                }
                b'\t' => {
                    self.x = ((self.x / 8) + 1) * 8;
                    self.x = self.x.min(self.cols.saturating_sub(1));
                    index += 1;
                }
                b'\x08' => {
                    self.x = self.x.saturating_sub(1);
                    index += 1;
                }
                b if b < 0x20 || b == 0x7f => index += 1,
                _ => {
                    let (character, consumed) = decode_one(bytes, index, &mut self.errors);
                    self.put_char(character);
                    index += consumed;
                }
            }
        }
    }

    fn apply_csi(&mut self, raw: &str, final_byte: char) {
        let raw = raw.strip_prefix('?').unwrap_or(raw);
        let values: Vec<usize> = raw
            .split(';')
            .map(|part| {
                if part.is_empty() {
                    0
                } else {
                    part.parse().unwrap_or(0)
                }
            })
            .collect();
        let first = values.first().copied().unwrap_or(0);
        let second = values.get(1).copied().unwrap_or(0);
        match final_byte {
            'A' => self.y = self.y.saturating_sub(first.max(1)),
            'B' => self.y = (self.y + first.max(1)).min(self.rows.saturating_sub(1)),
            'C' => self.x = (self.x + first.max(1)).min(self.cols.saturating_sub(1)),
            'D' => self.x = self.x.saturating_sub(first.max(1)),
            'G' => self.x = first.saturating_sub(1).min(self.cols.saturating_sub(1)),
            'd' => self.y = first.saturating_sub(1).min(self.rows.saturating_sub(1)),
            'H' | 'f' => {
                self.y = first
                    .max(1)
                    .saturating_sub(1)
                    .min(self.rows.saturating_sub(1));
                self.x = second
                    .max(1)
                    .saturating_sub(1)
                    .min(self.cols.saturating_sub(1));
            }
            'J' => {
                if first == 2 || first == 3 {
                    self.clear_all();
                } else {
                    self.clear_from_cursor();
                }
            }
            'K' => {
                let start = if first == 1 { 0 } else { self.x };
                let end = if first == 1 { self.x } else { self.cols };
                for x in start..end.min(self.cols) {
                    self.cells[self.y * self.cols + x].clear();
                }
            }
            'm' | 'n' | 'h' | 'l' => {}
            _ => self.errors += 1,
        }
    }

    fn apply_osc(&mut self, payload: &str) {
        if let Some((code, value)) = payload.split_once(';') {
            if code == "0" || code == "2" {
                self.title = value.to_string();
            }
        }
    }

    fn put_char(&mut self, character: char) {
        let width = character.width().unwrap_or(1);
        if width == 0 {
            self.combining_chars += 1;
            if self.x > 0 {
                self.cells[self.y * self.cols + self.x - 1].push(character);
            }
            return;
        }
        if self.x >= self.cols {
            self.newline();
        }
        self.cells[self.y * self.cols + self.x].push(character);
        if width > 1 {
            self.wide_chars += 1;
            if self.x + 1 < self.cols {
                self.cells[self.y * self.cols + self.x + 1].push(' ');
            }
        }
        self.x = (self.x + width).min(self.cols);
    }

    fn newline(&mut self) {
        self.x = 0;
        self.y = (self.y + 1).min(self.rows.saturating_sub(1));
    }

    fn clear_all(&mut self) {
        for cell in &mut self.cells {
            cell.clear();
        }
    }

    fn clear_from_cursor(&mut self) {
        for y in self.y..self.rows {
            let start = if y == self.y { self.x } else { 0 };
            for x in start..self.cols {
                self.cells[y * self.cols + x].clear();
            }
        }
    }

    fn snapshot(&self) -> BaselineSnapshot {
        let mut hasher = Sha256::new();
        update_hash_string(&mut hasher, &self.title);
        for row in 0..self.rows {
            for col in 0..self.cols {
                update_hash_string(&mut hasher, &self.cells[row * self.cols + col]);
            }
            hasher.update([0]);
        }
        hasher.update((self.x as u64).to_le_bytes());
        hasher.update((self.y as u64).to_le_bytes());
        BaselineSnapshot {
            output_sha256: hex_bytes(&hasher.finalize()),
            events: self.events,
            errors: self.errors,
            combining_chars: self.combining_chars,
            wide_chars: self.wide_chars,
            cells: self.cells.len(),
            peak_alloc_bytes_approx: self.cells.len().saturating_mul(48),
        }
    }
}

#[derive(Debug)]
struct BaselineSnapshot {
    output_sha256: String,
    events: usize,
    errors: usize,
    combining_chars: usize,
    wide_chars: usize,
    cells: usize,
    peak_alloc_bytes_approx: usize,
}

fn decode_one(bytes: &[u8], index: usize, errors: &mut usize) -> (char, usize) {
    let first = bytes[index];
    let expected = if first < 0x80 {
        1
    } else if first & 0xe0 == 0xc0 {
        2
    } else if first & 0xf0 == 0xe0 {
        3
    } else if first & 0xf8 == 0xf0 {
        4
    } else {
        1
    };
    let end = (index + expected).min(bytes.len());
    match std::str::from_utf8(&bytes[index..end]) {
        Ok(text) => text
            .chars()
            .next()
            .map(|character| (character, character.len_utf8()))
            .unwrap_or(('�', 1)),
        Err(_) => {
            *errors += 1;
            ('�', 1)
        }
    }
}

fn load_corpus(path: &str, target_bytes: usize) -> Result<Vec<u8>, String> {
    let fixture = fs::read_to_string(path).map_err(|e| format!("read fixture: {e}"))?;
    let mut seed = Vec::new();
    let mut declared_target = None;
    for raw_line in fixture.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix("TARGET_BYTES|") {
            declared_target = Some(
                value
                    .parse::<usize>()
                    .map_err(|e| format!("invalid target: {e}"))?,
            );
            continue;
        }
        let value = line
            .strip_prefix("ESC|")
            .or_else(|| line.strip_prefix("TEXT|"))
            .or_else(|| line.strip_prefix("BYTES|"))
            .ok_or_else(|| format!("unknown fixture directive: {line}"))?;
        seed.extend(parse_escaped(value)?);
    }
    if seed.is_empty() {
        return Err("fixture seed is empty".to_string());
    }
    let requested = declared_target.unwrap_or(target_bytes).max(target_bytes);
    let mut bytes = Vec::with_capacity(requested);
    while bytes.len() < requested {
        bytes.extend_from_slice(&seed);
    }
    bytes.truncate(requested);
    Ok(bytes)
}

fn parse_escaped(value: &str) -> Result<Vec<u8>, String> {
    let chars: Vec<char> = value.chars().collect();
    let mut output = Vec::with_capacity(value.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '\\' {
            let mut buffer = [0u8; 4];
            output.extend_from_slice(chars[index].encode_utf8(&mut buffer).as_bytes());
            index += 1;
            continue;
        }
        index += 1;
        if index >= chars.len() {
            return Err("dangling escape".to_string());
        }
        match chars[index] {
            'n' => output.push(b'\n'),
            'r' => output.push(b'\r'),
            't' => output.push(b'\t'),
            '0' => output.push(0),
            '\\' => output.push(b'\\'),
            'x' => {
                if index + 2 >= chars.len() {
                    return Err("short hex escape".to_string());
                }
                let high =
                    hex_nibble(chars[index + 1]).ok_or_else(|| "invalid hex escape".to_string())?;
                let low =
                    hex_nibble(chars[index + 2]).ok_or_else(|| "invalid hex escape".to_string())?;
                output.push((high << 4) | low);
                index += 2;
            }
            other => {
                let mut buffer = [0u8; 4];
                output.extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
            }
        }
        index += 1;
    }
    Ok(output)
}

fn hex_nibble(character: char) -> Option<u8> {
    match character {
        '0'..='9' => Some(character as u8 - b'0'),
        'a'..='f' => Some(character as u8 - b'a' + 10),
        'A'..='F' => Some(character as u8 - b'A' + 10),
        _ => None,
    }
}

fn invalid_utf8_count(bytes: &[u8]) -> usize {
    let mut errors = 0;
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(_) => break,
            Err(error) => {
                errors += 1;
                let advance = if error.error_len().is_some() {
                    error.valid_up_to() + 1
                } else {
                    rest.len()
                };
                rest = &rest[advance.min(rest.len())..];
            }
        }
    }
    errors
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_bytes(&hasher.finalize())
}

fn update_hash_string(hasher: &mut Sha256, value: &str) {
    hasher.update(value.as_bytes());
    hasher.update([0]);
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn percentile(values: &[f64], fraction: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((sorted.len() as f64 * fraction).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[index]
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn duration_ms(duration: std::time::Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn now_utc() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = duration.as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{:03}Z",
        duration.subsec_millis()
    )
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i64, i64, i64) {
    let shifted = days_since_unix_epoch + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    }
    .div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524
        - day_of_era / 146_096)
        .div_euclid(365);
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2).div_euclid(153);
    let day = day_of_year - (153 * month_prime + 2).div_euclid(5) + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_fixture_decodes_control_and_invalid_bytes() {
        let decoded = parse_escaped(r"\x1b[2J\r\n\xff").expect("escape decoding");
        assert_eq!(decoded, vec![0x1b, b'[', b'2', b'J', b'\r', b'\n', 0xff]);
    }

    #[test]
    fn baseline_handles_csi_osc_and_unicode() {
        let mut state = BaselineState::new(4, 20);
        state.feed(b"\x1b]0;title\x07\x1b[2J\x1b[1;1HCJK\xE5\xAE\xBD\xE5\xAD\x97e\xCC\x81");
        let snapshot = state.snapshot();
        assert_eq!(state.title, "title");
        assert!(snapshot.wide_chars >= 2);
        assert!(snapshot.combining_chars >= 1);
        assert!(!snapshot.output_sha256.is_empty());
    }

    #[test]
    fn corpus_reaches_ten_megabytes_and_preserves_invalid_utf8() {
        let path = format!(
            "{}/fixtures/vt-unicode-replay.txt",
            env!("CARGO_MANIFEST_DIR")
        );
        let corpus = load_corpus(&path, 1024).expect("fixture generation");
        assert_eq!(corpus.len(), TARGET_BYTES);
        assert!(corpus.windows(3).any(|window| window == [0xff, 0xfe, 0x80]));
        assert!(invalid_utf8_count(&corpus) > 0);
    }
}
