# terminal-state

- Layer: L1 (core services)
- Dependencies: `core-domain`, `core-protocol`, `unicode-width`, `unicode-segmentation`
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

## T064: Unicode width policy and grapheme clusters

`crates/terminal-state/src/unicode.rs` locks the Unicode version to the
`unicode-width` tables (`UNICODE_VERSION = "17.0.0"`, asserted against
`unicode_width::UNICODE_VERSION`) and exposes a configurable `WidthPolicy`:

| Policy | Behavior |
|---|---|
| `Unicode` (default) | UAX #11 non-CJK: East Asian Ambiguous = 1 column. |
| `EastAsian` | CJK context: Ambiguous = 2 columns. |
| `Legacy` | Every printable character occupies exactly 1 column. |

Text is segmented into extended grapheme clusters (UAX #29 via
`unicode-segmentation`), so combining sequences (`e` + U+0301) and ZWJ emoji
(family emoji) occupy a single cell with the width of their base. Wide
clusters mark a `wide_continuation` cell in the snapshot (`TerminalCell`),
which is cleared together with its base on overwrite or partial erase.
`ScreenModel::set_width_policy` selects the policy at runtime.

The deterministic golden (`crates/terminal-parser/tests/golden/vttest-basic.json`,
66,125 bytes) covers SGR, erase, cursor, scroll region, origin mode, alternate
screen, title, plus a CJK wide char, a combining sequence, and a ZWJ family
emoji.

## Verification

```text
cargo test -p terminal-state --locked    PASS (20 tests)
cargo test -p terminal-parser --locked   PASS (11 unit + 1 golden)
cargo test -p core-protocol --locked     PASS (14 tests)
node .\scripts\test-terminal-screen.mjs . PASS
node .\scripts\test-terminal-unicode.mjs . PASS
```