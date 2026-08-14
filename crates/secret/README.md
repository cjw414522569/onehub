# secret

- Layer: L0 (pure types)
- Dependencies: `zeroize` (safe API only; this crate is `forbid(unsafe_code)`).
- Scope: sensitive value types that auto-zero on drop and cannot be accidentally formatted, cloned, or serialized.

## T025: SecretBytes / SecretString

| Type | Purpose |
|---|---|
| `SecretBytes` | Sensitive byte buffer (Box<[u8]>, no spare capacity). |
| `SecretString` | Sensitive UTF-8 string backed by `SecretBytes`. |

Guarantees:

- `Drop` zeroizes the buffer with `zeroize` (volatile) plus `std::hint::black_box`.
- `Debug`, `Display`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, and `Serialize` are deliberately NOT implemented; `compile_fail` doctests prove attempts to use them do not build.
- Access is explicit through `expose_secret()`; there is no accidental formatting path.
- The crate forbids `unsafe_code`; `zeroize` is used through its safe API.

## Validation

```text
cargo test -p secret --locked
cargo check -p secret --locked
node scripts/test-secret.mjs .
```