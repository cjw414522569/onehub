//! Bounded-memory streaming transfer engine (T057): a chunked pipeline with
//! concurrent in-flight chunks, backpressure, and cooperative yielding so
//! interactive sessions are never starved by large file transfers.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use core_domain::transfer::{TransferError, TransferProgress};

/// Default chunk size (64 KiB).
pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;
/// Default number of in-flight chunks.
pub const DEFAULT_MAX_IN_FLIGHT: usize = 8;

/// Configuration for the streaming engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamConfig {
    /// Chunk size in bytes.
    pub chunk_size: usize,
    /// Max chunks buffered in flight (bounds memory).
    pub max_in_flight: usize,
    /// Yield to the runtime between chunks so interactive sessions progress.
    pub yield_between_chunks: bool,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            max_in_flight: DEFAULT_MAX_IN_FLIGHT,
            yield_between_chunks: true,
        }
    }
}

/// Final statistics of a streaming transfer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TransferStats {
    /// Bytes transferred.
    pub bytes_transferred: u64,
    /// Chunks written.
    pub chunks_transferred: u64,
    /// Peak bytes buffered in flight (memory high-water mark).
    pub peak_buffered_bytes: usize,
}

impl TransferStats {
    /// Builds a progress snapshot for the UI.
    pub fn progress(&self, total_bytes: Option<u64>) -> TransferProgress {
        TransferProgress {
            bytes_transferred: self.bytes_transferred,
            total_bytes,
            bytes_per_second: 0,
        }
    }
}

/// A chunked source (local file reader, SFTP read, or a generated stream).
pub trait ChunkReader: Send {
    /// Reads up to `buffer.len()` bytes; returns `Ok(0)` at EOF.
    fn read_chunk<'a>(
        &'a mut self,
        buffer: &'a mut [u8],
    ) -> Pin<Box<dyn Future<Output = Result<usize, TransferError>> + Send + 'a>>;
}

/// A chunked sink (local file writer, SFTP write, or a counting sink).
pub trait ChunkWriter: Send + 'static {
    /// Writes one chunk (owned so wrappers can move it into async blocks).
    fn write_chunk<'a>(
        &'a mut self,
        data: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), TransferError>> + Send + 'a>>;
}

