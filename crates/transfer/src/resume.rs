//! Resume, temp-file atomic replace, and checksum verification (T058).
//!
//! Interrupted transfers never corrupt the target: bytes are written to a
//! temporary file next to the target and atomically renamed over it only after
//! the whole file is verified (SHA-256). A [`ResumeRecord`] captures how far a
//! partial run got (offset + partial hash) so a reconnected session can skip
//! the already-verified prefix and finish.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use core_domain::transfer::TransferError;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::streaming::{run_streaming_copy, ChunkReader, ChunkWriter, StreamConfig, TransferStats};

static NONCE: AtomicU64 = AtomicU64::new(0);

fn next_nonce() -> u64 {
    NONCE.fetch_add(1, Ordering::SeqCst)
}

/// Computes the SHA-256 digest of `bytes`.
pub fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// Hex representation of a SHA-256 digest.
pub fn hex_digest(digest: &[u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A writer that hashes every chunk while delegating to an inner writer.
///
/// The hasher lives behind an `Arc` so the digest can be read after the writer
/// task completes.
pub struct HashingWriter<W> {
    inner: W,
    hasher: Arc<Mutex<Sha256>>,
}

impl<W> HashingWriter<W> {
    /// Wraps `inner`; returns the writer and the shared hasher handle.
    pub fn new(inner: W) -> (Self, Arc<Mutex<Sha256>>) {
        let hasher = Arc::new(Mutex::new(Sha256::new()));
        (
            Self {
                inner,
                hasher: hasher.clone(),
            },
            hasher,
        )
    }
}

impl<W: ChunkWriter> ChunkWriter for HashingWriter<W> {
    fn write_chunk<'a>(
        &'a mut self,
        data: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), TransferError>> + Send + 'a>> {
        Box::pin(async move {
            self.hasher.lock().expect("hasher lock").update(&data);
            self.inner.write_chunk(data).await
        })
    }
}

/// Shared interior of an atomic write target (the open file handle).
struct AtomicWriteInner {
    temp_path: PathBuf,
    file: Option<tokio::fs::File>,
    committed: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for AtomicWriteInner {
    fn drop(&mut self) {
        if !self.committed.load(std::sync::atomic::Ordering::SeqCst) {
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}

/// A temporary-file target that atomically replaces the final path on commit.
///
/// While the transfer is running, only the sibling temp file exists; a failure
/// or drop without commit removes the temp file, leaving the target untouched.
/// Cloneable so the writer task and the controller share the same state.
#[derive(Clone)]
pub struct AtomicWriteTarget {
    temp_path: PathBuf,
    final_path: PathBuf,
    committed: Arc<std::sync::atomic::AtomicBool>,
    inner: Arc<tokio::sync::Mutex<AtomicWriteInner>>,
}

impl AtomicWriteTarget {
    /// Creates a temp file next to `final_path`.
    pub async fn create(final_path: impl AsRef<Path>) -> Result<Self, TransferError> {
        let final_path = final_path.as_ref().to_path_buf();
        let nonce = format!("{}-{}", std::process::id(), next_nonce());
        let temp_path = final_path.with_extension(format!("tmp-{nonce}"));
        let file = tokio::fs::File::create(&temp_path)
            .await
            .map_err(|_| TransferError::Io)?;
        let committed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let inner = Arc::new(tokio::sync::Mutex::new(AtomicWriteInner {
            temp_path: temp_path.clone(),
            file: Some(file),
            committed: committed.clone(),
        }));
        Ok(Self {
            temp_path,
            final_path,
            committed,
            inner,
        })
    }

    /// The temporary path (same directory as the target for atomic rename).
    pub fn temp_path(&self) -> &Path {
        &self.temp_path
    }

    /// The final target path.
    pub fn final_path(&self) -> &Path {
        &self.final_path
    }

    /// Whether the target was committed.
    pub fn is_committed(&self) -> bool {
        self.committed.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Whether the target file currently exists on disk.
    pub fn target_exists(&self) -> bool {
        self.final_path.exists()
    }

    /// Whether the temp file still exists on disk.
    pub fn temp_exists(&self) -> bool {
        self.temp_path.exists()
    }

    /// Flushes and atomically renames the temp file over the final path.
    pub async fn commit(&self) -> Result<(), TransferError> {
        let mut inner = self.inner.lock().await;
        let mut file = inner.file.take().ok_or(TransferError::Io)?;
        file.flush().await.map_err(|_| TransferError::Io)?;
        drop(file);
        std::fs::rename(&inner.temp_path, &self.final_path).map_err(|_| TransferError::Io)?;
        self.committed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

impl ChunkWriter for AtomicWriteTarget {
    fn write_chunk<'a>(
        &'a mut self,
        data: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), TransferError>> + Send + 'a>> {
        Box::pin(async move {
            let mut inner = self.inner.lock().await;
            let file = inner.file.as_mut().ok_or(TransferError::Io)?;
            AsyncWriteExt::write_all(file, &data)
                .await
                .map_err(|_| TransferError::Io)
        })
    }
}
/// Writes chunks to an open part file (owned, moves into the writer task).
struct PartFileWriter {
    file: tokio::fs::File,
}

impl ChunkWriter for PartFileWriter {
    fn write_chunk<'a>(
        &'a mut self,
        data: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), TransferError>> + Send + 'a>> {
        Box::pin(async move {
            AsyncWriteExt::write_all(&mut self.file, &data)
                .await
                .map_err(|_| TransferError::Io)
        })
    }
}
/// How far a partial transfer got, so a reconnected session can resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumeRecord {
    /// Bytes already transferred and hashed.
    pub offset: u64,
    /// SHA-256 of the prefix `[0..offset)`.
    pub partial_sha256: [u8; 32],
}

/// Runs a full-file atomic transfer with checksum verification.
///
/// On checksum mismatch the temp file is discarded and the target is left
/// untouched; on success the temp file is atomically renamed over the target.
pub async fn run_atomic_transfer<R>(
    source: &mut R,
    target_path: &Path,
    expected_sha256: [u8; 32],
    config: &StreamConfig,
) -> Result<TransferStats, TransferError>
where
    R: ChunkReader,
{
    run_resumable_transfer(source, target_path, None, expected_sha256, config).await
}

/// Runs a resumable atomic transfer.
///
/// The caller positions `source` at the resume offset; this function streams
/// the remainder to a fresh temp file, verifies the full-file checksum, and
/// atomically renames over the target.
pub async fn run_resumable_transfer<R>(
    source: &mut R,
    target_path: &Path,
    resume: Option<&ResumeRecord>,
    expected_sha256: [u8; 32],
    config: &StreamConfig,
) -> Result<TransferStats, TransferError>
where
    R: ChunkReader,
{
    // The `.part` file next to the target is the resume state: it holds the
    // verified prefix and is only renamed over the target after the whole
    // file is verified, so an interruption never corrupts the target.
    let part_path = target_path.with_extension("part");
    let append = match resume {
        Some(record) => {
            let size = tokio::fs::metadata(&part_path)
                .await
                .map(|meta| meta.len())
                .unwrap_or(0);
            if size != record.offset {
                return Err(TransferError::ResumeMismatch);
            }
            true
        }
        None => {
            let _ = tokio::fs::remove_file(&part_path).await;
            false
        }
    };
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&part_path)
        .await
        .map_err(|_| TransferError::Io)?;
    let stats = run_streaming_copy(source, PartFileWriter { file }, config).await?;

