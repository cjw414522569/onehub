# core-errors

- Layer: L0 (pure types)
- Dependencies: `core-domain`, `serde` (derive); no UI, database, or SSH implementation types.
- Scope: stable, language-neutral error identifiers and metadata boundary.

## T022 model

| Model | Purpose |
|---|---|
| `ErrorCode` | Frozen stable string codes (`E_*`); never renumbered. |
| `Recoverability` | Whether the caller can recover. |
| `RetrySuggestion` | Language-neutral retry policy (`None`, `Once`, `WithBackoff`). |
| `MessageParam` | Typed, non-sensitive localization parameter. |
| `ErrorInfo` | FFI-safe error metadata: code, recoverability, retry, message key, params. |
| `From<DomainError>` | Compile-time exhaustive mapping of every `core-domain` error. |

Error metadata never carries language exceptions, panic text, credentials, keys, tokens, or terminal text. The exhaustive mapping test (`mapping::tests::mapping_is_exhaustive_over_all_domain_variants`) iterates every `DomainError` variant and verifies stable codes, message keys, and non-sensitive serialization.

## Validation

```text
cargo test -p core-errors --locked
cargo check -p core-errors --locked
node scripts/test-core-errors.mjs .
```