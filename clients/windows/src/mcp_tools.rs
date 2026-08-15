//! MCP (Model Context Protocol) settings & remote service (mxterm parity T014).
//!
//! Settings persist in the local SQLite store; remote tokens are SHA-256
//! hashed for verification and the plaintext is returned only at
//! generation/rotation time (mirroring mxterm's McpSettings contract). The
//! remote MCP service is a supervised child process (`mxterm-mcp.exe` next to
//! the app); when the sidecar is absent the runtime records a clear error and
//! keeps the McpRemoteServiceStatus shape stable so the UI can show it.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::store::Store;

const MCP_SETTINGS_KEY: &str = "mcp.default";
const DEFAULT_REMOTE_HOST: &str = "0.0.0.0";
const DEFAULT_REMOTE_PORT: u16 = 8765;
const REMOTE_LOG_TAIL_BYTES: usize = 128 * 1024;
const REMOTE_LOG_MAX_BYTES: u64 = 2 * 1024 * 1024;

fn now_ts() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn default_settings() -> Value {
    json!({
        "enabled": false,
        "expose_connections": false,
        "ssh_operations_enabled": false,
        "allow_dangerous_commands": false,
        "remote_enabled": false,
        "remote_host": DEFAULT_REMOTE_HOST,
        "remote_port": DEFAULT_REMOTE_PORT,
        "remote_token": Value::Null,
        "remote_token_hash": Value::Null,
        "remote_token_preview": Value::Null,
        "connection_exposure_mode": "all",
        "exposed_connection_ids": [],
    })
}

fn load_settings(store: &Store) -> Result<Value, String> {
    Ok(store
        .get_app_setting(MCP_SETTINGS_KEY)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(default_settings))
}

/// mcp_settings_get: returns persisted settings plus the live service status.
pub fn settings_get(store: &Store) -> Result<Value, String> {
    let settings = load_settings(store)?;
    let status = service_status(&settings);
    Ok(settings_output(settings, None, status))
}

/// mcp_settings_save: normalizes + persists settings, generating a remote
/// token on first enable, and reconciles the remote service.
pub fn settings_save(store: &mut Store, request: &Value) -> Result<Value, String> {
    let existing = load_settings(store)?;
    let enabled = request
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let expose_connections = request
        .get("expose_connections")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ssh_operations_enabled = request
        .get("ssh_operations_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let allow_dangerous_commands = request
        .get("allow_dangerous_commands")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let remote_enabled = request
        .get("remote_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let remote_host = normalize_remote_host(
        request
            .get("remote_host")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_REMOTE_HOST),
    )?;
    let remote_port = validate_remote_port(
        request
            .get("remote_port")
            .and_then(Value::as_u64)
            .unwrap_or(u64::from(DEFAULT_REMOTE_PORT)) as u16,
    )?;
    let connection_exposure_mode = request
        .get("connection_exposure_mode")
        .and_then(Value::as_str)
        .unwrap_or("all")
        .to_string();
    let exposed_connection_ids = normalize_connection_ids(
        request
            .get("exposed_connection_ids")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default(),
    );
    let input_token = request
        .get("remote_token")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let (remote_token, remote_token_hash, remote_token_preview, generated) =
        next_remote_token_state(&existing, remote_enabled, input_token)?;
    let settings = json!({
        "enabled": enabled,
        "expose_connections": expose_connections,
        "ssh_operations_enabled": ssh_operations_enabled,
        "allow_dangerous_commands": allow_dangerous_commands,
        "remote_enabled": remote_enabled,
        "remote_host": remote_host,
        "remote_port": remote_port,
        "remote_token": remote_token,
        "remote_token_hash": remote_token_hash,
        "remote_token_preview": remote_token_preview,
        "connection_exposure_mode": connection_exposure_mode,
        "exposed_connection_ids": exposed_connection_ids,
    });
    store
        .put_app_setting(MCP_SETTINGS_KEY, &settings)
        .map_err(|e| e.to_string())?;
    let status = reconcile(&settings);
    Ok(settings_output(settings, generated, status))
}

/// mcp_executable_path: resolves the MCP sidecar next to the running app.
pub fn executable_path() -> Result<String, String> {
    Ok(sidecar_path()?.to_string_lossy().to_string())
}

