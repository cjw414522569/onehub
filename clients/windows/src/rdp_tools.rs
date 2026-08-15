//! RDP session launching (mxterm parity T015).
//!
//! Windows-first: the external `mstsc.exe` runner is detected and used with a
//! generated .rdp file; the native ActiveX embedded host is not implemented in
//! this client, so embedded mode falls back to external mstsc with a clear
//! reason (mirroring mxterm's fallback behavior). Other platforms only keep
//! the approved interface boundary (probe reports the platform as unknown).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::store::Store;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_session_id() -> String {
    format!("rdp-{}-{:x}", std::process::id(), now_ms())
}

/// A launched (external) RDP session record.
#[derive(Clone)]
struct RdpSessionInfo {
    connection_id: String,
    process_id: u32,
    embedded: bool,
    rdp_file_path: Option<String>,
}

static SESSIONS: Mutex<Option<HashMap<String, RdpSessionInfo>>> = Mutex::new(None);

fn sessions_map() -> &'static Mutex<Option<HashMap<String, RdpSessionInfo>>> {
    &SESSIONS
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

/// rdp_test_runner: probes which RDP runners are available on this host.
pub fn probe_runner(request: &Value) -> Value {
    let config = request.get("config").cloned().unwrap_or(Value::Null);
    let platform = current_platform();
    let mut available_runners: Vec<String> = Vec::new();
    let mut default_runner: Option<String> = None;
    let mut default_executable: Option<String> = None;
    let mut setup_hint: Option<String> = None;

    let custom_executable = config
        .get("custom_executable")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let render_mode = config
        .get("render_mode")
        .and_then(Value::as_str)
        .unwrap_or("external");

    if !custom_executable.is_empty() && find_executable(&custom_executable).is_some() {
        available_runners.push("custom".to_string());
        if render_mode == "custom" {
            default_runner = Some("custom".to_string());
            default_executable = Some(custom_executable.clone());
        }
    }

    match platform {
        "windows" => {
            if let Some(mstscax) = find_mstscax() {
                available_runners.push("mstsc_activex".to_string());
                if default_runner.is_none() && render_mode == "embedded" {
                    default_runner = Some("mstsc_activex".to_string());
                    default_executable = Some(mstscax.to_string_lossy().to_string());
                }
            }
            if let Some(mstsc) = find_mstsc() {
                available_runners.push("mstsc".to_string());
                if default_runner.is_none() {
                    default_runner = Some("mstsc".to_string());
                    default_executable = Some(mstsc.to_string_lossy().to_string());
                }
            } else {
                setup_hint = Some("未找到 mstsc.exe，请确认系统远程桌面客户端可用。".to_string());
            }
            if !available_runners.iter().any(|r| r == "mstsc_activex") {
                setup_hint =
                    Some("未找到 mstscax.dll，嵌入式 RDP 将回退到外部 mstsc.exe。".to_string());
            }
        }
        "linux" => {
            if let Some(freerdp) = find_first_executable(&["wlfreerdp", "xfreerdp"]) {
                available_runners.push("freerdp".to_string());
                default_runner = Some("freerdp".to_string());
                default_executable = Some(freerdp.to_string_lossy().to_string());
            } else {
                setup_hint =
                    Some("未找到 wlfreerdp 或 xfreerdp，请安装 FreeRDP 客户端。".to_string());
            }
        }
        "macos" => {
            setup_hint = Some("未找到 macOS RDP 客户端适配（接口边界保留）。".to_string());
        }
        _ => {
            setup_hint = Some("当前平台的 RDP 客户端适配将在后续按平台单独启用。".to_string());
        }
    }

    let supports_embedded =
        platform == "windows" && available_runners.iter().any(|r| r == "mstsc_activex");
    json!({
        "platform": platform,
        "available_runners": available_runners,
        "default_runner": default_runner,
        "default_executable": default_executable,
        "supports_embedded": supports_embedded,
        "supports_remote_app": true,
        "supports_dynamic_resize": true,
        "setup_hint": setup_hint,
    })
}

