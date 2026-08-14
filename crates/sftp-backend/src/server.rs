//! In-memory SFTP v3 server (T056): a deterministic virtual filesystem plus a
//! packet dispatcher that implements list / stat / mkdir / rename / delete /
//! permissions / symlinks, with proper `SSH_FX_*` status codes. Used by the
//! integration tests (real OpenSSH SFTP is `blocked_environment` on this host).

use std::collections::{BTreeMap, HashMap, VecDeque};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::attrs::{decode_attrs, FileAttrs, S_IFLNK};
use crate::msg::{
    self, encode_attrs_body, encode_data, encode_handle, encode_name_list, encode_name_one,
    encode_status, frame_packet, parse_packet, push_string, take_string, SFTP_VERSION,
    SSH_FXF_CREAT, SSH_FXF_EXCL, SSH_FXF_READ, SSH_FXF_TRUNC, SSH_FXF_WRITE, SSH_FXP_CLOSE,
    SSH_FXP_EXTENDED, SSH_FXP_FSETSTAT, SSH_FXP_FSTAT, SSH_FXP_INIT, SSH_FXP_LSTAT, SSH_FXP_MKDIR,
    SSH_FXP_OPEN, SSH_FXP_OPENDIR, SSH_FXP_READ, SSH_FXP_READDIR, SSH_FXP_READLINK,
    SSH_FXP_REALPATH, SSH_FXP_REMOVE, SSH_FXP_RENAME, SSH_FXP_RMDIR, SSH_FXP_SETSTAT, SSH_FXP_STAT,
    SSH_FXP_SYMLINK, SSH_FXP_VERSION, SSH_FXP_WRITE, SSH_FX_BAD_MESSAGE, SSH_FX_EOF,
    SSH_FX_FAILURE, SSH_FX_NO_SUCH_FILE, SSH_FX_OK, SSH_FX_OP_UNSUPPORTED,
    SSH_FX_PERMISSION_DENIED,
};
use crate::SftpError;

/// Virtual filesystem error mapped to SFTP status codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    /// No such file or directory.
    NoSuchFile,
    /// Permission denied.
    PermissionDenied,
    /// End of file.
    Eof,
    /// General failure (e.g. directory not empty, target exists with EXCL).
    Failure,
}

impl FsError {
    /// The SFTP status code.
    pub fn status_code(self) -> u32 {
        match self {
            FsError::NoSuchFile => SSH_FX_NO_SUCH_FILE,
            FsError::PermissionDenied => SSH_FX_PERMISSION_DENIED,
            FsError::Eof => SSH_FX_EOF,
            FsError::Failure => SSH_FX_FAILURE,
        }
    }
}

/// An in-memory filesystem entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsEntry {
    /// A directory.
    Dir { attrs: FileAttrs },
    /// A regular file with contents.
    File { data: Vec<u8>, attrs: FileAttrs },
    /// A symbolic link to `target`.
    Symlink { target: String, attrs: FileAttrs },
}

impl FsEntry {
    /// A new directory entry.
    pub fn directory(mode: u32) -> Self {
        FsEntry::Dir {
            attrs: FileAttrs::directory(mode),
        }
    }

    /// A new regular file entry.
    pub fn file(data: Vec<u8>, mode: u32) -> Self {
        FsEntry::File {
            data,
            attrs: FileAttrs::file(0, mode),
        }
    }

    /// A new symlink entry.
    pub fn symlink(target: String, mode: u32) -> Self {
        FsEntry::Symlink {
            target,
            attrs: FileAttrs {
                permissions: Some(S_IFLNK | mode),
                ..FileAttrs::default()
            },
        }
    }

    /// The entry attributes (file size reflects contents).
    pub fn attrs(&self) -> FileAttrs {
        match self {
            FsEntry::Dir { attrs } => attrs.clone(),
            FsEntry::File { data, attrs } => {
                let mut attrs = attrs.clone();
                attrs.size = Some(data.len() as u64);
                attrs
            }
            FsEntry::Symlink { attrs, .. } => attrs.clone(),
        }
    }