/// mcp_local_network_info: discovers the primary LAN IP without sending any
/// packets (UDP connect only).
pub fn local_network_info() -> Value {
    let mut addresses: BTreeSet<String> = BTreeSet::new();
    for target in [
        SocketAddr::from((Ipv4Addr::new(8, 8, 8, 8), 80)),
        SocketAddr::from((Ipv4Addr::new(1, 1, 1, 1), 80)),
    ] {
        if let Some(ip) = local_ip_for_target(target) {
            addresses.insert(ip.to_string());
        }
    }
    let ip_addresses: Vec<String> = addresses.into_iter().collect();
    json!({
        "primary_ip": ip_addresses.first().cloned(),
        "ip_addresses": ip_addresses,
    })
}

/// mcp_remote_service_status.
pub fn local_ip_for_target(target: SocketAddr) -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(target).ok()?;
    let local = socket.local_addr().ok()?.ip();
    is_remote_usable_ip(&local).then_some(local)
}

fn is_remote_usable_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(value) => {
            !value.is_loopback()
                && !value.is_unspecified()
                && !value.is_broadcast()
                && !value.is_link_local()
        }
        IpAddr::V6(value) => !value.is_loopback() && !value.is_unspecified(),
    }
}

pub fn remote_service_status(store: &Store) -> Result<Value, String> {
    let settings = load_settings(store)?;
    Ok(service_status(&settings))
}

/// mcp_remote_service_start.
pub fn remote_service_start(store: &mut Store) -> Result<Value, String> {
    let settings = load_settings(store)?;
    Ok(reconcile(&settings))
}

/// mcp_remote_service_stop.
pub fn remote_service_stop(store: &mut Store) -> Result<Value, String> {
    let settings = load_settings(store)?;
    Ok(stop_service(&settings))
}

/// mcp_remote_service_restart.
pub fn remote_service_restart(store: &mut Store) -> Result<Value, String> {
    let settings = load_settings(store)?;
    {
        let mut rt = RUNTIME.lock().expect("runtime lock");
        kill_child(&mut rt);
        rt.last_error = None;
        rt.restart_count += 1;
    }
    Ok(reconcile(&settings))
}

/// mcp_update_blockers: counts external MCP processes that would block an app
/// update plus the managed remote service.
pub fn update_blockers(store: &Store) -> Result<Value, String> {
    let settings = load_settings(store)?;
    let managed_remote_running = service_status(&settings)["running"]
        .as_bool()
        .unwrap_or(false);
    let process_count = running_mcp_process_count().max(if managed_remote_running { 1 } else { 0 });
    Ok(json!({
        "process_count": process_count,
        "managed_remote_running": managed_remote_running,
    }))
}

/// mcp_prepare_for_update: stops the managed service and external MCP
/// processes so the app can be updated.
pub fn prepare_for_update(store: &mut Store) -> Result<Value, String> {
    let settings = load_settings(store)?;
    let managed_remote_running = service_status(&settings)["running"]
        .as_bool()
        .unwrap_or(false);
    let process_count = running_mcp_process_count().max(if managed_remote_running { 1 } else { 0 });
    let _ = stop_service(&settings);
    terminate_external_mcp_processes()?;
    append_remote_log("INFO", "MCP processes stopped for application update")?;
    Ok(json!({
        "process_count": process_count,
        "managed_remote_running": managed_remote_running,
    }))
}

/// mcp_remote_log_read: returns the tail of the remote MCP log.
pub fn remote_log_read() -> Result<Value, String> {
    let path = remote_log_path()?;
    log_read_at(&path)
}

/// mcp_remote_log_clear: truncates the remote MCP log.
pub fn remote_log_clear() -> Result<Value, String> {
    let path = remote_log_path()?;
    log_clear_at(&path)
}

/// mcp_remote_token_rotate: generates a fresh token, persists it, and
/// restarts the remote service.
pub fn remote_token_rotate(store: &mut Store) -> Result<Value, String> {
    let mut settings = load_settings(store)?;
    let token = generate_remote_token()?;
    settings["remote_token"] = json!(token.clone());
    settings["remote_token_hash"] = json!(hash_remote_token(&token));
    settings["remote_token_preview"] = json!(remote_token_preview(&token));
    store
        .put_app_setting(MCP_SETTINGS_KEY, &settings)
        .map_err(|e| e.to_string())?;
    let status = reconcile(&settings);
    Ok(settings_output(settings, Some(token), status))
}

// ---- token helpers (mirror mxterm) ----

pub fn generate_remote_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| e.to_string())?;
    use base64::Engine as _;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

pub fn hash_remote_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn remote_token_preview(token: &str) -> String {
    let suffix: String = token
        .chars()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("...{suffix}")
}

