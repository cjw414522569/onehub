# wgpu-renderer

- Layer: L2 (protocol adapters)
- Dependencies: `core-protocol`, `terminal-state`
- Scope: batched GPU terminal drawing on a single native wgpu surface.

## T077: batched draw plan and frame budget

`crates/wgpu-renderer/src/render.rs`:

- `RenderSurface::begin_frame(snapshot, budget)` — the only renderer entry
  point; cells are never individual UI controls (the API takes a snapshot and
  returns a batched plan).
- `build_plan` — groups cells by (glyph, fg, bg) into instanced `DrawCall`s;
  the draw-call count depends on distinct glyphs x styles, not cell count.
- `DrawBudget { max_draw_calls, max_instances_per_call }` — per-frame draw-call
  bound; `merge_to_budget` buckets by style and finally into a single
  full-grid call so a pathological frame never exceeds the budget.
- `frame_stats` — draw calls / instances / cells / under_budget.

The 4K fullscreen refresh GPU benchmark requires a real GPU and is
`blocked_environment` on CI hosts without one; a simulated 4K grid
(240x67 cells) is verified to fit the default budget with all cells
instanced.