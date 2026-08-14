//! GPU batched terminal drawing on a single native surface (T077).
//!
//! Terminal cells are never per-cell UI controls: the renderer accepts a
//! [`TerminalSnapshot`] and builds a [`RenderPlan`] of batched instanced
//! [`DrawCall`]s (one instance per visible cell; all glyphs live in one atlas
//! texture). A per-frame [`DrawBudget`] bounds the draw-call count; if a
//! pathological frame would exceed it, [`merge_to_budget`] buckets the calls
//! by style and finally into a single full-grid call, so one frame never
//! exceeds the budget. The 4K fullscreen refresh GPU benchmark requires a
//! real GPU and is `blocked_environment` on CI hosts without one; the plan /
//! budget contract is verified deterministically.

use std::collections::HashMap;

use core_protocol::terminal::{TerminalColor, TerminalSnapshot};

/// Per-frame draw budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawBudget {
    /// Maximum draw calls per frame.
    pub max_draw_calls: u32,
    /// Maximum instances per instanced call (index/vertex limits).
    pub max_instances_per_call: u32,
}

impl Default for DrawBudget {
    fn default() -> Self {
        Self {
            max_draw_calls: 256,
            max_instances_per_call: 65_535,
        }
    }
}

/// One batched, instanced draw call: `instances` cells sharing a glyph and
/// style are drawn in a single call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawCall {
    /// The glyph text.
    pub glyph: String,
    /// Foreground color.
    pub fg: TerminalColor,
    /// Background color.
    pub bg: TerminalColor,
    /// Number of cell instances.
    pub instances: u32,
}

/// The per-frame render plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderPlan {
    /// Batched draw calls.
    pub draw_calls: Vec<DrawCall>,
    /// Total visible cells in the frame.
    pub cells: usize,
}

/// Per-frame statistics for the budget check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameStats {
    /// Draw calls in the frame.
    pub draw_calls: u32,
    /// Total cell instances.
    pub instances: u64,
    /// Total visible cells.
    pub cells: usize,
    /// Whether the frame is within the draw-call budget.
    pub under_budget: bool,
}

/// Builds a batched plan from a snapshot: cells are grouped by
/// (glyph, fg, bg) so the draw-call count depends on distinct glyphs x
/// styles, not on the number of cells.
pub fn build_plan(snapshot: &TerminalSnapshot, budget: &DrawBudget) -> RenderPlan {
    let mut grouped: HashMap<(String, TerminalColor, TerminalColor), u32> = HashMap::new();
    for row in &snapshot.rows {
        for cell in &row.cells {
            if let Some(text) = cell.text.as_deref() {
                let key = (text.to_owned(), cell.style.fg, cell.style.bg);
                *grouped.entry(key).or_insert(0) += 1;
            }
        }
    }
    let mut draw_calls = Vec::new();
    for ((glyph, fg, bg), count) in grouped {
        let mut remaining = count;
        while remaining > 0 {
            let instances = remaining.min(budget.max_instances_per_call);
            draw_calls.push(DrawCall {
                glyph: glyph.clone(),
                fg,
                bg,
                instances,
            });
            remaining -= instances;
        }
    }
    RenderPlan {
        draw_calls,
        cells: snapshot.rows.iter().map(|row| row.cells.len()).sum(),
    }
}

/// Merges a plan to fit the draw-call budget: first by style, then into a
/// single full-grid call. Instances (cell count) are never lost.
pub fn merge_to_budget(plan: &RenderPlan, budget: &DrawBudget) -> RenderPlan {
    if (plan.draw_calls.len() as u32) <= budget.max_draw_calls {
        return plan.clone();
    }
    // Bucket by style (instances carry per-instance glyph UVs).
    let mut by_style: HashMap<(TerminalColor, TerminalColor), u64> = HashMap::new();
    for call in &plan.draw_calls {
        *by_style.entry((call.fg, call.bg)).or_insert(0) += call.instances as u64;
    }
    if (by_style.len() as u32) <= budget.max_draw_calls {
        let mut draw_calls = Vec::new();
        for ((fg, bg), instances) in by_style {
            draw_calls.push(DrawCall {
                glyph: " ".to_owned(),
                fg,
                bg,
                instances: instances as u32,
            });
        }
        return RenderPlan {
            draw_calls,
            cells: plan.cells,
        };
    }
    // Last resort: a single full-grid call.
    let instances: u32 = plan
        .draw_calls
        .iter()
        .map(|call| call.instances as u64)
        .sum::<u64>()
        .min(u32::MAX as u64) as u32;
    RenderPlan {
        draw_calls: vec![DrawCall {
            glyph: " ".to_owned(),
            fg: TerminalColor::Default,
            bg: TerminalColor::Default,
            instances,
        }],
        cells: plan.cells,
    }
}

