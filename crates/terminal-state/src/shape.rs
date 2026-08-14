//! Glyph shaping, ligature toggle, and cell-alignment constraints (T075).
//!
//! Terminal rendering must never let shaping break the grid: every shaped
//! glyph maps to a whole number of cells and the total cell count equals the
//! text's display width. [`shape_run`] models this contract — ligature
//! patterns merge adjacent clusters into one glyph that still spans the same
//! cells, and [`grid_fit`] maps an advance width to cells (ceil) so glyphs
//! stay inside their grid footprint. HarfBuzz-driven screenshot validation
//! (programming ligatures / CJK / RTL) requires a real renderer and is
//! `blocked_environment` on CI hosts without one.

use unicode_segmentation::UnicodeSegmentation;

use crate::unicode::WidthPolicy;

/// Whether ligatures are applied during shaping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LigaturePolicy {
    /// Merge known ligature sequences into single glyphs.
    Enabled,
    /// Keep every cluster as its own glyph.
    Disabled,
}

/// One shaped glyph with its grid-cell footprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapedGlyph {
    /// The cluster text (e.g. `"->"` or `"中"`).
    pub text: String,
    /// Grid cells this glyph occupies.
    pub cells: u16,
    /// Whether a ligature was applied to merge clusters.
    pub ligature: bool,
}

/// Common programming ligature sequences (the renderer's HarfBuzz font
/// features handle the actual glyph; the grid contract is modeled here).
const LIGATURES: [&str; 8] = ["->", "=>", "->>", "!=", ">=", "<=", "::", "=="];

/// Maps an advance width to whole grid cells (ceil), so a glyph's advance
/// never overflows its cell footprint.
pub fn grid_fit(advance_units: f32, cell_units: f32) -> u16 {
    if cell_units <= 0.0 {
        return 1;
    }
    (advance_units / cell_units).ceil().max(1.0) as u16
}

/// Shapes a run of text into glyphs with grid-cell footprints.
///
/// The output preserves terminal grid semantics: each glyph occupies whole
/// cells and the total cell count equals the text's display width under the
/// given width policy. When ligatures are enabled, adjacent clusters that
/// form a known ligature sequence merge into one glyph spanning the same
/// cells.
pub fn shape_run(text: &str, policy: WidthPolicy, ligatures: LigaturePolicy) -> Vec<ShapedGlyph> {
    let clusters: Vec<&str> = text.graphemes(true).collect();
    let mut glyphs: Vec<ShapedGlyph> = Vec::new();
    let mut index = 0usize;
    while index < clusters.len() {
        let cluster = clusters[index];
        let width = policy.cluster_width(cluster).max(1) as u16;
        if ligatures == LigaturePolicy::Enabled && index + 1 < clusters.len() {
            let pair = format!("{}{}", cluster, clusters[index + 1]);
            if LIGATURES.contains(&pair.as_str()) {
                glyphs.push(ShapedGlyph {
                    text: pair,
                    cells: width + policy.cluster_width(clusters[index + 1]).max(1) as u16,
                    ligature: true,
                });
                index += 2;
                continue;
            }
        }
        glyphs.push(ShapedGlyph {
            text: cluster.to_owned(),
            cells: width,
            ligature: false,
        });
        index += 1;
    }
    glyphs
}

/// Verifies the grid invariant: total glyph cells equal the text's display
/// width, and no glyph spans fewer cells than its own display width.
pub fn cells_align(glyphs: &[ShapedGlyph], policy: WidthPolicy) -> bool {
    let total: usize = glyphs.iter().map(|g| g.cells as usize).sum();
    let text: String = glyphs.iter().map(|g| g.text.as_str()).collect();
    total == policy.str_width(&text).max(1)
        && glyphs
            .iter()
            .all(|g| (g.cells as usize) >= policy.cluster_width(&g.text).max(1))
}

#[cfg(test)]
mod tests {
    use super::{cells_align, grid_fit, shape_run, LigaturePolicy, ShapedGlyph};
    use crate::unicode::WidthPolicy;

    #[test]
    fn ligature_merges_into_one_glyph_with_same_cells() {
        let glyphs = shape_run("->", WidthPolicy::Unicode, LigaturePolicy::Enabled);
        assert_eq!(
            glyphs,
            vec![ShapedGlyph {
                text: "->".to_owned(),
                cells: 2,
                ligature: true,
            }]
        );
        // Disabled: two separate glyphs, same total cells.
        let glyphs = shape_run("->", WidthPolicy::Unicode, LigaturePolicy::Disabled);
        assert_eq!(glyphs.len(), 2);
        assert!(glyphs.iter().all(|g| !g.ligature));
        assert!(cells_align(&glyphs, WidthPolicy::Unicode));
    }

    #[test]
    fn cjk_and_rtl_glyphs_keep_grid_semantics() {
        // CJK wide char: one glyph, 2 cells.
        let cjk = shape_run("中", WidthPolicy::Unicode, LigaturePolicy::Enabled);
        assert_eq!(cjk[0].cells, 2);
        // RTL Hebrew letters: one cell each, shaping does not change widths.
        let rtl = shape_run("אב", WidthPolicy::Unicode, LigaturePolicy::Enabled);
        assert_eq!(rtl.len(), 2);
        assert!(rtl.iter().all(|g| g.cells == 1));
        assert!(cells_align(&rtl, WidthPolicy::Unicode));
    }

    #[test]
    fn grid_invariant_holds_for_mixed_runs() {
        let text = "let x => 中😀 -> end";
        for ligatures in [LigaturePolicy::Enabled, LigaturePolicy::Disabled] {
            let glyphs = shape_run(text, WidthPolicy::Unicode, ligatures);
            assert!(
                cells_align(&glyphs, WidthPolicy::Unicode),
                "grid invariant must hold (ligatures={ligatures:?})"
            );
        }
    }

    #[test]
    fn grid_fit_rounds_advance_up_to_cells() {
        assert_eq!(grid_fit(13.0, 6.0), 3); // ceil(13/6)
        assert_eq!(grid_fit(12.0, 6.0), 2);
        assert_eq!(grid_fit(1.0, 6.0), 1); // at least one cell
        assert_eq!(grid_fit(10.0, 0.0), 1); // degenerate cell width
    }
}