/// rdp_preview_launch: builds the launch plan (args + .rdp content) without
/// executing it.
pub fn preview_launch(store: &Store, request: &Value) -> Result<Value, String> {
    let connection_id = request
        .get("connection_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let (profile, rdp) = resolve_rdp_connection(store, connection_id)?;
    let selected = select_runner(&rdp)?;
    let content = serialize_rdp_file(&profile, &rdp)?;
    let mut warnings = Vec::new();
    let fallback_reason = embedded_fallback_reason(&rdp);
    let (render_mode, runner, args) = match selected.runner.as_str() {
        "mstsc" => {
            if let Some(reason) = &fallback_reason {
                warnings.push(reason.clone());
            }
            (
                "external".to_string(),
                "mstsc".to_string(),
                vec!["<temp.rdp>".to_string()],
            )
        }
        "freerdp" => {
            let args = vec![
                format!(
                    "/v:{}:{}",
                    profile["host"].as_str().unwrap_or(""),
                    port_of(&profile)
                ),
                format!("/u:{}", profile["username"].as_str().unwrap_or("")),
            ];
            warnings
                .push("外部 FreeRDP runner 将自行提示凭据，不会通过命令行传递密码。".to_string());
            ("external".to_string(), "freerdp".to_string(), args)
        }
        "custom" => {
            let template = rdp
                .pointer("/runner/custom_args_template")
                .and_then(Value::as_str)
                .unwrap_or("{rdp_file}");
            let rendered = template
                .replace("{rdp_file}", "<temp.rdp>")
                .replace("{host}", profile["host"].as_str().unwrap_or(""))
                .replace("{port}", &port_of(&profile).to_string())
                .replace("{username}", profile["username"].as_str().unwrap_or(""));
            warnings
                .push("自定义 runner 参数模板已禁用密码占位符，请让客户端提示凭据。".to_string());
            (
                "external".to_string(),
                "custom".to_string(),
                split_args(&rendered),
            )
        }
        _ => {
            return Err("未找到可用的 RDP runner。".to_string());
        }
    };
    Ok(json!({
        "connection_id": connection_id,
        "runner": runner,
        "render_mode": render_mode,
        "executable": selected.executable,
        "args": args,
        "rdp_file_content": content,
        "fallback_reason": fallback_reason,
        "setup_hint": null,
        "warnings": warnings,
    }))
}

/// rdp_launch_connection: writes a temp .rdp file and starts the external
/// client (mstsc on Windows).
pub fn launch_connection(store: &mut Store, request: &Value) -> Result<Value, String> {
    let connection_id = request
        .get("connection_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let (profile, rdp) = resolve_rdp_connection(store, connection_id)?;
    let selected = select_runner(&rdp)?;
    let fallback_reason = embedded_fallback_reason(&rdp);

    let (args, rdp_file_path): (Vec<String>, Option<String>) = match selected.runner.as_str() {
        "mstsc" => {
            let content = serialize_rdp_file(&profile, &rdp)?;
            let path = write_temp_rdp_file(connection_id, &content)?;
            (
                vec![path.to_string_lossy().to_string()],
                Some(path.to_string_lossy().to_string()),
            )
        }
        "freerdp" => {
            let args = vec![
                format!(
                    "/v:{}:{}",
                    profile["host"].as_str().unwrap_or(""),
                    port_of(&profile)
                ),
                format!("/u:{}", profile["username"].as_str().unwrap_or("")),
            ];
            (args, None)
        }
        "custom" => {
            let content = serialize_rdp_file(&profile, &rdp)?;
            let path = write_temp_rdp_file(connection_id, &content)?;
            let template = rdp
                .pointer("/runner/custom_args_template")
                .and_then(Value::as_str)
                .unwrap_or("{rdp_file}");
            let rendered = template
                .replace("{rdp_file}", &path.to_string_lossy())
                .replace("{host}", profile["host"].as_str().unwrap_or(""))
                .replace("{port}", &port_of(&profile).to_string())
                .replace("{username}", profile["username"].as_str().unwrap_or(""));
            (
                split_args(&rendered),
                Some(path.to_string_lossy().to_string()),
            )
        }
        _ => return Err("未找到可用的 RDP runner。".to_string()),
    };

    let executable = selected
        .executable
        .ok_or_else(|| "RDP 客户端路径缺失。".to_string())?;
    let process_id = spawn_runner(&executable, &args)?;
    let session_id = new_session_id();
    sessions_map()
        .lock()
        .expect("sessions lock")
        .get_or_insert_with(HashMap::new)
        .insert(
            session_id.clone(),
            RdpSessionInfo {
                connection_id: connection_id.to_string(),
                process_id,
                embedded: false,
                rdp_file_path: rdp_file_path.clone(),
            },
        );

    Ok(json!({
        "session_id": session_id,
        "connection_id": connection_id,
        "launched": true,
        "embedded": false,
        "runner": selected.runner,
        "executable": executable,
        "args": args,
        "process_id": process_id,
        "rdp_file_path": rdp_file_path,
        "fallback_reason": fallback_reason,
        "setup_hint": null,
    }))
}

/// rdp_close_session: external sessions are managed by the external client
/// (mirroring mxterm); the record is dropped so a later reveal reports the
/// session as gone.
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
        Some(info) if info.embedded => json!({
            "ok": true,
            "message": format!("RDP 会话 {session_id} 已请求关闭。"),
        }),
        Some(info) => json!({
            "ok": false,
            "message": format!(
                "RDP 会话 {session_id}（连接 {}，pid {}，文件 {}）当前由外部客户端管理，请在客户端窗口中关闭。",
                info.connection_id, info.process_id, info.rdp_file_path.unwrap_or_default()
            ),
        }),
        None => json!({
            "ok": false,
            "message": format!("RDP 会话 {session_id} 不存在。"),
        }),
    }
}