/// Runs a bounded-memory chunked copy from `reader` to `writer`.
///
/// Memory is bounded by `max_in_flight × chunk_size` plus one in-reader and
/// one in-writer buffer, independent of the total file size. A slow sink
/// backpressures the reader through the bounded channel; `yield_between_chunks`
/// lets interactive sessions run between chunk sends.
pub async fn run_streaming_copy<R, W>(
    reader: &mut R,
    writer: W,
    config: &StreamConfig,
) -> Result<TransferStats, TransferError>
where
    R: ChunkReader,
    W: ChunkWriter,
{
    if config.chunk_size == 0 || config.max_in_flight == 0 {
        return Err(TransferError::Io);
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(config.max_in_flight);
    let written_counter = Arc::new(AtomicU64::new(0));
    let counter = written_counter.clone();
    let mut writer = writer;
    let writer_task = tokio::spawn(async move {
        let mut chunks = 0u64;
        while let Some(chunk) = rx.recv().await {
            let chunk_len = chunk.len() as u64;
            writer.write_chunk(chunk).await?;
            counter.fetch_add(chunk_len, Ordering::SeqCst);
            chunks += 1;
        }
        Ok::<u64, TransferError>(chunks)
    });

    let mut sent = 0u64;
    let mut stats = TransferStats::default();
    loop {
        let mut buffer = vec![0u8; config.chunk_size];
        let read = match reader.read_chunk(&mut buffer).await {
            Ok(n) => n,
            Err(error) => {
                // Always join the writer so resources (e.g. a temp file) are
                // released deterministically before returning.
                drop(tx);
                let _ = writer_task.await;
                return Err(error);
            }
        };
        if read == 0 {
            break;
        }
        buffer.truncate(read);
        sent += read as u64;
        if tx.send(buffer).await.is_err() {
            drop(tx);
            let _ = writer_task.await;
            return Err(TransferError::Cancelled);
        }
        let in_flight = sent - written_counter.load(Ordering::SeqCst);
        stats.peak_buffered_bytes = stats.peak_buffered_bytes.max(in_flight as usize);
        if config.yield_between_chunks {
            tokio::task::yield_now().await;
        }
    }
    drop(tx);
    let chunks = writer_task.await.map_err(|_| TransferError::Io)??;
    stats.bytes_transferred = sent;
    stats.chunks_transferred = chunks;
    Ok(stats)
}
#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use core_domain::transfer::TransferError;

    use super::{run_streaming_copy, ChunkReader, ChunkWriter, StreamConfig};

    /// A reader over an in-memory buffer.
    struct BufReader {
        data: Vec<u8>,
        offset: usize,
    }

    impl BufReader {
        fn new(data: Vec<u8>) -> Self {
            Self { data, offset: 0 }
        }
    }

    impl ChunkReader for BufReader {
        fn read_chunk<'a>(
            &'a mut self,
            buffer: &'a mut [u8],
        ) -> Pin<Box<dyn Future<Output = Result<usize, TransferError>> + Send + 'a>> {
            Box::pin(async move {
                let remaining = self.data.len() - self.offset;
                let take = remaining.min(buffer.len());
                buffer[..take].copy_from_slice(&self.data[self.offset..self.offset + take]);
                self.offset += take;
                Ok(take)
            })
        }
    }

    /// A sparse source that generates zero-filled chunks without
    /// materialising the whole file (the 10 GiB-class benchmark).
    struct ZeroSource {
        remaining: u64,
    }

    impl ZeroSource {
        fn new(size: u64) -> Self {
            Self { remaining: size }
        }
    }

    impl ChunkReader for ZeroSource {
        fn read_chunk<'a>(
            &'a mut self,
            buffer: &'a mut [u8],
        ) -> Pin<Box<dyn Future<Output = Result<usize, TransferError>> + Send + 'a>> {
            Box::pin(async move {
                if self.remaining == 0 {
                    return Ok(0);
                }
                let take = (self.remaining as usize).min(buffer.len());
                buffer[..take].fill(0);
                self.remaining -= take as u64;
                Ok(take)
            })
        }
    }

    /// A sink that collects bytes behind a shared mutex (owned, Clone, so it
    /// can move into the writer task).
    #[derive(Clone, Default)]
    struct SharedSink {
        data: Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl ChunkWriter for SharedSink {
        fn write_chunk<'a>(
            &'a mut self,
            data: Vec<u8>,
        ) -> Pin<Box<dyn Future<Output = Result<(), TransferError>> + Send + 'a>> {
            Box::pin(async move {
                self.data
                    .lock()
                    .expect("sink lock")
                    .extend_from_slice(&data);
                Ok(())
            })
        }
    }

    /// A counting sink with an optional per-chunk delay and an event counter
    /// (used to observe writer progress and backpressure).
    #[derive(Clone, Default)]
    struct CountingSink {
        bytes: Arc<AtomicUsize>,
        events: Arc<AtomicUsize>,
        delay: Option<Duration>,
    }

    impl ChunkWriter for CountingSink {
        fn write_chunk<'a>(
            &'a mut self,
            data: Vec<u8>,
        ) -> Pin<Box<dyn Future<Output = Result<(), TransferError>> + Send + 'a>> {
            Box::pin(async move {
                if let Some(delay) = self.delay {
                    tokio::time::sleep(delay).await;
                }
                self.bytes.fetch_add(data.len(), Ordering::SeqCst);
                self.events.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn round_trip_small_file() {
        let mut reader = BufReader::new(b"hello world".to_vec());
        let sink = SharedSink::default();
        let stats = run_streaming_copy(&mut reader, sink.clone(), &StreamConfig::default())
            .await
            .expect("copy");
        assert_eq!(*sink.data.lock().expect("lock"), b"hello world");
        assert_eq!(stats.bytes_transferred, 11);
        assert_eq!(stats.chunks_transferred, 1);
    }

    #[tokio::test]
    async fn round_trip_multi_chunk() {
        let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let mut reader = BufReader::new(payload.clone());
        let sink = SharedSink::default();
        let stats = run_streaming_copy(&mut reader, sink.clone(), &StreamConfig::default())
            .await
            .expect("copy");
        assert_eq!(*sink.data.lock().expect("lock"), payload);
        assert_eq!(stats.bytes_transferred, 300_000);
        assert_eq!(stats.chunks_transferred, 5); // ceil(300000 / 65536)
    }

    #[tokio::test]
    async fn large_file_memory_is_bounded() {
        // 256 MiB sparse source; memory must stay O(chunk x in_flight),
        // independent of the total file size (10 GiB-class files are bounded
        // by the same O(chunk x in_flight) formula).
        let size = 256 * 1024 * 1024u64;
        let config = StreamConfig {
            chunk_size: 64 * 1024,
            max_in_flight: 4,
            ..StreamConfig::default()
        };
        let mut reader = ZeroSource::new(size);
        let sink = CountingSink::default();
        let stats = run_streaming_copy(&mut reader, sink.clone(), &config)
            .await
            .expect("copy");
        assert_eq!(stats.bytes_transferred, size);
        assert_eq!(stats.chunks_transferred, size / config.chunk_size as u64);
        // At most (max_in_flight + 1) chunks buffered at any time.
        let bound = (config.max_in_flight + 1) * config.chunk_size;
        assert!(
            stats.peak_buffered_bytes <= bound,
            "peak buffered {} exceeds bound {}",
            stats.peak_buffered_bytes,
            bound
        );
        // Sink received everything.
        assert_eq!(sink.bytes.load(Ordering::SeqCst) as u64, size);
    }

    #[tokio::test]
    async fn slow_writer_backpressure_pipelines_chunks() {
        let config = StreamConfig {
            chunk_size: 64 * 1024,
            max_in_flight: 8,
            ..StreamConfig::default()
        };
        let mut reader = ZeroSource::new(32 * 1024 * 1024); // 32 MiB
        let sink = CountingSink {
            delay: Some(Duration::from_micros(500)),
            ..CountingSink::default()
        };
        let stats = run_streaming_copy(&mut reader, sink.clone(), &config)
            .await
            .expect("copy");
        // The fast reader ran ahead of the slow writer: at least two chunks
        // were buffered concurrently (proves pipelining / concurrent chunks).
        assert!(
            stats.peak_buffered_bytes >= 2 * config.chunk_size,
            "no pipelining observed: peak {}",
            stats.peak_buffered_bytes
        );
        assert_eq!(stats.bytes_transferred, 32 * 1024 * 1024);
        assert_eq!(sink.events.load(Ordering::SeqCst) as u64, 512);
    }

    #[tokio::test]
    async fn interactive_session_is_not_starved() {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
        // On the current-thread runtime, the transfer yields between chunks so
        // an interactive task keeps making progress.
        let done = Arc::new(AtomicBool::new(false));
        let done_flag = done.clone();
        let interactive_ticks = Arc::new(AtomicU64::new(0));
        let ticks = interactive_ticks.clone();
        let interactive = tokio::spawn(async move {
            let mut count = 0u64;
            while !done_flag.load(AtomicOrdering::SeqCst) {
                tokio::task::yield_now().await;
                count += 1;
            }
            ticks.store(count, AtomicOrdering::SeqCst);
        });

        let config = StreamConfig {
            chunk_size: 64 * 1024,
            max_in_flight: 4,
            yield_between_chunks: true,
        };
        let mut reader = ZeroSource::new(32 * 1024 * 1024); // 32 MiB
        let stats = run_streaming_copy(&mut reader, CountingSink::default(), &config)
            .await
            .expect("copy");
        assert_eq!(stats.bytes_transferred, 32 * 1024 * 1024);

        done.store(true, AtomicOrdering::SeqCst);
        interactive.await.expect("interactive joined");
        let ticks = interactive_ticks.load(AtomicOrdering::SeqCst);
        assert!(
            ticks > 10,
            "interactive session made no progress during the transfer: {ticks} ticks"
        );
    }

    #[tokio::test]
    async fn invalid_config_is_rejected() {
        let mut reader = ZeroSource::new(1024);
        let result = run_streaming_copy(
            &mut reader,
            CountingSink::default(),
            &StreamConfig {
                chunk_size: 0,
                ..StreamConfig::default()
            },
        )
        .await;
        assert_eq!(result, Err(TransferError::Io));
    }
}
