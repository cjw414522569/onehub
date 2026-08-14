//! Search, regex search, and result navigation (T071).
//!
//! [`SearchSession`] searches a buffer of lines (scrollback + visible screen)
//! in bounded chunks, checking a cancellation flag between lines, so a
//! million-line search can run on a worker thread, be cancelled, and never
//! block input handling. Plain-text and regex queries are supported, and
//! [`SearchNavigation`] cycles through the results.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use regex::Regex;

use super::scrollback::Scrollback;
use core_protocol::terminal::TerminalSnapshot;

/// A search query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    /// The pattern: a literal string or a regular expression.
    pub pattern: String,
    /// Case-sensitive matching.
    pub case_sensitive: bool,
    /// Interpret `pattern` as a regular expression.
    pub regex: bool,
}

impl SearchQuery {
    /// A case-sensitive literal search.
    pub fn literal(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            case_sensitive: true,
            regex: false,
        }
    }
}

/// One match within the search buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    /// Absolute 0-based line index (scrollback first, then visible screen).
    pub line: usize,
    /// Match start column (char offset within the line text).
    pub col: usize,
    /// Match length in characters.
    pub length: usize,
}

/// A read view over the lines to search: retained scrollback rows followed by
/// the visible screen rows.
pub struct SearchBuffer<'a> {
    scrollback: &'a Scrollback,
    screen: &'a TerminalSnapshot,
    /// Columns used to render each scrollback row to text.
    cols: usize,
}

impl<'a> SearchBuffer<'a> {
    /// A search buffer over scrollback + the visible screen.
    pub fn new(scrollback: &'a Scrollback, screen: &'a TerminalSnapshot, cols: usize) -> Self {
        Self {
            scrollback,
            screen,
            cols,
        }
    }

    /// Total number of lines (scrollback + screen).
    pub fn line_count(&self) -> usize {
        self.scrollback.len() + self.screen.rows.len()
    }

    /// The text of line `index`, or `None` when out of range.
    pub fn line_text(&self, index: usize) -> Option<String> {
        let scrolled = self.scrollback.len();
        if index < scrolled {
            let row = self.scrollback.get(index)?;
            Some(render_row(row, self.cols))
        } else {
            let screen_index = index - scrolled;
            let row = self.screen.rows.get(screen_index)?;
            Some(render_row(row, self.cols))
        }
    }
}

/// Renders a row's cells to text (continuation cells add nothing).
fn render_row(row: &core_protocol::terminal::TerminalRow, cols: usize) -> String {
    let mut out = String::new();
    for cell in row.cells.iter().take(cols) {
        if let Some(text) = cell.text.as_deref() {
            out.push_str(text);
        }
    }
    out
}

/// Finds all matches of `query` within `line`, returning `(col, length)` in
/// character offsets.
fn matches_in_line(line: &str, query: &SearchQuery, regex: &Option<Regex>) -> Vec<(usize, usize)> {
    let char_col = |byte: usize| line[..byte].chars().count();
    if let Some(regex) = regex {
        regex
            .find_iter(line)
            .map(|m| (char_col(m.start()), m.end() - m.start()))
            .collect()
    } else if query.case_sensitive {
        let needle = &query.pattern;
        let mut out = Vec::new();
        let mut start = 0usize;
        while let Some(relative) = line[start..].find(needle) {
            let absolute = start + relative;
            out.push((char_col(absolute), needle.chars().count()));
            start = absolute + needle.len();
        }
        out
    } else {
        let needle = query.pattern.to_lowercase();
        let haystack = line.to_lowercase();
        let mut out = Vec::new();
        let mut start = 0usize;
        while let Some(relative) = haystack[start..].find(&needle) {
            let absolute = start + relative;
            out.push((char_col(absolute), needle.chars().count()));
            start = absolute + needle.len();
        }
        out
    }
}

/// An incremental, cancellable search over a [`SearchBuffer`].
pub struct SearchSession<'a> {
    buffer: SearchBuffer<'a>,
    query: SearchQuery,
    regex: Option<Regex>,
    cancel: Arc<AtomicBool>,
    chunk_lines: usize,
    next_line: usize,
    results: Vec<SearchResult>,
    done: bool,
}

impl<'a> SearchSession<'a> {
    /// A new session searching `buffer` for `query` in chunks of
    /// `chunk_lines` lines per [`SearchSession::step`].
    pub fn new(
        buffer: SearchBuffer<'a>,
        query: SearchQuery,
        chunk_lines: usize,
    ) -> Result<Self, String> {
        let regex = if query.regex {
            let mut builder = regex::RegexBuilder::new(&query.pattern);
            builder.case_insensitive(!query.case_sensitive);
            Some(builder.build().map_err(|e| e.to_string())?)
        } else {
            None
        };
        Ok(Self {
            buffer,
            query,
            regex,
            cancel: Arc::new(AtomicBool::new(false)),
            chunk_lines: chunk_lines.max(1),
            next_line: 0,
            results: Vec::new(),
            done: false,
        })
    }

