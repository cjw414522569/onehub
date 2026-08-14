# terminal-parser

- Layer: L2 (protocol adapters)
- Dependencies: `core-domain`, `core-protocol`, `terminal-state`
- Scope: bounded byte-stream to terminal-parser pipeline.

## T062: bounded byte-stream pipeline

| Model | Purpose |
|---|---|
| `BoundedByteStreamParser` | Fragmentation-safe UTF-8 / CSI / OSC parser with hard memory bounds. |
| `ParseEvent` | Text / CR / LF / BS / EraseDisplay / EraseLine / CursorPosition / CursorMove / Sgr / SetMode / SetScrollRegion / Title / WorkingDirectory / Notification. |
| `ParserDiagnostic` | Stable codes (`invalid_utf8`, `sequence_too_long`, `truncated_sequence`), no secrets. |
| `ParseBatch` | Monotonic feed sequence + events + diagnostics. |
| `TerminalParser` | `feed` / `finish` contract. |

Memory bounds: incomplete UTF-8 is held at most `MAX_UTF8_LEN` (4) bytes,
ESC/CSI/OSC buffers are capped at `max_sequence_len`, and the coalesced text
buffer is capped at `max_text_len`; malicious input cannot grow memory without
bound and the parser always recovers. The fragmentation property (feeding
byte-by-byte equals feeding the whole stream) is verified by property tests.

## T063: shared vocabulary and golden test

The event vocabulary is owned by `terminal-state` (L1) and re-exported here.
`tests/golden.rs` diffs a deterministic vttest-basic script (SGR, erase,
cursor, scroll region, origin mode, alternate screen, title, working
directory, plus CJK / combining / emoji from T064) against
`tests/golden/vttest-basic.json`.

## T065: CSI sub-parameters and color-matrix golden

`parse_csi` parses `:`-separated sub-parameters; `Sgr` carries
`Vec<Vec<u16>>` (e.g. `4:2` = `[[4, 2]]` double underline).
`tests/golden/color-matrix.json` is a deterministic color golden covering
16/256/truecolor, inverse/dim, underline styles, and reset.

## T066: OSC title, working directory, notifications

`parse_osc` maps OSC 0/2 to `Title`, OSC 7 to `WorkingDirectory`, and OSC 9 /
OSC 777;notify to `Notification`; unknown codes are ignored. OSC payloads are
terminated by BEL or ST (`ESC \`); the parser tracks a pending-OSC ST flag so
ST-terminated sequences finalize correctly. The screen model applies the
`OscPolicy` security filter (gating, control-character stripping, length caps)
before storing anything.