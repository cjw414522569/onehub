//! Cursor / selection / decoration / link / search layer compositing (T078).
//!
//! The terminal grid is composed with several overlay layers (cursor,
//! selection, decorations, links, search matches). Each layer changes
//! independently: updating one layer only marks that layer dirty — the base
//! grid is never re-laid out for an overlay change. [`CompositeState`] tracks
//! per-layer dirty state and [`plan_frame`] reports exactly which layers need
//! redrawing, so the frame timeline is stable (unchanged layers are not
//! redrawn, avoiding flicker). Image-golden / frame-timeline screenshot
//! validation requires a real renderer and is `blocked_environment` on CI
//! hosts without one; the compositing contract is verified deterministically.

use core_protocol::terminal::{CursorState, TerminalSnapshot};

use terminal_state::{Selection, SelectionMode};

/// A composited layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// The base terminal grid.
    Base,
    /// Cursor.
    Cursor,
    /// Selection.
    Selection,
    /// Decorations (tab underline, scrollbar marks, etc.).
    Decoration,
    /// OSC 8 hyperlink spans.
    Link,
    /// Search-match highlights.
    Search,
}

/// A rectangular decoration (row/col range) above the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecorationRect {
    /// 0-based row.
    pub row: u16,
    /// 0-based start column.
    pub col: u16,
    /// Column span.
    pub cols: u16,
}

/// A hyperlink span to highlight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRect {
    /// 0-based row.
    pub row: u16,
    /// 0-based start column.
    pub col: u16,
    /// Column span.
    pub cols: u16,
    /// The link URL (for styling / tooltip).
    pub url: String,
}

/// A search-match span to highlight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatchRect {
    /// Absolute line index.
    pub line: u64,
    /// Start column.
    pub col: u16,
    /// Length in columns.
    pub length: u16,
}

/// Per-frame redraw plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramePlan {
    /// Frame number.
    pub frame: u64,
    /// Layers that must be redrawn this frame.
    pub redrawn: Vec<Layer>,
    /// True when nothing changed (stable animation, no redraw).
    pub stable: bool,
}

/// A timeline of frame plans (for animation-stability validation).
#[derive(Debug, Clone, Default)]
pub struct FrameTimeline {
    frames: Vec<FramePlan>,
}

impl FrameTimeline {
    /// Records a frame plan.
    pub fn record(&mut self, plan: FramePlan) {
        self.frames.push(plan);
    }

    /// The recorded frames.
    pub fn frames(&self) -> &[FramePlan] {
        &self.frames
    }
}

/// Tracks the composited layers and their dirty state.
#[derive(Debug, Clone)]
pub struct CompositeState {
    /// The base grid (last applied sequence).
    pub base_sequence: u64,
    /// Cursor overlay.
    pub cursor: Option<CursorState>,
    /// Selection overlay.
    pub selection: Option<Selection>,
    /// Decoration overlays.
    pub decorations: Vec<DecorationRect>,
    /// Link overlays.
    pub links: Vec<LinkRect>,
    /// Search-match overlays.
    pub search_matches: Vec<SearchMatchRect>,
    /// Per-layer dirty flags.
    dirty: Vec<Layer>,
}

impl Default for CompositeState {
    fn default() -> Self {
        Self::new()
    }
}

impl CompositeState {
    /// An empty composite state.
    pub fn new() -> Self {
        Self {
            base_sequence: 0,
            cursor: None,
            selection: None,
            decorations: Vec::new(),
            links: Vec::new(),
            search_matches: Vec::new(),
            dirty: Vec::new(),
        }
    }

    /// Applies a new base snapshot (only the Base layer becomes dirty; the
    /// overlays keep their positions and are not re-laid out).
    pub fn set_base(&mut self, snapshot: &TerminalSnapshot) {
        self.base_sequence = snapshot.sequence;
        self.mark_dirty(Layer::Base);
    }

    /// Sets the cursor overlay.
    pub fn set_cursor(&mut self, cursor: Option<CursorState>) {
        if self.cursor != cursor {
            self.cursor = cursor;
            self.mark_dirty(Layer::Cursor);
        }
    }

    /// Sets the selection overlay.
    pub fn set_selection(&mut self, selection: Option<Selection>) {
        if self.selection != selection {
            self.selection = selection;
            self.mark_dirty(Layer::Selection);
        }
    }

    /// Sets the decoration overlays.
    pub fn set_decorations(&mut self, decorations: Vec<DecorationRect>) {
        if self.decorations != decorations {
            self.decorations = decorations;
            self.mark_dirty(Layer::Decoration);
        }
    }

    /// Sets the link overlays.
    pub fn set_links(&mut self, links: Vec<LinkRect>) {
        if self.links != links {
            self.links = links;
            self.mark_dirty(Layer::Link);
        }
    }

    /// Sets the search-match overlays.
    pub fn set_search_matches(&mut self, matches: Vec<SearchMatchRect>) {
        if self.search_matches != matches {
            self.search_matches = matches;
            self.mark_dirty(Layer::Search);
        }
    }

