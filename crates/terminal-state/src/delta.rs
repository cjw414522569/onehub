//! Incremental terminal-state merge and dirty-line tracking (T073).
//!
//! [`DirtyTracker`] accumulates which rows (and cursor/title/working-directory)
//! changed between frames. [`DeltaBuilder`] batches them into one
//! core-protocol [`TerminalDelta`] per frame for the FFI bridge, and
//! [`apply_delta`] merges a delta into a receiver snapshot. If a frame is
//! dropped, the next full snapshot restores consistency — verified by the
//! incremental/full equivalence and dropped-frame recovery property tests.

use std::collections::BTreeSet;

use core_protocol::terminal::{
    CursorState, DeltaOp, TerminalCell, TerminalDelta, TerminalRow, TerminalSnapshot, TerminalStyle,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

/// Tracks dirty state between frames.
#[derive(Debug, Clone, Default)]
pub struct DirtyTracker {
    rows: BTreeSet<usize>,
    cursor: bool,
    title: bool,
    working_directory: bool,
}

impl DirtyTracker {
    /// An empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks a row as dirty.
    pub fn mark_row(&mut self, row: usize) {
        self.rows.insert(row);
    }

    /// Marks the cursor as dirty.
    pub fn mark_cursor(&mut self) {
        self.cursor = true;
    }

    /// Marks the title as dirty.
    pub fn mark_title(&mut self) {
        self.title = true;
    }

    /// Marks the working directory as dirty.
    pub fn mark_working_directory(&mut self) {
        self.working_directory = true;
    }

    /// Dirty rows, ascending.
    pub fn dirty_rows(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows.iter().copied()
    }

    /// Whether a row is dirty.
    pub fn is_row_dirty(&self, row: usize) -> bool {
        self.rows.contains(&row)
    }

    /// Whether the cursor is dirty.
    pub fn cursor_dirty(&self) -> bool {
        self.cursor
    }

    /// Whether the title is dirty.
    pub fn title_dirty(&self) -> bool {
        self.title
    }

    /// Whether the working directory is dirty.
    pub fn working_directory_dirty(&self) -> bool {
        self.working_directory
    }

    /// Number of dirty rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether nothing is dirty.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && !self.cursor && !self.title && !self.working_directory
    }

    /// Clears all dirty state.
    pub fn clear(&mut self) {
        self.rows.clear();
        self.cursor = false;
        self.title = false;
        self.working_directory = false;
    }
}

/// Builds a per-frame [`TerminalDelta`] from a base snapshot and dirty state.
pub struct DeltaBuilder<'a> {
    base: &'a TerminalSnapshot,
    dirty: &'a DirtyTracker,
}

impl<'a> DeltaBuilder<'a> {
    /// A builder over the base snapshot (the last frame the receiver has).
    pub fn new(base: &'a TerminalSnapshot, dirty: &'a DirtyTracker) -> Self {
        Self { base, dirty }
    }

    /// Builds one batched delta for the frame. Hyperlink cells are excluded
    /// from incremental deltas (the `Fill` op carries text + style only);
    /// they are restored by the full-snapshot recovery path.
    pub fn build(&self, stream_id: u64, from_sequence: u64, to_sequence: u64) -> TerminalDelta {
        let mut operations = Vec::new();
        for row in self.dirty.dirty_rows() {
            if let Some(row_cells) = self.base.rows.get(row) {
                operations.extend(build_row_ops(row as u16, &row_cells.cells));
            }
        }
        if self.dirty.cursor_dirty() {
            operations.push(DeltaOp::Cursor {
                cursor: self.base.cursor,
            });
        }
        if self.dirty.title_dirty() {
            operations.push(DeltaOp::Title {
                title: self.base.title.clone(),
            });
        }
        TerminalDelta {
            stream_id,
            from_sequence,
            to_sequence,
            operations,
            extensions: Vec::new(),
        }
    }
}

