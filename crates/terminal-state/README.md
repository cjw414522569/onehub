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
| `ParseEvent` | Text / CR / LF / BS / EraseDisplay / EraseLine / CursorPosition / CursorMove / Sgr / SetMode / SetScrollRegion / Title / WorkingDirectory / Notification. |
| `ParserDiagnostic` | Stable codes (`invalid_utf8`, `sequence_too_long`, `truncated_sequence`), no secrets. |
| `ParseBatch` | Monotonic feed sequence + events + diagnostics. |
| `TerminalParser` | `feed` / `finish` contract. |
| `Modes` | DEC/ANSI mode state (autowrap, insert, origin, alternate screen, cursor visible, app cursor/keypad, bracketed paste, reverse video, mouse tracking). |
| `ScreenBuffer` | Grid, cursor + saved cursor, scroll region, SGR style, wrap/pending-wrap, linefeed+scroll semantics. |
| `ScreenModel` | Primary + alternate buffers, mode routing, `?1049` enter/exit with cursor save/restore, `resize`, `apply_batch`/`apply_event`, `snapshot` -> `TerminalSnapshot` for the renderer. |

## T064: Unicode width policy and grapheme clusters

`crates/terminal-state/src/unicode.rs` locks the Unicode version
(`UNICODE_VERSION = "17.0.0"`) and exposes a configurable `WidthPolicy`
(`Unicode` / `EastAsian` / `Legacy`). Text is segmented into extended grapheme
clusters (UAX #29); wide clusters mark a `wide_continuation` cell. See
`crates/terminal-parser/tests/golden/vttest-basic.json`.

## T065: colors, attributes, underline styles, and palette

`crates/terminal-state/src/color.rs` provides `Rgb` and a configurable
`Palette` (default fg/bg + 16 ANSI colors) with xterm 256-color resolution
(cube + grayscale). `ScreenBuffer::apply_sgr` supports 16/256/truecolor,
bold/dim/italic/inverse, and underline styles (`4:N` colon sub-parameters,
`21`, `24`). `Sgr` carries `Vec<Vec<u16>>` sub-parameters. See
`crates/terminal-parser/tests/golden/color-matrix.json`.

## T066: OSC title, working directory, notifications, and security filter

`crates/terminal-state/src/osc.rs`:

- `Notification` — summary + body.
- `OscPolicy` — gates titles (`allow_title`), the working directory
  (`allow_working_directory`), and notifications (`allow_notifications`);
  sanitizes payloads (control characters stripped) and caps lengths.
  Notifications are **denied by default** so untrusted terminal output cannot
  bypass the notification policy; opt in explicitly.

`ScreenModel` applies the policy to `Title` / `WorkingDirectory` /
`Notification` events, stores the sanitized title and working directory in the
snapshot, and exposes `notification()` / `take_notification()` for the UI
layer. The parser emits `WorkingDirectory` for OSC 7 and `Notification` for
OSC 9 / OSC 777;notify, terminated by BEL or ST.

## Verification

```text
cargo test -p terminal-state --locked    PASS (35 tests)
cargo test -p terminal-parser --locked   PASS (13 unit + 2 golden)
cargo test -p core-protocol --locked     PASS (14 tests)
node .\scripts\test-terminal-screen.mjs . PASS
node .\scripts\test-terminal-unicode.mjs . PASS
node .\scripts\test-color-golden.mjs .   PASS
node .\scripts\test-osc-policy.mjs .     PASS
```