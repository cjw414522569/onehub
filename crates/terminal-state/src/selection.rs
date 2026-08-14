//! Selection and copy model (T070).
//!
//! [`Selection`] captures an anchor and focus point over a terminal snapshot
//! (the active buffer — primary or alternate — so alternate-screen text
//! selects correctly) and extracts copyable text in character, word, line, or
//! rectangle mode. Wide characters are handled by skipping continuation
//! cells, and rows are joined with newlines.

use core_protocol::terminal::{TerminalCell, TerminalSnapshot};

/// Selection expansion mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// Exact character range.
    Character,
    /// Expand the anchor to word boundaries.
    Word,
    /// Whole lines between the anchor and focus rows.
    Line,
    /// A rectangular block of columns across rows.
    Rectangle,
}

/// A selection over a snapshot grid (0-based rows/columns).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Anchor point `(row, col)`.
    pub anchor: (usize, usize),
    /// Focus point `(row, col)`.
    pub focus: (usize, usize),
}

impl Selection {
    /// A selection between two points; the range is normalized so start <= end.
    pub fn new(anchor: (usize, usize), focus: (usize, usize)) -> Self {
        Self { anchor, focus }
    }

    /// The normalized start `(row, col)`.
    pub fn start(&self) -> (usize, usize) {
        if self.anchor < self.focus {
            self.anchor
        } else {
            self.focus
        }
    }

    /// The normalized end `(row, col)` (inclusive).
    pub fn end(&self) -> (usize, usize) {
        if self.anchor <= self.focus {
            self.focus
        } else {
            self.anchor
        }
    }

    /// Extracts copyable text under the given mode.
    pub fn extract(&self, snapshot: &TerminalSnapshot, mode: SelectionMode) -> String {
        match mode {
            SelectionMode::Character => self.extract_character(snapshot),
            SelectionMode::Word => {
                let (start_row, start_col) = self.start();
                let (word_start, word_end) = word_bounds(snapshot, start_row, start_col);
                let selection = Selection::new((start_row, word_start), (start_row, word_end));
                selection.extract_character(snapshot)
            }
            SelectionMode::Line => self.extract_lines(snapshot),
            SelectionMode::Rectangle => self.extract_rectangle(snapshot),
        }
    }

    fn extract_character(&self, snapshot: &TerminalSnapshot) -> String {
        let (start_row, start_col) = self.start();
        let (end_row, end_col) = self.end();
        let mut lines = Vec::new();
        for row_index in start_row..=end_row.min(snapshot.rows.len().saturating_sub(1)) {
            let cells = &snapshot.rows[row_index].cells;
            let from = if row_index == start_row { start_col } else { 0 };
            let to = if row_index == end_row {
                end_col
            } else {
                cells.len().saturating_sub(1)
            };
            if from >= cells.len() {
                lines.push(String::new());
                continue;
            }
            let mut line = String::new();
            for cell in cells.iter().take(to + 1).skip(from) {
                line.push_str(cell_selection_text(cell));
            }
            lines.push(line.trim_end().to_owned());
        }
        lines.join("\n")
    }

    fn extract_lines(&self, snapshot: &TerminalSnapshot) -> String {
        let (start_row, _) = self.start();
        let (end_row, _) = self.end();
        let mut lines = Vec::new();
        for row_index in start_row..=end_row.min(snapshot.rows.len().saturating_sub(1)) {
            let cells = &snapshot.rows[row_index].cells;
            let mut line = String::new();
            for cell in cells {
                line.push_str(cell_selection_text(cell));
            }
            lines.push(line.trim().to_owned());
        }
        lines.join("\n")
    }

    fn extract_rectangle(&self, snapshot: &TerminalSnapshot) -> String {
        let (start_row, start_col) = self.start();
        let (end_row, end_col) = self.end();
        let mut lines = Vec::new();
        for row_index in start_row..=end_row.min(snapshot.rows.len().saturating_sub(1)) {
            let cells = &snapshot.rows[row_index].cells;
            let mut line = String::new();
            for cell in cells.iter().take(end_col + 1).skip(start_col) {
                line.push_str(cell_selection_text(cell));
            }
            lines.push(line.trim_end().to_owned());
        }
        lines.join("\n")
    }
}

