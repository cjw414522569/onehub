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
`unicode_width::UNICODE_VERSION`) and exposes a configurable `WidthPolicy`
(`Unicode` / `EastAsian` / `Legacy`). Text is segmented into extended grapheme
clusters (UAX #29), so combining sequences and ZWJ emoji occupy a single cell
with the width of their base; wide clusters mark a `wide_continuation` cell.
See `crates/terminal-parser/tests/golden/vttest-basic.json`.

## T065: colors, attributes, underline styles, and palette

`crates/terminal-state/src/color.rs`:

- `Rgb` — an 8-bit RGB color.
- `Palette` — configurable default fg/bg plus the 16 ANSI colors (8 regular +
  8 bright), with `resolve(TerminalColor) -> Rgb`; indices 16..=231 use the
  6x6x6 color cube and 232..=255 the 24-step grayscale ramp (xterm
  convention).

`ScreenBuffer::apply_sgr` now supports the full T065 set: 16-color fg/bg
(30-37/90-97, 40-47/100-107), 256-color (`38;5;n`), truecolor (`38;2;r;g;b`),
bold/dim/italic/inverse, and underline styles including `4:N` colon
sub-parameters (`4:0` off, `4:1` single, `4:2` double, `4:3` curly, `4:4`
dotted, `4:5` dashed) plus `21` double and `24` reset. `Sgr` carries
`Vec<Vec<u16>>` so each parameter keeps its `:`-separated sub-parameters.
The color-matrix golden (`crates/terminal-parser/tests/golden/color-matrix.json`,
66,615 bytes) covers 16/256/truecolor, inverse/dim, and underline styles.

## Verification

```text
cargo test -p terminal-state --locked    PASS (27 tests)
cargo test -p terminal-parser --locked   PASS (11 unit + 2 golden)
cargo test -p core-protocol --locked     PASS (14 tests)
node .\scripts\test-terminal-screen.mjs . PASS
node .\scripts\test-terminal-unicode.mjs . PASS
node .\scripts\test-color-golden.mjs .   PASS
```