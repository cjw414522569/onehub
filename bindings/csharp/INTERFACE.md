# C# binding interface

This directory reserves the L4 binding boundary for Windows/.NET clients.

- Input and output are versioned batches, never per-character callbacks.
- Every message carries `schema_version`, `message_type`, `byte_len`, `request_id`, cancellation state, backpressure state, and `error_code`.
- Ownership is creator-owned and release is idempotent.
- No Rust references, raw pointers, exceptions, closures, or secret debug values cross the boundary.
- Native compilation is intentionally deferred until a .NET toolchain is available.

