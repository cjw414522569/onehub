//! SFTP v3 file attributes and permission helpers (T056).

/// File attribute flags (SFTP v3).
pub const ATTR_SIZE: u32 = 0x0000_0001;
pub const ATTR_UIDGID: u32 = 0x0000_0002;
pub const ATTR_PERMISSIONS: u32 = 0x0000_0004;
pub const ATTR_ACMODTIME: u32 = 0x0000_0008;

/// POSIX file type bits.
pub const S_IFMT: u32 = 0o170000;
pub const S_IFSOCK: u32 = 0o140000;
pub const S_IFLNK: u32 = 0o120000;
pub const S_IFREG: u32 = 0o100000;
pub const S_IFBLK: u32 = 0o060000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFCHR: u32 = 0o020000;

/// SFTP v3 file attributes (only fields the peer sets are present).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileAttrs {
    /// File size in bytes.
    pub size: Option<u64>,
    /// Owner uid.
    pub uid: Option<u32>,
    /// Owner gid.
    pub gid: Option<u32>,
    /// Permission + type bits (`st_mode`).
    pub permissions: Option<u32>,
    /// Access time (unix seconds).
    pub atime: Option<u32>,
    /// Modification time (unix seconds).
    pub mtime: Option<u32>,
}

impl FileAttrs {
    /// A regular-file attribute set with the given size and mode.
    pub fn file(size: u64, mode: u32) -> Self {
        Self {
            size: Some(size),
            permissions: Some(S_IFREG | mode),
            ..Self::default()
        }
    }

    /// A directory attribute set with the given mode.
    pub fn directory(mode: u32) -> Self {
        Self {
            permissions: Some(S_IFDIR | mode),
            ..Self::default()
        }
    }

    /// Whether the entry is a directory.
    pub fn is_dir(&self) -> bool {
        self.permissions
            .is_some_and(|mode| mode & S_IFMT == S_IFDIR)
    }

    /// Whether the entry is a symbolic link.
    pub fn is_symlink(&self) -> bool {
        self.permissions
            .is_some_and(|mode| mode & S_IFMT == S_IFLNK)
    }

    /// Whether the entry is a regular file.
    pub fn is_regular(&self) -> bool {
        self.permissions
            .is_some_and(|mode| mode & S_IFMT == S_IFREG)
    }

    /// The permission bits without the file type.
    pub fn permission_bits(&self) -> u32 {
        self.permissions.unwrap_or(0) & 0o7777
    }

    /// `drwxr-xr-x`-style mode string (file type char + rwx triplets).
    pub fn mode_string(&self) -> String {
        let mode = self.permissions.unwrap_or(0);
        let type_char = if mode & S_IFMT == S_IFDIR {
            'd'
        } else if mode & S_IFMT == S_IFLNK {
            'l'
        } else if mode & S_IFMT == S_IFREG {
            '-'
        } else {
            '?'
        };
        let mut out = String::with_capacity(10);
        out.push(type_char);
        for shift in [6, 3, 0] {
            let bits = (mode >> shift) & 0o7;
            out.push(if bits & 0o4 != 0 { 'r' } else { '-' });
            out.push(if bits & 0o2 != 0 { 'w' } else { '-' });
            out.push(if bits & 0o1 != 0 { 'x' } else { '-' });
        }
        out
    }
}

/// Encodes the attribute flags + values.
pub fn encode_attrs(bytes: &mut Vec<u8>, attrs: &FileAttrs) {
    let mut flags = 0u32;
    if attrs.size.is_some() {
        flags |= ATTR_SIZE;
    }
    if attrs.uid.is_some() || attrs.gid.is_some() {
        flags |= ATTR_UIDGID;
    }
    if attrs.permissions.is_some() {
        flags |= ATTR_PERMISSIONS;
    }
    if attrs.atime.is_some() || attrs.mtime.is_some() {
        flags |= ATTR_ACMODTIME;
    }
    bytes.extend_from_slice(&flags.to_be_bytes());
    if let Some(size) = attrs.size {
        bytes.extend_from_slice(&size.to_be_bytes());
    }
    if let Some(uid) = attrs.uid {
        bytes.extend_from_slice(&uid.to_be_bytes());
    }
    if let Some(gid) = attrs.gid {
        bytes.extend_from_slice(&gid.to_be_bytes());
    }
    if let Some(permissions) = attrs.permissions {
        bytes.extend_from_slice(&permissions.to_be_bytes());
    }
    if let Some(atime) = attrs.atime {
        bytes.extend_from_slice(&atime.to_be_bytes());
    }
    if let Some(mtime) = attrs.mtime {
        bytes.extend_from_slice(&mtime.to_be_bytes());
    }
}

/// Decodes attribute flags + values; returns the decoded attrs and the rest.
pub fn decode_attrs(bytes: &[u8]) -> Result<(FileAttrs, &[u8]), super::SftpError> {
    let (flags, mut rest) = take_u32(bytes)?;
    let mut attrs = FileAttrs::default();
    if flags & ATTR_SIZE != 0 {
        let (value, remaining) = take_u64(rest)?;
        attrs.size = Some(value);
        rest = remaining;
    }
    if flags & ATTR_UIDGID != 0 {
        let (uid, remaining) = take_u32(rest)?;
        let (gid, remaining) = take_u32(remaining)?;
        attrs.uid = Some(uid);
        attrs.gid = Some(gid);
        rest = remaining;
    }
    if flags & ATTR_PERMISSIONS != 0 {
        let (permissions, remaining) = take_u32(rest)?;
        attrs.permissions = Some(permissions);
        rest = remaining;
    }
    if flags & ATTR_ACMODTIME != 0 {
        let (atime, remaining) = take_u32(rest)?;
        let (mtime, remaining) = take_u32(remaining)?;
        attrs.atime = Some(atime);
        attrs.mtime = Some(mtime);
        rest = remaining;
    }
    Ok((attrs, rest))
}

pub(super) fn take_u32(bytes: &[u8]) -> Result<(u32, &[u8]), super::SftpError> {
    if bytes.len() < 4 {
        return Err(super::SftpError::protocol("truncated u32"));
    }
    let value = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    Ok((value, &bytes[4..]))
}

pub(super) fn take_u64(bytes: &[u8]) -> Result<(u64, &[u8]), super::SftpError> {
    if bytes.len() < 8 {
        return Err(super::SftpError::protocol("truncated u64"));
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[..8]);
    Ok((u64::from_be_bytes(raw), &bytes[8..]))
}
