# terminal-parser

- Layer: L2 (protocol adapters)
- Dependencies: `core-domain`, `core-protocol`, `terminal-state`
- Scope: bounded byte-stream to terminal-parser pipeline.

## T062: bounded byte-stream pipeline

| Model | Purpose |
|---|---|
| `BoundedByteStreamParser` | Fragmentation-safe UTF-8 / CSI / OSC parser with hard memory bounds. |
| `ParseEvent` | Text / CR / LF / BS / EraseDisplay / EraseLine / CursorPosition / CursorMove / Sgr / SetMode / SetScrollRegion / Title. |
| `ParserDiagnostic` | Stable codes (`invalid_utf8`, `sequence_too_long`, `truncated_sequence`), no secrets. |
| `ParseBatch` | Monotonic feed sequence + events + diagnostics. |
| `TerminalParser` | `feed` / `finish` contract. |

Memory bounds: incomplete UTF-8 is held at most `MAX_UTF8_LEN` (4) bytes,
ESC/CSI/OSC buffers are capped at `max_sequence_len`, and the coalesced text
buffer is capped at `max_text_len`; malicious input cannot grow memory without
bound and the parser always recovers. The fragmentation property (feeding
byte-by-byte equals feeding the whole stream) is verified by property tests.

## T063: shared vocabulary and golden test

The event vocabulary (`ParseEvent`, `ParseBatch`, `ParserDiagnostic`,
`TerminalParser`) is owned by `terminal-state` (L1) and re-exported here. The
vocabulary includes `SetScrollRegion` (`CSI Ps ; Ps r`).
`tests/golden.rs` diffs a deterministic vttest-basic script (SGR, erase,
cursor, scroll region, origin mode, alternate screen, title, plus CJK /
combining / emoji from T064) against `tests/golden/vttest-basic.json`.

## T065: CSI sub-parameters and color-matrix golden

`parse_csi` now parses `:`-separated sub-parameters: `Sgr` carries
`Vec<Vec<u16>>` where each parameter keeps its sub-parameters (e.g. `4:2` is
`[[4, 2]]`). This enables `4:N` underline styles without ambiguity against
background colors.

`tests/golden/color-matrix.json` (66,615 bytes) is a deterministic color
golden covering 16-color fg/bg, 256-color, truecolor, inverse/dim
combinations, underline styles (single/double/curly), and reset. Image-based
cross-platform color golden tests require a real renderer/GPU and are
`blocked_environment` on CI hosts without one.