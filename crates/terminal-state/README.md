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
| `ParseEvent` | Text / CR / LF / BS / EraseDisplay / EraseLine / CursorPosition / CursorMove / Sgr / SetMode / SetScrollRegion / Title / WorkingDirectory / Notification / Hyperlink. |
| `ParserDiagnostic` | Stable codes (`invalid_utf8`, `sequence_too_long`, `truncated_sequence`), no secrets. |
| `ParseBatch` | Monotonic feed sequence + events + diagnostics. |
| `TerminalParser` | `feed` / `finish` contract. |
| `Modes` | DEC/ANSI mode state (autowrap, insert, origin, alternate screen, cursor visible, app cursor/keypad, bracketed paste, reverse video, mouse tracking). |
| `ScreenBuffer` | Grid, cursor + saved cursor, scroll region, SGR style, active hyperlink, wrap/pending-wrap, linefeed+scroll semantics. |
| `ScreenModel` | Primary + alternate buffers, mode routing, `?1049` enter/exit with cursor save/restore, `resize`, `apply_batch`/`apply_event`, `snapshot` -> `TerminalSnapshot` for the renderer. |

## T064: Unicode width policy and grapheme clusters

`crates/terminal-state/src/unicode.rs` locks the Unicode version
(`UNICODE_VERSION = "17.0.0"`) and exposes a configurable `WidthPolicy`
(`Unicode` / `EastAsian` / `Legacy`). Text is segmented into extended grapheme
clusters (UAX #29); wide clusters mark a `wide_continuation` cell.

## T065: colors, attributes, underline styles, and palette

`crates/terminal-state/src/color.rs` provides `Rgb` and a configurable
`Palette` with xterm 256-color resolution. `ScreenBuffer::apply_sgr` supports
16/256/truecolor, bold/dim/italic/inverse, and underline styles (`4:N` colon
sub-parameters, `21`, `24`).

## T066: OSC title, working directory, notifications, and security filter

`crates/terminal-state/src/osc.rs` provides `Notification` and `OscPolicy`
(gating + control-character stripping + length caps; notifications denied by
default). The parser emits `Title` (OSC 0/2), `WorkingDirectory` (OSC 7), and
`Notification` (OSC 9 / OSC 777;notify), terminated by BEL or ST.

## T067: OSC 8 hyperlinks and open-confirmation policy

`crates/terminal-state/src/hyperlink.rs`:

- `HyperlinkPolicy` — scheme whitelist (`https`, `http`, `ssh`, `sftp`,
  `mailto`) + URI length cap; `javascript:` / `data:` / `vbscript:` / `file:`
  are forbidden.
- `review(uri)` — returns a `HyperlinkReview` (raw URI, lowercased scheme,
  effective host after stripping userinfo and port) so the UI can show the
  real target in an explicit open-confirmation dialog. Phishing samples such
  as `https://example.com@evil.com/path` surface `evil.com`.

`ScreenModel` keeps an active hyperlink per buffer (`set_hyperlink` /
`clear_hyperlink`); cells written while a link is active carry it, and an
empty-URI OSC 8 ends the link. The golden `vttest-basic.json` captures a
"click" cell with `id=doc` and `url=https://example.com/path`.

## Verification

```text
cargo test -p terminal-state --locked    PASS (41 tests)
cargo test -p terminal-parser --locked   PASS (14 unit + 2 golden)
cargo test -p core-protocol --locked     PASS (14 tests)
node .\scripts\test-terminal-screen.mjs . PASS
node .\scripts\test-terminal-unicode.mjs . PASS
node .\scripts\test-color-golden.mjs .   PASS
node .\scripts\test-osc-policy.mjs .     PASS
node .\scripts\test-hyperlink-policy.mjs . PASS
```