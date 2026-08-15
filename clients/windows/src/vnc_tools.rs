//! VNC session launching (mxterm parity T016).
//!
//! Embedded/windowed modes run a real local WebSocket bridge: a listener on
//! 127.0.0.1 accepts the noVNC browser connection, upgrades it with the
//! standard Sec-WebSocket-Accept handshake, and relays binary frames to the
//! VNC target (masking/unmasking per RFC 6455). External modes spawn a system
//! viewer. Other platforms keep only the approved interface boundary.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::store::Store;

const VNC_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_WS_FRAME_BYTES: usize = 16 * 1024 * 1024;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_session_id() -> String {
    format!("vnc-{}-{:x}", std::process::id(), now_ms())
}

struct VncSessionInfo {
    connection_id: String,
    embedded: bool,
    bridge: Option<tokio::task::JoinHandle<()>>,
}

static SESSIONS: Mutex<Option<HashMap<String, VncSessionInfo>>> = Mutex::new(None);

fn sessions_map() -> &'static Mutex<Option<HashMap<String, VncSessionInfo>>> {
    &SESSIONS
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("tokio runtime"))
}

fn current_platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unknown"
    }
}

fn viewer_candidates(platform: &str) -> Vec<String> {
    match platform {
        "windows" => vec![
            "vncviewer.exe".to_string(),
            "C:\\Program Files\\TigerVNC\\vncviewer.exe".to_string(),
            "C:\\Program Files\\RealVNC\\VNC Viewer\\vncviewer.exe".to_string(),
        ],
        "linux" => vec![
            "vncviewer".to_string(),
            "xtigervncviewer".to_string(),
            "tigervnc-viewer".to_string(),
        ],
        "macos" => vec![
            "/Applications/TigerVNC Viewer.app/Contents/MacOS/TigerVNC Viewer".to_string(),
            "/Applications/VNC Viewer.app/Contents/MacOS/vncviewer".to_string(),
        ],
        _ => vec!["vncviewer".to_string()],
    }
}

/// vnc_test_runner: probes available VNC runners (embedded noVNC is always
/// available; external viewers are detected on disk/PATH).
pub fn probe_runner(request: &Value) -> Value {
    let config = request.get("config").cloned().unwrap_or(Value::Null);
    let platform = current_platform();
    let mut available_runners: Vec<String> = vec!["novnc".to_string()];
    let mut default_runner: Option<String> = Some("novnc".to_string());
    let mut default_executable: Option<String> = None;

    let custom_executable = config
        .get("custom_executable")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let render_mode = config
        .get("render_mode")
        .and_then(Value::as_str)
        .unwrap_or("embedded");

    if !custom_executable.is_empty() {
        if let Some(path) = find_custom_executable(&custom_executable) {
            available_runners.push("custom".to_string());
            if render_mode == "custom" {
                default_runner = Some("custom".to_string());
                default_executable = Some(path.to_string_lossy().to_string());
            }
        }
    }

    if let Some(viewer) = find_first_executable(&viewer_candidates(platform)) {
        let runner = if platform == "windows" {
            "realvnc"
        } else {
            "tigervnc"
        };
        available_runners.push(runner.to_string());
        if default_executable.is_none() && !matches!(render_mode, "embedded" | "windowed") {
            default_runner = Some(runner.to_string());
            default_executable = Some(viewer.to_string_lossy().to_string());
        }
    }

    json!({
        "platform": platform,
        "available_runners": available_runners,
        "default_runner": default_runner,
        "default_executable": default_executable,
        "supports_embedded": true,
        "supports_clipboard": true,
        "supports_resize_session": true,
        "setup_hint": null,
    })
}

