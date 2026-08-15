//! Real interactive SSH terminal sessions (mxterm parity / ssh 连接修复).
//!
//! Connects with russh, authenticates with the saved password or private key,
//! requests a PTY + interactive shell, and relays channel output into a
//! per-session queue that main.rs drains on a timer (terminal:output events).
//! Input typed in the UI is written back through the client handle.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use russh::client;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use serde_json::Value;

const DEFAULT_TERM: &str = "xterm-256color";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// A live SSH terminal session: the client handle (for writing), the channel
/// id, and the output queue pumped by the reader task.
struct SshSession {
    handle: client::Handle<AcceptAllHostKey>,
    channel_id: russh::ChannelId,
    rx: Receiver<Vec<u8>>,
    request_id: Option<String>,
    closed: bool,
}

static SESSIONS: Mutex<Option<HashMap<String, SshSession>>> = Mutex::new(None);

fn sessions_map() -> &'static Mutex<Option<HashMap<String, SshSession>>> {
    &SESSIONS
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("tokio runtime"))
}

fn new_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{nanos:x}")
}

/// Accepts all host keys (known-hosts trust is a separate row).
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

/// Opens an interactive SSH terminal session (terminal_connect).
pub fn open(
    profile: &Value,
    request_id: Option<String>,
    cols: u32,
    rows: u32,
) -> Result<String, String> {
    let host = profile
        .get("host")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let port = profile.get("port").and_then(Value::as_u64).unwrap_or(22) as u16;
    let username = profile
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let password = profile
        .get("password")
        .and_then(Value::as_str)
        .map(str::to_string);
    let private_key_path = profile
        .get("private_key_path")
        .and_then(Value::as_str)
        .map(str::to_string);
    let private_key_passphrase = profile
        .get("private_key_passphrase")
        .and_then(Value::as_str)
        .map(str::to_string);
    if host.is_empty() {
        return Err("缺少主机地址。".to_string());
    }
    if username.is_empty() {
        return Err("缺少用户名。".to_string());
    }
    if password.is_none() && private_key_path.is_none() {
        return Err("缺少认证凭据（密码或私钥）。".to_string());
    }

    let (tx_ready, rx_ready): (
        Sender<Result<(client::Handle<AcceptAllHostKey>, russh::ChannelId), String>>,
        Receiver<Result<(client::Handle<AcceptAllHostKey>, russh::ChannelId), String>>,
    ) = std::sync::mpsc::channel();
    let (tx_output, rx_output): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = std::sync::mpsc::channel();
    let id = new_id("ssh");

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("ssh terminal runtime");
        rt.block_on(run_session(
            host,
            port,
            username,
            password,
            private_key_path,
            private_key_passphrase,
            cols,
            rows,
            tx_ready,
            tx_output,
        ));
    });

    let (handle, channel_id) = rx_ready
        .recv_timeout(CONNECT_TIMEOUT)
        .map_err(|_| "SSH 连接超时。".to_string())??;
    sessions_map()
        .lock()
        .expect("ssh sessions lock")
        .get_or_insert_with(HashMap::new)
        .insert(
            id.clone(),
            SshSession {
                handle,
                channel_id,
                rx: rx_output,
                request_id,
                closed: false,
            },
        );
    Ok(id)
}

/// Connects, authenticates, opens a PTY shell, then relays output until EOF.
async fn run_session(
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    private_key_path: Option<String>,
    private_key_passphrase: Option<String>,
    cols: u32,
    rows: u32,
    tx_ready: Sender<Result<(client::Handle<AcceptAllHostKey>, russh::ChannelId), String>>,
    tx_output: Sender<Vec<u8>>,
) {
    let connect_result: Result<client::Handle<AcceptAllHostKey>, String> = async {
        let config = std::sync::Arc::new(client::Config::default());
        let mut session = client::connect(config, (host.as_str(), port), AcceptAllHostKey)
            .await
            .map_err(|e| format!("SSH 连接失败：{e}"))?;
        let authenticated = if let Some(password) = &password {
            session
                .authenticate_password(username.as_str(), password.as_str())
                .await
                .map_err(|e| format!("SSH 认证失败：{e}"))?
                .success()
        } else if let Some(key_path) = &private_key_path {
            let key = load_secret_key(key_path, private_key_passphrase.as_deref())
                .map_err(|e| format!("私钥加载失败：{e}"))?;
            let hash_alg = session
                .best_supported_rsa_hash()
                .await
                .map_err(|e| format!("SSH 认证失败：{e}"))?
                .flatten();
            session
                .authenticate_publickey(
                    username.clone(),
                    PrivateKeyWithHashAlg::new(std::sync::Arc::new(key), hash_alg),
                )
                .await
                .map_err(|e| format!("SSH 认证失败：{e}"))?
                .success()
        } else {
            false
        };
        if !authenticated {
            return Err("SSH 认证失败：凭据被拒绝。".to_string());
        }
        Ok(session)
    }
    .await;
    let session = match connect_result {
        Ok(session) => session,
        Err(e) => {
            let _ = tx_ready.send(Err(e));
            return;
        }
    };
    let mut channel = match session.channel_open_session().await {
        Ok(channel) => channel,
        Err(e) => {
            let _ = tx_ready.send(Err(format!("SSH 通道打开失败：{e}")));
            return;
        }
    };
    if let Err(e) = channel
        .request_pty(true, DEFAULT_TERM, cols, rows, 0, 0, &[])
        .await
    {
        let _ = tx_ready.send(Err(format!("SSH PTY 请求失败：{e}")));
        return;
    }
    if let Err(e) = channel.request_shell(true).await {
        let _ = tx_ready.send(Err(format!("SSH shell 请求失败：{e}")));
        return;
    }
    let channel_id = channel.id();
    let _ = tx_ready.send(Ok((session, channel_id)));
    // Reader loop: relay channel data into the output queue until EOF/close.
    loop {
        match channel.wait().await {
            Some(russh::ChannelMsg::Data { data }) => {
                if tx_output.send(data.to_vec()).is_err() {
                    break;
                }
            }
            Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                if tx_output.send(data.to_vec()).is_err() {
                    break;
                }
            }
            Some(russh::ChannelMsg::Eof) | None => break,
            _ => {}
        }
    }
}