/// rdp_reveal_session: embedded-only in mxterm; external sessions cannot be
/// raised from here.
pub fn reveal_session(request: &Value) -> Value {
    let session_id = request
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let found = sessions_map()
        .lock()
        .expect("sessions lock")
        .as_ref()
        .and_then(|m| m.get(session_id))
        .cloned();
    match found {
        Some(info) if info.embedded => json!({
            "ok": false,
            "message": format!("RDP 会话 {session_id} 当前没有可用宿主窗口。"),
        }),
        Some(info) => json!({
            "ok": false,
            "message": format!(
                "RDP 会话 {session_id}（连接 {}）当前不是可唤起的嵌入宿主。",
                info.connection_id
            ),
        }),
        None => json!({
            "ok": false,
            "message": format!("RDP 会话 {session_id} 不存在。"),
        }),
    }
}

/// rdp_resize_embedded_session: no embedded host in this client, so resizing
/// is reported as not applied.
pub fn resize_embedded_session(request: &Value) -> Value {
    let session_id = request
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let bounds = request.get("bounds").cloned().unwrap_or(Value::Null);
    let found = sessions_map()
        .lock()
        .expect("sessions lock")
        .as_ref()
        .and_then(|m| m.get(session_id))
        .cloned();
    match found {
        Some(info) if info.embedded => json!({
            "ok": true,
            "applied": true,
            "message": format!("已调整 RDP 会话 {session_id} 到 {bounds}。"),
        }),
        Some(_) => json!({
            "ok": false,
            "applied": false,
            "message": format!("RDP 会话 {session_id} 没有可调整的嵌入窗口。"),
        }),
        None => json!({
            "ok": false,
            "applied": false,
            "message": format!("RDP 会话 {session_id} 不存在。"),
        }),
    }
}

// ---- resolution & selection ----

fn resolve_rdp_connection(store: &Store, connection_id: &str) -> Result<(Value, Value), String> {
    let connection_id = connection_id.trim();
    if connection_id.is_empty() {
        return Err("请选择 RDP 连接。".to_string());
    }
    let profile = store
        .get_connection(connection_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "连接不存在。".to_string())?;
    let protocol = profile
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !protocol.is_empty() && protocol != "rdp" {
        return Err("该操作仅支持 RDP 连接。".to_string());
    }
    let rdp = profile.get("rdp").cloned().unwrap_or(Value::Null);
    let rdp = if rdp.is_null() {
        default_rdp_config()
    } else {
        rdp
    };
    Ok((profile, rdp))
}

fn default_rdp_config() -> Value {
    json!({
        "domain": null,
        "display": {
            "mode": "windowed",
            "width": null,
            "height": null,
            "dynamic_resize": true,
            "use_multimon": false,
        },
        "resources": {
            "clipboard": true,
            "audio": "local",
            "drives": false,
            "printers": false,
            "smart_cards": false,
        },
        "gateway": null,
        "remote_app": { "enabled": false, "program": null, "working_dir": null, "args": null },
        "performance": {
            "preset": "auto",
            "desktop_background": true,
            "font_smoothing": true,
            "visual_styles": true,
        },
        "security": {
            "credential_mode": "prompt",
            "nla": "auto",
            "certificate_policy": "prompt",
        },
        "runner": {
            "render_mode": "external",
            "preferred_runner": null,
            "custom_executable": null,
            "custom_args_template": null,
        },
        "raw_rdp_settings": null,
        "raw_runner_args": null,
    })
}

