# abi-c

- Layer: L4 (ABI/platform bridge)
- Dependencies: `core-domain`, `core-errors`, `core-protocol`, `session-orchestrator`
- Scope: versioned batch ABI with explicit ownership, cancellation, backpressure, and error fields.
- T016 status: buildable workspace skeleton; ABI implementation is deferred to its control row.



## T097: stable, versioned C ABI

`crates/abi-c/src/lib.rs` + `crates/abi-c/include/ssh_abi.h`:

- `AbiMessageHeader` - `#[repr(C)]` versioned header: `schema_version`,
  `message_type`, `byte_len`, `request_id`, `cancel`, `backpressure`,
  `error_code` (size 32; offsets 0/4/8/16/24/25/28).
- Exported `extern "C"` functions (`lib` + `cdylib`):
  `ssh_abi_version`, `ssh_abi_codegen_version`, `ssh_abi_header_size`,
  `ssh_abi_field_offset`, `ssh_abi_header_is_valid`,
  `ssh_abi_validate_field_offsets`.
- `crates/abi-c/include/ssh_abi.h` is **generated** by
  `scripts/generate-abi.mjs` and checked in; codegen version is pinned
  (`ABI_CODEGEN_VERSION = "1.0.0"`, `SSH_ABI_CODEGEN_VERSION`).
- Reproducible generation + drift detection: `scripts/test-abi.mjs`
  regenerates and requires a byte-identical header; Rust tests parse the
  header and assert the layout matches the exported ABI (the host-side
  equivalent of a C link test - no C toolchain required on Windows).

Note: this crate intentionally does not `forbid(unsafe_code)` because the
stable ABI needs `#[no_mangle]` export attributes (an unsafe attribute); the
crate contains no unsafe blocks.