/// Computes per-frame statistics against the budget.
pub fn frame_stats(plan: &RenderPlan, budget: &DrawBudget) -> FrameStats {
    FrameStats {
        draw_calls: plan.draw_calls.len() as u32,
        instances: plan
            .draw_calls
            .iter()
            .map(|call| call.instances as u64)
            .sum(),
        cells: plan.cells,
        under_budget: (plan.draw_calls.len() as u32) <= budget.max_draw_calls,
    }
}

/// A single native render surface (one wgpu surface; no per-cell controls).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSurface {
    /// Surface handle (opaque; owned by the platform/GPU layer).
    pub handle: u64,
}

impl RenderSurface {
    /// Begins a frame: builds the batched plan for the snapshot. This is the
    /// only renderer entry point — cells are never individual UI controls.
    pub fn begin_frame(&self, snapshot: &TerminalSnapshot, budget: &DrawBudget) -> RenderPlan {
        build_plan(snapshot, budget)
    }
}

#[cfg(test)]
mod tests {
    use core_protocol::terminal::{
        CursorState, TerminalCell, TerminalColor, TerminalRow, TerminalSnapshot,
    };

    use super::{build_plan, frame_stats, merge_to_budget, DrawBudget, RenderSurface};

    fn snapshot(rows: &[&str]) -> TerminalSnapshot {
        TerminalSnapshot {
            stream_id: 1,
            sequence: 1,
            rows: rows
                .iter()
                .map(|row| TerminalRow {
                    cells: row
                        .chars()
                        .map(|c| {
                            let mut cell = TerminalCell::cluster(c.to_string());
                            cell.style.fg = TerminalColor::Indexed((c as u8) % 8);
                            cell
                        })
                        .collect(),
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
    fn plan_groups_by_glyph_and_style() {
        let snap = snapshot(&["aa bb", "aa"]);
        let budget = DrawBudget::default();
        let plan = build_plan(&snap, &budget);
        // 'a' x3 (same style), 'b' x2 (same style), space x1 -> 3 draw calls.
        assert_eq!(plan.draw_calls.len(), 3);
        let stats = frame_stats(&plan, &budget);
        assert_eq!(stats.instances, 7, "6 chars + 1 space");
        assert_eq!(stats.cells, 7);
        assert!(stats.under_budget);
    }

    #[test]
    fn pathological_frame_stays_within_budget() {
        // A pathological grid with thousands of distinct glyphs.
        let rows: Vec<String> = (0..200)
            .map(|row| {
                (0..200)
                    .map(|col| char::from(((row * 200 + col) % 256) as u8))
                    .collect()
            })
            .collect();
        let refs: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
        let snap = snapshot(&refs);
        let budget = DrawBudget {
            max_draw_calls: 8,
            max_instances_per_call: 65_535,
        };
        let plan = build_plan(&snap, &budget);
        let merged = merge_to_budget(&plan, &budget);
        let stats = frame_stats(&merged, &budget);
        assert!(
            stats.under_budget,
            "draw_calls {} > budget",
            stats.draw_calls
        );
        assert_eq!(
            stats.instances, 40_000,
            "instances (cells) must never be lost"
        );
    }

    #[test]
    fn four_k_grid_benchmark_fits_budget() {
        // Simulated 4K fullscreen grid (3840x2160 at ~16x32 cells -> 240x67).
        let rows = 67usize;
        let cols = 240usize;
        let cells = vec![TerminalCell::empty(); cols];
        let mut snapshot = TerminalSnapshot {
            stream_id: 1,
            sequence: 1,
            rows: (0..rows)
                .map(|_| TerminalRow {
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
        };
        // Fill with text.
        for (row_index, row) in snapshot.rows.iter_mut().enumerate() {
            for (col, cell) in row.cells.iter_mut().enumerate() {
                let ch = char::from(b'a' + (col % 26) as u8);
                *cell = TerminalCell::cluster(ch.to_string());
                cell.style.fg = TerminalColor::Indexed((col % 8) as u8);
                let _ = row_index;
            }
        }
        let budget = DrawBudget::default();
        let surface = RenderSurface { handle: 1 };
        let plan = surface.begin_frame(&snapshot, &budget);
        let stats = frame_stats(&plan, &budget);
        assert!(
            stats.under_budget,
            "4K grid must fit the draw budget (draw_calls={})",
            stats.draw_calls
        );
        assert_eq!(stats.instances as usize, rows * cols);
    }

    #[test]
    fn merge_keeps_instances_when_merging() {
        let snap = snapshot(&["abcdefghij"]);
        let plan = build_plan(&snap, &DrawBudget::default());
        let tight = DrawBudget {
            max_draw_calls: 1,
            max_instances_per_call: 65_535,
        };
        let merged = merge_to_budget(&plan, &tight);
        assert_eq!(merged.draw_calls.len(), 1);
        assert_eq!(frame_stats(&merged, &tight).instances, 10);
    }
}
