# terminal-state

- Layer: L1 (core services)
- Dependencies: `core-domain`, `core-protocol`
- Scope: deterministic terminal screen state, bounded snapshots, and the shared
  parser event vocabulary.

## T062/T063: screen model and parser contract

`terminal-state` owns the terminal event vocabulary used by the L2
`terminal-parser` pipeline and the screen model that consumes it.

| Model | Purpose |
|---|---|
| `ParseEvent` | Text / CR / LF / BS / EraseDisplay / EraseLine / CursorPosition / CursorMove / Sgr / SetMode / SetScrollRegion / Title. |
| `ParserDiagnostic` | Stable codes (`invalid_utf8`, `sequence_too_long`, `truncated_sequence`), no secrets. |
| `ParseBatch` | Monotonic feed sequence + events + diagnostics. |
| `TerminalParser` | `feed` / `finish` contract. |
| `Modes` | DEC/ANSI mode state (autowrap, insert, origin, alternate screen, cursor visible, app cursor/keypad, bracketed paste, reverse video, mouse tracking). |
| `ScreenBuffer` | Grid, cursor + saved cursor, scroll region, SGR style, wrap/pending-wrap, linefeed+scroll semantics. |
| `ScreenModel` | Primary + alternate buffers, mode routing, `?1049` enter/exit with cursor save/restore, `resize`, `apply_batch`/`apply_event`, `snapshot` -> `TerminalSnapshot` for the renderer. |

Screen semantics: writing wraps at the right edge (autowrap off pins the
cursor), linefeed scrolls inside the scroll region, origin mode positions the
cursor relative to the region top, SGR covers 16/256/truecolor plus
bold/italic/underline/inverse/dim, and `?1049` preserves the primary buffer
while the alternate screen is active. A deterministic golden test
(`crates/terminal-parser/tests/golden/vttest-basic.json`, 8x20) feeds a
vttest-basic-style script through the parser into the model and diffs the
snapshot; the full interactive vttest suite requires a real terminal
(`blocked_environment` on CI hosts without one).

## Verification

```text
cargo test -p terminal-state --locked    PASS (10 tests)
cargo test -p terminal-parser --locked   PASS (11 unit + 1 golden)
node .\scripts\test-terminal-screen.mjs . PASS
```