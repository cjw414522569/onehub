# terminal-parser

- Layer: L2 (protocol adapters)
- Dependencies: `core-domain`, `core-protocol`, `terminal-state`
- Scope: bounded byte-stream to terminal-parser pipeline.

## T062: bounded byte-stream pipeline

| Model | Purpose |
|---|---|
| `BoundedByteStreamParser` | Fragmentation-safe UTF-8 / CSI / OSC parser with hard memory bounds. |
| `ParseEvent` | Text / CR / LF / BS / EraseDisplay / EraseLine / CursorPosition / CursorMove / Sgr / SetMode / SetScrollRegion / Title / WorkingDirectory / Notification / Hyperlink. |
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
directory, CJK / combining / emoji, and an OSC 8 hyperlink) against
`tests/golden/vttest-basic.json`.

## T065: CSI sub-parameters and color-matrix golden

`parse_csi` parses `:`-separated sub-parameters; `Sgr` carries
`Vec<Vec<u16>>` (e.g. `4:2` = `[[4, 2]]` double underline).
`tests/golden/color-matrix.json` is a deterministic color golden.

## T082: recorded-replay compatibility regression set

`tests/compat_replay.rs` + `tests/compat-corpus.json` replay recorded vim /
tmux / screen / htop / fzf / lazygit interaction scripts through the parser
into `ScreenModel` and assert the expected markers (alternate screen, status
lines, box-drawing borders, highlights) with clean diagnostics. The full
interactive vttest / esctest suites and live TUI sessions require a real
PTY/terminal and are `blocked_environment` on CI hosts without one.

## T066/T067: OSC handling

`parse_osc` maps OSC 0/2 -> Title, OSC 7 -> WorkingDirectory, OSC 8 ->
Hyperlink (`id` + uri; empty uri ends the link), OSC 9 / OSC 777;notify ->
Notification; unknown codes are ignored. OSC payloads are terminated by BEL
or ST (`ESC \`). Hyperlinks and notifications are policy-gated in the screen
model (`HyperlinkPolicy` / `OscPolicy`).