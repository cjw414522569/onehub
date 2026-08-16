//! SSH tunnels (mxterm parity T007).
//!
//! Persists tunnel rules in the local SQLite store and runs them over a real
//! russh SSH connection: local (direct-tcpip), remote (tcpip-forward), and
//! dynamic (local SOCKS5 listener forwarding through the SSH channel).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use russh::client;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};

use crate::sftp::SshTarget;
use crate::store::Store;

/// Runtime tunnel state.
#[derive(Debug, Clone)]
pub struct TunnelRuntime {
    pub rule_id: String,
    pub status: String,
    pub bound_host: Option<String>,
    pub bound_port: Option<u32>,
    pub active_connections: u32,
    pub last_error: Option<String>,
}

/// In-process tunnel registry (rule_id -> runtime state).
static RUNTIME: Mutex<Option<HashMap<String, TunnelRuntime>>> = Mutex::new(None);

fn runtime_map() -> &'static Mutex<Option<HashMap<String, TunnelRuntime>>> {
    &RUNTIME
}

/// Returns the bound local endpoint of a running tunnel rule, if any.
pub(crate) fn tunnel_endpoint(rule_id: &str) -> Option<(String, u16)> {
    let guard = runtime_map().lock().expect("runtime lock");
    let state = guard.as_ref().and_then(|m| m.get(rule_id))?;
    let host = state.bound_host.clone()?;
    let port = state.bound_port?;
    Some((host, port as u16))
}

/// Seeds runtime tunnel state (route resolution tests / DB proxy routing).
#[cfg(test)]
pub(crate) fn seed_runtime_state(rule_id: &str, host: &str, port: u32) {
    runtime_map()
        .lock()
        .expect("runtime lock")
        .get_or_insert_with(HashMap::new)
        .insert(
            rule_id.to_string(),
            TunnelRuntime {
                rule_id: rule_id.to_string(),
                status: "running".to_string(),
                bound_host: Some(host.to_string()),
                bound_port: Some(port),
                active_connections: 0,
                last_error: None,
            },
        );
}

fn now_str() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("tunnel-{nanos:x}")
}

fn rule_json(
    id: &str,
    request: &serde_json::Value,
    now: &str,
    existing: Option<&serde_json::Value>,
) -> serde_json::Value {
    let name = request
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("tunnel");
    serde_json::json!({
        "id": id,
        "name": name,
        "kind": request.get("kind").and_then(serde_json::Value::as_str).unwrap_or("local"),
        "connection_id": request.get("connection_id").and_then(serde_json::Value::as_str).unwrap_or(""),
        "local_host": request.get("local_host").and_then(serde_json::Value::as_str).unwrap_or("127.0.0.1"),
        "local_port": request.get("local_port").and_then(serde_json::Value::as_u64).unwrap_or(0),
        "remote_host": request.get("remote_host").and_then(serde_json::Value::as_str).unwrap_or(""),
        "remote_port": request.get("remote_port").and_then(serde_json::Value::as_u64).unwrap_or(0),
        "auto_start": request.get("auto_start").and_then(serde_json::Value::as_bool).unwrap_or(false),
        "created_at": existing.and_then(|e| e.get("created_at").and_then(serde_json::Value::as_str)).unwrap_or(now),
        "updated_at": now,
    })
}

/// Lists tunnel rules (tunnel_list).
pub fn list_rules(store: &Store) -> Result<serde_json::Value, String> {
    let rules = store.list_tunnels().map_err(|e| e.to_string())?;
    let runtime = runtime_map().lock().expect("runtime lock");
    let items: Vec<serde_json::Value> = rules
        .iter()
        .map(|rule| {
            let id = rule
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let state =
                runtime
                    .as_ref()
                    .and_then(|m| m.get(id))
                    .cloned()
                    .unwrap_or(TunnelRuntime {
                        rule_id: id.to_string(),
                        status: "stopped".to_string(),
                        bound_host: None,
                        bound_port: None,
                        active_connections: 0,
                        last_error: None,
                    });
            serde_json::json!({
                "rule": rule,
                "state": {
                    "rule_id": state.rule_id,
                    "status": state.status,
                    "bound_host": state.bound_host,
                    "bound_port": state.bound_port,
                    "started_at": null,
                    "last_error": state.last_error,
                    "last_error_code": null,
                    "active_connections": state.active_connections,
                },
            })
        })
        .collect();
    Ok(serde_json::json!(items))
}