#[derive(Clone)]
struct SelectedRunner {
    runner: String,
    executable: Option<String>,
}

fn select_runner(rdp: &Value) -> Result<SelectedRunner, String> {
    let render_mode = rdp
        .pointer("/runner/render_mode")
        .and_then(Value::as_str)
        .unwrap_or("external");
    let preferred = rdp
        .pointer("/runner/preferred_runner")
        .and_then(Value::as_str);
    let custom_executable = rdp
        .pointer("/runner/custom_executable")
        .and_then(Value::as_str)
        .unwrap_or("");
    let platform = current_platform();

    if render_mode == "custom" || preferred == Some("custom") {
        let exe = if custom_executable.is_empty() {
            None
        } else {
            find_executable(custom_executable)
        };
        return match exe {
            Some(path) => Ok(SelectedRunner {
                runner: "custom".to_string(),
                executable: Some(path.to_string_lossy().to_string()),
            }),
            None => Err("请先配置自定义 RDP 客户端路径。".to_string()),
        };
    }

    match platform {
        "windows" => {
            if render_mode == "embedded" {
                if let Some(mstscax) = find_mstscax() {
                    let _ = mstscax;
                    // No ActiveX host in this client: fall back to mstsc.
                }
            }
            let mstsc = find_mstsc().ok_or_else(|| "未找到 mstsc.exe。".to_string())?;
            Ok(SelectedRunner {
                runner: "mstsc".to_string(),
                executable: Some(mstsc.to_string_lossy().to_string()),
            })
        }
        "linux" => {
            let freerdp = find_first_executable(&["wlfreerdp", "xfreerdp"])
                .ok_or_else(|| "未找到 FreeRDP 客户端。".to_string())?;
            Ok(SelectedRunner {
                runner: "freerdp".to_string(),
                executable: Some(freerdp.to_string_lossy().to_string()),
            })
        }
        _ => Err("当前平台暂不支持 RDP 启动（接口边界保留）。".to_string()),
    }
}

fn embedded_fallback_reason(rdp: &Value) -> Option<String> {
    let render_mode = rdp
        .pointer("/runner/render_mode")
        .and_then(Value::as_str)
        .unwrap_or("external");
    if render_mode != "embedded" {
        return None;
    }
    if current_platform() != "windows" {
        return Some("当前平台不支持 Windows RDP 嵌入式宿主，已使用外部 runner。".to_string());
    }
    let remote_app_enabled = rdp
        .pointer("/remote_app/enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if remote_app_enabled {
        return Some("RemoteApp 暂走 mstsc.exe 外部模式，避免嵌入宿主兼容性问题。".to_string());
    }
    Some("本客户端未内置 ActiveX 嵌入宿主，RDP 会话由外部 mstsc.exe 打开。".to_string())
}

// ---- rdp file serialization ----

fn port_of(profile: &Value) -> u64 {
    profile.get("port").and_then(Value::as_u64).unwrap_or(3389)
}

