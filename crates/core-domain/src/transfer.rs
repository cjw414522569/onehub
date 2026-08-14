use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// Direction of a file transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransferDirection {
    /// Remote reads from local.
    Upload,
    /// Local reads from remote.
    Download,
}

/// Conflict policy when the target exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransferMode {
    /// Overwrite the target.
    Overwrite,
    /// Skip if the target exists.
    SkipIfExists,
    /// Resume an interrupted transfer at the existing offset.
    Resume,
    /// Write to a temporary file and atomically rename over the target.
    AtomicReplace,
}

impl TransferMode {
    /// Whether the mode preserves an existing target.
    pub fn preserves_existing(&self) -> bool {
        matches!(self, TransferMode::SkipIfExists | TransferMode::Resume)
    }
}

/// Specification of a single file transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferSpec {
    /// Direction.
    pub direction: TransferDirection,
    /// Source path (local for upload, remote for download).
    pub source: String,
    /// Target path (remote for upload, local for download).
    pub target: String,
    /// Conflict policy.
    pub mode: TransferMode,
    /// Whether to verify a checksum after completion.
    pub verify_checksum: bool,
}

impl TransferSpec {
    /// Creates a transfer spec, rejecting empty paths.
    pub fn new(
        direction: TransferDirection,
        source: impl Into<String>,
        target: impl Into<String>,
        mode: TransferMode,
        verify_checksum: bool,
    ) -> Result<Self, DomainError> {
        let source = source.into();
        let target = target.into();
        if source.trim().is_empty() || target.trim().is_empty() {
            return Err(DomainError::EmptyName);
        }
        Ok(Self {
            direction,
            source,
            target,
            mode,
            verify_checksum,
        })
    }
}

/// Lifecycle status of a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransferStatus {
    /// Waiting to start.
    Queued,
    /// Actively transferring.
    Transferring,
    /// Paused by the user or platform; resumable.
    Paused,
    /// Completed successfully.
    Completed,
    /// Failed; the error carries no secret context.
    Failed,
    /// Cancelled by the user.
    Cancelled,
}

/// Immutable progress snapshot of a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferProgress {
    /// Bytes transferred so far.
    pub bytes_transferred: u64,
    /// Total bytes, if known.
    pub total_bytes: Option<u64>,
    /// Measured bytes per second.
    pub bytes_per_second: u64,
}

impl TransferProgress {
    /// Progress between 0.0 and 1.0, or `None` when the total is unknown.
    pub fn fraction(&self) -> Option<f64> {
        let total = self.total_bytes?;
        if total == 0 {
            return Some(1.0);
        }
        Some(self.bytes_transferred as f64 / total as f64)
    }
}

/// A remote file operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteFileOp {
    /// Stat a remote path.
    Stat { path: String },
    /// Create a directory, optionally recursively.
    Mkdir { path: String, recursive: bool },
    /// Rename or move.
    Rename { from: String, to: String },
    /// Delete a file or directory.
    Delete { path: String, recursive: bool },
    /// Change permissions.
    Chmod { path: String, mode: u32 },
    /// Create a symbolic link.
    Symlink { target: String, link_path: String },
    /// Read a symbolic link.
    ReadLink { path: String },
}

/// Transfer/operation error with a stable, language-neutral code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferError {
    /// The source path does not exist.
    SourceNotFound,
    /// The target exists and the mode does not allow overwrite.
    TargetExists,
    /// Permission denied.
    PermissionDenied,
    /// Resume offset does not match the existing file.
    ResumeMismatch,
    /// Checksum verification failed.
    ChecksumMismatch,
    /// The operation was cancelled.
    Cancelled,
    /// An I/O failure occurred.
    Io,
    /// The operation is unsupported by the server.
    Unsupported,
}

impl TransferError {
    /// Stable string code (never renumbered).
    pub const fn stable_code(&self) -> &'static str {
        match self {
            TransferError::SourceNotFound => "E_TRANSFER_SOURCE_NOT_FOUND",
            TransferError::TargetExists => "E_TRANSFER_TARGET_EXISTS",
            TransferError::PermissionDenied => "E_TRANSFER_PERMISSION_DENIED",
            TransferError::ResumeMismatch => "E_TRANSFER_RESUME_MISMATCH",
            TransferError::ChecksumMismatch => "E_TRANSFER_CHECKSUM_MISMATCH",
            TransferError::Cancelled => "E_TRANSFER_CANCELLED",
            TransferError::Io => "E_TRANSFER_IO",
            TransferError::Unsupported => "E_TRANSFER_UNSUPPORTED",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RemoteFileOp, TransferDirection, TransferError, TransferMode, TransferProgress,
        TransferSpec, TransferStatus,
    };