pub fn verify_remote_token(token: &str, expected_hash: &str) -> bool {
    let actual = hash_remote_token(token.trim());
    constant_time_eq(actual.as_bytes(), expected_hash.trim().as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// (token, token_hash, token_preview, generated_token)
type TokenState = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn next_remote_token_state(
    existing: &Value,
    remote_enabled: bool,
    input_token: Option<String>,
) -> Result<TokenState, String> {
    if let Some(token) = input_token.map(|value| value.trim().to_string()) {
        if token.is_empty() {
            return Err("请输入远程 MCP token。".to_string());
        }
        return Ok((
            Some(token.clone()),
            Some(hash_remote_token(&token)),
            Some(remote_token_preview(&token)),
            None,
        ));
    }
    if !remote_enabled
        || existing
            .get("remote_token_hash")
            .and_then(Value::as_str)
            .is_some()
    {
        return Ok((
            existing
                .get("remote_token")
                .and_then(Value::as_str)
                .map(str::to_string),
            existing
                .get("remote_token_hash")
                .and_then(Value::as_str)
                .map(str::to_string),
            existing
                .get("remote_token_preview")
                .and_then(Value::as_str)
                .map(str::to_string),
            None,
        ));
    }
    let token = generate_remote_token()?;
    Ok((
        Some(token.clone()),
        Some(hash_remote_token(&token)),
        Some(remote_token_preview(&token)),
        Some(token),
    ))
}

fn normalize_remote_host(host: &str) -> Result<String, String> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return Err("请输入远程 MCP 监听地址。".to_string());
    }
    Ok(trimmed.to_string())
}

fn validate_remote_port(port: u16) -> Result<u16, String> {
    if port == 0 {
        return Err("远程 MCP 端口必须在 1 到 65535 之间。".to_string());
    }
    Ok(port)
}

fn normalize_connection_ids(ids: Vec<String>) -> Vec<String> {
    ids.into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

// ---- remote service runtime ----

#[derive(Default)]
struct Runtime {
    pid: Option<u32>,
    started_at: Option<String>,
    healthy: bool,
    last_error: Option<String>,
    restart_count: u32,
    consecutive_failures: u32,
    log_path: Option<String>,
}

static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime {
    pid: None,
    started_at: None,
    healthy: false,
    last_error: None,
    restart_count: 0,
    consecutive_failures: 0,
    log_path: None,
});

fn sidecar_path() -> Result<PathBuf, String> {
    let parent = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "无法定位可执行文件目录。".to_string())?;
    Ok(parent.join(if cfg!(windows) {
        "mxterm-mcp.exe"
    } else {
        "mxterm-mcp"
    }))
}

/// Starts (or keeps) the remote MCP service per settings. Mirrors mxterm's
/// `reconcile`: disabled or missing token stops the child; a missing sidecar
/// is reported as a clear error instead of panicking.
fn reconcile(settings: &Value) -> Value {
    let mut rt = RUNTIME.lock().expect("runtime lock");
    if !settings["remote_enabled"].as_bool().unwrap_or(false) {
        kill_child(&mut rt);
        rt.last_error = None;
        return service_status_inner(settings, &rt);
    }
    if settings
        .get("remote_token_hash")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        kill_child(&mut rt);
        rt.last_error = Some("远程 MCP token 尚未生成。".to_string());
        return service_status_inner(settings, &rt);
    }
    if rt.pid.is_some() {
        return service_status_inner(settings, &rt);
    }
    match sidecar_path() {
        Ok(exe) if exe.exists() => match spawn_child(&exe) {
            Ok(pid) => {
                rt.pid = Some(pid);
                rt.started_at = Some(now_ts());
                rt.healthy = true;
                rt.last_error = None;
                rt.log_path = remote_log_path()
                    .ok()
                    .map(|p| p.to_string_lossy().to_string());
                let _ =
                    append_remote_log("INFO", &format!("remote MCP service started (pid={pid})"));
            }
            Err(e) => {
                rt.last_error = Some(e.clone());
                let _ =
                    append_remote_log("ERROR", &format!("remote MCP service start failed: {e}"));
            }
        },
        Ok(exe) => {
            rt.last_error = Some(format!("未找到 MCP sidecar：{}", exe.display()));
        }
        Err(e) => {
            rt.last_error = Some(e);
        }
    }
    service_status_inner(settings, &rt)
}

fn stop_service(settings: &Value) -> Value {
    let mut rt = RUNTIME.lock().expect("runtime lock");
    kill_child(&mut rt);
    rt.last_error = None;
    service_status_inner(settings, &rt)
}

