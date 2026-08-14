#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # sftp-backend
//!
//! In-house SFTP v3 (OpenSSH-compatible) protocol: attribute codec, message
//! codec, a typed client with capability probing, and an in-memory server +
//! virtual filesystem for integration tests (real OpenSSH SFTP is
//! `blocked_environment` on this host).

pub mod attrs;
pub mod client;
pub mod edit;
pub mod msg;
pub mod server;

pub use attrs::{
    decode_attrs, encode_attrs, FileAttrs, S_IFCHR, S_IFDIR, S_IFLNK, S_IFREG, S_IFSOCK,
};
pub use client::{SftpCapabilities, SftpClient, SftpStatus};
pub use edit::{
    read_entire_file, RemoteEditSession, RemoteFileVersion, SaveOutcome, EDIT_READ_CHUNK,
};
pub use msg::{
    encode_close, encode_data, encode_extended, encode_fsetstat, encode_fstat, encode_init,
    encode_lstat, encode_mkdir, encode_open, encode_opendir, encode_read, encode_readdir,
    encode_readlink, encode_realpath, encode_remove, encode_rename, encode_rmdir, encode_setstat,
    encode_stat, encode_symlink, encode_write, frame_packet, parse_packet, MAX_PACKET_LEN,
    SFTP_VERSION, SSH_FXF_APPEND, SSH_FXF_CREAT, SSH_FXF_EXCL, SSH_FXF_READ, SSH_FXF_TRUNC,
    SSH_FXF_WRITE, SSH_FX_BAD_MESSAGE, SSH_FX_CONNECTION_LOST, SSH_FX_EOF, SSH_FX_FAILURE,
    SSH_FX_NO_CONNECTION, SSH_FX_NO_SUCH_FILE, SSH_FX_OK, SSH_FX_OP_UNSUPPORTED,
    SSH_FX_PERMISSION_DENIED,
};
pub use server::{FsEntry, FsError, SftpServer, VirtualFs, SERVER_EXTENSIONS};

/// SFTP error (no secret context).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SftpError {
    /// Underlying I/O failure.
    Io,
    /// A protocol-level violation.
    Protocol(String),
    /// The server replied with a non-OK status.
    Status(SftpStatus),
    /// An unexpected response message type.
    UnexpectedType(u8),
}

impl SftpError {
    /// A protocol violation.
    pub fn protocol(detail: impl Into<String>) -> Self {
        Self::Protocol(detail.into())
    }
}

impl core::fmt::Display for SftpError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SftpError::Io => write!(formatter, "SFTP I/O error"),
            SftpError::Protocol(detail) => write!(formatter, "{detail}"),
            SftpError::Status(status) => write!(formatter, "SFTP status: {status:?}"),
            SftpError::UnexpectedType(ty) => write!(formatter, "unexpected SFTP type 0x{ty:02x}"),
        }
    }
}

impl core::error::Error for SftpError {}

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "sftp-backend";