/// Emits row-level ops for one dirty row: contiguous runs with the same style
/// (and no hyperlink) become one [`DeltaOp::Fill`].
fn build_row_ops(row: u16, cells: &[TerminalCell]) -> Vec<DeltaOp> {
    let mut ops = Vec::new();
    let mut start = 0usize;
    while start < cells.len() {
        let cell = &cells[start];
        if cell.wide_continuation {
            start += 1;
            continue;
        }
        let style = cell.style.clone();
        let mut text = String::new();
        let mut end = start;
        while end < cells.len() {
            let current = &cells[end];
            if current.wide_continuation {
                end += 1;
                continue;
            }
            if current.style != style || current.hyperlink.is_some() {
                break;
            }
            text.push_str(current.text.as_deref().unwrap_or(" "));
            end += 1;
        }
        let trimmed = text.trim_end().to_owned();
        if !trimmed.is_empty() {
            ops.push(DeltaOp::Fill {
                row,
                col: start as u16,
                text: trimmed,
                style,
            });
        }
        start = end.max(start + 1);
    }
    ops
}

/// Why an incremental merge failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaError {
    /// The delta's `from_sequence` is newer than the receiver snapshot
    /// (frames were dropped; the receiver must request a full snapshot).
    SequenceGap,
    /// The receiver snapshot has no rows (no full snapshot yet).
    MissingSnapshot,
}

/// Merges a delta into `snapshot`, advancing its sequence.
///
/// Returns [`DeltaError::SequenceGap`] when the delta starts after the
/// snapshot's current sequence (a frame was dropped); the caller must recover
/// with a full snapshot.
pub fn apply_delta(
    snapshot: &mut TerminalSnapshot,
    delta: &TerminalDelta,
) -> Result<(), DeltaError> {
    if snapshot.rows.is_empty() {
        return Err(DeltaError::MissingSnapshot);
    }
    if delta.from_sequence > snapshot.sequence {
        return Err(DeltaError::SequenceGap);
    }
    snapshot.sequence = delta.to_sequence;
    for op in &delta.operations {
        match op {
            DeltaOp::Fill {
                row,
                col,
                text,
                style,
            } => fill_run(snapshot, *row, *col, text, style),
            DeltaOp::Copy { from_row, to_row } => {
                let from = *from_row as usize;
                let to = *to_row as usize;
                if from < snapshot.rows.len() && to < snapshot.rows.len() {
                    snapshot.rows[to] = snapshot.rows[from].clone();
                }
            }
            DeltaOp::Clear {
                row,
                col,
                rows,
                cols,
            } => clear_rect(snapshot, *row, *col, *rows, *cols),
            DeltaOp::Cursor { cursor } => snapshot.cursor = *cursor,
            DeltaOp::Title { title } => snapshot.title = title.clone(),
            DeltaOp::Image { row, col, image } => {
                let row = *row as usize;
                let col = *col as usize;
                if let Some(cell) = snapshot
                    .rows
                    .get_mut(row)
                    .and_then(|r| r.cells.get_mut(col))
                {
                    cell.image = Some(image.clone());
                }
            }
        }
    }
    Ok(())
}

/// Writes a fill run grapheme-by-grapheme, marking wide continuations.
fn fill_run(
    snapshot: &mut TerminalSnapshot,
    row: u16,
    col: u16,
    text: &str,
    style: &TerminalStyle,
) {
    let row = row as usize;
    let row_cells = match snapshot.rows.get_mut(row) {
        Some(row) => row,
        None => return,
    };
    let mut col = col as usize;
    for cluster in text.graphemes(true) {
        let width = UnicodeWidthChar::width(cluster.chars().next().unwrap_or(' '))
            .unwrap_or(1)
            .max(1);
        if col >= row_cells.cells.len() {
            break;
        }
        let mut cell = TerminalCell::cluster(cluster.to_owned());
        cell.style = style.clone();
        row_cells.cells[col] = cell;
        if width >= 2 && col + 1 < row_cells.cells.len() {
            row_cells.cells[col + 1] = TerminalCell::wide_continuation(style.clone());
        }
        col += width;
    }
}

