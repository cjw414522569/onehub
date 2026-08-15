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

## T140: PWA offline shell, update, and data-clearing policy

- `src/service-worker.ts` - versioned app-shell cache names, update /
  activation flow (skipWaiting), stale-cache purge, and a memory-only
  session cache.
- `src/cache-policy.ts` - cache classification (app-shell vs session /
  forbidden), the audit that guarantees offline caches hold no session
  secrets, and `clearSessionData` which purges session entries while
  keeping the app shell.
- `src/connectivity.ts` + `ShellModel.setConnectivity` - offline never
  claims connectivity: a live session is suspended (status
  `Offline - not connected`) and reconnects are only offered online.
- `test/pwa-offline.ts` + `pwa/cache-audit.snapshot.json` - service-worker
  upgrade, cache audit, and clear-session-data E2E with a byte-identical
  regenerable snapshot.

## Tests

- `test/e2e.ts` - critical-path E2E (headless, deterministic): responsive
  layout, host select, gateway handshake to ready, terminal render, input
  byte encoding, resize, mobile switch, disconnect.
- `test/pwa-offline.ts` - service-worker upgrade + purge, cache policy +
  audit, offline connectivity (no connectivity claim while offline).
- `test/screenshot-matrix.ts` - regenerates the browser-engine screenshot
  matrix (`screenshots/*.svg`) for Chromium / Firefox / Safari desktop and
  mobile viewports. Regenerate with `node --experimental-strip-types
  test/screenshot-matrix.ts --write`; the contract verifies byte-identical
  regeneration.

## Gates

- `npm run typecheck` (strict `tsc --noEmit`).
- `scripts/test-web-shell.mjs` - type gate + critical-path E2E +
  screenshot-matrix snapshot check (T139).
- `scripts/test-pwa-offline.mjs` - type gate + service-worker/cache-audit
  E2E + cache-audit snapshot check (T140).