/// vnc_preview_launch: builds the launch plan without executing.
pub fn preview_launch(store: &Store, request: &Value) -> Result<Value, String> {
    let connection_id = request
        .get("connection_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let (profile, vnc) = resolve_vnc_connection(store, connection_id)?;
    let selected = select_runner(&vnc)?;
    let warnings = preview_warnings(&vnc, &selected.runner);
    if selected.runner == "novnc" {
        return Ok(json!({
            "connection_id": profile["id"],
            "runner": "novnc",
            "render_mode": vnc.pointer("/runner/render_mode").and_then(Value::as_str).unwrap_or("embedded"),
            "embedded": true,
            "executable": null,
            "args": [],
            "websocket_url": "ws://127.0.0.1:<port>/vnc/<session>/<token>",
            "fallback_reason": null,
            "setup_hint": null,
            "warnings": warnings,
        }));
    }
    let args = build_external_launch_plan(&profile, &vnc, &selected, true)?;
    Ok(json!({
        "connection_id": profile["id"],
        "runner": selected.runner,
        "render_mode": vnc.pointer("/runner/render_mode").and_then(Value::as_str).unwrap_or("external"),
        "embedded": false,
        "executable": selected.executable,
        "args": args,
        "websocket_url": null,
        "fallback_reason": null,
        "setup_hint": null,
        "warnings": warnings,
    }))
}

/// vnc_launch_connection: starts the embedded noVNC bridge or the external
/// viewer.
pub fn launch_connection(store: &mut Store, request: &Value) -> Result<Value, String> {
    let connection_id = request
        .get("connection_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let (profile, vnc) = resolve_vnc_connection(store, connection_id)?;
    let selected = select_runner(&vnc)?;
    if selected.runner == "novnc" {
        return launch_embedded_bridge(&profile, &vnc, &selected);
    }
    launch_external_runner(&profile, &vnc, &selected)
}

/// vnc_close_session: aborts the embedded bridge (or reports external).
pub fn close_session(request: &Value) -> Value {
    let session_id = request
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let removed = sessions_map()
        .lock()
        .expect("sessions lock")
        .as_mut()
        .and_then(|m| m.remove(session_id));
    match removed {
        Some(info) => {
            if let Some(bridge) = info.bridge {
                bridge.abort();
            }
            let _ = info.connection_id;
            let mode = if info.embedded { "嵌入式" } else { "外部" };
            json!({
                "ok": true,
                "message": format!("VNC 会话 {session_id} 已关闭（{mode}）。"),
            })
        }
        None => json!({
            "ok": false,
            "message": format!("VNC 会话 {session_id} 不存在或已关闭。"),
        }),
    }
}

fn launch_embedded_bridge(
    profile: &Value,
    vnc: &Value,
    selected: &SelectedVncRunner,
) -> Result<Value, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("VNC 本地桥接端口绑定失败：{e}"))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    let session_id = new_session_id();
    let token = format!("{:x}", now_ms());
    let path = format!("/vnc/{session_id}/{token}");
    let websocket_url = format!("ws://127.0.0.1:{}{path}", address.port());
    let target_host = profile
        .get("host")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let target_port = profile.get("port").and_then(Value::as_u64).unwrap_or(5900) as u16;
    let connection_id = profile
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let bridge = runtime().spawn(run_bridge(listener, path, target_host, target_port));
    sessions_map()
        .lock()
        .expect("sessions lock")
        .get_or_insert_with(HashMap::new)
        .insert(
            session_id.clone(),
            VncSessionInfo {
                connection_id,
                embedded: true,
                bridge: Some(bridge),
            },
        );
    let password = profile
        .get("password")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    Ok(json!({
        "session_id": session_id,
        "connection_id": profile["id"],
        "launched": true,
        "embedded": true,
        "runner": "novnc",
        "websocket_url": websocket_url,
        "password": password,
        "executable": null,
        "args": [],
        "process_id": null,
        "fallback_reason": selected.fallback_reason.clone(),
        "setup_hint": null,
        "warnings": preview_warnings(vnc, "novnc"),
    }))
}

