//! SFTP v3 wire codec (T056): packet framing, message types, and typed
//! request / response encoders and decoders.

use crate::attrs::{decode_attrs, encode_attrs, take_u32, FileAttrs};
use crate::SftpError;

/// A directory/name entry: `(filename, longname, attrs)`.
pub type NameEntry = (String, String, FileAttrs);

/// SFTP protocol version implemented (v3, matching OpenSSH).
pub const SFTP_VERSION: u32 = 3;
/// Maximum packet payload we accept (defensive bound).
pub const MAX_PACKET_LEN: usize = 256 * 1024;

// Client -> server.
pub const SSH_FXP_INIT: u8 = 1;
pub const SSH_FXP_OPEN: u8 = 3;
pub const SSH_FXP_CLOSE: u8 = 4;
pub const SSH_FXP_READ: u8 = 5;
pub const SSH_FXP_WRITE: u8 = 6;
pub const SSH_FXP_LSTAT: u8 = 7;
pub const SSH_FXP_FSTAT: u8 = 8;
pub const SSH_FXP_SETSTAT: u8 = 9;
pub const SSH_FXP_FSETSTAT: u8 = 10;
pub const SSH_FXP_OPENDIR: u8 = 11;
pub const SSH_FXP_READDIR: u8 = 12;
pub const SSH_FXP_REMOVE: u8 = 13;
pub const SSH_FXP_MKDIR: u8 = 14;
pub const SSH_FXP_RMDIR: u8 = 15;
pub const SSH_FXP_REALPATH: u8 = 16;
pub const SSH_FXP_STAT: u8 = 17;
pub const SSH_FXP_RENAME: u8 = 18;
pub const SSH_FXP_READLINK: u8 = 19;
pub const SSH_FXP_SYMLINK: u8 = 20;
pub const SSH_FXP_EXTENDED: u8 = 200;

// Server -> client.
pub const SSH_FXP_VERSION: u8 = 2;
pub const SSH_FXP_STATUS: u8 = 101;
pub const SSH_FXP_HANDLE: u8 = 102;
pub const SSH_FXP_DATA: u8 = 103;
pub const SSH_FXP_NAME: u8 = 104;
pub const SSH_FXP_ATTRS: u8 = 105;
pub const SSH_FXP_EXTENDED_REPLY: u8 = 201;

// Open flags (SSH_FXF_*).
pub const SSH_FXF_READ: u32 = 0x0000_0001;
pub const SSH_FXF_WRITE: u32 = 0x0000_0002;
pub const SSH_FXF_APPEND: u32 = 0x0000_0004;
pub const SSH_FXF_CREAT: u32 = 0x0000_0008;
pub const SSH_FXF_TRUNC: u32 = 0x0000_0010;
pub const SSH_FXF_EXCL: u32 = 0x0000_0020;

// Status codes.
pub const SSH_FX_OK: u32 = 0;
pub const SSH_FX_EOF: u32 = 1;
pub const SSH_FX_NO_SUCH_FILE: u32 = 2;
pub const SSH_FX_PERMISSION_DENIED: u32 = 3;
pub const SSH_FX_FAILURE: u32 = 4;
pub const SSH_FX_BAD_MESSAGE: u32 = 5;
pub const SSH_FX_NO_CONNECTION: u32 = 6;
pub const SSH_FX_CONNECTION_LOST: u32 = 7;
pub const SSH_FX_OP_UNSUPPORTED: u32 = 8;

/// Frames a packet: `uint32 length` + payload (type byte is the first payload
/// byte).
pub fn frame_packet(payload: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(4 + payload.len());
    framed.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    framed.extend_from_slice(payload);
    framed
}

/// Parses one packet from a buffer: `(type, body, consumed)`.
pub fn parse_packet(buffer: &[u8]) -> Result<Option<(u8, Vec<u8>, usize)>, SftpError> {
    if buffer.len() < 4 {
        return Ok(None);
    }
    let length = u32::from_be_bytes(buffer[..4].try_into().expect("4 bytes")) as usize;
    if length > MAX_PACKET_LEN {
        return Err(SftpError::protocol("packet too large"));
    }
    if buffer.len() < 4 + length {
        return Ok(None);
    }
    if length == 0 {
        return Err(SftpError::protocol("empty packet"));
    }
    let payload = &buffer[4..4 + length];
    Ok(Some((payload[0], payload[1..].to_vec(), 4 + length)))
}

pub(super) fn push_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