    /// Whether the entry is a directory.
    pub fn is_dir(&self) -> bool {
        matches!(self, FsEntry::Dir { .. })
    }
}

/// An in-memory virtual filesystem rooted at `/`, stored as a flat map of
/// canonical absolute paths (deterministic, cross-platform).
#[derive(Debug, Clone, Default)]
pub struct VirtualFs {
    entries: BTreeMap<String, FsEntry>,
}

impl VirtualFs {
    /// A filesystem with an empty root directory.
    pub fn new() -> Self {
        let mut entries = BTreeMap::new();
        entries.insert("/".to_owned(), FsEntry::directory(0o755));
        Self { entries }
    }

    /// Canonicalizes an absolute path (resolves `.` and `..`).
    pub fn canonicalize(&self, path: &str) -> String {
        let mut parts: Vec<&str> = Vec::new();
        for component in path.split('/') {
            match component {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                other => parts.push(other),
            }
        }
        if parts.is_empty() {
            "/".to_owned()
        } else {
            format!("/{}", parts.join("/"))
        }
    }

    fn parent(path: &str) -> (&str, &str) {
        let trimmed = path.trim_end_matches('/');
        match trimmed.rfind('/') {
            Some(index) => {
                if index == 0 {
                    ("/", &trimmed[1..])
                } else {
                    (&trimmed[..index], &trimmed[index + 1..])
                }
            }
            None => ("/", trimmed),
        }
    }

    /// Looks up the entry at `path` without following symlinks.
    pub fn lookup(&self, path: &str) -> Result<&FsEntry, FsError> {
        self.entries
            .get(&self.canonicalize(path))
            .ok_or(FsError::NoSuchFile)
    }

    /// `lstat` semantics: attrs without following the final symlink.
    pub fn lstat(&self, path: &str) -> Result<FileAttrs, FsError> {
        Ok(self.lookup(path)?.attrs())
    }

    /// `stat` semantics: follows the final symlink (cycle-guarded).
    pub fn stat(&self, path: &str) -> Result<FileAttrs, FsError> {
        let mut current = self.canonicalize(path);
        for _ in 0..16 {
            match self.entries.get(&current) {
                Some(FsEntry::Symlink { target, .. }) => {
                    let resolved = if target.starts_with('/') {
                        target.to_owned()
                    } else {
                        let (parent, _) = Self::parent(&current);
                        format!("{parent}/{target}")
                    };
                    current = self.canonicalize(&resolved);
                }
                Some(entry) => return Ok(entry.attrs()),
                None => return Err(FsError::NoSuchFile),
            }
        }
        Err(FsError::Failure)
    }

    /// Lists the direct children of `path`.
    pub fn list_dir(&self, path: &str) -> Result<Vec<(String, String, FileAttrs)>, FsError> {
        let dir = self.canonicalize(path);
        if !self.lookup(&dir)?.is_dir() {
            return Err(FsError::Failure);
        }
        let mut children = Vec::new();
        for (key, entry) in &self.entries {
            if key == &dir {
                continue;
            }
            let (parent, name) = Self::parent(key);
            if parent == dir {
                let attrs = entry.attrs();
                let longname = format!(
                    "{} {} {} {}",
                    attrs.mode_string(),
                    attrs.size.unwrap_or(0),
                    name,
                    name
                );
                children.push((name.to_owned(), longname, attrs));
            }
        }
        Ok(children)
    }

    /// Creates a directory.
    pub fn mkdir(&mut self, path: &str, attrs: &FileAttrs) -> Result<(), FsError> {
        let canonical = self.canonicalize(path);
        if self.entries.contains_key(&canonical) {
            return Err(FsError::Failure);
        }
        let (parent, _) = Self::parent(&canonical);
        if !self.lookup(parent)?.is_dir() {
            return Err(FsError::NoSuchFile);
        }
        let requested = attrs.permission_bits() & 0o777;
        let mode = if requested != 0 { requested } else { 0o755 };
        self.entries.insert(canonical, FsEntry::directory(mode));
        Ok(())
    }

