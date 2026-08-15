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

## T098: opaque handle lifecycle and cross-ABI ownership

`crates/abi-c/src/handle.rs`:

- `HandleTable<T>` - opaque `u64` handles (never `0`); `insert` / `get` /
  `get_mut` / `remove` / `contains` / `len`.
- Idempotent release: a double release (or a managed runtime's GC/ARC
  finalizer racing an explicit release) is a safe no-op, never a
  use-after-free; stale handles are rejected, never dereferenced.
- Dropping the table (process exit) drops every remaining resource, so
  cancellation and exit leak nothing (verified with a drop counter).
- Exported ABI: `ssh_abi_handle_create`, `ssh_abi_handle_release`,
  `ssh_abi_handle_is_valid`, `ssh_abi_handle_count`, `ssh_abi_handle_cancel`,
  `ssh_abi_handle_is_cancelled`.
- Tests: idempotent release, drop-on-exit, cancellation, 10k create/release
  stress with zero residual handles, stale-handle rejection, opaque unique
  ids. Sanitizer runs are blocked (no MSan/ASan toolchain on this host);
  leak/stress tests run in-crate.

## T099: batch event streams, backpressure, and UI scheduler adapters

`crates/abi-c/src/event_stream.rs` + `crates/abi-c/src/scheduler.rs`:

- `EventStream` - events accumulate into versioned `EventBatch`es (the ABI
  transfer unit); never one character at a time. Flushes at size/count
  thresholds. Never blocks the producer: a full queue drops the oldest batch
  (bounded memory), counts `dropped`, and the next batch carries
  `BatchItem::SnapshotRequired`; the consumer requests `produce_snapshot` and
  rebuilds the full state, so a stalled UI recovers from a snapshot.
- `Scheduler` - non-blocking UI dispatch contract; `UiScheduler`
  (deterministic memory-backed) and `WindowsUiScheduler` (Windows-first
  adapter; real Win32 message-loop posting is blocked_environment without a
  native loop). Other platforms stay interface-only.
- Tests: batching (1000 events -> 16 batches), backpressure drops +
  snapshot request, slow-UI snapshot recovery converges to the latest state,
  10k-event flood with a stalled consumer (producer never blocks, bounded
  queue, snapshot recovery), threshold flushing, scheduler dispatch/poll and
  backpressure.