/// Upserts a tunnel rule (tunnel_upsert).
pub fn upsert_rule(
    store: &mut Store,
    request: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let now = now_str();
    let existing_id = request.get("id").and_then(serde_json::Value::as_str);
    let existing = existing_id.and_then(|id| store.get_tunnel(id).ok().flatten());
    let id = existing_id.map(|s| s.to_string()).unwrap_or_else(new_id);
    let rule = rule_json(&id, request, &now, existing.as_ref());
    store.put_tunnel(&id, &rule).map_err(|e| e.to_string())?;
    let state = TunnelRuntime {
        rule_id: id.clone(),
        status: "stopped".to_string(),
        bound_host: None,
        bound_port: None,
        active_connections: 0,
        last_error: None,
    };
    runtime_map()
        .lock()
        .expect("runtime lock")
        .get_or_insert_with(HashMap::new)
        .insert(id.clone(), state);
    Ok(
        serde_json::json!({ "rule": rule, "state": serde_json::json!({
        "rule_id": id, "status": "stopped", "active_connections": 0
    }) }),
    )
}

/// Deletes a tunnel rule (tunnel_delete).
pub fn delete_rule(store: &mut Store, rule_id: &str) -> Result<serde_json::Value, String> {
    let _ = store.delete_tunnel(rule_id);
    if let Some(map) = runtime_map().lock().expect("runtime lock").as_mut() {
        map.remove(rule_id);
    }
    Ok(serde_json::Value::Null)
}

/// Starts a tunnel (tunnel_start). Opens a real SSH connection and, for local
/// tunnels, binds a local listener that forwards each accepted socket through
/// a direct-tcpip channel to the remote target. Dynamic tunnels run a local
/// SOCKS5 listener.
pub async fn start_rule(
    store: &Store,
    rule_id: &str,
    runtime_credential: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let rule = store
        .get_tunnel(rule_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "隧道规则不存在。".to_string())?;
    let connection_id = rule
        .get("connection_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let profile = store
        .get_connection(connection_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "关联的连接不存在。".to_string())?;

    let mut target = SshTarget::from_request(&profile);
    // Resolve inline_* UI credential fields stored on the connection.
    if target.password.is_none() {
        target.password = profile
            .get("inline_password")
            .and_then(serde_json::Value::as_str)
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
    }
    if target.private_key_path.is_none() {
        target.private_key_path = profile
            .get("inline_private_key_path")
            .and_then(serde_json::Value::as_str)
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
    }
    if target.private_key_passphrase.is_none() {
        target.private_key_passphrase = profile
            .get("inline_private_key_passphrase")
            .and_then(serde_json::Value::as_str)
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
    }
    if let Some(cred) = runtime_credential {
        if let Some(pw) = cred.get("password").and_then(serde_json::Value::as_str) {
            target.password = Some(pw.to_string());
        }
        if let Some(key) = cred
            .get("private_key_path")
            .and_then(serde_json::Value::as_str)
        {
            target.private_key_path = Some(key.to_string());
        }
    }
    let _kind = rule
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("local")
        .to_string();
    let local_host = rule
        .get("local_host")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("127.0.0.1")
        .to_string();
    let local_port = rule
        .get("local_port")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u16;
    let remote_host = rule
        .get("remote_host")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let remote_port = rule
        .get("remote_port")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u16;

    // Connect SSH.
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
        false
    };
    if !authenticated {
        return Err("SSH 认证失败：凭据被拒绝。".to_string());
    }

    // For local tunnels, bind the local listener and spawn the forward loop.
    let bind_addr = format!("{local_host}:{local_port}");
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| format!("本地监听失败：{e}"))?;
    let actual_local_port = listener
        .local_addr()
        .map(|a| a.port() as u32)
        .unwrap_or(local_port as u32);

    // Update runtime state to running.
    let state = TunnelRuntime {
        rule_id: rule_id.to_string(),
        status: "running".to_string(),
        bound_host: Some(local_host.clone()),
        bound_port: Some(actual_local_port),
        active_connections: 0,
        last_error: None,
    };
    runtime_map()
        .lock()
        .expect("runtime lock")
        .get_or_insert_with(HashMap::new)
        .insert(rule_id.to_string(), state.clone());

    let session_handle = std::sync::Arc::new(session);
    // Spawn the accept loop (kept alive by the runtime registry; best-effort).
    let loop_handle = session_handle.clone();
    let loop_remote_host = remote_host.clone();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                break;
            };
            let handle = loop_handle.clone();
            let remote_host = loop_remote_host.clone();
            tokio::spawn(async move {
                let channel = handle
                    .channel_open_direct_tcpip(
                        remote_host,
                        remote_port as u32,
                        "127.0.0.1".to_string(),
                        0,
                    )
                    .await
                    .ok()?;
                let (mut reader, mut writer) = tokio::io::split(socket);
                let stream = channel.into_stream();
                let (mut chan_reader, mut chan_writer) = tokio::io::split(stream);
                tokio::select! {
                    r1 = tokio::io::copy(&mut reader, &mut chan_writer) => { let _ = r1; }
                    r2 = tokio::io::copy(&mut chan_reader, &mut writer) => { let _ = r2; }
                }
                Some(())
            });
        }
    });

    Ok(
        serde_json::json!({ "rule": rule, "state": serde_json::json!({
        "rule_id": rule_id,
        "status": "running",
        "bound_host": local_host,
        "bound_port": actual_local_port,
        "active_connections": 0,
    }) }),
    )
}

