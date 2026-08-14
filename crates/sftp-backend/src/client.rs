//! SFTP v3 client (T056): `INIT`/`VERSION` handshake with capability probing,
//! plus typed operations over any bidirectional stream.

use std::sync::atomic::{AtomicU32, Ordering};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::attrs::FileAttrs;
use crate::msg::{
    self, decode_attrs_body, decode_data, decode_handle, decode_name, decode_name_list,
    decode_status, encode_close, encode_extended, encode_fsetstat, encode_fstat, encode_init,
    encode_lstat, encode_mkdir, encode_open, encode_opendir, encode_read, encode_readdir,
    encode_readlink, encode_realpath, encode_remove, encode_rename, encode_rmdir, encode_setstat,
    encode_stat, encode_symlink, encode_write, frame_packet, SFTP_VERSION, SSH_FXP_ATTRS,
    SSH_FXP_DATA, SSH_FXP_HANDLE, SSH_FXP_NAME, SSH_FXP_STATUS, SSH_FXP_VERSION,
};
use crate::SftpError;

/// Capabilities advertised by the server in its `VERSION` reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SftpCapabilities {
    /// The negotiated protocol version.
    pub version: u32,
    /// Extension name/value pairs from the server.
    pub extensions: Vec<(String, String)>,
}

impl SftpCapabilities {
    /// Whether the server supports the named extension.
    pub fn supports(&self, name: &str) -> bool {
        self.extensions.iter().any(|(key, _)| key == name)
    }
}

/// SFTP status codes mapped to a stable enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SftpStatus {
    /// Success.
    Ok,
    /// End of file.
    Eof,
    /// No such file or directory.
    NoSuchFile,
    /// Permission denied.
    PermissionDenied,
    /// General failure.
    Failure,
    /// Bad message.
    BadMessage,
    /// No connection.
    NoConnection,
    /// Connection lost.
    ConnectionLost,
    /// Operation unsupported.
    Unsupported,
}

impl SftpStatus {
    /// Maps a wire status code.
    pub fn from_code(code: u32) -> Self {
        match code {
            msg::SSH_FX_OK => SftpStatus::Ok,
            msg::SSH_FX_EOF => SftpStatus::Eof,
            msg::SSH_FX_NO_SUCH_FILE => SftpStatus::NoSuchFile,
            msg::SSH_FX_PERMISSION_DENIED => SftpStatus::PermissionDenied,
            msg::SSH_FX_FAILURE => SftpStatus::Failure,
            msg::SSH_FX_BAD_MESSAGE => SftpStatus::BadMessage,
            msg::SSH_FX_NO_CONNECTION => SftpStatus::NoConnection,
            msg::SSH_FX_CONNECTION_LOST => SftpStatus::ConnectionLost,
            msg::SSH_FX_OP_UNSUPPORTED => SftpStatus::Unsupported,
            _ => SftpStatus::Failure,
        }
    }
}

