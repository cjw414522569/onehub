# terminal-state

- Layer: L1 (core services)
- Dependencies: `core-domain`, `core-protocol`, `unicode-width`, `unicode-segmentation`
- Scope: deterministic terminal screen state, bounded snapshots, input
  encoders, and the shared parser event vocabulary.

## T062/T063: screen model and parser contract

`terminal-state` owns the terminal event vocabulary used by the L2
`terminal-parser` pipeline and the screen model that consumes it.

| Model | Purpose |
|---|---|
| `ParseEvent` | Text / CR / LF / BS / EraseDisplay / EraseLine / CursorPosition / CursorMove / Sgr / SetMode / SetScrollRegion / Title / WorkingDirectory / Notification / Hyperlink. |
| `ParserDiagnostic` | Stable codes (`invalid_utf8`, `sequence_too_long`, `truncated_sequence`), no secrets. |
| `ParseBatch` | Monotonic feed sequence + events + diagnostics. |
| `TerminalParser` | `feed` / `finish` contract. |
| `Modes` | DEC/ANSI mode state including bracketed paste (?2004), mouse mode (?1000/?1002/?1003), SGR mouse (?1006), focus events (?1004), and the negotiated keyboard protocol. |
| `ScreenBuffer` | Grid, cursor + saved cursor, scroll region, SGR style, active hyperlink, wrap/pending-wrap, linefeed+scroll semantics. |
| `ScreenModel` | Primary + alternate buffers, mode routing, `?1049` enter/exit, `resize`, `apply_batch`/`apply_event`, `snapshot` -> `TerminalSnapshot`. |

## T064: Unicode width policy and grapheme clusters

`crates/terminal-state/src/unicode.rs` locks the Unicode version
(`UNICODE_VERSION = "17.0.0"`) and exposes a configurable `WidthPolicy`
(`Unicode` / `EastAsian` / `Legacy`). Text is segmented into extended grapheme
clusters (UAX #29); wide clusters mark a `wide_continuation` cell.

## T065: colors, attributes, underline styles, and palette

`crates/terminal-state/src/color.rs` provides `Rgb` and a configurable
`Palette` with xterm 256-color resolution. `ScreenBuffer::apply_sgr` supports
16/256/truecolor, bold/dim/italic/inverse, and underline styles.

## T066: OSC title, working directory, notifications, and security filter

`crates/terminal-state/src/osc.rs` provides `Notification` and `OscPolicy`
(gating + control-character stripping + length caps; notifications denied by
default).

## T067: OSC 8 hyperlinks and open-confirmation policy

`crates/terminal-state/src/hyperlink.rs` provides `HyperlinkPolicy` (scheme
whitelist + length cap; dangerous schemes forbidden) and `review(uri)` which
surfaces the effective host for explicit open confirmation.

## T068: bracketed paste, focus, mouse, and keyboard protocols

`crates/terminal-state/src/input.rs`:

- `encode_paste` — wraps pasted text in `CSI 200~` / `CSI 201~` when ?2004 is
  active.
- `encode_focus` — reports `CSI I` / `CSI O` when ?1004 is active.
- `encode_mouse` — X10 or SGR (?1006) encoding for press/release/motion/wheel
  with the xterm mouse modifier bits (shift=4, meta/alt=8, ctrl=16).
- `encode_key` — xterm sequences (application cursor mode, control bytes,
  `CSI 1;modA` modifiers), modifyOtherKeys (`CSI 27;mod;code~`), and the kitty
  keyboard protocol (`CSI code:mod[:repeat] u`, functional-key codes,
  UTF-8 for unmodified printable keys).
- `KeyboardProtocol::kitty_probe` / `from_kitty_reply` — kitty capability
  detection from the remote's `CSI ? 1;2 u` reply.

`Modes` gained `mouse_mode`, `mouse_sgr`, `focus_events`, and
`keyboard_protocol`; `set_mode` wires ?1000/?1002/?1003/?1004/?1006.

## T069: configurable scrollback and disk dump policy

`crates/terminal-state/src/scrollback.rs`:

- `Scrollback` ? a bounded ring buffer (`VecDeque`) of rows scrolled off the
  primary screen top. `max_lines` is explicit; the buffer never exceeds it
  and `lines_dropped` counts evictions. A million-line benchmark stays within
  the configured bound.
- `ScrollbackConfig` ? configurable capacity (0 disables capture).
- `ScrollbackDumpPolicy` ? disk dumps are **off by default**; `dump()` renders
  a length-bounded text dump only when explicitly permitted.

`ScreenModel` captures scrolled lines (primary screen only; the alternate
screen has no scrollback), exposes `scrollback()`, `scrollback_len()`,
`set_scrollback_config()`, `set_scrollback_dump_policy()`, `dump_scrollback()`,
and reports `scrollback_start` in the snapshot.

## Verification

```text
cargo test -p terminal-state --locked    PASS (50 tests)
cargo test -p terminal-parser --locked   PASS (14 unit + 2 golden)
cargo test -p core-protocol --locked     PASS (14 tests)
node .\scripts\test-input-protocol.mjs . PASS
node .\scripts\test-terminal-screen.mjs . PASS
node .\scripts\test-terminal-unicode.mjs . PASS
node .\scripts\test-color-golden.mjs .   PASS
node .\scripts\test-osc-policy.mjs .     PASS
node .\scripts\test-hyperlink-policy.mjs . PASS
```