    /// Creates a regular file (for tests / CREAT open).
    pub fn create_file(&mut self, path: &str, data: Vec<u8>) -> Result<(), FsError> {
        let canonical = self.canonicalize(path);
        if self.entries.contains_key(&canonical) {
            return Err(FsError::Failure);
        }
        let (parent, _) = Self::parent(&canonical);
        if !self.lookup(parent)?.is_dir() {
            return Err(FsError::NoSuchFile);
        }
        self.entries.insert(canonical, FsEntry::file(data, 0o644));
        Ok(())
    }

    /// Creates a symlink at `link` pointing to `target`.
    pub fn symlink(&mut self, link: &str, target: &str) -> Result<(), FsError> {
        let canonical = self.canonicalize(link);
        if self.entries.contains_key(&canonical) {
            return Err(FsError::Failure);
        }
        let (parent, _) = Self::parent(&canonical);
        if !self.lookup(parent)?.is_dir() {
            return Err(FsError::NoSuchFile);
        }
        self.entries
            .insert(canonical, FsEntry::symlink(target.to_owned(), 0o777));
        Ok(())
    }

    /// Reads the symlink target.
    pub fn readlink(&self, path: &str) -> Result<String, FsError> {
        match self.lookup(path)? {
            FsEntry::Symlink { target, .. } => Ok(target.clone()),
            _ => Err(FsError::Failure),
        }
    }

    /// Reads bytes from a file.
    pub fn read(&self, path: &str, offset: u64, length: usize) -> Result<Vec<u8>, FsError> {
        match self.lookup(path)? {
            FsEntry::File { data, .. } => {
                let start = offset as usize;
                if start >= data.len() {
                    return Err(FsError::Eof);
                }
                let end = (start + length).min(data.len());
                Ok(data[start..end].to_vec())
            }
            _ => Err(FsError::Failure),
        }
    }

    /// Writes bytes to a file, extending it as needed.
    pub fn write(&mut self, path: &str, offset: u64, data: &[u8]) -> Result<(), FsError> {
        let canonical = self.canonicalize(path);
        let entry = self
            .entries
            .get_mut(&canonical)
            .ok_or(FsError::NoSuchFile)?;
        match entry {
            FsEntry::File {
                data: file_data, ..
            } => {
                let start = offset as usize;
                if start > file_data.len() {
                    file_data.resize(start, 0);
                }
                if start + data.len() > file_data.len() {
                    file_data.resize(start + data.len(), 0);
                }
                file_data[start..start + data.len()].copy_from_slice(data);
                Ok(())
            }
            _ => Err(FsError::Failure),
        }
    }

    /// Truncates a file to zero length.
    pub fn truncate(&mut self, path: &str) -> Result<(), FsError> {
        let canonical = self.canonicalize(path);
        let entry = self
            .entries
            .get_mut(&canonical)
            .ok_or(FsError::NoSuchFile)?;
        match entry {
            FsEntry::File { data, .. } => {
                data.clear();
                Ok(())
            }
            _ => Err(FsError::Failure),
        }
    }

    /// Removes a file or symlink.
    pub fn remove(&mut self, path: &str) -> Result<(), FsError> {
        let canonical = self.canonicalize(path);
        match self.entries.get(&canonical) {
            Some(FsEntry::Dir { .. }) => Err(FsError::Failure),
            Some(_) => {
                self.entries.remove(&canonical);
                Ok(())
            }
            None => Err(FsError::NoSuchFile),
        }
    }

    /// Removes an empty directory.
    pub fn rmdir(&mut self, path: &str) -> Result<(), FsError> {
        let canonical = self.canonicalize(path);
        match self.entries.get(&canonical) {
            Some(FsEntry::Dir { .. }) => {
                if self
                    .entries
                    .keys()
                    .any(|key| Self::parent(key).0 == canonical)
                {
                    return Err(FsError::Failure);
                }
                self.entries.remove(&canonical);
                Ok(())
            }
            Some(_) => Err(FsError::Failure),
            None => Err(FsError::NoSuchFile),
        }
    }