fn service_status(settings: &Value) -> Value {
    let rt = RUNTIME.lock().expect("runtime lock");
    service_status_inner(settings, &rt)
}

fn service_status_inner(settings: &Value, rt: &Runtime) -> Value {
    let host = settings["remote_host"]
        .as_str()
        .unwrap_or(DEFAULT_REMOTE_HOST);
    let port = settings["remote_port"]
        .as_u64()
        .unwrap_or(u64::from(DEFAULT_REMOTE_PORT));
    json!({
        "enabled": settings["enabled"],
        "running": rt.pid.is_some(),
        "host": host,
        "port": port,
        "url": format!("http://{host}:{port}"),
        "sse_url": format!("http://{host}:{port}/sse"),
        "pid": rt.pid,
        "token_saved": settings.get("remote_token_hash").and_then(Value::as_str).is_some(),
        "token_preview": settings.get("remote_token_preview").cloned().unwrap_or(Value::Null),
        "error": rt.last_error.clone(),
        "healthy": rt.healthy,
        "started_at": rt.started_at.clone(),
        "last_health_at": Value::Null,
        "restart_count": rt.restart_count,
        "consecutive_failures": rt.consecutive_failures,
        "log_path": rt.log_path.clone(),
    })
}

#[cfg(windows)]
fn spawn_child(exe: &Path) -> Result<u32, String> {
    use std::os::windows::process::CommandExt;
    let child = Command::new(exe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|e| format!("启动 MCP 服务失败：{e}"))?;
    let id = child.id();
    if id == 0 {
        return Err("无法获取 MCP 服务进程 id。".to_string());
    }
    Ok(id)
}

#[cfg(not(windows))]
fn spawn_child(exe: &Path) -> Result<u32, String> {
    let child = Command::new(exe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("启动 MCP 服务失败：{e}"))?;
    let id = child.id();
    if id == 0 {
        return Err("无法获取 MCP 服务进程 id。".to_string());
    }
    Ok(id)
}

fn kill_child(rt: &mut Runtime) {
    if let Some(pid) = rt.pid.take() {
        let pid_text = pid.to_string();
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = Command::new("taskkill")
                .args(["/PID", &pid_text, "/F", "/T"])
                .creation_flags(0x0800_0000)
                .output();
        }
        #[cfg(not(windows))]
        {
            let _ = Command::new("kill").arg(&pid_text).output();
        }
    }
    rt.pid = None;
    rt.healthy = false;
    rt.started_at = None;
}

#[cfg(windows)]
fn running_mcp_process_count() -> u32 {
    use std::os::windows::process::CommandExt;
    match Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq mxterm-mcp.exe", "/FO", "CSV", "/NH"])
        .creation_flags(0x0800_0000)
        .output()
    {
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| {
                line.trim_start()
                    .to_ascii_lowercase()
                    .starts_with("\"mxterm-mcp.exe\",")
            })
            .count() as u32,
        Err(_) => 0,
    }
}

#[cfg(not(windows))]
fn running_mcp_process_count() -> u32 {
    0
}

#[cfg(windows)]
fn terminate_external_mcp_processes() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    if running_mcp_process_count() == 0 {
        return Ok(());
    }
    let output = Command::new("taskkill")
        .args(["/IM", "mxterm-mcp.exe", "/F", "/T"])
        .creation_flags(0x0800_0000)
        .output();
    match output {
        Ok(o) if o.status.success() || running_mcp_process_count() == 0 => Ok(()),
        Ok(o) => Err(format!(
            "无法关闭 MCP 进程：{}",
            String::from_utf8_lossy(&o.stderr)
        )),
        Err(e) => Err(format!("无法关闭 MCP 进程：{e}")),
    }
}

#[cfg(not(windows))]
fn terminate_external_mcp_processes() -> Result<(), String> {
    Ok(())
}

// ---- settings output & logs ----

fn settings_output(settings: Value, generated: Option<String>, status: Value) -> Value {
    let remote_token_hash = settings.get("remote_token_hash").and_then(Value::as_str);
    json!({
        "enabled": settings["enabled"],
        "expose_connections": settings["expose_connections"],
        "ssh_operations_enabled": settings["ssh_operations_enabled"],
        "allow_dangerous_commands": settings["allow_dangerous_commands"],
        "remote_enabled": settings["remote_enabled"],
        "remote_host": settings["remote_host"],
        "remote_port": settings["remote_port"],
        "remote_token": settings.get("remote_token").cloned().unwrap_or(Value::Null),
        "remote_token_saved": remote_token_hash.is_some(),
        "remote_token_preview": settings.get("remote_token_preview").cloned().unwrap_or(Value::Null),
        "generated_remote_token": generated,
        "remote_status": status,
        "connection_exposure_mode": settings.get("connection_exposure_mode").cloned().unwrap_or(json!("all")),
        "exposed_connection_ids": settings.get("exposed_connection_ids").cloned().unwrap_or(json!([])),
    })
}