fn launch_external_runner(
    profile: &Value,
    vnc: &Value,
    selected: &SelectedVncRunner,
) -> Result<Value, String> {
    let args = build_external_launch_plan(profile, vnc, selected, false)?;
    let executable = selected
        .executable
        .clone()
        .ok_or_else(|| "未找到可用的 VNC 客户端。".to_string())?;
    let child = std::process::Command::new(&executable)
        .args(&args)
        .spawn()
        .map_err(|e| format!("VNC 客户端启动失败：{e}"))?;
    let process_id = child.id();
    let session_id = new_session_id();
    if process_id != 0 {
        sessions_map()
            .lock()
            .expect("sessions lock")
            .get_or_insert_with(HashMap::new)
            .insert(
                session_id.clone(),
                VncSessionInfo {
                    connection_id: profile
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    embedded: false,
                    bridge: None,
                },
            );
    }
    Ok(json!({
        "session_id": session_id,
        "connection_id": profile["id"],
        "launched": true,
        "embedded": false,
        "runner": selected.runner,
        "websocket_url": null,
        "password": null,
        "executable": executable,
        "args": args,
        "process_id": if process_id == 0 { Value::Null } else { json!(process_id) },
        "fallback_reason": selected.fallback_reason.clone(),
        "setup_hint": null,
        "warnings": preview_warnings(vnc, &selected.runner),
    }))
}
// ---- resolution & selection ----

fn resolve_vnc_connection(store: &Store, connection_id: &str) -> Result<(Value, Value), String> {
    let connection_id = connection_id.trim();
    if connection_id.is_empty() {
        return Err("请选择 VNC 连接。".to_string());
    }
    let profile = store
        .get_connection(connection_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "连接不存在。".to_string())?;
    let protocol = profile
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !protocol.is_empty() && protocol != "vnc" {
        return Err("该操作仅支持 VNC 连接。".to_string());
    }
    let vnc = profile.get("vnc").cloned().unwrap_or(Value::Null);
    let vnc = if vnc.is_null() {
        default_vnc_config()
    } else {
        vnc
    };
    Ok((profile, vnc))
}

fn default_vnc_config() -> Value {
    json!({
        "display": { "scale_mode": "fit", "resize_session": true, "clip_viewport": false },
        "input": { "view_only": false, "clipboard": true, "shared": false },
        "performance": { "preset": "auto", "quality_level": null, "compression_level": null },
        "security": { "credential_mode": "prompt" },
        "runner": { "render_mode": "embedded", "preferred_runner": null, "custom_executable": null, "custom_args_template": null },
        "raw_runner_args": null,
    })
}

#[derive(Clone)]
struct SelectedVncRunner {
    runner: String,
    executable: Option<String>,
    fallback_reason: Option<String>,
}

fn select_runner(vnc: &Value) -> Result<SelectedVncRunner, String> {
    let render_mode = vnc
        .pointer("/runner/render_mode")
        .and_then(Value::as_str)
        .unwrap_or("embedded");
    if matches!(render_mode, "embedded" | "windowed") {
        return Ok(SelectedVncRunner {
            runner: "novnc".to_string(),
            executable: None,
            fallback_reason: None,
        });
    }
    if render_mode == "custom" {
        let executable = vnc
            .pointer("/runner/custom_executable")
            .and_then(Value::as_str)
            .and_then(find_custom_executable)
            .ok_or_else(|| "未找到自定义 VNC 客户端。".to_string())?;
        return Ok(SelectedVncRunner {
            runner: "custom".to_string(),
            executable: Some(executable.to_string_lossy().to_string()),
            fallback_reason: Some(
                "外部 VNC 客户端不会接收保存的密码，将由客户端自行提示。".to_string(),
            ),
        });
    }
    let executable = find_first_executable(&viewer_candidates(current_platform()))
        .ok_or_else(|| "未找到可用的 VNC 客户端。".to_string())?;
    let preferred = vnc
        .pointer("/runner/preferred_runner")
        .and_then(Value::as_str)
        .unwrap_or("vncviewer");
    let runner = if preferred == "novnc" {
        "vncviewer"
    } else {
        preferred
    }
    .to_string();
    Ok(SelectedVncRunner {
        runner,
        executable: Some(executable.to_string_lossy().to_string()),
        fallback_reason: Some(
            "外部 VNC 客户端不会接收保存的密码，将由客户端自行提示。".to_string(),
        ),
    })
}