    /// Renames `old` to `new`, moving any subtree.
    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), FsError> {
        let old_canonical = self.canonicalize(old);
        let new_canonical = self.canonicalize(new);
        let entry = self
            .entries
            .remove(&old_canonical)
            .ok_or(FsError::NoSuchFile)?;
        let prefix = format!("{old_canonical}/");
        let subtree: Vec<(String, FsEntry)> = self
            .entries
            .iter()
            .filter(|(key, _)| key.starts_with(&prefix))
            .map(|(key, entry)| (key.clone(), entry.clone()))
            .collect();
        for (key, _) in &subtree {
            self.entries.remove(key);
        }
        self.entries.insert(new_canonical.clone(), entry);
        for (key, entry) in subtree {
            let suffix = &key[old_canonical.len()..];
            self.entries
                .insert(format!("{new_canonical}{suffix}"), entry);
        }
        Ok(())
    }

    /// Updates attributes on a path.
    pub fn set_attrs(&mut self, path: &str, attrs: &FileAttrs) -> Result<(), FsError> {
        let canonical = self.canonicalize(path);
        let entry = self
            .entries
            .get_mut(&canonical)
            .ok_or(FsError::NoSuchFile)?;
        match entry {
            FsEntry::Dir { attrs: current }
            | FsEntry::File { attrs: current, .. }
            | FsEntry::Symlink { attrs: current, .. } => {
                if let Some(permissions) = attrs.permissions {
                    current.permissions = Some(permissions);
                }
                if let Some(mtime) = attrs.mtime {
                    current.mtime = Some(mtime);
                }
                if let Some(atime) = attrs.atime {
                    current.atime = Some(atime);
                }
                Ok(())
            }
        }
    }
}
enum HandleKind {
    File { read: bool, write: bool },
    Dir,
}

struct Handle {
    path: String,
    kind: HandleKind,
    dir_pending: Option<VecDeque<(String, String, FileAttrs)>>,
}

/// Extensions advertised by the in-memory server.
pub const SERVER_EXTENSIONS: &[(&str, &str)] = &[
    ("posix-rename@openssh.com", "1"),
    ("statvfs@openssh.com", "2"),
    ("lsetstat@openssh.com", "1"),
    ("hardlink@openssh.com", "1"),
];

/// An in-memory SFTP v3 server.
#[derive(Default)]
pub struct SftpServer {
    fs: VirtualFs,
    handles: HashMap<String, Handle>,
    next_handle: u64,
}

impl SftpServer {
    /// A server with an empty virtual filesystem.
    pub fn new() -> Self {
        Self {
            fs: VirtualFs::new(),
            handles: HashMap::new(),
            next_handle: 1,
        }
    }

    /// Access to the virtual filesystem (for seeding / asserting).
    pub fn fs(&self) -> &VirtualFs {
        &self.fs
    }

    /// Mutable access to the virtual filesystem.
    pub fn fs_mut(&mut self) -> &mut VirtualFs {
        &mut self.fs
    }