    /// Renders the selection overlay to rects (for the compositor).
    pub fn selection_rects(&self, rows: usize, cols: usize) -> Vec<DecorationRect> {
        let mut rects = Vec::new();
        if let Some(selection) = self.selection {
            let (start_row, start_col) = selection.start();
            let (end_row, end_col) = selection.end();
            for row in start_row.min(rows.saturating_sub(1))..=end_row.min(rows.saturating_sub(1)) {
                let from = if row == start_row { start_col } else { 0 };
                let to = if row == end_row {
                    end_col.min(cols.saturating_sub(1))
                } else {
                    cols.saturating_sub(1)
                };
                if from <= to {
                    rects.push(DecorationRect {
                        row: row as u16,
                        col: from as u16,
                        cols: (to - from + 1) as u16,
                    });
                }
            }
        }
        rects
    }

    /// Plans the current frame: returns the layers to redraw, then resets the
    /// dirty state. A frame with no changes is `stable`.
    pub fn plan_frame(&mut self, frame: u64) -> FramePlan {
        let redrawn = std::mem::take(&mut self.dirty);
        FramePlan {
            frame,
            stable: redrawn.is_empty(),
            redrawn,
        }
    }

    fn mark_dirty(&mut self, layer: Layer) {
        if !self.dirty.contains(&layer) {
            self.dirty.push(layer);
        }
    }
}

/// Convenience: extracts the selection text under the current overlay.
pub fn selected_text(
    state: &CompositeState,
    snapshot: &TerminalSnapshot,
    mode: SelectionMode,
) -> Option<String> {
    state
        .selection
        .map(|selection| selection.extract(snapshot, mode))
}

#[cfg(test)]
mod tests {
    use core_protocol::terminal::{CursorState, TerminalCell, TerminalRow, TerminalSnapshot};

    use super::{selected_text, CompositeState, FrameTimeline, Layer, SearchMatchRect};

    fn snapshot() -> TerminalSnapshot {
        TerminalSnapshot {
            stream_id: 1,
            sequence: 5,
            rows: vec![
                TerminalRow {
                    cells: vec![TerminalCell::cluster("a"), TerminalCell::cluster("b")],
                },
                TerminalRow {
                    cells: vec![TerminalCell::cluster("c"), TerminalCell::cluster("d")],
                },
            ],
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
    fn selection_change_does_not_touch_base() {
        let mut state = CompositeState::new();
        let snap = snapshot();
        state.set_base(&snap);
        let _ = state.plan_frame(1); // base frame
        state.set_selection(Some(terminal_state::Selection::new((0, 0), (1, 1))));
        let plan = state.plan_frame(2);
        assert_eq!(
            plan.redrawn,
            vec![Layer::Selection],
            "only the selection layer"
        );
        assert!(
            !plan.redrawn.contains(&Layer::Base),
            "base must not be re-laid out"
        );
    }

    #[test]
    fn unchanged_frame_is_stable() {
        let mut state = CompositeState::new();
        let snap = snapshot();
        state.set_base(&snap);
        state.set_cursor(Some(snap.cursor));
        let _ = state.plan_frame(1);
        let plan = state.plan_frame(2); // nothing changed
        assert!(plan.stable);
        assert!(plan.redrawn.is_empty());
    }

    #[test]
    fn cursor_blink_only_redraws_cursor() {
        let mut state = CompositeState::new();
        state.set_cursor(Some(CursorState {
            row: 0,
            col: 0,
            visible: true,
        }));
        let _ = state.plan_frame(1);
        // Blink: hide.
        state.set_cursor(Some(CursorState {
            row: 0,
            col: 0,
            visible: false,
        }));
        let plan = state.plan_frame(2);
        assert_eq!(plan.redrawn, vec![Layer::Cursor]);
    }

    #[test]
    fn timeline_records_per_frame_redraws() {
        let mut state = CompositeState::new();
        let mut timeline = FrameTimeline::default();
        let snap = snapshot();
        state.set_base(&snap);
        timeline.record(state.plan_frame(1));
        state.set_cursor(Some(snap.cursor));
        timeline.record(state.plan_frame(2));
        let frames = timeline.frames();
        assert_eq!(frames.len(), 2);
        assert!(frames[0].redrawn.contains(&Layer::Base));
        assert!(frames[1].redrawn.contains(&Layer::Cursor));
        assert!(!frames[1].redrawn.contains(&Layer::Base));
    }

    #[test]
    fn search_layer_is_independent() {
        let mut state = CompositeState::new();
        state.set_search_matches(vec![SearchMatchRect {
            line: 0,
            col: 0,
            length: 1,
        }]);
        let plan = state.plan_frame(1);
        assert_eq!(plan.redrawn, vec![Layer::Search]);
        // Selection overlay can be composed independently.
        state.set_selection(Some(terminal_state::Selection::new((0, 0), (0, 1))));
        let plan = state.plan_frame(2);
        assert_eq!(plan.redrawn, vec![Layer::Selection]);
    }

    #[test]
    fn selection_text_extraction() {
        let mut state = CompositeState::new();
        let snap = snapshot();
        state.set_base(&snap);
        state.set_selection(Some(terminal_state::Selection::new((0, 0), (1, 1))));
        assert_eq!(
            selected_text(&state, &snap, terminal_state::SelectionMode::Character),
            Some("ab\ncd".to_owned())
        );
    }
}
