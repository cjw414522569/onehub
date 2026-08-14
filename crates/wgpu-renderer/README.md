# wgpu-renderer

- Layer: L2 (protocol adapters)
- Dependencies: `core-protocol`, `terminal-state`
- Scope: batched GPU terminal drawing on a single native wgpu surface.

## T077: batched draw plan and frame budget

`crates/wgpu-renderer/src/render.rs`:

- `RenderSurface::begin_frame(snapshot, budget)` - the only renderer entry
  point; cells are never individual UI controls (the API takes a snapshot and
  returns a batched plan).
- `build_plan` - groups cells by (glyph, fg, bg) into instanced `DrawCall`s;
  the draw-call count depends on distinct glyphs x styles, not cell count.
- `DrawBudget { max_draw_calls, max_instances_per_call }` - per-frame draw-call
  bound; `merge_to_budget` buckets by style and finally into a single
  full-grid call so a pathological frame never exceeds the budget.
- `frame_stats` - draw calls / instances / cells / under_budget.

## T078: cursor/selection/decoration/link/search layer compositing

`crates/wgpu-renderer/src/composite.rs`:

- `CompositeState` - tracks the base grid plus cursor / selection /
  decorations / links / search overlays with per-layer dirty flags.
- `plan_frame` - returns exactly the layers that need redrawing; unchanged
  frames are `stable` (nothing redrawn, no flicker).
- `FrameTimeline` - records per-frame plans for animation-stability
  validation.
- `selection_rects` / `selected_text` - compositor helpers.

Updating one overlay only marks that layer dirty - the base grid is never
re-laid out for an overlay change.

## T079: frame coalescing, refresh throttling, background throttling

`crates/wgpu-renderer/src/throttle.rs`:

- `FrameCoalescer` - folds many update notifications into one pending frame.
- `RefreshThrottle` - caps the frame rate at a minimum interval.
- `BoundedUpdateQueue` - a bounded update queue; over-cap updates are
  coalesced/dropped and counted, so high throughput never explodes the event
  queue.
- `SessionThrottler` - per-priority throttling: foreground sessions render at
  full rate (input prioritized), background sessions at a reduced rate; a
  rendered frame drains all pending updates.

The 100 MB/s synthetic output stress test runs deterministically as a unit
test (1,000,000 updates, bounded queue, drops counted).

## Verification

```text
cargo test -p wgpu-renderer --locked    PASS (10 tests)
```

The 4K fullscreen refresh GPU benchmark and image-golden / frame-timeline
screenshot validation require a real GPU/renderer and are `blocked_environment`
on CI hosts without one; a simulated 4K grid (240x67 cells) fits the default
draw budget with all cells instanced, and the compositing contract is verified
deterministically.