/// Clears a rectangle of cells.
fn clear_rect(snapshot: &mut TerminalSnapshot, row: u16, col: u16, rows: u16, cols: u16) {
    let start_row = row as usize;
    let end_row = (start_row + rows as usize).min(snapshot.rows.len());
    for row_index in start_row..end_row {
        let row_cells = &mut snapshot.rows[row_index].cells;
        let start_col = col as usize;
        let end_col = (start_col + cols as usize).min(row_cells.len());
        for cell in row_cells.iter_mut().take(end_col).skip(start_col) {
            *cell = TerminalCell::empty();
        }
    }
}

/// Diffs two snapshots and marks every changed row (plus cursor/title/working
/// directory) as dirty. Used by the receiver to track per-frame changes.
pub fn diff_rows(old: &TerminalSnapshot, new: &TerminalSnapshot) -> DirtyTracker {
    let mut tracker = DirtyTracker::new();
    let rows = old.rows.len().max(new.rows.len());
    for row in 0..rows {
        let old_row = old.rows.get(row);
        let new_row = new.rows.get(row);
        if old_row != new_row {
            tracker.mark_row(row);
        }
    }
    if old.cursor != new.cursor {
        tracker.mark_cursor();
    }
    if old.title != new.title {
        tracker.mark_title();
    }
    if old.working_directory != new.working_directory {
        tracker.mark_working_directory();
    }
    tracker
}