pub(super) fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn take_string(bytes: &[u8]) -> Result<(String, &[u8]), SftpError> {
    let (length, rest) = take_u32(bytes)?;
    let length = length as usize;
    if rest.len() < length {
        return Err(SftpError::protocol("truncated string"));
    }
    let value = std::str::from_utf8(&rest[..length])
        .map_err(|_| SftpError::protocol("string is not UTF-8"))?
        .to_owned();
    Ok((value, &rest[length..]))
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// `SSH_FXP_INIT` with a version.
pub fn encode_init(version: u32) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_INIT];
    push_u32(&mut bytes, version);
    bytes
}

/// `SSH_FXP_OPEN`.
pub fn encode_open(id: u32, filename: &str, pflags: u32, attrs: &FileAttrs) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_OPEN];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, filename);
    push_u32(&mut bytes, pflags);
    encode_attrs(&mut bytes, attrs);
    bytes
}

/// `SSH_FXP_CLOSE`.
pub fn encode_close(id: u32, handle: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_CLOSE];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, handle);
    bytes
}

/// `SSH_FXP_READ`.
pub fn encode_read(id: u32, handle: &str, offset: u64, length: u32) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_READ];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, handle);
    push_u64(&mut bytes, offset);
    push_u32(&mut bytes, length);
    bytes
}

/// `SSH_FXP_WRITE`.
pub fn encode_write(id: u32, handle: &str, offset: u64, data: &[u8]) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_WRITE];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, handle);
    push_u64(&mut bytes, offset);
    push_u32(&mut bytes, data.len() as u32);
    bytes.extend_from_slice(data);
    bytes
}

/// `SSH_FXP_LSTAT`.
pub fn encode_lstat(id: u32, path: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_LSTAT];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, path);
    bytes
}

/// `SSH_FXP_FSTAT`.
pub fn encode_fstat(id: u32, handle: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_FSTAT];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, handle);
    bytes
}

/// `SSH_FXP_SETSTAT`.
pub fn encode_setstat(id: u32, path: &str, attrs: &FileAttrs) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_SETSTAT];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, path);
    encode_attrs(&mut bytes, attrs);
    bytes
}

/// `SSH_FXP_FSETSTAT`.
pub fn encode_fsetstat(id: u32, handle: &str, attrs: &FileAttrs) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_FSETSTAT];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, handle);
    encode_attrs(&mut bytes, attrs);
    bytes
}

/// `SSH_FXP_OPENDIR`.
pub fn encode_opendir(id: u32, path: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_OPENDIR];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, path);
    bytes
}

/// `SSH_FXP_READDIR`.
pub fn encode_readdir(id: u32, handle: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_READDIR];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, handle);
    bytes
}

/// `SSH_FXP_REMOVE`.
pub fn encode_remove(id: u32, path: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_REMOVE];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, path);
    bytes
}

/// `SSH_FXP_MKDIR`.
pub fn encode_mkdir(id: u32, path: &str, attrs: &FileAttrs) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_MKDIR];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, path);
    encode_attrs(&mut bytes, attrs);
    bytes
}

/// `SSH_FXP_RMDIR`.
pub fn encode_rmdir(id: u32, path: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_RMDIR];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, path);
    bytes
}

/// `SSH_FXP_REALPATH`.
pub fn encode_realpath(id: u32, path: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_REALPATH];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, path);
    bytes
}

/// `SSH_FXP_STAT`.
pub fn encode_stat(id: u32, path: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_STAT];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, path);
    bytes
}

/// `SSH_FXP_RENAME`.
pub fn encode_rename(id: u32, old_path: &str, new_path: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_RENAME];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, old_path);
    push_string(&mut bytes, new_path);
    bytes
}

/// `SSH_FXP_READLINK`.
pub fn encode_readlink(id: u32, path: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_READLINK];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, path);
    bytes
}

/// `SSH_FXP_SYMLINK` (linkpath first, then target, per v3).
pub fn encode_symlink(id: u32, link_path: &str, target: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_SYMLINK];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, link_path);
    push_string(&mut bytes, target);
    bytes
}

/// `SSH_FXP_EXTENDED`.
pub fn encode_extended(id: u32, name: &str, data: &[u8]) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_EXTENDED];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, name);
    bytes.extend_from_slice(data);
    bytes
}

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// Decodes `SSH_FXP_STATUS` -> `(code, message, language)`.
pub fn decode_status(body: &[u8]) -> Result<(u32, String, String), SftpError> {
    let (id, rest) = take_u32(body)?;
    let _ = id; // echoed request id
    let (code, rest) = take_u32(rest)?;
    let (message, rest) = take_string(rest)?;
    let (language, _) = take_string(rest)?;
    Ok((code, message, language))
}

/// Decodes `SSH_FXP_HANDLE`.
pub fn decode_handle(body: &[u8]) -> Result<String, SftpError> {
    let (id, rest) = take_u32(body)?;
    let _ = id; // echoed request id
    let (handle, rest) = take_string(rest)?;
    if !rest.is_empty() {
        return Err(SftpError::protocol("trailing bytes in handle"));
    }
    Ok(handle)
}