fn build_external_launch_plan(
    profile: &Value,
    vnc: &Value,
    selected: &SelectedVncRunner,
    preview: bool,
) -> Result<Vec<String>, String> {
    let host = profile.get("host").and_then(Value::as_str).unwrap_or("");
    let port = profile.get("port").and_then(Value::as_u64).unwrap_or(5900);
    let target = format!("{host}::{port}");
    let mut args = vec![target];
    if vnc
        .pointer("/input/view_only")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        args.push("-ViewOnly".to_string());
    }
    if !vnc
        .pointer("/input/shared")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        args.push("-Shared=0".to_string());
    }
    if selected.runner == "custom" {
        if let Some(template) = vnc
            .pointer("/runner/custom_args_template")
            .and_then(Value::as_str)
        {
            let rendered = template
                .replace("{host}", host)
                .replace("{port}", &port.to_string())
                .replace("{target}", &format!("{host}:{port}"));
            args = split_args(&rendered);
        }
    }
    if let Some(raw) = vnc.get("raw_runner_args").and_then(Value::as_str) {
        args.extend(split_args(raw));
    }
    if preview {
        for arg in args.iter_mut() {
            if arg.to_ascii_lowercase().contains("password") {
                *arg = "<redacted>".to_string();
            }
        }
    }
    Ok(args)
}

fn preview_warnings(vnc: &Value, runner: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    if runner != "novnc" {
        warnings.push("外部 VNC 客户端不会接收保存的密码。".to_string());
    }
    let render_mode = vnc
        .pointer("/runner/render_mode")
        .and_then(Value::as_str)
        .unwrap_or("embedded");
    if matches!(render_mode, "embedded" | "windowed") && runner != "novnc" {
        warnings.push("嵌入式 noVNC 不可用时才会回退到外部客户端。".to_string());
    }
    warnings
}

// ---- embedded WebSocket bridge ----

async fn run_bridge(
    listener: std::net::TcpListener,
    expected_path: String,
    target_host: String,
    target_port: u16,
) {
    let _ = listener.set_nonblocking(true);
    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
        return;
    };
    while let Ok((stream, _)) = listener.accept().await {
        let path = expected_path.clone();
        let host = target_host.clone();
        tokio::spawn(async move {
            let _ = relay_single_client(stream, path, host, target_port).await;
        });
    }
}

