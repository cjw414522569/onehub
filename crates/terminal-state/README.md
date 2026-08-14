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

## T070: selection, copy, word/line/rectangle selection

`crates/terminal-state/src/selection.rs`:

- `Selection` ? anchor + focus `(row, col)` over a terminal snapshot (the
  active buffer, so alternate-screen text selects correctly); normalized so
  start <= end.
- `SelectionMode` ? `Character` (exact range), `Word` (expand to word
  boundaries via `word_bounds`), `Line` (whole lines, whitespace-trimmed),
  `Rectangle` (column block).
- `cell_selection_text` ? copies cell text, skips wide-continuation cells,
  and emits spaces for empty cells so grid gaps are preserved; rows join with
  newlines.

## T071: search, regex search, and result navigation

`crates/terminal-state/src/search.rs`:

- `SearchQuery` ? literal or regex pattern with case sensitivity.
- `SearchSession` ? searches a `SearchBuffer` (scrollback + visible screen)
  in bounded chunks, checking an `AtomicBool` cancellation token between
  lines; `step()` is incremental so a worker thread can drive a million-line
  search without blocking input, and `cancel()` stops it early.
- `SearchNavigation` ? cycles through results (`next_result` / `prev_result`
  with wraparound).
- Results carry absolute line index + char column + length. Regex support uses
  the `regex` crate (`RegexBuilder`, case-insensitive option).

## T072: terminal reflow preserving semantic selection

`ScreenBuffer` tracks per-row soft-wrap flags (`wrapped`). `ScreenModel::resize`
reflows each buffer: soft-wrapped rows are unwrapped into logical lines and
re-wrapped at the new width, preserving cell styles, hyperlinks, and wide-cell
continuation markers. The cursor is remapped through the layout change
(pending-wrap aware), and `remap_point(ReflowInfo)` maps old `(row, col)`
points to the new layout so semantic selections survive a resize.

Random-resize property tests verify logical content is preserved, the cursor
stays in bounds, and selection characters survive reflow.

## T073: incremental merge and dirty-line tracking

`crates/terminal-state/src/delta.rs`:

- `DirtyTracker` ? accumulates dirty rows plus cursor / title / working-directory
  flags between frames; `clear()` resets after each flush.
- `DeltaBuilder` ? batches all dirty state into one core-protocol
  `TerminalDelta` per frame (row `Fill` runs grouped by style, `Cursor`,
  `Title`), for the FFI bridge.
- `apply_delta` ? merges a delta into a receiver snapshot
  (Fill/Copy/Clear/Cursor/Title/Image; grapheme-safe with wide-continuation
  marking). `SequenceGap` / `MissingSnapshot` errors signal dropped frames so
  the receiver can recover with a full snapshot.
- `diff_rows(old, new)` ? marks changed rows/state for the receiver.

Incremental/full equivalence and dropped-frame recovery are verified by
property tests.

## T074: font discovery, fallback, bold/italic, variable-font policy

`crates/terminal-state/src/font.rs`:

- `Script` classification (`script_for_char`) ? Latin / CJK / Emoji /
  Powerline / Other.
- `FallbackPolicy` ? maps each script to a font family (Windows-first
  defaults: Cascadia Mono, Microsoft YaHei UI, Segoe UI Emoji, a Nerd Font for
  Powerline) and maps `FontStyle` to variable-font weight / italic axes
  (normal 400 / bold 700).
- `resolve(ch, style, size_pt) -> FontSpec` ? picks the family by script and
  the axes by style, so CJK / emoji / Powerline missing glyphs fall back
  predictably. The cross-platform screenshot coverage matrix requires a real
  renderer/GPU and is `blocked_environment` on CI hosts without one.

## T075: glyph shaping, ligature toggle, cell alignment

`crates/terminal-state/src/shape.rs`:

- `LigaturePolicy` ? enable/disable ligature merging.
- `shape_run(text, policy, ligatures)` ? segments into grapheme clusters and
  produces `ShapedGlyph` entries with grid-cell footprints; known programming
  ligature sequences (`->`, `=>`, `!=`, ...) merge into one glyph that spans
  the same cells.
- `cells_align` ? verifies the grid invariant: total glyph cells equal the
  text's display width and no glyph spans fewer cells than its width.
- `grid_fit(advance, cell)` ? ceil-maps a shaped advance to whole cells so a
  glyph never overflows its footprint.

HarfBuzz screenshot validation (programming ligatures / CJK / RTL) requires a
real renderer and is `blocked_environment` on CI hosts without one; the grid
semantics are verified deterministically by unit tests.

## T076: glyph atlas, cache eviction, DPI bucketing

`crates/terminal-state/src/atlas.rs`:

- `GlyphAtlas` ? bounded glyph cache (`GlyphKey` -> `AtlasEntry`) with true
  LRU eviction; `AtlasLimits { max_entries, max_bytes }` keeps cache memory
  under an explicit budget. `clear()` invalidates everything (device loss).
- `AtlasSet` ? one atlas per DPI bucket (`dpi_bucket(scale)`), so zoom / DPI
  hot-switching never serves a wrong texture (each bucket has its own atlas),
  and `clear_all()` handles device loss.

DPI hot-switch and device-loss behavior are covered by unit tests; real GPU
texture upload is the renderer's job.

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