/// Writes bytes to the SSH channel stdin (terminal_write).
pub fn write(session_id: &str, data: &[u8]) -> Result<(), String> {
    let guard = sessions_map().lock().expect("ssh sessions lock");
    let session = guard
        .as_ref()
        .and_then(|m| m.get(session_id))
        .ok_or_else(|| "SSH 会话不存在。".to_string())?;
    if session.closed {
        return Err("SSH 会话已关闭。".to_string());
    }
    let bytes = data.to_vec();
    let channel_id = session.channel_id;
    runtime().block_on(async move {
        session
            .handle
            .data(channel_id, bytes)
            .await
            .map_err(|_| "SSH 写入失败。".to_string())
    })
}

/// Drains all pending output chunks from an SSH session (WM_TIMER poll).
pub fn drain_output(session_id: &str) -> Vec<Vec<u8>> {
    let mut guard = sessions_map().lock().expect("ssh sessions lock");
    let Some(session) = guard.as_mut().and_then(|m| m.get_mut(session_id)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    loop {
        match session.rx.try_recv() {
            Ok(chunk) => out.push(chunk),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                session.closed = true;
                break;
            }
            Err(_) => break,
        }
    }
    out
}

/// Returns (request_id, closed) for an SSH session.
pub fn session_info(session_id: &str) -> Option<(Option<String>, bool)> {
    let guard = sessions_map().lock().expect("ssh sessions lock");
    guard
        .as_ref()
        .and_then(|m| m.get(session_id))
        .map(|s| (s.request_id.clone(), s.closed))
}

/// Lists active SSH session ids (for the WM_TIMER output drain).
pub fn active_session_ids() -> Vec<String> {
    let guard = sessions_map().lock().expect("ssh sessions lock");
    guard
        .as_ref()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// Closes an SSH session (removes the registry entry; dropping the handle
/// sends the SSH disconnect).
pub fn close(session_id: &str) -> bool {
    let mut guard = sessions_map().lock().expect("ssh sessions lock");
    guard.as_mut().and_then(|m| m.remove(session_id)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_rejects_missing_credentials() {
        let profile = serde_json::json!({
            "host": "example.invalid",
            "port": 22,
            "username": "root",
        });
        let err = open(&profile, Some("req-1".to_string()), 80, 24).expect_err("should fail");
        assert!(
            err.contains("认证凭据"),
            "expected credential error, got {err:?}"
        );
    }

    #[test]
    fn open_rejects_empty_host_or_user() {
        let profile = serde_json::json!({
            "host": "",
            "port": 22,
            "username": "root",
            "password": "x",
        });
        let err = open(&profile, None, 80, 24).expect_err("should fail");
        assert!(err.contains("主机地址"), "got {err:?}");

        let profile = serde_json::json!({
            "host": "example.invalid",
            "port": 22,
            "username": "",
            "password": "x",
        });
        let err = open(&profile, None, 80, 24).expect_err("should fail");
        assert!(err.contains("用户名"), "got {err:?}");
    }

    #[test]
    fn open_unreachable_host_fails_gracefully() {
        // 127.0.0.1:1 should refuse/never accept SSH, exercising the connect
        // error path without depending on external DNS or credentials.
        let profile = serde_json::json!({
            "host": "127.0.0.1",
            "port": 1,
            "username": "root",
            "password": "x",
        });
        let err = open(&profile, Some("req-2".to_string()), 80, 24).expect_err("should fail");
        assert!(
            err.contains("SSH 连接失败") || err.contains("连接超时"),
            "expected connection failure, got {err:?}"
        );
        assert!(active_session_ids().is_empty());
    }
}