fn serialize_rdp_file(profile: &Value, rdp: &Value) -> Result<String, String> {
    let host = profile.get("host").and_then(Value::as_str).unwrap_or("");
    let username = profile
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or("");
    if host.is_empty() {
        return Err("RDP 连接缺少 host。".to_string());
    }
    let port = port_of(profile);
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("full address:s:{host}:{port}"));
    lines.push(format!("username:s:{username}"));
    if let Some(domain) = rdp.get("domain").and_then(Value::as_str) {
        if !domain.is_empty() {
            lines.push(format!("domain:s:{domain}"));
        }
    }
    let display_mode = rdp
        .pointer("/display/mode")
        .and_then(Value::as_str)
        .unwrap_or("windowed");
    let screen_mode = if matches!(display_mode, "fullscreen" | "all_monitors") {
        2
    } else {
        1
    };
    lines.push(format!("screen mode id:i:{screen_mode}"));
    if let Some(width) = rdp.pointer("/display/width").and_then(Value::as_u64) {
        lines.push(format!("desktopwidth:i:{width}"));
    }
    if let Some(height) = rdp.pointer("/display/height").and_then(Value::as_u64) {
        lines.push(format!("desktopheight:i:{height}"));
    }
    let use_multimon = rdp
        .pointer("/display/use_multimon")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    lines.push(format!(
        "use multimon:i:{}",
        if use_multimon { 1 } else { 0 }
    ));
    let preset = rdp
        .pointer("/performance/preset")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let session_bpp = if preset == "low_bandwidth" { 16 } else { 32 };
    lines.push(format!("session bpp:i:{session_bpp}"));
    let connection_type = match preset {
        "lan" => 7,
        "balanced" => 4,
        "low_bandwidth" => 2,
        _ => 6,
    };
    lines.push(format!("connection type:i:{connection_type}"));
    lines.push("networkautodetect:i:0".to_string());
    lines.push("bandwidthautodetect:i:0".to_string());
    let desktop_background = rdp
        .pointer("/performance/desktop_background")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let font_smoothing = rdp
        .pointer("/performance/font_smoothing")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let visual_styles = rdp
        .pointer("/performance/visual_styles")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    lines.push(format!(
        "disable wallpaper:i:{}",
        if desktop_background { 0 } else { 1 }
    ));
    lines.push(format!(
        "allow font smoothing:i:{}",
        if font_smoothing { 1 } else { 0 }
    ));
    lines.push(format!(
        "allow desktop composition:i:{}",
        if visual_styles { 1 } else { 0 }
    ));
    lines.push(format!(
        "disable full window drag:i:{}",
        if preset == "low_bandwidth" { 1 } else { 0 }
    ));
    lines.push(format!(
        "disable menu anims:i:{}",
        if preset == "low_bandwidth" { 1 } else { 0 }
    ));
    lines.push(format!(
        "disable themes:i:{}",
        if visual_styles { 0 } else { 1 }
    ));
    lines.push("disable cursor setting:i:0".to_string());
    lines.push("bitmapcachepersistenable:i:1".to_string());
    let clipboard = rdp
        .pointer("/resources/clipboard")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    lines.push(format!(
        "redirectclipboard:i:{}",
        if clipboard { 1 } else { 0 }
    ));
    let audio = rdp
        .pointer("/resources/audio")
        .and_then(Value::as_str)
        .unwrap_or("local");
    let audio_mode = match audio {
        "local" => 0,
        "remote" => 1,
        _ => 2,
    };
    lines.push(format!("audiomode:i:{audio_mode}"));
    let drives = rdp
        .pointer("/resources/drives")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    lines.push(format!("redirectdrives:i:{}", if drives { 1 } else { 0 }));
    let printers = rdp
        .pointer("/resources/printers")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    lines.push(format!(
        "redirectprinters:i:{}",
        if printers { 1 } else { 0 }
    ));
    let smart_cards = rdp
        .pointer("/resources/smart_cards")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    lines.push(format!(
        "redirectsmartcards:i:{}",
        if smart_cards { 1 } else { 0 }
    ));
    let nla = rdp
        .pointer("/security/nla")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    lines.push(format!(
        "enablecredsspsupport:i:{}",
        if nla == "disabled" { 0 } else { 1 }
    ));
    let cert_policy = rdp
        .pointer("/security/certificate_policy")
        .and_then(Value::as_str)
        .unwrap_or("prompt");
    let auth_level = match cert_policy {
        "trust" => 0,
        "strict" => 1,
        _ => 2,
    };
    lines.push(format!("authentication level:i:{auth_level}"));
    lines.push("prompt for credentials:i:1".to_string());
    if let Some(gateway) = rdp.get("gateway") {
        if let Some(host) = gateway.get("host").and_then(Value::as_str) {
            if !host.is_empty() {
                lines.push(format!("gatewayhostname:s:{host}"));
            }
        }
        let mode = gateway
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("disabled");
        let usage = match mode {
            "explicit" => 1,
            "auto" => 2,
            _ => 0,
        };
        lines.push(format!("gatewayusagemethod:i:{usage}"));
        lines.push("gatewaycredentialssource:i:4".to_string());
    }
    let remote_app_enabled = rdp
        .pointer("/remote_app/enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if remote_app_enabled {
        lines.push("remoteapplicationmode:i:1".to_string());
        if let Some(program) = rdp.pointer("/remote_app/program").and_then(Value::as_str) {
            lines.push(format!("remoteapplicationprogram:s:{program}"));
        }
        if let Some(working_dir) = rdp
            .pointer("/remote_app/working_dir")
            .and_then(Value::as_str)
        {
            lines.push(format!("remoteapplicationcmdline:s:{working_dir}"));
        }
    }
    if let Some(raw) = rdp.get("raw_rdp_settings").and_then(Value::as_str) {
        for line in raw.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
        }
    }
    Ok(format!("{}\r\n", lines.join("\r\n")))
}

