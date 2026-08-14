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
