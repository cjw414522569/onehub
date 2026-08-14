# core-protocol

- Layer: L0 (pure types)
- Dependencies: `core-domain`, `core-errors`, `serde` (derive); no UI, database, or SSH implementation types.
- Scope: versioned protocol domain model aligned with `protocol/schema/domain-v1.json` and `protocol/terminal/terminal-contract-v1.json`.

## T026: capability negotiation

`Capability`, `CapabilitySet`, `NegotiationResult`, `negotiate` (intersection-with-explicit-rejection), `PlatformId` / `PlatformProfile`, `negotiate_with_platform`.

## T033: terminal snapshot/delta protocol

| Model | Purpose |
|---|---|
| `TerminalColor` / `UnderlineStyle` / `TerminalStyle` | Cell colors and attributes. |
| `Hyperlink` | OSC 8 hyperlink (id + url). |
| `ImagePlaceholder` | Out-of-band image reference + cell footprint (records stay bounded). |
| `TerminalCell` / `TerminalRow` | Cell and row records. |
| `TerminalSnapshot` | Full snapshot (rows, cursor, title, cwd, scrollback origin, extensions). |
| `TerminalDelta` / `DeltaOp` | Incremental change; op kinds match the terminal contract's `render_op_kinds` (fill/copy/clear/cursor/title) plus image. |
| `TerminalBatch` | Length-delimited batch of delta/full messages. |
| `Extension` | Tagged extension; unknown names parse and are safely ignored. |
| `TerminalProtocolVersion` | Same-major compatibility for forward/backward reading. |

`snapshot_golden_serialization_is_stable` pins the canonical JSON; `backward_compatible_old_record_parses_with_defaults` and `forward_compatible_unknown_extension_is_ignored` prove optional-field backward compatibility and safe unknown-extension handling.

## Validation

```text
cargo test -p core-protocol --locked
cargo check -p core-protocol --locked
node scripts/test-capabilities.mjs .
node scripts/test-terminal-protocol.mjs .
```