    /// Serves packets from `stream` until EOF.
    pub async fn serve<S>(&mut self, stream: &mut S) -> Result<(), SftpError>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut buffer = Vec::new();
        loop {
            let mut chunk = [0u8; 4096];
            let n = stream.read(&mut chunk).await.map_err(|_| SftpError::Io)?;
            if n == 0 {
                return Ok(());
            }
            buffer.extend_from_slice(&chunk[..n]);
            while let Some((ty, body, consumed)) = parse_packet(&buffer)? {
                buffer.drain(..consumed);
                self.dispatch(stream, ty, &body).await?;
            }
            if buffer.len() > msg::MAX_PACKET_LEN {
                return Err(SftpError::protocol("request buffer overflow"));
            }
        }
    }

    async fn dispatch<S>(&mut self, stream: &mut S, ty: u8, body: &[u8]) -> Result<(), SftpError>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let response = match ty {
            SSH_FXP_INIT => {
                let mut bytes = vec![SSH_FXP_VERSION];
                bytes.extend_from_slice(&SFTP_VERSION.to_be_bytes());
                for (name, value) in SERVER_EXTENSIONS {
                    push_string(&mut bytes, name);
                    push_string(&mut bytes, value);
                }
                bytes
            }
            SSH_FXP_OPEN => self.handle_open(body),
            SSH_FXP_CLOSE => self.handle_close(body),
            SSH_FXP_READ => self.handle_read(body),
            SSH_FXP_WRITE => self.handle_write(body),
            SSH_FXP_LSTAT => self.handle_lstat(body),
            SSH_FXP_STAT => self.handle_stat(body),
            SSH_FXP_FSTAT => self.handle_fstat(body),
            SSH_FXP_SETSTAT => self.handle_setstat(body),
            SSH_FXP_FSETSTAT => self.handle_fsetstat(body),
            SSH_FXP_OPENDIR => self.handle_opendir(body),
            SSH_FXP_READDIR => self.handle_readdir(body),
            SSH_FXP_REMOVE => self.handle_remove(body),
            SSH_FXP_MKDIR => self.handle_mkdir(body),
            SSH_FXP_RMDIR => self.handle_rmdir(body),
            SSH_FXP_REALPATH => self.handle_realpath(body),
            SSH_FXP_RENAME => self.handle_rename(body),
            SSH_FXP_READLINK => self.handle_readlink(body),
            SSH_FXP_SYMLINK => self.handle_symlink(body),
            SSH_FXP_EXTENDED => self.handle_extended(body),
            _ => encode_status(0, SSH_FX_OP_UNSUPPORTED, "unsupported"),
        };
        stream
            .write_all(&frame_packet(&response))
            .await
            .map_err(|_| SftpError::Io)?;
        Ok(())
    }

    fn handle_open(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad open");
        };
        let Ok((path, rest)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad path");
        };
        let Ok((pflags, rest)) = crate::attrs::take_u32(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad pflags");
        };
        let (attrs, _) = decode_attrs(rest).unwrap_or((FileAttrs::default(), &[]));
        let canonical = self.fs.canonicalize(&path);
        let exists = self.fs.entries.contains_key(&canonical);
        let want_read = pflags & SSH_FXF_READ != 0;
        let want_write = pflags & SSH_FXF_WRITE != 0;
        let create = pflags & SSH_FXF_CREAT != 0;
        let truncate = pflags & SSH_FXF_TRUNC != 0;
        let exclusive = pflags & SSH_FXF_EXCL != 0;
        if !exists {
            if !create {
                return encode_status(id, SSH_FX_NO_SUCH_FILE, "no such file");
            }
            if self.fs.create_file(&path, Vec::new()).is_err() {
                return encode_status(id, SSH_FX_FAILURE, "create failed");
            }
            if attrs.permissions.is_some() {
                let _ = self.fs.set_attrs(&path, &attrs);
            }
        } else if exclusive {
            return encode_status(id, SSH_FX_FAILURE, "file exists with EXCL");
        }
        if truncate && want_write {
            let _ = self.fs.truncate(&path);
        }
        let handle_id = format!("h{}", self.next_handle);
        self.next_handle += 1;
        self.handles.insert(
            handle_id.clone(),
            Handle {
                path: canonical,
                kind: HandleKind::File {
                    read: want_read,
                    write: want_write,
                },
                dir_pending: None,
            },
        );
        encode_handle(id, &handle_id)
    }

    fn handle_close(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad close");
        };
        let Ok((handle, _)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad handle");
        };
        if self.handles.remove(&handle).is_some() {
            encode_status(id, SSH_FX_OK, "ok")
        } else {
            encode_status(id, SSH_FX_FAILURE, "unknown handle")
        }
    }

    fn handle_read(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad read");
        };
        let Ok((handle, rest)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad handle");
        };
        let Ok((offset, rest)) = crate::attrs::take_u64(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad offset");
        };
        let Ok((length, _)) = crate::attrs::take_u32(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad length");
        };
        let Some(entry) = self.handles.get_mut(&handle) else {
            return encode_status(id, SSH_FX_FAILURE, "unknown handle");
        };
        match &entry.kind {
            HandleKind::File { read: true, .. } => {
                let path = entry.path.clone();
                match self.fs.read(&path, offset, length as usize) {
                    Ok(data) => encode_data(id, &data),
                    Err(FsError::Eof) => encode_status(id, SSH_FX_EOF, "eof"),
                    Err(_) => encode_status(id, SSH_FX_FAILURE, "read failed"),
                }
            }
            _ => encode_status(id, SSH_FX_PERMISSION_DENIED, "not readable"),
        }
    }

    fn handle_write(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad write");
        };
        let Ok((handle, rest)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad handle");
        };
        let Ok((offset, rest)) = crate::attrs::take_u64(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad offset");
        };
        let Ok((length, rest)) = crate::attrs::take_u32(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad length");
        };
        let length = length as usize;
        if rest.len() < length {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "truncated data");
        }
        let data = &rest[..length];
        let Some(entry) = self.handles.get_mut(&handle) else {
            return encode_status(id, SSH_FX_FAILURE, "unknown handle");
        };
        match &entry.kind {
            HandleKind::File { write: true, .. } => {
                let path = entry.path.clone();
                match self.fs.write(&path, offset, data) {
                    Ok(()) => encode_status(id, SSH_FX_OK, "ok"),
                    Err(error) => encode_status(id, error.status_code(), "write failed"),
                }
            }
            _ => encode_status(id, SSH_FX_PERMISSION_DENIED, "not writable"),
        }
    }
    fn handle_lstat(&self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad lstat");
        };
        let Ok((path, _)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad path");
        };
        match self.fs.lstat(&path) {
            Ok(attrs) => encode_attrs_body(id, &attrs),
            Err(error) => encode_status(id, error.status_code(), "lstat failed"),
        }
    }

    fn handle_stat(&self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad stat");
        };
        let Ok((path, _)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad path");
        };
        match self.fs.stat(&path) {
            Ok(attrs) => encode_attrs_body(id, &attrs),
            Err(error) => encode_status(id, error.status_code(), "stat failed"),
        }
    }

    fn handle_fstat(&self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad fstat");
        };
        let Ok((handle, _)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad handle");
        };
        let Some(entry) = self.handles.get(&handle) else {
            return encode_status(id, SSH_FX_FAILURE, "unknown handle");
        };
        match self.fs.lstat(&entry.path) {
            Ok(attrs) => encode_attrs_body(id, &attrs),
            Err(error) => encode_status(id, error.status_code(), "fstat failed"),
        }
    }

    fn handle_setstat(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad setstat");
        };
        let Ok((path, rest)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad path");
        };
        let (attrs, _) = decode_attrs(rest).unwrap_or((FileAttrs::default(), &[]));
        match self.fs.set_attrs(&path, &attrs) {
            Ok(()) => encode_status(id, SSH_FX_OK, "ok"),
            Err(error) => encode_status(id, error.status_code(), "setstat failed"),
        }
    }

    fn handle_fsetstat(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad fsetstat");
        };
        let Ok((handle, rest)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad handle");
        };
        let (attrs, _) = decode_attrs(rest).unwrap_or((FileAttrs::default(), &[]));
        let Some(entry) = self.handles.get(&handle) else {
            return encode_status(id, SSH_FX_FAILURE, "unknown handle");
        };
        let path = entry.path.clone();
        match self.fs.set_attrs(&path, &attrs) {
            Ok(()) => encode_status(id, SSH_FX_OK, "ok"),
            Err(error) => encode_status(id, error.status_code(), "fsetstat failed"),
        }
    }

    fn handle_opendir(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad opendir");
        };
        let Ok((path, _)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad path");
        };
        let canonical = self.fs.canonicalize(&path);
        match self.fs.list_dir(&canonical) {
            Ok(entries) => {
                let handle_id = format!("d{}", self.next_handle);
                self.next_handle += 1;
                self.handles.insert(
                    handle_id.clone(),
                    Handle {
                        path: canonical,
                        kind: HandleKind::Dir,
                        dir_pending: Some(entries.into()),
                    },
                );
                encode_handle(id, &handle_id)
            }
            Err(error) => encode_status(id, error.status_code(), "opendir failed"),
        }
    }

    fn handle_readdir(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad readdir");
        };
        let Ok((handle, _)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad handle");
        };
        let Some(entry) = self.handles.get_mut(&handle) else {
            return encode_status(id, SSH_FX_FAILURE, "unknown handle");
        };
        let Some(pending) = &mut entry.dir_pending else {
            return encode_status(id, SSH_FX_FAILURE, "not a directory handle");
        };
        let batch: Vec<(String, String, FileAttrs)> =
            pending.drain(..pending.len().min(64)).collect();
        if batch.is_empty() {
            encode_status(id, SSH_FX_EOF, "eof")
        } else {
            encode_name_list(id, &batch)
        }
    }

    fn handle_remove(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad remove");
        };
        let Ok((path, _)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad path");
        };
        match self.fs.remove(&path) {
            Ok(()) => encode_status(id, SSH_FX_OK, "ok"),
            Err(error) => encode_status(id, error.status_code(), "remove failed"),
        }
    }

    fn handle_mkdir(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad mkdir");
        };
        let Ok((path, rest)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad path");
        };
        let (attrs, _) = decode_attrs(rest).unwrap_or((FileAttrs::default(), &[]));
        match self.fs.mkdir(&path, &attrs) {
            Ok(()) => encode_status(id, SSH_FX_OK, "ok"),
            Err(error) => encode_status(id, error.status_code(), "mkdir failed"),
        }
    }

    fn handle_rmdir(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad rmdir");
        };
        let Ok((path, _)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad path");
        };
        match self.fs.rmdir(&path) {
            Ok(()) => encode_status(id, SSH_FX_OK, "ok"),
            Err(error) => encode_status(id, error.status_code(), "rmdir failed"),
        }
    }

    fn handle_realpath(&self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad realpath");
        };
        let Ok((path, _)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad path");
        };
        let canonical = self.fs.canonicalize(&path);
        let attrs = self.fs.lstat(&canonical).unwrap_or_default();
        encode_name_one(id, &canonical, &canonical, &attrs)
    }

    fn handle_rename(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad rename");
        };
        let Ok((old_path, rest)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad old path");
        };
        let Ok((new_path, _)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad new path");
        };
        match self.fs.rename(&old_path, &new_path) {
            Ok(()) => encode_status(id, SSH_FX_OK, "ok"),
            Err(error) => encode_status(id, error.status_code(), "rename failed"),
        }
    }

    fn handle_readlink(&self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad readlink");
        };
        let Ok((path, _)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad path");
        };
        match self.fs.readlink(&path) {
            Ok(target) => {
                let attrs = self.fs.lstat(&path).unwrap_or_default();
                encode_name_one(id, &target, &target, &attrs)
            }
            Err(error) => encode_status(id, error.status_code(), "readlink failed"),
        }
    }

    fn handle_symlink(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad symlink");
        };
        let Ok((link_path, rest)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad link path");
        };
        let Ok((target, _)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad target");
        };
        match self.fs.symlink(&link_path, &target) {
            Ok(()) => encode_status(id, SSH_FX_OK, "ok"),
            Err(error) => encode_status(id, error.status_code(), "symlink failed"),
        }
    }

    fn handle_extended(&mut self, body: &[u8]) -> Vec<u8> {
        let Ok((id, rest)) = crate::attrs::take_u32(body) else {
            return encode_status(0, SSH_FX_BAD_MESSAGE, "bad extended");
        };
        let Ok((name, rest)) = take_string(rest) else {
            return encode_status(id, SSH_FX_BAD_MESSAGE, "bad extension name");
        };
        match name.as_str() {
            "posix-rename@openssh.com" => {
                let Ok((old_path, rest)) = take_string(rest) else {
                    return encode_status(id, SSH_FX_BAD_MESSAGE, "bad old path");
                };
                let Ok((new_path, _)) = take_string(rest) else {
                    return encode_status(id, SSH_FX_BAD_MESSAGE, "bad new path");
                };
                match self.fs.rename(&old_path, &new_path) {
                    Ok(()) => encode_status(id, SSH_FX_OK, "ok"),
                    Err(error) => encode_status(id, error.status_code(), "posix-rename failed"),
                }
            }
            _ => encode_status(id, SSH_FX_OP_UNSUPPORTED, "unsupported extension"),
        }
    }
}