fn write_temp_rdp_file(connection_id: &str, content: &str) -> Result<PathBuf, String> {
    let stem = connection_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let dir = std::env::temp_dir().join("onehub-rdp");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{stem}-{}.rdp", now_ms()));
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(path)
}

// ---- process helpers ----

fn spawn_runner(executable: &str, args: &[String]) -> Result<u32, String> {
    let child = std::process::Command::new(executable)
        .args(args)
        .spawn()
        .map_err(|e| format!("RDP 客户端启动失败：{e}"))?;
    let id = child.id();
    if id == 0 {
        return Err("无法获取 RDP 客户端进程 id。".to_string());
    }
    Ok(id)
}

fn find_mstsc() -> Option<PathBuf> {
    if let Some(path) = find_first_executable(&["mstsc.exe", "mstsc"]) {
        return Some(path);
    }
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| root.join("System32").join("mstsc.exe"))
        .filter(|path| path.is_file())
}

fn find_mstscax() -> Option<PathBuf> {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| root.join("System32").join("mstscax.dll"))
        .filter(|path| path.is_file())
        .or_else(|| find_first_executable(&["mstscax.dll"]))
}

fn find_first_executable(names: &[&str]) -> Option<PathBuf> {
    names.iter().find_map(|name| find_executable(name))
}

fn find_executable(name: &str) -> Option<PathBuf> {
    if let Some(path) = candidate_path(name) {
        if path.is_file() {
            return Some(path);
        }
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
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
    }
    None
}

fn candidate_path(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(name);
    if path.is_absolute() && path.is_file() {
        Some(path)
    } else {
        None
    }
}

fn split_args(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(|s| s.to_string()).collect()
}

// ---- tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_reports_windows_mstsc_when_present() {
        let result = probe_runner(&json!({ "config": null }));
        assert_eq!(result["platform"], "windows");
        assert!(!result["available_runners"]
            .as_array()
            .expect("arr")
            .is_empty());
        assert_eq!(result["supports_dynamic_resize"], true);
    }

    #[test]
    fn rdp_file_serialization_contains_address_and_settings() {
        let profile = json!({
            "id": "c1", "name": "win", "protocol": "rdp",
            "host": "10.0.0.5", "port": 3389, "username": "Administrator",
            "rdp": null,
        });
        let rdp = default_rdp_config();
        let content = serialize_rdp_file(&profile, &rdp).expect("rdp file");
        assert!(content.contains("full address:s:10.0.0.5:3389"));
        assert!(content.contains("username:s:Administrator"));
        assert!(content.contains("screen mode id:i:1"));
        assert!(content.contains("prompt for credentials:i:1"));
    }

    #[test]
    fn embedded_mode_reports_fallback_reason() {
        let mut rdp = default_rdp_config();
        rdp["runner"]["render_mode"] = json!("embedded");
        let reason = embedded_fallback_reason(&rdp);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("mstsc.exe"));
    }

    #[test]
    fn close_and_reveal_missing_session_reports_gone() {
        let close = close_session(&json!({ "session_id": "nope" }));
        assert_eq!(close["ok"], false);
        assert!(close["message"].as_str().expect("msg").contains("不存在"));
        let reveal = reveal_session(&json!({ "session_id": "nope" }));
        assert_eq!(reveal["ok"], false);
    }

    #[test]
    fn select_runner_requires_mstsc_on_windows() {
        let rdp = default_rdp_config();
        let selected = select_runner(&rdp);
        if cfg!(windows) {
            assert_eq!(selected.expect("selected").runner, "mstsc");
        } else {
            assert!(selected.is_err());
        }
    }
}
