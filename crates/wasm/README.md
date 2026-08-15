# wasm

- Layer: L4 (ABI/platform bridge)
- Dependencies: `core-protocol`, `terminal-state`, `terminal-parser`, `wgpu-renderer`, `wasm-bindgen`
- Scope: browser boundary that preserves the versioned batch contract.

## T138: WASM/WebGPU compile and JS interop boundary

- `crates/wasm/src/bridge.rs` - `TerminalBridge` drives the exact same
  pipeline as native: `terminal-parser`'s bounded byte-stream parser feeds
  `terminal-state`'s screen model, and the `wgpu-renderer` plan builder
  consumes the same `TerminalSnapshot` the native renderer uses. Bridge
  tests reuse the native terminal test vectors (text, SGR, OSC title,
  resize/reflow, fragmented-feed equivalence, Unicode width).
- `crates/wasm/src/ffi.rs` - versioned `wasm-bindgen` boundary:
  `boundary_version()` (1), `JsTerminal` (`push`/`finish`/`resize`/text/
  cursor/title/`render_plan_stats`), `JsOutput`, `JsPlanStats`.
- The crate compiles for `wasm32-unknown-unknown` (`cdylib`) together with
  `terminal-state`, `core-domain`, and `wgpu-renderer`; the release `.wasm`
  is gated to a bundle budget.
- WebGPU compatibility: render plans are built from the same snapshot the
  native renderer consumes, so the WASM boundary uses the identical
  test vectors and frame contract as native.