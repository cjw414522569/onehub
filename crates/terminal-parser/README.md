# terminal-parser

- Layer: L2 (protocol adapters)
- Dependencies: `core-domain`, `core-protocol`, `terminal-state`
- Scope: bounded byte-stream to terminal-parser pipeline.

## T062: bounded byte-stream pipeline

| Model | Purpose |
|---|---|
| `BoundedByteStreamParser` | Fragmentation-safe UTF-8 / CSI / OSC parser with hard memory bounds. |
| `ParseEvent` | Text / CR / LF / BS / EraseDisplay / EraseLine / CursorPosition / CursorMove / Sgr / SetMode / Title. |
| `ParserDiagnostic` | Stable codes (`invalid_utf8`, `sequence_too_long`, `truncated_sequence`), no secrets. |
| `ParseBatch` | Monotonic feed sequence + events + diagnostics. |
| `TerminalParser` | `feed` / `finish` contract. |

Memory bounds: incomplete UTF-8 is held at most `MAX_UTF8_LEN` (4) bytes,
ESC/CSI/OSC buffers are capped at `max_sequence_len`, and the coalesced text
buffer is capped at `max_text_len`; malicious input cannot grow memory without
bound and the parser always recovers. The fragmentation property (feeding
byte-by-byte equals feeding the whole stream) is verified by property tests.