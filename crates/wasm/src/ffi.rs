//! Versioned JS interop boundary (T138).
//!
//! Exports a small, versioned surface to JavaScript via `wasm-bindgen`:
//! [`JsTerminal`] owns a [`TerminalBridge`] and exposes `push` (Uint8Array),
//! `resize`, text/cursor/title accessors, and batched WebGPU render-plan
//! statistics built from the same snapshot the native renderer consumes.

use wasm_bindgen::prelude::*;

use crate::bridge::TerminalBridge;

/// The JS boundary version.
pub const WASM_BOUNDARY_VERSION: u32 = 1;

/// Batch statistics returned to JS for one `push`.
#[wasm_bindgen]
pub struct JsOutput {
    sequence: u64,
    event_count: usize,
    diagnostics: usize,
}

#[wasm_bindgen]
impl JsOutput {
    /// Monotonic parser sequence after the feed.
    #[wasm_bindgen(getter)]
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Number of parsed events in this batch.
    #[wasm_bindgen(getter)]
    pub fn event_count(&self) -> usize {
        self.event_count
    }

    /// Number of parser diagnostics produced.
    #[wasm_bindgen(getter)]
    pub fn diagnostics(&self) -> usize {
        self.diagnostics
    }
}

/// Batched WebGPU render-plan statistics for one frame.
#[wasm_bindgen]
pub struct JsPlanStats {
    draw_calls: usize,
    cells: usize,
}

#[wasm_bindgen]
impl JsPlanStats {
    /// Draw calls in the batched plan.
    #[wasm_bindgen(getter)]
    pub fn draw_calls(&self) -> usize {
        self.draw_calls
    }

    /// Total visible cells in the frame.
    #[wasm_bindgen(getter)]
    pub fn cells(&self) -> usize {
        self.cells
    }
}

/// A terminal session owned by JS, backed by the native pipeline.
#[wasm_bindgen]
pub struct JsTerminal {
    bridge: TerminalBridge,
}

#[wasm_bindgen]
impl JsTerminal {
    /// Creates a terminal with `rows` x `cols` and a stream id.
    #[wasm_bindgen(constructor)]
    pub fn new(stream_id: u64, rows: usize, cols: usize) -> JsTerminal {
        JsTerminal {
            bridge: TerminalBridge::new(stream_id, rows, cols),
        }
    }

    /// Feeds a byte chunk (Uint8Array) through the native pipeline.
    pub fn push(&mut self, bytes: &[u8]) -> JsOutput {
        let out = self.bridge.push(bytes);
        JsOutput {
            sequence: out.sequence,
            event_count: out.event_count,
            diagnostics: out.diagnostics,
        }
    }

    /// Flushes incomplete parser state at end of stream.
    pub fn finish(&mut self) -> JsOutput {
        let out = self.bridge.finish();
        JsOutput {
            sequence: out.sequence,
            event_count: out.event_count,
            diagnostics: out.diagnostics,
        }
    }

    /// Resizes the screen, preserving content via reflow.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.bridge.resize(rows, cols);
    }

    /// The visible text (same pipeline as native).
    pub fn text(&self) -> String {
        self.bridge.text()
    }

    /// Cursor row (0-based).
    pub fn cursor_row(&self) -> usize {
        self.bridge.cursor().0
    }

    /// Cursor column (0-based).
    pub fn cursor_col(&self) -> usize {
        self.bridge.cursor().1
    }

    /// The window title, if any.
    pub fn title(&self) -> Option<String> {
        self.bridge.title().map(str::to_owned)
    }

    /// The parser's currently held bytes (memory bound visibility).
    pub fn pending_len(&self) -> usize {
        self.bridge.pending_len()
    }

    /// Batched WebGPU render-plan statistics for the current frame.
    pub fn render_plan_stats(&self) -> JsPlanStats {
        let plan = self.bridge.render_plan();
        JsPlanStats {
            draw_calls: plan.draw_calls.len(),
            cells: plan.cells,
        }
    }
}

/// The JS boundary version.
#[wasm_bindgen]
pub fn boundary_version() -> u32 {
    WASM_BOUNDARY_VERSION
}
