//! Real SSH + SFTP client for remote file browsing (mxterm parity T004).
//!
//! Uses russh (SSH transport) + russh-sftp (SFTP v3), the same stack as
//! mXterm. Implements remote_file_list/metadata/read/write/delete/rename/
//! create_file/create_directory/check_path/check_download_target.

use std::sync::Arc;

use russh::client;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh_sftp::client::{Config as SftpConfig, SftpSession};
use russh_sftp::protocol::{FileAttributes, Status, StatusCode};

/// SSH connection parameters resolved from a connection profile.
#[derive(Debug, Clone)]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub private_key_path: Option<String>,
    pub private_key_passphrase: Option<String>,
}

impl SshTarget {
    /// Builds a target from a request JSON.
    pub fn from_request(request: &serde_json::Value) -> Self {
        let str_field = |key: &str| {
            request
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(|s| s.to_string())
        };
        let opt_field = |key: &str| request.get(key).and_then(serde_json::Value::as_str);
        Self {
            host: str_field("host").unwrap_or_default(),
            port: request
                .get("port")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(22) as u16,
            username: str_field("username").unwrap_or_default(),
            password: opt_field("password").map(|s| s.to_string()),
            private_key_path: opt_field("private_key_path").map(|s| s.to_string()),
            private_key_passphrase: opt_field("private_key_passphrase").map(|s| s.to_string()),
        }
    }
}

/// Accepts all host keys for now (known-hosts trust is a later row).
#[derive(Clone)]
struct AcceptAllHostKey;

impl client::Handler for AcceptAllHostKey {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Connects, authenticates, and opens an SFTP session.
async fn connect_sftp(target: &SshTarget) -> Result<SftpSession, String> {
    if target.host.is_empty() {
        return Err("缺少主机地址。".to_string());
    }
    let config = Arc::new(client::Config::default());
    let mut session = client::connect(
        config,
        (target.host.as_str(), target.port),
        AcceptAllHostKey,
    )
    .await
    .map_err(|e| format!("SSH 连接失败：{e}"))?;

    let authenticated = if let Some(password) = &target.password {
        session
            .authenticate_password(target.username.as_str(), password.as_str())
            .await
            .map_err(|e| format!("SSH 认证失败：{e}"))?
            .success()
    } else if let Some(key_path) = &target.private_key_path {
        let key = load_secret_key(key_path, target.private_key_passphrase.as_deref())
            .map_err(|e| format!("私钥加载失败：{e}"))?;
        let hash_alg = session
            .best_supported_rsa_hash()
            .await
            .map_err(|e| format!("SSH 认证失败：{e}"))?
            .flatten();
        session
            .authenticate_publickey(
                target.username.clone(),
                PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
            )
            .await
            .map_err(|e| format!("SSH 认证失败：{e}"))?
            .success()
    } else {
        return Err("缺少认证凭据（密码或私钥）。".to_string());
    };
    if !authenticated {
        return Err("SSH 认证失败：凭据被拒绝。".to_string());
    }

    let channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("SSH 通道打开失败：{e}"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("SFTP 子系统请求失败：{e}"))?;
    let sftp = SftpSession::new_with_config(channel.into_stream(), SftpConfig::default())
        .await
        .map_err(|e| format!("SFTP 会话初始化失败：{e}"))?;
    Ok(sftp)
}

fn attr_kind(attrs: &FileAttributes) -> &'static str {
    if attrs.is_dir() {
        "directory"
    } else if attrs.is_symlink() {
        "symlink"
    } else if attrs.is_regular() {
        "file"
    } else {
        "other"
    }
}

fn entry_json(name: &str, path: &str, attrs: &FileAttributes) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "path": path,
        "type": attr_kind(attrs),
    })
}

fn metadata_json(name: &str, path: &str, attrs: &FileAttributes) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "path": path,
        "size": attrs.size.unwrap_or(0),
        "mtime": attrs.mtime.unwrap_or(0) as u64,
        "mode": attrs.permissions.map(|p| format!("{:o}", p)),
    })
}