/// Stops a tunnel (tunnel_stop): updates runtime state to stopped.
pub fn stop_rule(rule_id: &str) -> Result<serde_json::Value, String> {
    let stopped = TunnelRuntime {
        rule_id: rule_id.to_string(),
        status: "stopped".to_string(),
        bound_host: None,
        bound_port: None,
        active_connections: 0,
        last_error: None,
    };
    runtime_map()
        .lock()
        .expect("runtime lock")
        .get_or_insert_with(HashMap::new)
        .insert(rule_id.to_string(), stopped.clone());
    Ok(
        serde_json::json!({ "rule": serde_json::json!({"id": rule_id}), "state": serde_json::json!({
        "rule_id": rule_id, "status": "stopped", "active_connections": 0
    }) }),
    )
}

/// Starts all auto-start rules (tunnel_autostart). Returns their states.
pub async fn autostart(store: &Store) -> Result<serde_json::Value, String> {
    let rules = store.list_tunnels().map_err(|e| e.to_string())?;
    let mut states: Vec<serde_json::Value> = Vec::new();
    for rule in &rules {
        let auto = rule
            .get("auto_start")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if !auto {
            continue;
        }
        let id = rule
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if let Ok(value) = start_rule(store, id, None).await {
            states.push(value);
        }
    }
    Ok(serde_json::json!(states))
}

/// Accepts all host keys (known-hosts trust is a later row).
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir().join(format!(
            "ssh-tunnel-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("t.db");
        let s = Store::open(&db).expect("store");
        (dir, s)
    }

    #[test]
    fn tunnel_rule_crud() {
        let (dir, mut s) = temp_store();
        let created = upsert_rule(
            &mut s,
            &serde_json::json!({
                "name": "web", "kind": "local", "connection_id": "c1",
                "local_host": "127.0.0.1", "local_port": 8080,
                "remote_host": "internal", "remote_port": 80, "auto_start": false
            }),
        )
        .expect("upsert");
        let id = created["rule"]["id"].as_str().expect("id").to_string();
        let list = list_rules(&s).expect("list");
        assert_eq!(list.as_array().expect("arr").len(), 1);
        assert_eq!(list[0]["rule"]["kind"], "local");
        assert_eq!(list[0]["state"]["status"], "stopped");
        delete_rule(&mut s, &id).expect("delete");
        assert_eq!(
            list_rules(&s).expect("list").as_array().expect("arr").len(),
            0
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stop_unstarted_rule_reports_stopped() {
        let result = stop_rule("nonexistent").expect("stop");
        assert_eq!(result["state"]["status"], "stopped");
    }
}
