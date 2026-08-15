//! Pure-Rust WASM bridge (T138).
//!
//! [`TerminalBridge`] drives the exact same pipeline as native consumers:
//! `terminal-parser`'s bounded byte-stream parser feeds `terminal-state`'s
//! screen model, and the `wgpu-renderer` plan builder consumes the same
//! [`TerminalSnapshot`] the native renderer uses. The bridge tests reuse the
//! native terminal test vectors, so the WASM boundary is verified against
//! the same expectations as the native path.

use core_protocol::terminal::TerminalSnapshot;
use terminal_parser::BoundedByteStreamParser;
use terminal_state::ScreenModel;
use terminal_state::TerminalParser;
use wgpu_renderer::render::{build_plan, DrawBudget, RenderPlan};

/// The bridge version (must match the JS boundary version).
pub const BRIDGE_VERSION: u32 = 1;

/// One `push` result: sequence, event count, and diagnostics count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeOutput {
    /// Monotonic parser sequence after the feed.
    pub sequence: u64,
    /// Number of parsed events in this batch.
    pub event_count: usize,
    /// Number of parser diagnostics produced.
    pub diagnostics: usize,
}

/// A terminal session: parser + screen model, WebGPU render-plan ready.
pub struct TerminalBridge {
    parser: BoundedByteStreamParser,
    screen: ScreenModel,
    stream_id: u64,
}

impl TerminalBridge {
    /// A fresh bridge with `rows` x `cols` screen and a bounded parser.
    pub fn new(stream_id: u64, rows: usize, cols: usize) -> Self {
        Self {
            parser: BoundedByteStreamParser::new(),
            screen: ScreenModel::new(stream_id, rows, cols),
            stream_id,
        }
    }

    /// Feeds a byte chunk through the native pipeline and returns batch
    /// statistics.
    pub fn push(&mut self, bytes: &[u8]) -> BridgeOutput {
        let batch = self.parser.feed(bytes);
        let diagnostics = self.screen.apply_batch(&batch);
        BridgeOutput {
            sequence: batch.sequence,
            event_count: batch.events.len(),
            diagnostics: diagnostics.len(),
        }
    }

    /// Flushes incomplete parser state (end of stream).
    pub fn finish(&mut self) -> BridgeOutput {
        let batch = self.parser.finish();
        let diagnostics = self.screen.apply_batch(&batch);
        BridgeOutput {
            sequence: batch.sequence,
            event_count: batch.events.len(),
            diagnostics: diagnostics.len(),
        }
    }