    /// Requests cancellation; the next step stops early.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Whether cancellation was requested.
    pub fn was_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Whether the search finished (all lines searched or cancelled).
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// The cancellation token (for driving from another thread).
    pub fn cancel_token(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    /// Searches the next chunk. Returns the number of new matches found, or
    /// `None` when the search is finished or cancelled.
    pub fn step(&mut self) -> Option<usize> {
        if self.done {
            return None;
        }
        let mut found = 0usize;
        let end = (self.next_line + self.chunk_lines).min(self.buffer.line_count());
        while self.next_line < end {
            if self.cancel.load(Ordering::Relaxed) {
                self.done = true;
                return None;
            }
            if let Some(line) = self.buffer.line_text(self.next_line) {
                for (col, length) in matches_in_line(&line, &self.query, &self.regex) {
                    self.results.push(SearchResult {
                        line: self.next_line,
                        col,
                        length,
                    });
                    found += 1;
                }
            }
            self.next_line += 1;
        }
        if self.next_line >= self.buffer.line_count() {
            self.done = true;
        }
        Some(found)
    }

    /// Runs the full search to completion (used when a dedicated worker thread
    /// drives it); returns the results.
    pub fn run(mut self) -> Vec<SearchResult> {
        while !self.done {
            let _ = self.step();
        }
        self.results
    }

    /// All results found so far.
    pub fn results(&self) -> &[SearchResult] {
        &self.results
    }

    /// The next line to search (progress metric).
    pub fn lines_searched(&self) -> usize {
        self.next_line
    }
}

/// Cycles through search results (previous/next navigation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchNavigation {
    total: usize,
    current: usize,
}

impl SearchNavigation {
    /// Navigation over `total` results (starts at index 0 when non-empty).
    pub fn new(total: usize) -> Self {
        Self { total, current: 0 }
    }

    /// Total number of results.
    pub fn total(&self) -> usize {
        self.total
    }

    /// The current result index (0-based); `None` when there are no results.
    pub fn current(&self) -> Option<usize> {
        if self.total == 0 {
            None
        } else {
            Some(self.current)
        }
    }

    /// Advances to the next result (wraps around).
    pub fn next_result(&mut self) -> Option<usize> {
        if self.total == 0 {
            return None;
        }
        self.current = (self.current + 1) % self.total;
        Some(self.current)
    }

    /// Moves to the previous result (wraps around).
    pub fn prev_result(&mut self) -> Option<usize> {
        if self.total == 0 {
            return None;
        }
        self.current = (self.current + self.total - 1) % self.total;
        Some(self.current)
    }
}

#[cfg(test)]
mod tests {
    use core_protocol::terminal::{TerminalCell, TerminalRow};

    use super::{Scrollback, SearchBuffer, SearchNavigation, SearchQuery, SearchSession};