/// Lists a remote directory (remote_file_list).
pub async fn list_dir(target: &SshTarget, path: &str) -> Result<serde_json::Value, String> {
    let sftp = connect_sftp(target).await?;
    let path = if path.is_empty() { "." } else { path };
    let read_dir = sftp
        .read_dir(path)
        .await
        .map_err(|e| format!("列目录失败：{e}"))?;
    let mut items: Vec<serde_json::Value> = Vec::new();
    for entry in read_dir {
        let name = entry.file_name();
        let attrs = entry.metadata();
        let joined = if path == "." {
            name.clone()
        } else {
            format!("{}/{}", path.trim_end_matches('/'), name)
        };
        let mut item = entry_json(&name, &joined, &attrs);
        // DirEntry metadata may carry size/mtime too; include them.
        item["size"] = serde_json::json!(attrs.size.unwrap_or(0));
        item["mtime"] = serde_json::json!(attrs.mtime.unwrap_or(0));
        items.push(item);
    }
    Ok(serde_json::json!(items))
}

/// Reads file metadata (remote_file_metadata).
pub async fn metadata(target: &SshTarget, path: &str) -> Result<serde_json::Value, String> {
    let sftp = connect_sftp(target).await?;
    let attrs = sftp
        .metadata(path)
        .await
        .map_err(|e| format!("读取元数据失败：{e}"))?;
    let name = path.rsplit('/').next().unwrap_or(path);
    Ok(metadata_json(name, path, &attrs))
}

/// Reads a small file as UTF-8 (remote_file_read).
pub async fn read_file(target: &SshTarget, path: &str) -> Result<serde_json::Value, String> {
    let sftp = connect_sftp(target).await?;
    let content = sftp
        .read(path)
        .await
        .map_err(|e| format!("读取文件失败：{e}"))?;
    let text = String::from_utf8_lossy(&content).to_string();
    let attrs = sftp
        .metadata(path)
        .await
        .unwrap_or_else(|_| FileAttributes::default());
    let name = path.rsplit('/').next().unwrap_or(path);
    Ok(serde_json::json!({
        "content": text,
        "editable": true,
        "encoding": "utf-8",
        "is_binary": content.contains(&0),
        "metadata": metadata_json(name, path, &attrs),
        "mode": attrs.permissions.map(|p| format!("{:o}", p)),
        "mtime": attrs.mtime.unwrap_or(0) as u64,
        "name": name,
        "path": path,
        "size": attrs.size.unwrap_or(0),
    }))
}

/// Writes a small file (remote_file_write).
pub async fn write_file(
    target: &SshTarget,
    path: &str,
    content: &str,
    expected_mtime: Option<u64>,
    expected_size: Option<u64>,
    overwrite: bool,
) -> Result<serde_json::Value, String> {
    let sftp = connect_sftp(target).await?;
    if !overwrite {
        if let Ok(attrs) = sftp.metadata(path).await {
            let conflict = expected_size
                .map(|s| attrs.size.unwrap_or(0) != s)
                .unwrap_or(false)
                || expected_mtime
                    .map(|m| attrs.mtime.unwrap_or(0) as u64 != m)
                    .unwrap_or(false);
            if conflict {
                return Ok(serde_json::json!({
                    "conflict": true,
                    "metadata": metadata_json(
                        path.rsplit('/').next().unwrap_or(path),
                        path,
                        &attrs,
                    ),
                }));
            }
        }
    }
    sftp.write(path, content.as_bytes())
        .await
        .map_err(|e| format!("写入文件失败：{e}"))?;
    let attrs = sftp
        .metadata(path)
        .await
        .unwrap_or_else(|_| FileAttributes::default());
    Ok(serde_json::json!({
        "conflict": false,
        "metadata": metadata_json(path.rsplit('/').next().unwrap_or(path), path, &attrs),
    }))
}