    /// Resizes the screen (content preserved via reflow).
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.screen.resize(rows, cols);
    }

    /// The stream id.
    pub fn stream_id(&self) -> u64 {
        self.stream_id
    }

    /// Visible rows of the active buffer as strings (right-trimmed).
    pub fn text_rows(&self) -> Vec<String> {
        let snapshot = self.screen.snapshot();
        snapshot
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .filter_map(|cell| cell.text.as_deref())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    /// The visible text with trailing blank rows removed (newline-joined).
    pub fn text(&self) -> String {
        let rows = self.text_rows();
        let last = rows
            .iter()
            .rposition(|row| !row.is_empty())
            .map(|index| index + 1)
            .unwrap_or(0);
        rows[..last].join("\n")
    }

    /// The current snapshot (same type the native renderer consumes).
    pub fn snapshot(&self) -> TerminalSnapshot {
        self.screen.snapshot()
    }

    /// The batched WebGPU render plan built from the same snapshot the
    /// native renderer uses.
    pub fn render_plan(&self) -> RenderPlan {
        build_plan(&self.snapshot(), &DrawBudget::default())
    }

    /// Cursor position (0-based row, column) of the active buffer.
    pub fn cursor(&self) -> (usize, usize) {
        let buffer = self.screen.active_buffer();
        (buffer.cursor_row(), buffer.cursor_col())
    }

    /// The window title, if the OSC policy accepted one.
    pub fn title(&self) -> Option<&str> {
        self.screen.title()
    }

    /// The working directory from OSC 7, if accepted.
    pub fn working_directory(&self) -> Option<&str> {
        self.screen.working_directory()
    }

    /// The parser's current held bytes (the observable memory bound).
    pub fn pending_len(&self) -> usize {
        self.parser.pending_len()
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalBridge;

    #[test]
    fn same_text_vector_as_native_parser() {
        // The same vector the native terminal-parser contract uses:
        // text + SGR + text + newline.
        let mut bridge = TerminalBridge::new(1, 24, 80);
        let out = bridge.push(b"hello\x1b[31m world\r\n");
        assert!(out.sequence > 0);
        assert_eq!(out.event_count, 5); // Text + Sgr + Text + CR + LF
        assert_eq!(out.diagnostics, 0);
        assert_eq!(bridge.text(), "hello world");
        assert_eq!(bridge.cursor(), (1, 0));
    }

    #[test]
    fn cursor_tracks_columns() {
        let mut bridge = TerminalBridge::new(2, 24, 80);
        bridge.push(b"ab\r\ncd");
        assert_eq!(bridge.cursor(), (1, 0)); // "cd" is still buffered (fragmentation-safe)
        bridge.push(b"e");
        bridge.finish(); // flush the trailing text
        assert_eq!(bridge.cursor(), (1, 3));
        assert_eq!(bridge.text(), "ab\ncde");
    }

    #[test]
    fn resize_preserves_content() {
        let mut bridge = TerminalBridge::new(3, 24, 80);
        bridge.push(b"hello world\r\nsecond line");
        bridge.finish();
        assert_eq!(bridge.text(), "hello world\nsecond line");
        // Same-width resize preserves the exact rows; the content survives.
        bridge.resize(20, 80);
        assert_eq!(bridge.text(), "hello world\nsecond line");
        assert_eq!(bridge.snapshot().rows.len(), 20);
        // A width-changing resize preserves the content (reflowed).
        bridge.resize(20, 40);
        assert!(bridge.text().contains("hello world"));
        assert!(bridge.text().contains("second line"));
    }

    #[test]
    fn osc_title_is_captured() {
        let mut bridge = TerminalBridge::new(4, 24, 80);
        bridge.push(b"\x1b]0;my title\x07");
        assert_eq!(bridge.title(), Some("my title"));
    }

    #[test]
    fn render_plan_builds_from_same_snapshot() {
        let mut bridge = TerminalBridge::new(5, 24, 80);
        bridge.push(b"hello\x1b[31m world\nsecond");
        let plan = bridge.render_plan();
        // The plan is built from the snapshot the native renderer consumes;
        // visible non-empty cells must appear in the batched plan.
        assert!(plan.cells > 0);
        assert!(!plan.draw_calls.is_empty());
        assert_eq!(
            plan.cells,
            bridge
                .snapshot()
                .rows
                .iter()
                .map(|row| row.cells.len())
                .sum()
        );
    }

    #[test]
    fn fragmented_feed_matches_whole_feed() {
        // The native fragmentation property: byte-by-byte feeding yields the
        // same screen as feeding the whole stream.
        let mut whole = TerminalBridge::new(6, 24, 80);
        whole.push(b"\x1b[1mhello\x1b[0m world\n");
        let mut fragmented = TerminalBridge::new(6, 24, 80);
        for byte in b"\x1b[1mhello\x1b[0m world\n" {
            fragmented.push(&[*byte]);
        }
        assert_eq!(whole.text(), fragmented.text());
        assert_eq!(whole.cursor(), fragmented.cursor());
    }

    #[test]
    fn empty_and_finish_are_benign() {
        let mut bridge = TerminalBridge::new(7, 24, 80);
        let out = bridge.push(b"");
        assert_eq!(out.event_count, 0);
        assert_eq!(out.diagnostics, 0);
        let out = bridge.finish();
        assert_eq!(out.event_count, 0);
        assert_eq!(bridge.pending_len(), 0);
    }

    #[test]
    fn unicode_vector_matches_native_width_policy() {
        let mut bridge = TerminalBridge::new(8, 24, 80);
        bridge.push("你好\r\n".as_bytes());
        assert_eq!(bridge.text(), "你好");
        bridge.push("world".as_bytes());
        bridge.finish();
        assert_eq!(bridge.text(), "你好\nworld");
    }
}