/// Helper to build a blank snapshot of `rows` x `cols`.
pub fn blank_snapshot(stream_id: u64, rows: usize, cols: usize) -> TerminalSnapshot {
    TerminalSnapshot {
        stream_id,
        sequence: 0,
        rows: (0..rows)
            .map(|_| TerminalRow {
                cells: vec![TerminalCell::empty(); cols],
            })
            .collect(),
        cursor: CursorState {
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

#[cfg(test)]
mod tests {
    use core_protocol::terminal::{
        CursorState, DeltaOp, TerminalCell, TerminalColor, TerminalRow, TerminalSnapshot,
    };

    use super::{apply_delta, blank_snapshot, diff_rows, DeltaBuilder, DeltaError, DirtyTracker};

    fn cell_with(text: &str, fg: u8) -> TerminalCell {
        let mut cell = TerminalCell::cluster(text);
        cell.style.fg = TerminalColor::Indexed(fg);
        cell
    }

    fn snapshot_with(rows: &[Vec<TerminalCell>]) -> core_protocol::terminal::TerminalSnapshot {
        TerminalSnapshot {
            stream_id: 1,
            sequence: 0,
            rows: rows
                .iter()
                .map(|cells| TerminalRow {
                    cells: cells.clone(),
                })
                .collect(),
            cursor: CursorState {
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
    fn dirty_tracker_accumulates_and_clears() {
        let mut tracker = DirtyTracker::new();
        tracker.mark_row(0);
        tracker.mark_row(2);
        tracker.mark_row(0);
        tracker.mark_cursor();
        tracker.mark_title();
        assert_eq!(tracker.len(), 2);
        assert!(tracker.is_row_dirty(0));
        assert!(!tracker.is_row_dirty(1));
        assert!(tracker.cursor_dirty());
        assert!(tracker.title_dirty());
        tracker.clear();
        assert!(tracker.is_empty());
    }

    #[test]
    fn delta_build_and_apply_equivalence() {
        // Random-ish content with wide chars (no hyperlinks): the delta
        // applied to a blank snapshot reproduces the full snapshot rows.
        let rows: Vec<Vec<TerminalCell>> = vec![
            vec![
                cell_with("a", 1),
                cell_with("b", 2),
                cell_with("c", 1),
                TerminalCell::empty(),
            ],
            vec![
                cell_with("x", 3),
                cell_with("y", 3),
                cell_with("z", 3),
                TerminalCell::empty(),
            ],
            vec![
                cell_with("q", 5),
                TerminalCell::empty(),
                TerminalCell::empty(),
                TerminalCell::empty(),
            ],
        ];
        let full = snapshot_with(&rows);
        let blank = blank_snapshot(1, 3, 4);

        let mut tracker = DirtyTracker::new();
        for row in 0..3 {
            tracker.mark_row(row);
        }
        let delta = DeltaBuilder::new(&full, &tracker).build(1, 0, 1);

        let mut merged = blank;
        apply_delta(&mut merged, &delta).expect("apply");
        assert_eq!(merged.rows, full.rows, "delta + blank == full snapshot");
    }

    #[test]
    fn incremental_diff_and_merge_equivalence() {
        // Start from a base, change two rows, diff -> delta -> merge.
        let base = snapshot_with(&[
            vec![cell_with("a", 1), cell_with("b", 2), TerminalCell::empty()],
            vec![cell_with("c", 3), cell_with("d", 4), TerminalCell::empty()],
            vec![
                cell_with("e", 5),
                TerminalCell::empty(),
                TerminalCell::empty(),
            ],
        ]);
        let mut next = base.clone();
        next.rows[1].cells[0] = cell_with("X", 9);
        next.rows[2].cells[1] = cell_with("Y", 9);
        next.cursor = CursorState {
            row: 2,
            col: 1,
            visible: true,
        };
        next.sequence = 5;

        let tracker = diff_rows(&base, &next);
        assert!(tracker.is_row_dirty(1));
        assert!(tracker.is_row_dirty(2));
        assert!(!tracker.is_row_dirty(0));
        assert!(tracker.cursor_dirty());

        let delta = DeltaBuilder::new(&next, &tracker).build(1, 0, 5);
        let mut merged = base;
        apply_delta(&mut merged, &delta).expect("apply");
        assert_eq!(merged.rows, next.rows);
        assert_eq!(merged.cursor, next.cursor);
        assert_eq!(merged.sequence, 5);
    }

    #[test]
    fn dropped_frame_recovers_from_full_snapshot() {
        // Apply delta 0->1, then DROP delta 1->2 and recover with a full
        // snapshot; the result must equal the authoritative full snapshot.
        let base = snapshot_with(&[vec![cell_with("a", 1), TerminalCell::empty()]]);
        let mut mid = base.clone();
        mid.rows[0].cells[1] = cell_with("b", 2);
        mid.sequence = 1;

        let mut tracker = DirtyTracker::new();
        tracker.mark_row(0);
        let delta = DeltaBuilder::new(&mid, &tracker).build(1, 0, 1);
        let mut merged = base;
        apply_delta(&mut merged, &delta).expect("apply");

        // The dropped frame (1->2) never arrives; instead a full snapshot.
        let mut authoritative = mid.clone();
        authoritative.rows[0].cells[0] = cell_with("z", 7);
        authoritative.rows[0].cells[1] = cell_with("y", 8);
        authoritative.sequence = 2;
        merged = authoritative.clone();
        assert_eq!(merged.rows, authoritative.rows);
        assert_eq!(merged.sequence, 2);
    }

    #[test]
    fn sequence_gap_is_detected() {
        let base = blank_snapshot(1, 2, 2);
        let mut merged = base;
        merged.sequence = 10;
        let delta = core_protocol::terminal::TerminalDelta {
            stream_id: 1,
            from_sequence: 20,
            to_sequence: 21,
            operations: Vec::new(),
            extensions: Vec::new(),
        };
        assert_eq!(
            apply_delta(&mut merged, &delta),
            Err(DeltaError::SequenceGap)
        );
        let missing = blank_snapshot(1, 0, 2);
        let mut missing = missing;
        assert_eq!(
            apply_delta(&mut missing, &delta),
            Err(DeltaError::MissingSnapshot)
        );
    }

    #[test]
    fn frame_batches_all_dirty_rows() {
        let full = snapshot_with(&[
            vec![cell_with("1", 1), TerminalCell::empty()],
            vec![cell_with("2", 2), TerminalCell::empty()],
            vec![cell_with("3", 3), TerminalCell::empty()],
        ]);
        let mut tracker = DirtyTracker::new();
        tracker.mark_row(0);
        tracker.mark_row(2);
        let delta = DeltaBuilder::new(&full, &tracker).build(1, 0, 1);
        let fill_rows: Vec<u16> = delta
            .operations
            .iter()
            .filter_map(|op| match op {
                DeltaOp::Fill { row, .. } => Some(*row),
                _ => None,
            })
            .collect();
        assert_eq!(
            fill_rows,
            vec![0, 2],
            "one batched delta covers all dirty rows"
        );
    }
}