/// The copy text of one cell: the cell text, empty for wide-continuation
/// cells, or a space for empty cells (so gaps in the grid are preserved).
pub fn cell_selection_text(cell: &TerminalCell) -> &str {
    match &cell.text {
        Some(text) => text,
        None => {
            if cell.wide_continuation {
                ""
            } else {
                " "
            }
        }
    }
}

/// Whether a character participates in a word (alphanumeric or underscore).
pub fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// The inclusive word boundaries `(start_col, end_col)` around `(row, col)`.
pub fn word_bounds(snapshot: &TerminalSnapshot, row: usize, col: usize) -> (usize, usize) {
    let row = row.min(snapshot.rows.len().saturating_sub(1));
    let cells = &snapshot.rows[row].cells;
    if cells.is_empty() {
        return (0, 0);
    }
    let col = col.min(cells.len().saturating_sub(1));
    // Per-cell leading character; continuation cells are not word chars.
    let chars: Vec<char> = cells
        .iter()
        .map(|cell| cell_selection_text(cell).chars().next().unwrap_or(' '))
        .collect();
    let mut start = col;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }
    (start, end.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use core_protocol::terminal::{TerminalCell, TerminalRow, TerminalSnapshot};

    use super::{word_bounds, Selection, SelectionMode};

    fn cell(text: &str) -> TerminalCell {
        TerminalCell::cluster(text)
    }

    fn snapshot(rows: &[&str], cols: usize) -> TerminalSnapshot {
        TerminalSnapshot {
            stream_id: 1,
            sequence: 1,
            rows: rows
                .iter()
                .map(|row| {
                    let mut cells: Vec<TerminalCell> =
                        row.chars().map(|ch| cell(&ch.to_string())).collect();
                    cells.resize(cols, TerminalCell::empty());
                    TerminalRow { cells }
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
        }
    }

    #[test]
    fn character_selection_preserves_spaces_and_newlines() {
        let snap = snapshot(&["ab  cd", "efgh  ", "ij"], 8);
        let selection = Selection::new((0, 1), (1, 3));
        assert_eq!(
            selection.extract(&snap, SelectionMode::Character),
            "b  cd\nefgh"
        );
    }

    #[test]
    fn wide_character_selection_skips_continuation() {
        // Row: 中 (wide at cols 0-1) followed by "x" at col 2.
        let cells = vec![
            cell("中"),
            TerminalCell::wide_continuation(Default::default()),
            cell("x"),
            TerminalCell::empty(),
        ];
        let snap = TerminalSnapshot {
            stream_id: 1,
            sequence: 1,
            rows: vec![TerminalRow { cells }],
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
        let selection = Selection::new((0, 0), (0, 2));
        assert_eq!(selection.extract(&snap, SelectionMode::Character), "中x");
    }

    #[test]
    fn line_selection_returns_whole_lines() {
        let snap = snapshot(&["alpha", "beta", "gamma"], 8);
        let selection = Selection::new((0, 0), (2, 0));
        assert_eq!(
            selection.extract(&snap, SelectionMode::Line),
            "alpha\nbeta\ngamma"
        );
    }

    #[test]
    fn rectangle_selection_extracts_columns() {
        let snap = snapshot(&["abcde", "fghij", "klmno"], 6);
        let selection = Selection::new((0, 1), (2, 3));
        assert_eq!(
            selection.extract(&snap, SelectionMode::Rectangle),
            "bcd\nghi\nlmn"
        );
    }

    #[test]
    fn word_selection_expands_to_boundaries() {
        let snap = snapshot(&["foo bar_baz qux"], 16);
        // Click inside "bar_baz".
        let (start, end) = word_bounds(&snap, 0, 6);
        assert_eq!((start, end), (4, 10));
        let selection = Selection::new((0, 6), (0, 6));
        assert_eq!(selection.extract(&snap, SelectionMode::Word), "bar_baz");
    }

    #[test]
    fn selection_normalizes_reverse_ranges() {
        let snap = snapshot(&["abcdef"], 8);
        // Focus before anchor.
        let selection = Selection::new((0, 5), (0, 1));
        assert_eq!(selection.start(), (0, 1));
        assert_eq!(selection.end(), (0, 5));
        assert_eq!(selection.extract(&snap, SelectionMode::Character), "bcdef");
    }
}
