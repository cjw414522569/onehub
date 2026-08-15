# web/app — TypeScript Web/PWA shell (T139)

Layer: Web/PWA (Tier 2). Browser shell that connects through the gateway
(WebSocket); no direct TCP.

## Modules

- `src/types.ts` - shared model types; `Snapshot` mirrors the native
  `core-protocol` terminal snapshot so the surface consumes the same
  vectors the Rust WASM bridge (T138) produces.
- `src/terminal-surface.ts` - `TerminalSurface`: cell-grid display model
  with `applySnapshot`, same-width resize, `text()`, and deterministic SVG
  rendering for the screenshot matrix.
- `src/vt-parser.ts` - minimal VT byte-stream parser for the browser
  critical path (full contract parser lives in the Rust core, T062).
- `src/input-adapter.ts` - key / paste / focus encoders mirroring the
  native `encode_key_xterm` contract (Enter->CR, Ctrl+letter->control
  byte, arrows->CSI/SS3, Alt->ESC prefix, bracketed paste).
- `src/gateway-transport.ts` - versioned session messages mirroring the
  gateway protocol (T135); `MockGateway` performs the hello->auth->
  capabilities->ready handshake for E2E.
- `src/shell.ts` - `ShellModel`: responsive layout (desktop sidebar /
  mobile collapsed), connect state machine, terminal wiring, and input
  plumbing. `layoutFor()` computes the layout from the viewport.
- `src/index.ts` - `createShell()` factory.

## Tests

- `test/e2e.ts` - critical-path E2E (headless, deterministic): responsive
  layout, host select, gateway handshake to ready, terminal render, input
  byte encoding, resize, mobile switch, disconnect.
- `test/screenshot-matrix.ts` - regenerates the browser-engine screenshot
  matrix (`screenshots/*.svg`) for Chromium / Firefox / Safari desktop and
  mobile viewports. Regenerate with `node --experimental-strip-types
  test/screenshot-matrix.ts --write`; the contract verifies byte-identical
  regeneration.

## Gates

- `npm run typecheck` (strict `tsc --noEmit`).
- `scripts/test-web-shell.mjs` runs the type gate, the E2E critical paths,
  and the screenshot-matrix snapshot check.