/// Deletes a file or (recursive) directory (remote_file_delete). Recursion is
/// iterative over an explicit stack to keep the async fn non-recursive.
pub async fn delete(
    target: &SshTarget,
    path: &str,
    recursive: bool,
) -> Result<serde_json::Value, String> {
    let mut stack = vec![path.to_string()];
    while let Some(current) = stack.pop() {
        let sftp = connect_sftp(target).await?;
        let attrs = sftp
            .metadata(&current)
            .await
            .map_err(|e| format!("读取元数据失败：{e}"))?;
        if attrs.is_dir() && recursive {
            let read_dir = sftp
                .read_dir(&current)
                .await
                .map_err(|e| format!("列目录失败：{e}"))?;
            let children: Vec<String> = read_dir
                .into_iter()
                .map(|entry| format!("{}/{}", current.trim_end_matches('/'), entry.file_name()))
                .collect();
            if children.is_empty() {
                sftp.remove_dir(&current)
                    .await
                    .map_err(|e| format!("删除目录失败：{e}"))?;
            } else {
                // Remove this directory after its children.
                stack.push(current.clone());
                stack.extend(children);
            }
        } else if attrs.is_dir() {
            sftp.remove_dir(&current)
                .await
                .map_err(|e| format!("删除目录失败：{e}"))?;
        } else {
            sftp.remove_file(&current)
                .await
                .map_err(|e| format!("删除文件失败：{e}"))?;
        }
    }
    Ok(serde_json::Value::Null)
}

/// Renames a remote path (remote_file_rename).
pub async fn rename(
    target: &SshTarget,
    old_path: &str,
    new_path: &str,
) -> Result<serde_json::Value, String> {
    let sftp = connect_sftp(target).await?;
    sftp.rename(old_path, new_path)
        .await
        .map_err(|e| format!("重命名失败：{e}"))?;
    Ok(serde_json::Value::Null)
}

/// Creates a directory (remote_file_create_directory).
pub async fn create_directory(target: &SshTarget, path: &str) -> Result<serde_json::Value, String> {
    let sftp = connect_sftp(target).await?;
    sftp.create_dir(path)
        .await
        .map_err(|e| format!("创建目录失败：{e}"))?;
    Ok(serde_json::Value::Null)
}

/// Creates an empty file (remote_file_create_file).
pub async fn create_file(target: &SshTarget, path: &str) -> Result<serde_json::Value, String> {
    let sftp = connect_sftp(target).await?;
    sftp.write(path, b"")
        .await
        .map_err(|e| format!("创建文件失败：{e}"))?;
    Ok(serde_json::Value::Null)
}

/// Checks a remote path (remote_file_check_path).
pub async fn check_path(target: &SshTarget, path: &str) -> Result<serde_json::Value, String> {
    let sftp = connect_sftp(target).await?;
    match sftp.metadata(path).await {
        Ok(attrs) => Ok(serde_json::json!({
            "exists": true,
            "path": path,
            "type": attr_kind(&attrs),
        })),
        Err(error) if is_not_found(&error) => {
            Ok(serde_json::json!({ "exists": false, "path": path, "type": null }))
        }
        Err(e) => Err(format!("路径检查失败：{e}")),
    }
}

/// Checks a download target path (remote_file_check_download_target).
pub async fn check_download_target(
    target: &SshTarget,
    path: &str,
) -> Result<serde_json::Value, String> {
    check_path(target, path).await
}

fn is_not_found(error: &russh_sftp::client::error::Error) -> bool {
    matches!(
        error,
        russh_sftp::client::error::Error::Status(Status {
            status_code: StatusCode::NoSuchFile,
            ..
        })
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_from_request_reads_fields() {
        let req = serde_json::json!({
            "host": "10.0.0.1", "port": 2222, "username": "root",
            "password": "secret", "private_key_path": "/tmp/key"
        });
        let t = SshTarget::from_request(&req);
        assert_eq!(t.host, "10.0.0.1");
        assert_eq!(t.port, 2222);
        assert_eq!(t.password.as_deref(), Some("secret"));
        assert_eq!(t.private_key_path.as_deref(), Some("/tmp/key"));
    }

    #[test]
    fn empty_host_fails() {
        let req = serde_json::json!({ "host": "", "port": 22 });
        let t = SshTarget::from_request(&req);
        let rt = tokio::runtime::Runtime::new().expect("rt");
        let result = rt.block_on(connect_sftp(&t));
        assert!(result.is_err());
    }
}