    #[test]
    fn transfer_spec_validates_and_round_trips() {
        let spec = TransferSpec::new(
            TransferDirection::Upload,
            "/local/file.bin",
            "/remote/file.bin",
            TransferMode::AtomicReplace,
            true,
        )
        .expect("valid spec");
        assert_eq!(spec.direction, TransferDirection::Upload);
        assert!(!spec.mode.preserves_existing());
        let json = serde_json::to_string(&spec).expect("serialize");
        let decoded: TransferSpec = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, spec);
    }

    #[test]
    fn transfer_spec_rejects_empty_paths() {
        assert!(TransferSpec::new(
            TransferDirection::Download,
            "",
            "/remote",
            TransferMode::Overwrite,
            false
        )
        .is_err());
    }

    #[test]
    fn skip_and_resume_preserve_existing() {
        assert!(TransferMode::SkipIfExists.preserves_existing());
        assert!(TransferMode::Resume.preserves_existing());
        assert!(!TransferMode::Overwrite.preserves_existing());
        assert!(!TransferMode::AtomicReplace.preserves_existing());
    }

    #[test]
    fn progress_fraction_is_bounded() {
        let progress = TransferProgress {
            bytes_transferred: 50,
            total_bytes: Some(200),
            bytes_per_second: 1000,
        };
        let fraction = progress.fraction().expect("fraction");
        assert!((0.0..=1.0).contains(&fraction));
        assert!((fraction - 0.25).abs() < 1e-9);

        let unknown = TransferProgress {
            bytes_transferred: 50,
            total_bytes: None,
            bytes_per_second: 1000,
        };
        assert_eq!(unknown.fraction(), None);

        let empty = TransferProgress {
            bytes_transferred: 0,
            total_bytes: Some(0),
            bytes_per_second: 0,
        };
        assert_eq!(empty.fraction(), Some(1.0));
    }

    #[test]
    fn transfer_statuses_round_trip() {
        for status in [
            TransferStatus::Queued,
            TransferStatus::Transferring,
            TransferStatus::Paused,
            TransferStatus::Completed,
            TransferStatus::Failed,
            TransferStatus::Cancelled,
        ] {
            let json = serde_json::to_string(&status).expect("serialize status");
            let decoded: TransferStatus = serde_json::from_str(&json).expect("deserialize status");
            assert_eq!(decoded, status);
        }
    }

    #[test]
    fn remote_file_ops_round_trip() {
        let ops = vec![
            RemoteFileOp::Stat {
                path: "/a".to_owned(),
            },
            RemoteFileOp::Mkdir {
                path: "/a/b".to_owned(),
                recursive: true,
            },
            RemoteFileOp::Rename {
                from: "/a".to_owned(),
                to: "/b".to_owned(),
            },
            RemoteFileOp::Delete {
                path: "/a".to_owned(),
                recursive: true,
            },
            RemoteFileOp::Chmod {
                path: "/a".to_owned(),
                mode: 0o644,
            },
            RemoteFileOp::Symlink {
                target: "/b".to_owned(),
                link_path: "/a".to_owned(),
            },
            RemoteFileOp::ReadLink {
                path: "/a".to_owned(),
            },
        ];
        for op in ops {
            let json = serde_json::to_string(&op).expect("serialize op");
            let decoded: RemoteFileOp = serde_json::from_str(&json).expect("deserialize op");
            assert_eq!(decoded, op);
        }
    }

    #[test]
    fn error_mapping_is_exhaustive_and_stable() {
        let all = [
            TransferError::SourceNotFound,
            TransferError::TargetExists,
            TransferError::PermissionDenied,
            TransferError::ResumeMismatch,
            TransferError::ChecksumMismatch,
            TransferError::Cancelled,
            TransferError::Io,
            TransferError::Unsupported,
        ];
        let mut seen = std::collections::HashSet::new();
        for error in &all {
            let code = error.stable_code();
            assert!(code.starts_with("E_TRANSFER_"), "prefix required: {code}");
            assert!(seen.insert(code), "duplicate stable code: {code}");
        }
        assert_eq!(all.len(), 8);
    }
}