/// Decodes `SSH_FXP_DATA`.
pub fn decode_data(body: &[u8]) -> Result<Vec<u8>, SftpError> {
    let (id, rest) = take_u32(body)?;
    let _ = id; // echoed request id
    let (length, rest) = take_u32(rest)?;
    let length = length as usize;
    if rest.len() < length {
        return Err(SftpError::protocol("truncated data"));
    }
    if rest.len() != length {
        return Err(SftpError::protocol("trailing bytes in data"));
    }
    Ok(rest[..length].to_vec())
}

/// Decodes a `SSH_FXP_NAME` with exactly one entry: `(filename, longname, attrs)`.
pub fn decode_name(body: &[u8]) -> Result<NameEntry, SftpError> {
    let (id, rest) = take_u32(body)?;
    let _ = id; // echoed request id
    let (count, rest) = take_u32(rest)?;
    if count != 1 {
        return Err(SftpError::protocol(format!(
            "expected 1 name entry, got {count}"
        )));
    }
    decode_name_entry(rest).map(|(entry, _)| entry)
}

/// Decodes a `SSH_FXP_NAME` list into `(filename, longname, attrs)` entries.
pub fn decode_name_list(body: &[u8]) -> Result<Vec<NameEntry>, SftpError> {
    let (id, body) = take_u32(body)?;
    let _ = id; // echoed request id
    let (count, mut rest) = take_u32(body)?;
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (entry, remaining) = decode_name_entry(rest)?;
        entries.push(entry);
        rest = remaining;
    }
    Ok(entries)
}

fn decode_name_entry(bytes: &[u8]) -> Result<(NameEntry, &[u8]), SftpError> {
    let (filename, rest) = take_string(bytes)?;
    let (longname, rest) = take_string(rest)?;
    let (attrs, rest) = decode_attrs(rest)?;
    Ok(((filename, longname, attrs), rest))
}

/// Decodes `SSH_FXP_ATTRS`.
pub fn decode_attrs_body(body: &[u8]) -> Result<FileAttrs, SftpError> {
    let (id, rest) = take_u32(body)?;
    let _ = id; // echoed request id
    let (attrs, rest) = decode_attrs(rest)?;
    if !rest.is_empty() {
        return Err(SftpError::protocol("trailing bytes in attrs"));
    }
    Ok(attrs)
}

/// Encodes `SSH_FXP_ATTRS` with the request id.
pub fn encode_attrs_body(id: u32, attrs: &FileAttrs) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_ATTRS];
    bytes.extend_from_slice(&id.to_be_bytes());
    encode_attrs(&mut bytes, attrs);
    bytes
}

/// Encodes `SSH_FXP_STATUS` with the request id.
pub fn encode_status(id: u32, code: u32, message: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_STATUS];
    push_u32(&mut bytes, id);
    push_u32(&mut bytes, code);
    push_string(&mut bytes, message);
    push_string(&mut bytes, "");
    bytes
}

/// Encodes `SSH_FXP_HANDLE`.
pub fn encode_handle(id: u32, handle: &str) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_HANDLE];
    push_u32(&mut bytes, id);
    push_string(&mut bytes, handle);
    bytes
}

/// Encodes `SSH_FXP_DATA`.
pub fn encode_data(id: u32, data: &[u8]) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_DATA];
    push_u32(&mut bytes, id);
    push_u32(&mut bytes, data.len() as u32);
    bytes.extend_from_slice(data);
    bytes
}

/// Encodes `SSH_FXP_NAME` with one entry (realpath / readlink).
pub fn encode_name_one(id: u32, filename: &str, longname: &str, attrs: &FileAttrs) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_NAME];
    push_u32(&mut bytes, id);
    push_u32(&mut bytes, 1);
    encode_name_entry(&mut bytes, filename, longname, attrs);
    bytes
}

/// Encodes `SSH_FXP_NAME` with a list of entries.
pub fn encode_name_list(id: u32, entries: &[(String, String, FileAttrs)]) -> Vec<u8> {
    let mut bytes = vec![SSH_FXP_NAME];
    push_u32(&mut bytes, id);
    push_u32(&mut bytes, entries.len() as u32);
    for (filename, longname, attrs) in entries {
        encode_name_entry(&mut bytes, filename, longname, attrs);
    }
    bytes
}

fn encode_name_entry(bytes: &mut Vec<u8>, filename: &str, longname: &str, attrs: &FileAttrs) {
    push_string(bytes, filename);
    push_string(bytes, longname);
    encode_attrs(bytes, attrs);
}