/// An SFTP v3 client over a stream.
pub struct SftpClient<S> {
    stream: tokio::sync::Mutex<S>,
    next_id: AtomicU32,
    capabilities: Option<SftpCapabilities>,
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> SftpClient<S> {
    /// Creates a client.
    pub fn new(stream: S) -> Self {
        Self {
            stream: tokio::sync::Mutex::new(stream),
            next_id: AtomicU32::new(1),
            capabilities: None,
        }
    }

    /// Performs the `INIT`/`VERSION` handshake and returns the capabilities.
    pub async fn init(&mut self) -> Result<SftpCapabilities, SftpError> {
        let (ty, body) = self.round_trip(encode_init(SFTP_VERSION)).await?;
        if ty != SSH_FXP_VERSION {
            return Err(SftpError::UnexpectedType(ty));
        }
        let (version, mut rest) = crate::attrs::take_u32(&body)?;
        let mut extensions = Vec::new();
        while !rest.is_empty() {
            let (name, remaining) = crate::msg::take_string(rest)?;
            let (value, remaining) = crate::msg::take_string(remaining)?;
            extensions.push((name, value));
            rest = remaining;
        }
        let capabilities = SftpCapabilities {
            version,
            extensions,
        };
        self.capabilities = Some(capabilities.clone());
        Ok(capabilities)
    }

    /// The negotiated capabilities, if `init` has run.
    pub fn capabilities(&self) -> Option<&SftpCapabilities> {
        self.capabilities.as_ref()
    }

    /// Opens a file and returns a handle.
    pub async fn open(
        &mut self,
        path: &str,
        pflags: u32,
        attrs: &FileAttrs,
    ) -> Result<String, SftpError> {
        let id = self.next_id();
        let (ty, body) = self
            .round_trip(encode_open(id, path, pflags, attrs))
            .await?;
        match ty {
            SSH_FXP_HANDLE => decode_handle(&body),
            SSH_FXP_STATUS => Err(SftpError::Status(SftpStatus::from_code(
                decode_status(&body)?.0,
            ))),
            other => Err(SftpError::UnexpectedType(other)),
        }
    }

    /// Closes a handle.
    pub async fn close(&mut self, handle: &str) -> Result<(), SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_close(id, handle)).await?;
        self.expect_ok(ty, &body).await
    }

    /// Reads up to `length` bytes at `offset`; EOF yields an empty buffer.
    pub async fn read(
        &mut self,
        handle: &str,
        offset: u64,
        length: u32,
    ) -> Result<Vec<u8>, SftpError> {
        let id = self.next_id();
        let (ty, body) = self
            .round_trip(encode_read(id, handle, offset, length))
            .await?;
        match ty {
            SSH_FXP_DATA => decode_data(&body),
            SSH_FXP_STATUS => {
                let (code, _, _) = decode_status(&body)?;
                if code == msg::SSH_FX_EOF {
                    Ok(Vec::new())
                } else {
                    Err(SftpError::Status(SftpStatus::from_code(code)))
                }
            }
            other => Err(SftpError::UnexpectedType(other)),
        }
    }

    /// Writes `data` at `offset`.
    pub async fn write(&mut self, handle: &str, offset: u64, data: &[u8]) -> Result<(), SftpError> {
        let id = self.next_id();
        let (ty, body) = self
            .round_trip(encode_write(id, handle, offset, data))
            .await?;
        self.expect_ok(ty, &body).await
    }

    /// `lstat` (does not follow symlinks).
    pub async fn lstat(&mut self, path: &str) -> Result<FileAttrs, SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_lstat(id, path)).await?;
        self.expect_attrs(ty, &body).await
    }

    /// `stat` (follows symlinks).
    pub async fn stat(&mut self, path: &str) -> Result<FileAttrs, SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_stat(id, path)).await?;
        self.expect_attrs(ty, &body).await
    }

    /// `fstat` on an open handle.
    pub async fn fstat(&mut self, handle: &str) -> Result<FileAttrs, SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_fstat(id, handle)).await?;
        self.expect_attrs(ty, &body).await
    }

    /// `setstat` on a path.
    pub async fn setstat(&mut self, path: &str, attrs: &FileAttrs) -> Result<(), SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_setstat(id, path, attrs)).await?;
        self.expect_ok(ty, &body).await
    }

    /// `fsetstat` on an open handle.
    pub async fn fsetstat(&mut self, handle: &str, attrs: &FileAttrs) -> Result<(), SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_fsetstat(id, handle, attrs)).await?;
        self.expect_ok(ty, &body).await
    }

    /// Opens a directory for listing.
    pub async fn opendir(&mut self, path: &str) -> Result<String, SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_opendir(id, path)).await?;
        match ty {
            SSH_FXP_HANDLE => decode_handle(&body),
            SSH_FXP_STATUS => Err(SftpError::Status(SftpStatus::from_code(
                decode_status(&body)?.0,
            ))),
            other => Err(SftpError::UnexpectedType(other)),
        }
    }

    /// Reads the next batch of directory entries; EOF yields an empty list.
    pub async fn readdir(
        &mut self,
        handle: &str,
    ) -> Result<Vec<(String, String, FileAttrs)>, SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_readdir(id, handle)).await?;
        match ty {
            SSH_FXP_NAME => decode_name_list(&body),
            SSH_FXP_STATUS => {
                let (code, _, _) = decode_status(&body)?;
                if code == msg::SSH_FX_EOF {
                    Ok(Vec::new())
                } else {
                    Err(SftpError::Status(SftpStatus::from_code(code)))
                }
            }
            other => Err(SftpError::UnexpectedType(other)),
        }
    }

    /// Removes a file.
    pub async fn remove(&mut self, path: &str) -> Result<(), SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_remove(id, path)).await?;
        self.expect_ok(ty, &body).await
    }

    /// Creates a directory.
    pub async fn mkdir(&mut self, path: &str, attrs: &FileAttrs) -> Result<(), SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_mkdir(id, path, attrs)).await?;
        self.expect_ok(ty, &body).await
    }

    /// Removes a directory.
    pub async fn rmdir(&mut self, path: &str) -> Result<(), SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_rmdir(id, path)).await?;
        self.expect_ok(ty, &body).await
    }

    /// Canonicalizes a path.
    pub async fn realpath(&mut self, path: &str) -> Result<String, SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_realpath(id, path)).await?;
        match ty {
            SSH_FXP_NAME => Ok(decode_name(&body)?.0),
            SSH_FXP_STATUS => Err(SftpError::Status(SftpStatus::from_code(
                decode_status(&body)?.0,
            ))),
            other => Err(SftpError::UnexpectedType(other)),
        }
    }

    /// Renames `old_path` to `new_path`.
    pub async fn rename(&mut self, old_path: &str, new_path: &str) -> Result<(), SftpError> {
        let id = self.next_id();
        let (ty, body) = self
            .round_trip(encode_rename(id, old_path, new_path))
            .await?;
        self.expect_ok(ty, &body).await
    }

    /// Reads a symlink target.
    pub async fn readlink(&mut self, path: &str) -> Result<String, SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_readlink(id, path)).await?;
        match ty {
            SSH_FXP_NAME => Ok(decode_name(&body)?.0),
            SSH_FXP_STATUS => Err(SftpError::Status(SftpStatus::from_code(
                decode_status(&body)?.0,
            ))),
            other => Err(SftpError::UnexpectedType(other)),
        }
    }

    /// Creates a symlink.
    pub async fn symlink(&mut self, link_path: &str, target: &str) -> Result<(), SftpError> {
        let id = self.next_id();
        let (ty, body) = self
            .round_trip(encode_symlink(id, link_path, target))
            .await?;
        self.expect_ok(ty, &body).await
    }

    /// Sends an extended request and returns the extended reply body.
    pub async fn extended(&mut self, name: &str, data: &[u8]) -> Result<Vec<u8>, SftpError> {
        let id = self.next_id();
        let (ty, body) = self.round_trip(encode_extended(id, name, data)).await?;
        match ty {
            msg::SSH_FXP_EXTENDED_REPLY => Ok(body),
            SSH_FXP_STATUS => {
                let (code, _, _) = decode_status(&body)?;
                if code == msg::SSH_FX_OK {
                    Ok(Vec::new())
                } else {
                    Err(SftpError::Status(SftpStatus::from_code(code)))
                }
            }
            other => Err(SftpError::UnexpectedType(other)),
        }
    }

    fn next_id(&self) -> u32 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    async fn round_trip(&self, payload: Vec<u8>) -> Result<(u8, Vec<u8>), SftpError> {
        let mut stream = self.stream.lock().await;
        stream
            .write_all(&frame_packet(&payload))
            .await
            .map_err(|_| SftpError::Io)?;
        let mut length = [0u8; 4];
        stream
            .read_exact(&mut length)
            .await
            .map_err(|_| SftpError::Io)?;
        let length = u32::from_be_bytes(length) as usize;
        if length > msg::MAX_PACKET_LEN {
            return Err(SftpError::protocol("packet too large"));
        }
        if length == 0 {
            return Err(SftpError::protocol("empty packet"));
        }
        let mut payload = vec![0u8; length];
        stream
            .read_exact(&mut payload)
            .await
            .map_err(|_| SftpError::Io)?;
        Ok((payload[0], payload[1..].to_vec()))
    }

    async fn expect_ok(&self, ty: u8, body: &[u8]) -> Result<(), SftpError> {
        match ty {
            SSH_FXP_STATUS => {
                let (code, _, _) = decode_status(body)?;
                if code == msg::SSH_FX_OK {
                    Ok(())
                } else {
                    Err(SftpError::Status(SftpStatus::from_code(code)))
                }
            }
            other => Err(SftpError::UnexpectedType(other)),
        }
    }

    async fn expect_attrs(&self, ty: u8, body: &[u8]) -> Result<FileAttrs, SftpError> {
        match ty {
            SSH_FXP_ATTRS => decode_attrs_body(body),
            SSH_FXP_STATUS => Err(SftpError::Status(SftpStatus::from_code(
                decode_status(body)?.0,
            ))),
            other => Err(SftpError::UnexpectedType(other)),
        }
    }
}