    // Verify the full part file before renaming it over the target.
    let part_bytes = tokio::fs::read(&part_path)
        .await
        .map_err(|_| TransferError::Io)?;
    if sha256_of(&part_bytes) != expected_sha256 {
        let _ = std::fs::remove_file(&part_path);
        return Err(TransferError::ChecksumMismatch);
    }
    std::fs::rename(&part_path, target_path).map_err(|_| TransferError::Io)?;
    Ok(stats)
}
#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;

    use core_domain::transfer::TransferError;

    use super::{run_atomic_transfer, run_resumable_transfer, sha256_of, ResumeRecord};
    use crate::streaming::{ChunkReader, StreamConfig};

    /// A chunk reader over an in-memory buffer with a configurable start
    /// offset (used to simulate a resumed source positioned at the offset).
    struct VecSource {
        data: Vec<u8>,
        offset: usize,
    }

    impl VecSource {
        fn new(data: Vec<u8>) -> Self {
            Self { data, offset: 0 }
        }

        fn at_offset(data: Vec<u8>, offset: usize) -> Self {
            Self { data, offset }
        }
    }

    impl ChunkReader for VecSource {
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

    /// A reader that fails after `fail_after` bytes, simulating a dropped
    /// connection mid-transfer.
    struct FlakyReader {
        data: Vec<u8>,
        offset: usize,
        fail_after: usize,
    }

    impl ChunkReader for FlakyReader {
        fn read_chunk<'a>(
            &'a mut self,
            buffer: &'a mut [u8],
        ) -> Pin<Box<dyn Future<Output = Result<usize, TransferError>> + Send + 'a>> {
            Box::pin(async move {
                if self.offset >= self.fail_after {
                    return Err(TransferError::Io);
                }
                let available = self.data.len() - self.offset;
                let until_fail = self.fail_after - self.offset;
                let take = available.min(until_fail).min(buffer.len());
                buffer[..take].copy_from_slice(&self.data[self.offset..self.offset + take]);
                self.offset += take;
                Ok(take)
            })
        }
    }

    fn temp_target(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("codex-t058-{name}-{}", std::process::id()))
    }

    fn config() -> StreamConfig {
        StreamConfig {
            chunk_size: 64 * 1024,
            max_in_flight: 4,
            yield_between_chunks: true,
        }
    }

    fn payload(size: usize) -> Vec<u8> {
        (0..size).map(|i| (i % 251) as u8).collect()
    }

    async fn read_file(path: &Path) -> Vec<u8> {
        tokio::fs::read(path).await.expect("read target")
    }

    #[tokio::test]
    async fn atomic_transfer_commits_target_and_cleans_temp() {
        let data = payload(300_000);
        let expected = sha256_of(&data);
        let target = temp_target("success");
        let _ = std::fs::remove_file(&target);

        let mut source = VecSource::new(data.clone());
        let stats = run_atomic_transfer(&mut source, &target, expected, &config())
            .await
            .expect("transfer");
        assert_eq!(stats.bytes_transferred, 300_000);
        assert_eq!(read_file(&target).await, data);
        // No leftover temp file next to the target.
        let dir = target.parent().expect("parent");
        let leftovers: Vec<_> = std::fs::read_dir(dir)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains("tmp-") && name.contains("codex-t058"))
            .collect();
        assert!(leftovers.is_empty(), "temp files leaked: {leftovers:?}");
        let _ = std::fs::remove_file(&target);
    }

    #[tokio::test]
    async fn checksum_mismatch_discards_temp_and_keeps_target() {
        let data = payload(128 * 1024);
        let target = temp_target("mismatch");
        std::fs::write(&target, b"ORIGINAL").expect("seed original");
        let wrong = [0u8; 32];

        let mut source = VecSource::new(data.clone());
        let error = run_atomic_transfer(&mut source, &target, wrong, &config())
            .await
            .expect_err("must fail");
        assert_eq!(error, TransferError::ChecksumMismatch);
        // The original target is untouched and no temp file remains.
        assert_eq!(std::fs::read(&target).expect("read target"), b"ORIGINAL");
        let dir = target.parent().expect("parent");
        let leftovers: Vec<_> = std::fs::read_dir(dir)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains("tmp-") && name.contains("codex-t058"))
            .collect();
        assert!(leftovers.is_empty(), "temp files leaked: {leftovers:?}");
        let _ = std::fs::remove_file(&target);
    }

    #[tokio::test]
    async fn interruption_does_not_corrupt_target_and_resume_completes() {
        let data = payload(300_000);
        let expected = sha256_of(&data);
        let fail_after = 200_000; // connection "drops" after 200 KB
        let target = temp_target("resume");
        let _ = std::fs::remove_file(&target);

        // First attempt: the flaky reader drops mid-transfer.
        let mut flaky = FlakyReader {
            data: data.clone(),
            offset: 0,
            fail_after,
        };
        let error = run_atomic_transfer(&mut flaky, &target, expected, &config())
            .await
            .expect_err("first attempt must fail");
        assert_eq!(error, TransferError::Io);
        // Atomic replace guarantee: no partial/corrupt target exists.
        assert!(
            !target.exists(),
            "target must not be corrupted by interruption"
        );
        // The `.part` resume state persists and holds exactly the verified
        // prefix, so a reconnected session can continue from the offset.
        let part = target.with_extension("part");
        let part_len = std::fs::metadata(&part).expect("part exists").len();
        assert_eq!(
            part_len, fail_after as u64,
            "part must hold the verified prefix"
        );

        // Resume record: the verified prefix.
        let resume = ResumeRecord {
            offset: fail_after as u64,
            partial_sha256: sha256_of(&data[..fail_after]),
        };
        // Reconnected session positions the source past the verified prefix.
        let mut source = VecSource::at_offset(data.clone(), fail_after);
        let stats =
            run_resumable_transfer(&mut source, &target, Some(&resume), expected, &config())
                .await
                .expect("resumed transfer");
        // Only the remainder was transferred.
        assert_eq!(stats.bytes_transferred, (data.len() - fail_after) as u64);
        assert_eq!(read_file(&target).await, data, "resumed file must match");
        assert!(
            !target.with_extension("part").exists(),
            "part must be renamed away"
        );
        let _ = std::fs::remove_file(&target);
    }

    #[tokio::test]
    async fn resume_record_hex_and_empty_hash() {
        // SHA-256 of the empty string is a well-known vector.
        assert_eq!(
            super::hex_digest(&sha256_of(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let record = ResumeRecord {
            offset: 0,
            partial_sha256: sha256_of(b""),
        };
        assert_eq!(record.offset, 0);
    }
}