    // The tests build a SearchBuffer from raw line strings via a small helper
    // in the test module (SearchBuffer wraps Scrollback + TerminalSnapshot).
    fn buffer_from_lines(
        lines: &[&str],
    ) -> (Scrollback, core_protocol::terminal::TerminalSnapshot, usize) {
        let cols = lines
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0)
            .max(1);
        let mut scrollback = Scrollback::new(0);
        scrollback.set_max_lines(0);
        // Put all lines in the screen snapshot for simplicity.
        let snapshot = core_protocol::terminal::TerminalSnapshot {
            stream_id: 1,
            sequence: 1,
            rows: lines
                .iter()
                .map(|line| TerminalRow {
                    cells: line
                        .chars()
                        .map(|c| TerminalCell::cluster(c.to_string()))
                        .collect(),
                })
                .collect(),
            cursor: core_protocol::terminal::CursorState {
                row: 0,
                col: 0,
                visible: true,
            },
            title: None,
            working_directory: None,
            scrollback_start: 0,
            extensions: Vec::new(),
        };
        (scrollback, snapshot, cols)
    }

    #[test]
    fn literal_search_finds_all_occurrences() {
        let (sb, snap, cols) = buffer_from_lines(&["foo bar foo", "bar baz", "foo"]);
        let buffer = SearchBuffer::new(&sb, &snap, cols);
        let session = SearchSession::new(buffer, SearchQuery::literal("foo"), 4).expect("query");
        let results = session.run();
        let lines: Vec<usize> = results.iter().map(|r| r.line).collect();
        let cols_found: Vec<usize> = results.iter().map(|r| r.col).collect();
        assert_eq!(lines, vec![0, 0, 2]);
        assert_eq!(cols_found, vec![0, 8, 0]);
    }

    #[test]
    fn regex_search_and_offsets() {
        let (sb, snap, cols) = buffer_from_lines(&["abc123", "xyz", "a1b2c3"]);
        let buffer = SearchBuffer::new(&sb, &snap, cols);
        let session = SearchSession::new(
            buffer,
            SearchQuery {
                pattern: "\\d+".to_owned(),
                case_sensitive: true,
                regex: true,
            },
            4,
        )
        .expect("regex compiles");
        let results = session.run();
        let spans: Vec<(usize, usize, usize)> =
            results.iter().map(|r| (r.line, r.col, r.length)).collect();
        assert_eq!(spans, vec![(0, 3, 3), (2, 1, 1), (2, 3, 1), (2, 5, 1)]);
    }

    #[test]
    fn case_insensitive_literal() {
        let (sb, snap, cols) = buffer_from_lines(&["Foo fOO", "bar"]);
        let buffer = SearchBuffer::new(&sb, &snap, cols);
        let session = SearchSession::new(
            buffer,
            SearchQuery {
                pattern: "foo".to_owned(),
                case_sensitive: false,
                regex: false,
            },
            4,
        )
        .expect("query");
        let results = session.run();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn chunked_search_is_incremental_and_non_blocking() {
        let (sb, snap, cols) = buffer_from_lines(&["needle", "x", "y", "z", "needle", "w"]);
        let buffer = SearchBuffer::new(&sb, &snap, cols);
        let mut session = SearchSession::new(buffer, SearchQuery::literal("needle"), 2).expect("q");
        // Step 1 searches lines 0-1: 1 match.
        assert_eq!(session.step(), Some(1));
        assert_eq!(session.lines_searched(), 2);
        assert!(!session.is_done());
        // Step 2 searches lines 2-3: 0 matches.
        assert_eq!(session.step(), Some(0));
        // Step 3 searches lines 4-5: 1 match, done.
        assert_eq!(session.step(), Some(1));
        assert!(session.is_done());
        assert_eq!(session.step(), None);
        assert_eq!(session.results().len(), 2);
    }

    #[test]
    fn cancellable_search_stops_early() {
        let lines: Vec<String> = (0..1000).map(|i| format!("line {i} alpha")).collect();
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let (sb, snap, cols) = buffer_from_lines(&refs);
        let buffer = SearchBuffer::new(&sb, &snap, cols);
        let mut session = SearchSession::new(buffer, SearchQuery::literal("alpha"), 10).expect("q");
        // Run a few chunks then cancel.
        assert!(session.step().is_some());
        assert!(session.step().is_some());
        session.cancel();
        let after_cancel = session.step();
        assert_eq!(after_cancel, None, "cancelled search must stop");
        assert!(session.was_cancelled());
        assert!(session.is_done());
        assert!(
            session.lines_searched() < 1000,
            "must not search everything"
        );
    }

    #[test]
    fn million_line_search_benchmark_and_cancellation() {
        use std::time::Instant;
        // Materialize a million-line buffer and search it in chunks; measure
        // latency and verify the full result count.
        let (sb, snap, cols) = {
            // Build a screen snapshot with 1,000,000 single-cell rows.
            let rows: Vec<TerminalRow> = (0..1_000_000u32)
                .map(|i| TerminalRow {
                    cells: vec![TerminalCell::cluster(i.to_string())],
                })
                .collect();
            let snapshot = core_protocol::terminal::TerminalSnapshot {
                stream_id: 1,
                sequence: 1,
                rows,
                cursor: core_protocol::terminal::CursorState {
                    row: 0,
                    col: 0,
                    visible: true,
                },
                title: None,
                working_directory: None,
                scrollback_start: 0,
                extensions: Vec::new(),
            };
            (Scrollback::new(0), snapshot, 1usize)
        };
        let buffer = SearchBuffer::new(&sb, &snap, cols);
        let start = Instant::now();
        let session =
            SearchSession::new(buffer, SearchQuery::literal("999999"), 50_000).expect("q");
        let results = session.run();
        let elapsed = start.elapsed();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line, 999_999);
        // The benchmark must complete without blocking for an unbounded time
        // (generous debug-build bound).
        assert!(
            elapsed.as_secs() < 30,
            "million-line search took {elapsed:?}"
        );

        // Cancellation: cancel after the first chunk of a re-run.
        let buffer = SearchBuffer::new(&sb, &snap, cols);
        let mut session =
            SearchSession::new(buffer, SearchQuery::literal("999999"), 10_000).expect("q");
        session.cancel();
        assert_eq!(session.step(), None);
        assert!(session.lines_searched() < 10_000);
    }

    #[test]
    fn navigation_cycles_through_results() {
        let mut nav = SearchNavigation::new(3);
        assert_eq!(nav.current(), Some(0));
        assert_eq!(nav.next_result(), Some(1));
        assert_eq!(nav.next_result(), Some(2));
        assert_eq!(nav.next_result(), Some(0), "wraps");
        assert_eq!(nav.prev_result(), Some(2));
        assert_eq!(nav.prev_result(), Some(1));
        let mut empty = SearchNavigation::new(0);
        assert_eq!(empty.current(), None);
        assert_eq!(empty.next_result(), None);
    }
}
