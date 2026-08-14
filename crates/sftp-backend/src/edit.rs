//! Remote file editing with conflict detection and safe save (T059).
//!
//! An edit session captures a version fingerprint (SHA-256 of content + size +
//! mtime) of the remote file when editing begins. On save, the remote is
//! re-read and its fingerprint compared: if it changed, the save is refused
//! (the remote is never overwritten blindly) and the edited content is kept as
//! a recovery copy on the local side.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::client::SftpClient;
use crate::{FileAttrs, SftpError, SSH_FXF_CREAT, SSH_FXF_READ, SSH_FXF_TRUNC, SSH_FXF_WRITE};

/// Chunk size used when reading the remote file.
pub const EDIT_READ_CHUNK: u32 = 64 * 1024;

/// A version fingerprint of a remote file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFileVersion {
    /// SHA-256 of the file content.
    pub fingerprint: [u8; 32],
    /// File size in bytes.
    pub size: u64,
    /// Modification time (unix seconds), when the server reports it.
    pub mtime: Option<u32>,
}

impl RemoteFileVersion {
    /// Computes a version from content and attributes.
    pub fn from_content(content: &[u8], attrs: &FileAttrs) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(content);
        Self {
            fingerprint: hasher.finalize().into(),
            size: content.len() as u64,
            mtime: attrs.mtime,
        }
    }
}

/// Outcome of a safe save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    /// The remote was unchanged since the edit began; the save succeeded.
    Saved,
    /// The remote changed since the edit began; the save was refused and the
    /// edited content was kept at `recovery_path`.
    Conflict {
        /// The current remote version that caused the conflict.
        remote: RemoteFileVersion,
        /// Local path where the edited content was preserved.
        recovery_path: String,
    },
}

/// An edit session over one remote file.
#[derive(Debug)]
pub struct RemoteEditSession {
    path: String,
    baseline: RemoteFileVersion,
}

impl RemoteEditSession {
    /// Begins an edit: reads the remote file and captures its version.
    pub async fn begin<S>(
        client: &mut SftpClient<S>,
        path: &str,
    ) -> Result<(Self, RemoteFileVersion), SftpError>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
    {
        let content = read_entire_file(client, path).await?;
        let attrs = client.stat(path).await?;
        let version = RemoteFileVersion::from_content(&content, &attrs);
        Ok((
            Self {
                path: path.to_owned(),
                baseline: version.clone(),
            },
            version,
        ))
    }

    /// The baseline version captured when the edit began.
    pub fn baseline(&self) -> &RemoteFileVersion {
        &self.baseline
    }

    /// Saves `new_content` safely: refuses if the remote changed since the
    /// edit began, and preserves the edited content as a recovery copy.
    pub async fn save<S>(
        &self,
        client: &mut SftpClient<S>,
        new_content: &[u8],
    ) -> Result<SaveOutcome, SftpError>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
    {
        let current = read_entire_file(client, &self.path).await?;
        let attrs = client.stat(&self.path).await?;
        let remote_version = RemoteFileVersion::from_content(&current, &attrs);
        if remote_version != self.baseline {
            let recovery_path = write_recovery_copy(&self.path, new_content)?;
            return Ok(SaveOutcome::Conflict {
                remote: remote_version,
                recovery_path,
            });
        }
        // Safe overwrite: TRUNC + write (production builds on T058 atomic
        // replace on the remote side via posix-rename).
        let handle = client
            .open(
                &self.path,
                SSH_FXF_WRITE | SSH_FXF_CREAT | SSH_FXF_TRUNC,
                &FileAttrs::default(),
            )
            .await?;
        client.write(&handle, 0, new_content).await?;
        client.close(&handle).await?;
        Ok(SaveOutcome::Saved)
    }
}

/// Reads an entire remote file into memory (bounded per-chunk).
pub async fn read_entire_file<S>(
    client: &mut SftpClient<S>,
    path: &str,
) -> Result<Vec<u8>, SftpError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
{
    let handle = client
        .open(path, SSH_FXF_READ, &FileAttrs::default())
        .await?;
    let mut data = Vec::new();
    loop {
        let chunk = client
            .read(&handle, data.len() as u64, EDIT_READ_CHUNK)
            .await?;
        if chunk.is_empty() {
            break;
        }
        data.extend_from_slice(&chunk);
    }
    client.close(&handle).await?;
    Ok(data)
}

/// Writes the edited content to a recovery copy in the system temp directory.
fn write_recovery_copy(remote_path: &str, content: &[u8]) -> Result<String, SftpError> {
    let name = remote_path
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("recovery");
    let recovery: PathBuf = std::env::temp_dir().join(format!(
        "{name}.conflict-{}-{}",
        std::process::id(),
        crate::edit::next_recovery_nonce()
    ));
    std::fs::write(&recovery, content).map_err(|_| SftpError::Io)?;
    Ok(recovery.to_string_lossy().to_string())
}

static RECOVERY_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn next_recovery_nonce() -> u64 {
    RECOVERY_NONCE.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}
