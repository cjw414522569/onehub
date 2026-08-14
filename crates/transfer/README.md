# transfer

- Layer: L1 (core services)
- Dependencies: `core-domain`, `core-protocol`
- Scope: bounded file-transfer orchestration contracts.
- T016 status: buildable workspace skeleton; behavior is specified by later control rows.

## T057: streaming upload/download, concurrent chunks, backpressure

| Model | Purpose |
|---|---|
| `StreamConfig` | chunk_size, max_in_flight, yield_between_chunks. |
| `ChunkReader` / `ChunkWriter` | Injectable chunk source/sink (local file, SFTP read/write, generated stream). |
| `run_streaming_copy` | Bounded-memory chunked pipeline: concurrent in-flight chunks, backpressure via a bounded channel, cooperative yielding. |
| `TransferStats` | bytes/chunks transferred + peak buffered bytes (memory high-water mark); `progress` -> `core-domain` TransferProgress. |

Tests: exact small/multi-chunk round trips; 256 MiB sparse source stays within
O(chunk x in_flight) peak buffered bytes (10 GiB-class files are bounded by the
same formula); a slow writer backpressures the reader and the pipeline fills
(concurrent chunks); on a current-thread runtime an interactive task keeps
making progress during a large transfer (no starvation); invalid configs are
rejected.

## T058: resume, temp files, atomic replace, checksum

| Model | Purpose |
|---|---|
| `sha256_of` / `hex_digest` | SHA-256 checksum helpers. |
| `HashingWriter` | Hashes every chunk while delegating to an inner writer. |
| `AtomicWriteTarget` | Sibling temp file + atomic rename over the target; drop without commit removes the temp. |
| `ResumeRecord` | offset + partial SHA-256 of the verified prefix. |
| `run_atomic_transfer` | Full-file transfer with pre-commit checksum verification (target never exposed partial). |
| `run_resumable_transfer` | Resumes from a `.part` file (the resume state) and renames over the target only after full verification. |

Fault-injection test: a reader drops mid-transfer at 200 KB; the target is
untouched, the `.part` file persists holding exactly the verified prefix, and
a resumed run (source positioned past the prefix) completes with a full-file
hash match. Also covered: checksum mismatch discards the `.part` and keeps the
original target; commit cleans the temp; SHA-256 empty-string vector.
