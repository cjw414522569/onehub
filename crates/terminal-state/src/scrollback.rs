//! Configurable scrollback ring buffer and disk dump policy (T069).
//!
//! Lines that scroll off the top of the primary screen are captured in a
//! bounded ring buffer instead of being dropped. The memory limit is explicit
//! ([`ScrollbackConfig::max_lines`]); the buffer never exceeds it, so a
//! million-line session stays within a known bound. Dumping scrollback to
//! disk is policy-gated and **off by default** so sensitive terminal content
//! is never persisted without explicit opt-in.

use std::collections::VecDeque;

use core_protocol::terminal::TerminalRow;

/// Configurable scrollback limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbackConfig {
    /// Maximum number of lines retained (0 disables scrollback capture).
    pub max_lines: usize,
}

impl Default for ScrollbackConfig {
    fn default() -> Self {
        Self { max_lines: 10_000 }
    }
}

/// A bounded ring buffer of rows scrolled off the primary screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scrollback {
    rows: VecDeque<TerminalRow>,
    max_lines: usize,
    /// Total lines ever pushed (for stats / benchmarks).
    lines_pushed: u64,
    /// Lines evicted because the capacity was reached.
    lines_dropped: u64,
}

impl Scrollback {
    /// A scrollback buffer holding at most `max_lines` rows.
    pub fn new(max_lines: usize) -> Self {
        Self {
            rows: VecDeque::with_capacity(max_lines.min(4096)),
            max_lines,
            lines_pushed: 0,
            lines_dropped: 0,
        }
    }

    /// A scrollback buffer from a [`ScrollbackConfig`].
    pub fn with_config(config: &ScrollbackConfig) -> Self {
        Self::new(config.max_lines)
    }

    /// Appends a row, evicting the oldest when over capacity. When
    /// `max_lines == 0` capture is disabled and nothing is retained.
    pub fn push(&mut self, row: TerminalRow) {
        self.lines_pushed += 1;
        if self.max_lines == 0 {
            self.lines_dropped += 1;
            return;
        }
        self.rows.push_back(row);
        if self.rows.len() > self.max_lines {
            self.rows.pop_front();
            self.lines_dropped += 1;
        }
    }

    /// Number of retained rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The configured capacity.
    pub fn max_lines(&self) -> usize {
        self.max_lines
    }

    /// Total lines pushed (monotonic).
    pub fn lines_pushed(&self) -> u64 {
        self.lines_pushed
    }

    /// Lines evicted due to the capacity bound.
    pub fn lines_dropped(&self) -> u64 {
        self.lines_dropped
    }

    /// Adjusts the capacity; oldest rows are evicted when shrinking.
    pub fn set_max_lines(&mut self, max_lines: usize) {
        self.max_lines = max_lines;
        while self.rows.len() > self.max_lines {
            self.rows.pop_front();
            self.lines_dropped += 1;
        }
    }

    /// Clears all retained rows (stats are kept).
    pub fn clear(&mut self) {
        self.rows.clear();
    }

    /// Iterates retained rows from oldest to newest.
    pub fn iter(&self) -> impl Iterator<Item = &TerminalRow> {
        self.rows.iter()
    }

    /// The `n`-th most recent row, if within the retained window.
    pub fn get(&self, index: usize) -> Option<&TerminalRow> {
        self.rows.get(index)
    }
}

/// Disk dump policy for scrollback content. Sensitive dumps are **off by
/// default**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollbackDumpPolicy {
    /// Whether scrollback may be dumped to disk.
    pub allow_dump: bool,
    /// Maximum dump size in bytes (UTF-8 text).
    pub max_bytes: usize,
}

impl Default for ScrollbackDumpPolicy {
    fn default() -> Self {
        Self {
            allow_dump: false,
            max_bytes: 16 * 1024 * 1024,
        }
    }
}

impl ScrollbackDumpPolicy {
    /// Whether a dump is currently permitted.
    pub fn permitted(&self) -> bool {
        self.allow_dump
    }

    /// Renders a bounded text dump of the scrollback, or `None` when dumps
    /// are not permitted.
    ///
    /// The dump concatenates cell text per retained row, joined by newlines,
    /// and truncates to `max_bytes`; the returned string is always bounded.
    pub fn dump(&self, scrollback: &Scrollback, cols: usize) -> Option<String> {
        if !self.allow_dump {
            return None;
        }
        let mut out = String::new();
        for row in scrollback.iter() {
            let mut line = String::new();
            for cell in row.cells.iter().take(cols) {
                if let Some(text) = cell.text.as_deref() {
                    line.push_str(text);
                }
            }
            if out.len() + line.len() + 1 > self.max_bytes {
                break;
            }
            out.push_str(&line);
            out.push('\n');
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use core_protocol::terminal::{TerminalCell, TerminalRow};

    use super::{Scrollback, ScrollbackConfig, ScrollbackDumpPolicy};

    fn row(text: &str) -> TerminalRow {
        TerminalRow {
            cells: vec![TerminalCell::cluster(text)],
        }
    }

    #[test]
    fn ring_buffer_bounds_memory_explicitly() {
        let mut scrollback = Scrollback::new(3);
        for i in 0..10 {
            scrollback.push(row(&i.to_string()));
        }
        assert_eq!(scrollback.len(), 3, "never exceeds capacity");
        assert_eq!(scrollback.max_lines(), 3);
        assert_eq!(scrollback.lines_pushed(), 10);
        assert_eq!(scrollback.lines_dropped(), 7);
        // Oldest retained row is 7, newest is 9.
        assert_eq!(
            scrollback.get(0).unwrap().cells[0].text.as_deref(),
            Some("7")
        );
        assert_eq!(
            scrollback.get(2).unwrap().cells[0].text.as_deref(),
            Some("9")
        );
    }

    #[test]
    fn million_line_scroll_memory_benchmark_is_bounded() {
        // Benchmark: 1,000,000 lines through a 10,000-line ring buffer stays
        // within the explicit bound.
        let mut scrollback = Scrollback::new(10_000);
        for i in 0..1_000_000u32 {
            scrollback.push(row(&i.to_string()));
        }
        assert_eq!(scrollback.len(), 10_000);
        assert_eq!(scrollback.lines_pushed(), 1_000_000);
        assert_eq!(scrollback.lines_dropped(), 1_000_000 - 10_000);
    }

    #[test]
    fn zero_capacity_disables_capture() {
        let mut scrollback = Scrollback::new(0);
        scrollback.push(row("x"));
        assert!(scrollback.is_empty());
        assert_eq!(scrollback.lines_dropped(), 1);
    }

    #[test]
    fn config_controls_capacity() {
        let scrollback = Scrollback::with_config(&ScrollbackConfig { max_lines: 5 });
        assert_eq!(scrollback.max_lines(), 5);
    }

    #[test]
    fn dump_is_off_by_default_and_bounded_when_enabled() {
        let policy = ScrollbackDumpPolicy::default();
        assert!(
            !policy.permitted(),
            "sensitive dumps must be off by default"
        );
        let mut scrollback = Scrollback::new(4);
        for text in ["alpha", "beta", "gamma"] {
            scrollback.push(row(text));
        }
        assert_eq!(policy.dump(&scrollback, 20), None, "dump refused");

        let policy = ScrollbackDumpPolicy {
            allow_dump: true,
            max_bytes: 12,
        };
        let dump = policy.dump(&scrollback, 20).expect("dump allowed");
        // "alpha\nbeta\n" = 11 bytes; "gamma" would exceed 12 -> truncated.
        assert_eq!(dump, "alpha\nbeta\n");
        assert!(dump.len() <= 12);
    }
}