async fn relay_single_client(
    browser: tokio::net::TcpStream,
    expected_path: String,
    target_host: String,
    target_port: u16,
) -> Result<(), String> {
    let _ = browser.set_nodelay(true);
    let mut browser = browser;
    let (mut b_reader, mut b_writer) = browser.split();
    handshake(&mut b_reader, &mut b_writer, &expected_path).await?;
    let mut target = connect_vnc_target(&target_host, target_port).await?;
    let (mut t_reader, mut t_writer) = target.split();

    let browser_to_target = async {
        loop {
            match read_ws_frame(&mut b_reader).await {
                Ok(WsFrame::Data(data)) => {
                    if t_writer.write_all(&data).await.is_err() {
                        break;
                    }
                }
                // Browsers do not send pings over the JS WebSocket API; consume
                // any ping frame without replying so framing stays in sync.
                Ok(WsFrame::Ping) => {}
                Ok(WsFrame::Close) | Err(_) => break,
            }
        }
        let _ = t_writer.shutdown().await;
    };

    let target_to_browser = async {
        let mut buffer = vec![0u8; 8192];
        loop {
            match t_reader.read(&mut buffer).await {
                Ok(0) => break,
                Ok(n) => {
                    if write_ws_frame(&mut b_writer, &buffer[..n]).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = b_writer.shutdown().await;
    };

    tokio::select! {
        _ = browser_to_target => {}
        _ = target_to_browser => {}
    }
    Ok(())
}

enum WsFrame {
    Data(Vec<u8>),
    Ping,
    Close,
}

async fn handshake<R, W>(reader: &mut R, writer: &mut W, expected_path: &str) -> Result<(), String>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut request = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match reader.read(&mut byte).await {
            Ok(0) => return Err("VNC WebSocket 握手被中断。".to_string()),
            Ok(_) => {}
            Err(e) => return Err(format!("VNC WebSocket 握手读取失败：{e}")),
        }
        request.push(byte[0]);
        if request.ends_with(b"\r\n\r\n") {
            break;
        }
        if request.len() > 8192 {
            return Err("VNC WebSocket 握手头过大。".to_string());
        }
    }
    let text = String::from_utf8_lossy(&request);
    let mut lines = text.lines();
    let request_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 || parts[0] != "GET" || parts[1] != expected_path {
        return Err("无效的 VNC 会话 token。".to_string());
    }
    let key = lines
        .find_map(|line| line.strip_prefix("Sec-WebSocket-Key:"))
        .map(|value| value.trim().to_string())
        .ok_or_else(|| "VNC WebSocket 握手缺少 Sec-WebSocket-Key。".to_string())?;
    let accept = websocket_accept(&key);
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    writer
        .write_all(response.as_bytes())
        .await
        .map_err(|e| format!("VNC WebSocket 握手响应失败：{e}"))
}

fn websocket_accept(key: &str) -> String {
    use base64::Engine as _;
    use sha1::{Digest, Sha1};
    const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

async fn read_ws_frame<R>(reader: &mut R) -> Result<WsFrame, String>
where
    R: AsyncRead + Unpin,
{
    let mut first = [0u8; 1];
    match reader.read(&mut first).await {
        Ok(0) => return Ok(WsFrame::Close),
        Ok(_) => {}
        Err(e) => return Err(format!("VNC WebSocket 帧读取失败：{e}")),
    }
    let mut second = [0u8; 1];
    reader
        .read_exact(&mut second)
        .await
        .map_err(|e| format!("VNC WebSocket 帧头读取失败：{e}"))?;
    let opcode = first[0] & 0x0f;
    let masked = second[0] & 0x80 != 0;
    let mut len = u64::from(second[0] & 0x7f);
    if len == 126 {
        let mut bytes = [0u8; 2];
        reader
            .read_exact(&mut bytes)
            .await
            .map_err(|e| format!("VNC WebSocket 帧长度读取失败：{e}"))?;
        len = u64::from(u16::from_be_bytes(bytes));
    } else if len == 127 {
        let mut bytes = [0u8; 8];
        reader
            .read_exact(&mut bytes)
            .await
            .map_err(|e| format!("VNC WebSocket 帧长度读取失败：{e}"))?;
        len = u64::from_be_bytes(bytes);
    }
    if len as usize > MAX_WS_FRAME_BYTES {
        return Err("VNC WebSocket 帧过大。".to_string());
    }
    let mut mask = [0u8; 4];
    if masked {
        reader
            .read_exact(&mut mask)
            .await
            .map_err(|e| format!("VNC WebSocket 掩码读取失败：{e}"))?;
    }
    if opcode == 0x8 {
        let mut remaining = len;
        while remaining > 0 {
            let chunk = remaining.min(4096) as usize;
            let mut buf = vec![0u8; chunk];
            reader
                .read_exact(&mut buf)
                .await
                .map_err(|e| format!("VNC WebSocket close 帧读取失败：{e}"))?;
            remaining -= chunk as u64;
        }
        return Ok(WsFrame::Close);
    }
    if opcode == 0x9 {
        let mut payload = vec![0u8; len as usize];
        reader
            .read_exact(&mut payload)
            .await
            .map_err(|e| format!("VNC WebSocket ping 帧读取失败：{e}"))?;
        if masked {
            for (i, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[i % 4];
            }
        }
        return Ok(WsFrame::Ping);
    }
    if opcode == 0x1 || opcode == 0x2 || opcode == 0x0 {
        let mut payload = vec![0u8; len as usize];
        reader
            .read_exact(&mut payload)
            .await
            .map_err(|e| format!("VNC WebSocket 数据帧读取失败：{e}"))?;
        if masked {
            for (i, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[i % 4];
            }
        }
        return Ok(WsFrame::Data(payload));
    }
    Ok(WsFrame::Data(Vec::new()))
}

async fn write_ws_frame<W>(writer: &mut W, payload: &[u8]) -> Result<(), String>
where
    W: AsyncWrite + Unpin,
{
    let len = payload.len();
    let mut header = Vec::with_capacity(10);
    header.push(0x82);
    if len < 126 {
        header.push(len as u8);
    } else if len <= u16::MAX as usize {
        header.push(126);
        header.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        header.push(127);
        header.extend_from_slice(&(len as u64).to_be_bytes());
    }
    writer
        .write_all(&header)
        .await
        .map_err(|e| format!("VNC WebSocket 帧写入失败：{e}"))?;
    writer
        .write_all(payload)
        .await
        .map_err(|e| format!("VNC WebSocket 帧写入失败：{e}"))
}

async fn connect_vnc_target(host: &str, port: u16) -> Result<tokio::net::TcpStream, String> {
    let addr = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("VNC 目标解析失败：{e}"))?
        .next()
        .ok_or_else(|| "VNC 目标解析无结果。".to_string())?;
    tokio::time::timeout(VNC_CONNECT_TIMEOUT, tokio::net::TcpStream::connect(addr))
        .await
        .map_err(|_| "VNC 目标连接超时。".to_string())?
        .map_err(|e| format!("VNC 目标连接失败：{e}"))
}
// ---- helpers ----

fn find_first_executable(candidates: &[String]) -> Option<PathBuf> {
    candidates.iter().find_map(|candidate| {
        let path = PathBuf::from(candidate);
        if path.components().count() > 1 && path.is_file() {
            return Some(path);
        }
        find_on_path(candidate)
    })
}

fn find_custom_executable(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    if path.is_file() {
        Some(path)
    } else {
        find_on_path(trimmed)
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    if name.is_empty() || PathBuf::from(name).components().count() > 1 {
        return None;
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).find_map(|dir| {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
        None
    })
}

fn split_args(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Test helper: connects to a local bridge URL, performs the WebSocket
/// handshake, sends a masked binary frame, and returns the echoed payload.
pub async fn ws_roundtrip(url: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let rest = url
        .strip_prefix("ws://")
        .ok_or_else(|| "无效的 ws url".to_string())?;
    let host_port = rest
        .split('/')
        .next()
        .ok_or_else(|| "无效的 ws url".to_string())?;
    let mut parts = host_port.rsplitn(2, ':');
    let port = parts
        .next()
        .ok_or_else(|| "无效端口".to_string())?
        .parse::<u16>()
        .map_err(|e| e.to_string())?;
    let host = parts.next().unwrap_or("127.0.0.1").to_string();
    let path = rest.find('/').map(|idx| &rest[idx..]).unwrap_or("/");
    let mut stream = tokio::net::TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|e| format!("bridge 连接失败：{e}"))?;
    let (mut reader, mut writer) = stream.split();
    let key = "dGhlIHNhbXBsZSBub25jZQ==";
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    writer
        .write_all(request.as_bytes())
        .await
        .map_err(|e| format!("握手发送失败：{e}"))?;
    let mut response = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if reader.read(&mut byte).await.map_err(|e| e.to_string())? == 0 {
            return Err("握手响应被中断。".to_string());
        }
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
        if response.len() > 8192 {
            return Err("握手响应过大。".to_string());
        }
    }
    let response_text = String::from_utf8_lossy(&response);
    if !response_text.starts_with("HTTP/1.1 101") {
        return Err(format!("握手失败：{response_text}"));
    }
    // Send a masked binary frame.
    let len = payload.len();
    let mut frame = Vec::with_capacity(len + 14);
    frame.push(0x82);
    if len < 126 {
        frame.push(0x80 | len as u8);
    } else if len <= u16::MAX as usize {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    let mask = [0x12u8, 0x34, 0x56, 0x78];
    frame.extend_from_slice(&mask);
    for (i, byte) in payload.iter().enumerate() {
        frame.push(byte ^ mask[i % 4]);
    }
    writer
        .write_all(&frame)
        .await
        .map_err(|e| format!("数据帧发送失败：{e}"))?;
    // Read the server's unmasked binary echo frame.
    match read_ws_frame(&mut reader).await? {
        WsFrame::Data(data) => Ok(data),
        WsFrame::Ping | WsFrame::Close => Err("收到非数据帧。".to_string()),
    }
}

// ---- tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_reports_novnc_available() {
        let result = probe_runner(&json!({ "config": null }));
        assert!(result["available_runners"]
            .as_array()
            .expect("arr")
            .iter()
            .any(|r| r == "novnc"));
        assert_eq!(result["supports_embedded"], true);
    }

    #[test]
    fn external_plan_contains_target_and_flags() {
        let profile = json!({
            "id": "v1", "name": "vnc-host", "protocol": "vnc",
            "host": "10.0.0.9", "port": 5901, "username": "u",
            "vnc": {
                "display": { "scale_mode": "fit", "resize_session": true, "clip_viewport": false },
                "input": { "view_only": true, "clipboard": true, "shared": false },
                "performance": { "preset": "auto", "quality_level": null, "compression_level": null },
                "security": { "credential_mode": "prompt" },
                "runner": { "render_mode": "external", "preferred_runner": "vncviewer", "custom_executable": null, "custom_args_template": null },
                "raw_runner_args": null
            }
        });
        let vnc = profile["vnc"].clone();
        let selected = SelectedVncRunner {
            runner: "vncviewer".to_string(),
            executable: Some("vncviewer".to_string()),
            fallback_reason: None,
        };
        let plan = build_external_launch_plan(&profile, &vnc, &selected, true).expect("plan");
        assert!(plan.iter().any(|arg| arg == "10.0.0.9::5901"));
        assert!(plan.iter().any(|arg| arg == "-ViewOnly"));
    }

    #[test]
    fn embedded_selects_novnc() {
        let vnc = default_vnc_config();
        let selected = select_runner(&vnc).expect("select");
        assert_eq!(selected.runner, "novnc");
    }

    #[test]
    fn custom_missing_runner_errors() {
        let mut vnc = default_vnc_config();
        vnc["runner"]["render_mode"] = json!("custom");
        vnc["runner"]["custom_executable"] = json!("C:/definitely/missing-vnc.exe");
        assert!(select_runner(&vnc).is_err());
    }

    #[test]
    fn websocket_accept_matches_rfc_example() {
        // RFC 6455 example: key "dGhlIHNhbXBsZSBub25jZQ==" -> "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        assert_eq!(
            websocket_accept("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[tokio::test]
    async fn embedded_bridge_relays_to_vnc_target() {
        // Fake VNC echo server.
        let fake = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake");
        let fake_port = fake.local_addr().expect("addr").port();
        let fake_handle = tokio::spawn(async move {
            let (mut stream, _) = fake.accept().await.expect("fake accept");
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).await.expect("fake read");
            stream.write_all(&buf[..n]).await.expect("fake echo");
        });

        // Store a VNC connection pointing at the fake server.
        let dir = std::env::temp_dir().join(format!("vnc-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("v.db");
        let mut store = Store::open(&db).expect("store");
        store
            .upsert_connection(&json!({
                "id": "conn-vnc",
                "name": "vnc",
                "protocol": "vnc",
                "host": "127.0.0.1",
                "port": fake_port,
                "username": "u",
                "vnc": default_vnc_config(),
            }))
            .expect("upsert");
        let launched =
            launch_connection(&mut store, &json!({ "connection_id": "conn-vnc" })).expect("launch");
        assert_eq!(launched["embedded"].as_bool(), Some(true));
        let url = launched["websocket_url"].as_str().expect("url").to_string();
        let session_id = launched["session_id"].as_str().expect("sid").to_string();

        let payload = b"hello-vnc-echo";
        let echoed = ws_roundtrip(&url, payload).await.expect("roundtrip");
        assert_eq!(echoed, payload);

        let close = close_session(&json!({ "session_id": session_id }));
        assert_eq!(close["ok"].as_bool(), Some(true));
        let _ = fake_handle.await;
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
