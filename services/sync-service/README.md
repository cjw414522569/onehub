# sync-service

- Layer: L6 (entrypoint)
- Dependencies: `sync-core`, `storage-sqlite`, `policy-engine`
- Scope: synchronization service entrypoint over public interfaces.
- T016 status: buildable workspace skeleton; service behavior is deferred to later control rows.

## T094: minimal trusted sync backend

`services/sync-service/src/lib.rs`:

- `SyncBackend` - ciphertext-only storage of `SyncEnvelope`s (never
  plaintext): `put` / `get` / `delete` / `list` / `usage`.
- Per-device storage **quota** (`quota_bytes`) and per-device **rate limit**
  (token bucket with configurable burst and refill).
- Explicit authorization: a device may only write its own envelopes (sender)
  and only read envelopes it sends or receives.
- Content-free **audit log**: `AuditRecord` keeps only device id, envelope id,
  action, byte length, and timestamp - never envelope contents.
- `ServiceError`: `Forbidden`, `UnsupportedVersion`, `QuotaExceeded`,
  `RateLimited` (with `retry_after_secs`).
- Backed by `storage-sqlite`'s `AtomicStore` (mailbox + per-device id index);
  the envelope storage format is versioned (1 + sender + recipient + nonce +
  ciphertext length + ciphertext).