fn remote_log_path() -> Result<PathBuf, String> {
    let dir = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "无法定位可执行文件目录。".to_string())?;
    Ok(dir.join("logs").join("mcp-remote.log"))
}

fn append_remote_log(level: &str, message: &str) -> Result<(), String> {
    let path = remote_log_path()?;
    append_remote_log_at(&path, level, message)
}

fn append_remote_log_at(path: &Path, level: &str, message: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) >= REMOTE_LOG_MAX_BYTES {
        let previous = path.with_extension("log.1");
        let _ = std::fs::remove_file(&previous);
        let _ = std::fs::rename(path, previous);
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    writeln!(file, "[{}] [{level}] {message}", now_ts()).map_err(|e| e.to_string())
}

fn log_read_at(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, []).map_err(|e| e.to_string())?;
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let truncated = bytes.len() > REMOTE_LOG_TAIL_BYTES;
    let tail_start = bytes.len().saturating_sub(REMOTE_LOG_TAIL_BYTES);
    let content = String::from_utf8_lossy(&bytes[tail_start..]).into_owned();
    Ok(json!({
        "content": content,
        "path": path.to_string_lossy(),
        "truncated": truncated,
        "updated_at": now_ts(),
    }))
}

fn log_clear_at(path: &Path) -> Result<Value, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, []).map_err(|e| e.to_string())?;
    Ok(json!({
        "content": "",
        "path": path.to_string_lossy(),
        "truncated": false,
        "updated_at": now_ts(),
    }))
}

// ---- tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_token_hash_verify_preview() {
        let token = generate_remote_token().expect("token");
        assert_eq!(token.len(), 43); // URL_SAFE_NO_PAD of 32 bytes
        let hash = hash_remote_token(&token);
        assert_eq!(hash.len(), 64);
        assert!(verify_remote_token(&token, &hash));
        assert!(!verify_remote_token("wrong", &hash));
        let preview = remote_token_preview(&token);
        assert!(preview.starts_with("..."));
        assert_eq!(preview.len(), 9);
    }

    #[test]
    fn next_token_state_generates_once() {
        let existing = default_settings();
        let (token, hash, preview, generated) =
            next_remote_token_state(&existing, true, None).expect("gen");
        assert!(generated.is_some());
        assert!(token.is_some());
        assert!(hash.is_some());
        assert!(preview.is_some());
        // enabling again with an existing hash keeps it (no new token)
        let mut saved = existing.clone();
        saved["remote_token_hash"] = json!(hash.clone());
        let (_, _, _, generated2) = next_remote_token_state(&saved, true, None).expect("keep");
        assert!(generated2.is_none());
        // empty custom token is rejected
        assert!(next_remote_token_state(&existing, true, Some("  ".to_string())).is_err());
    }

    #[test]
    fn host_and_port_validation() {
        assert_eq!(normalize_remote_host(" 0.0.0.0 ").expect("host"), "0.0.0.0");
        assert!(normalize_remote_host("").is_err());
        assert_eq!(validate_remote_port(8765).expect("port"), 8765);
        assert!(validate_remote_port(0).is_err());
        assert_eq!(
            normalize_connection_ids(vec![" a ".into(), "".into(), "b".into()]),
            vec!["a", "b"]
        );
    }

    #[test]
    fn network_info_shape() {
        let info = local_network_info();
        assert!(info["ip_addresses"].is_array());
        assert!(info.get("primary_ip").is_some());
    }

    #[test]
    fn log_roundtrip_on_temp_file() {
        let dir = std::env::temp_dir().join(format!("mcp-log-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("mcp-remote.log");
        append_remote_log_at(&path, "INFO", "hello world").expect("append");
        let read = log_read_at(&path).expect("read");
        assert!(read["content"]
            .as_str()
            .expect("content")
            .contains("hello world"));
        assert_eq!(read["truncated"], false);
        let cleared = log_clear_at(&path).expect("clear");
        assert_eq!(cleared["content"], "");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
