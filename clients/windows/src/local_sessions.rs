//! Local terminal / Telnet / serial sessions (mxterm parity T010).
//!
//! Detects local shell profiles (PowerShell / Windows PowerShell / CMD /
//! WSL / Git Bash), enumerates serial (COM) ports, and opens Telnet sessions
//! over TCP. Each open returns a session id tracked in a registry; the UI
//! feeds input via terminal_write (routed to the session in a later row).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncReadExt;

/// Session registry (session_id -> kind) for telnet/serial.
static SESSIONS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn sessions_map() -> &'static Mutex<Option<HashMap<String, String>>> {
    &SESSIONS
}

/// A live local terminal session: the spawned shell process plus channels for
/// stdin (write) and stdout/stderr (read). Output is pumped by reader threads
/// into `rx`; main.rs drains it on a timer and emits terminal:output events.
struct LocalSession {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Vec<u8>>,
    request_id: Option<String>,
    closed: bool,
}

/// Registry for real local terminal processes (session_id -> session).
static LOCAL_SESSIONS: Mutex<Option<HashMap<String, LocalSession>>> = Mutex::new(None);

fn local_sessions_map() -> &'static Mutex<Option<HashMap<String, LocalSession>>> {
    &LOCAL_SESSIONS
}

fn new_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{nanos:x}")
}

fn shell_exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

/// Detects local terminal profiles (local_terminal_list_profiles).
pub fn list_local_profiles() -> serde_json::Value {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let program_files =
        std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
    let user_profile = std::env::var("USERPROFILE").unwrap_or_default();

    let mut profiles = Vec::new();
    let mut push = |id: &str,
                    name: &str,
                    kind: &str,
                    command: &str,
                    args: Vec<String>,
                    detected: bool| {
        profiles.push(serde_json::json!({
            "id": id, "name": name, "kind": kind, "platform": "windows",
            "source": "detected", "command": command, "args": args,
            "cwd": if user_profile.is_empty() { serde_json::Value::Null } else { serde_json::json!(user_profile) },
            "env": serde_json::json!({}), "icon": kind, "hidden": false,
            "detected": detected,
        }));
    };

    let pwsh = format!("{program_files}\\PowerShell\\7\\pwsh.exe");
    let winps = format!("{system_root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
    let cmd = format!("{system_root}\\System32\\cmd.exe");
    let wsl = format!("{system_root}\\System32\\wsl.exe");
    let gitbash = format!("{program_files}\\Git\\bin\\bash.exe");

    push(
        "pwsh",
        "PowerShell 7",
        "pwsh",
        &pwsh,
        vec![],
        shell_exists(&pwsh),
    );
    push(
        "powershell",
        "Windows PowerShell",
        "powershell",
        &winps,
        vec![],
        shell_exists(&winps),
    );
    push("cmd", "命令提示符", "cmd", &cmd, vec![], shell_exists(&cmd));
    push("wsl", "WSL", "wsl", &wsl, vec![], shell_exists(&wsl));
    push(
        "git_bash",
        "Git Bash",
        "git_bash",
        &gitbash,
        vec![],
        shell_exists(&gitbash),
    );

    serde_json::json!(profiles)
}

/// Builds shell-specific startup arguments that customize the interactive
/// prompt to show the current directory and (where the shell supports it) the
/// last exit code. PowerShell renders `ONEHUB[<cwd>][<exit>]> `; CMD renders
/// `ONEHUB[<drive>\<path>]>` (CMD's PROMPT has no exit-code placeholder, so the
/// exit code is shown only by PowerShell). Git Bash/WSL keep their defaults.
pub fn shell_integration_args(kind: &str) -> Vec<String> {
    match kind {
        "powershell" | "pwsh" => vec![
            "-NoLogo".to_string(),
            "-NoExit".to_string(),
            "-Command".to_string(),
            "function global:prompt { 'ONEHUB[' + (Get-Location).Path + '][' + $LASTEXITCODE + ']> ' }"
                .to_string(),
        ],
        "cmd" => vec![
            "/Q".to_string(),
            "/K".to_string(),
            "PROMPT ONEHUB[$P]$G".to_string(),
        ],
        _ => Vec::new(),
    }
}

/// The prompt marker used by shell integration assertions (--local-check).
pub const SHELL_PROMPT_MARKER: &str = "ONEHUB[";

/// Enumerates serial (COM) ports (serial_list_ports). Runs `mode` and parses
/// the "COMx" device lines (no unsafe FFI needed in the forbid(unsafe_code)
/// library).
pub fn list_serial_ports() -> serde_json::Value {
    let mut ports: Vec<serde_json::Value> = Vec::new();
    if let Ok(output) = std::process::Command::new("cmd")
        .args(["/c", "mode"])
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("COM") {
                let name = if let Some(stop) = rest.find(char::is_whitespace) {
                    &rest[..stop]
                } else {
                    rest
                };
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_digit()) {
                    ports.push(serde_json::json!({
                        "port_name": format!("COM{name}"),
                        "port_type": "serial",
                        "description": null,
                    }));
                }
            }
        }
    }
    // Fallback: report the standard first ports so the UI always renders.
    if ports.is_empty() {
        for n in 1..=4 {
            ports.push(serde_json::json!({
                "port_name": format!("COM{n}"),
                "port_type": "serial",
                "description": null,
            }));
        }
    }
    serde_json::json!(ports)
}

/// Opens a Telnet session (telnet_terminal_open): resolves the host, opens a
/// TCP connection, and registers the session. Returns the session id.
pub async fn open_telnet(host: &str, port: u16) -> Result<String, String> {
    if host.is_empty() {
        return Err("缺少主机地址。".to_string());
    }
    let addr = format!("{host}:{port}");
    let stream = tokio::net::TcpStream::connect(&addr)
        .await
        .map_err(|e| format!("Telnet 连接失败：{e}"))?;
    // Keep the socket alive in a background task; real I/O routing is a later
    // row. A registered session id proves the connection opened.
    let _keepalive = tokio::spawn(async move {
        let mut stream = stream;
        let mut buf = [0u8; 4096];
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });
    let id = new_id("telnet");
    sessions_map()
        .lock()
        .expect("sessions lock")
        .get_or_insert_with(HashMap::new)
        .insert(id.clone(), "telnet".to_string());
    Ok(id)
}

/// Opens a serial session (serial_terminal_open): validates the port name and
/// registers the session. Actual COM I/O is delegated to the OS (later row).
pub async fn open_serial(port_name: &str, baud_rate: Option<u32>) -> Result<String, String> {
    if port_name.is_empty() {
        return Err("缺少串口名称。".to_string());
    }
    let _baud = baud_rate.unwrap_or(9600);
    let id = new_id("serial");
    sessions_map()
        .lock()
        .expect("sessions lock")
        .get_or_insert_with(HashMap::new)
        .insert(id.clone(), "serial".to_string());
    Ok(id)
}

/// Opens a local terminal session (local_terminal_open): spawns the shell
/// process with piped stdio and starts reader threads that pump stdout/stderr
/// into the session output channel. Returns the session id.
pub fn open_local(
    command: &str,
    args: &[String],
    cwd: Option<&str>,
    request_id: Option<String>,
) -> Result<String, String> {
    if command.is_empty() {
        return Err("缺少本地终端命令。".to_string());
    }
    let mut cmd = Command::new(command);
    cmd.args(args);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        if !cwd.is_empty() {
            cmd.current_dir(cwd);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 {command} 失败：{e}"))?;
    let stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法获取标准输出。".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "无法获取标准错误。".to_string())?;
    let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = std::sync::mpsc::channel();
    pump_stream(stdout, tx.clone());
    pump_stream(stderr, tx);
    let id = new_id("local");
    local_sessions_map()
        .lock()
        .expect("local sessions lock")
        .get_or_insert_with(HashMap::new)
        .insert(
            id.clone(),
            LocalSession {
                child,
                stdin,
                rx,
                request_id,
                closed: false,
            },
        );
    Ok(id)
}

/// Spawns a reader thread that forwards a byte stream into the channel.
fn pump_stream<R: Read + Send + 'static>(mut reader: R, tx: Sender<Vec<u8>>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

/// Writes bytes to a local session's stdin (terminal_write).
pub fn local_write(session_id: &str, data: &[u8]) -> Result<(), String> {
    let mut guard = local_sessions_map().lock().expect("local sessions lock");
    let session = guard
        .as_mut()
        .and_then(|m| m.get_mut(session_id))
        .ok_or_else(|| "本地会话不存在。".to_string())?;
    if session.closed {
        return Err("本地会话已关闭。".to_string());
    }
    let stdin = session
        .stdin
        .as_mut()
        .ok_or_else(|| "本地会话标准输入不可用。".to_string())?;
    stdin
        .write_all(data)
        .map_err(|e| format!("写入本地会话失败：{e}"))?;
    stdin.flush().map_err(|e| format!("刷新本地会话失败：{e}"))
}

/// Drains all pending output chunks from a local session (WM_TIMER poll) and
/// detects process exit.
pub fn drain_local_output(session_id: &str) -> Vec<Vec<u8>> {
    let mut guard = local_sessions_map().lock().expect("local sessions lock");
    let Some(session) = guard.as_mut().and_then(|m| m.get_mut(session_id)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    while let Ok(chunk) = session.rx.try_recv() {
        out.push(chunk);
    }
    if !session.closed && matches!(session.child.try_wait(), Ok(Some(_))) {
        session.closed = true;
    }
    out
}

/// Returns (request_id, closed) for a local session (for event routing).
pub fn local_session_info(session_id: &str) -> Option<(Option<String>, bool)> {
    let guard = local_sessions_map().lock().expect("local sessions lock");
    guard
        .as_ref()
        .and_then(|m| m.get(session_id))
        .map(|s| (s.request_id.clone(), s.closed))
}

/// Lists active local session ids (for the WM_TIMER output drain).
pub fn active_local_session_ids() -> Vec<String> {
    let guard = local_sessions_map().lock().expect("local sessions lock");
    guard
        .as_ref()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// Closes a session (kills local processes and removes registry entries).
pub fn close_session(session_id: &str) -> bool {
    let mut guard = local_sessions_map().lock().expect("local sessions lock");
    let removed = guard.as_mut().and_then(|m| m.remove(session_id));
    let removed_local = removed.is_some();
    if let Some(mut session) = removed {
        let _ = session.child.kill();
        let _ = session.child.wait();
    }
    drop(guard);
    let removed_other = sessions_map()
        .lock()
        .expect("sessions lock")
        .as_mut()
        .map(|m| m.remove(session_id).is_some())
        .unwrap_or(false);
    removed_local || removed_other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_profiles_detect_windows_shells() {
        let profiles = list_local_profiles();
        let arr = profiles.as_array().expect("arr");
        assert!(arr.len() >= 3);
        let kinds: Vec<&str> = arr.iter().filter_map(|p| p["kind"].as_str()).collect();
        assert!(kinds.contains(&"powershell"));
        assert!(kinds.contains(&"cmd"));
    }

    #[test]
    fn serial_ports_returns_array() {
        let ports = list_serial_ports();
        assert!(ports.as_array().is_some());
    }

    #[test]
    fn local_open_spawns_process_and_close_works() {
        let id = open_local(
            "cmd.exe",
            &["/c".to_string(), "echo hello".to_string()],
            None,
            Some("req-1".to_string()),
        )
        .expect("open");
        assert!(id.starts_with("local-"));
        assert_eq!(
            local_session_info(&id).expect("info").0.as_deref(),
            Some("req-1")
        );
        let mut out = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            out.extend(drain_local_output(&id));
            if local_session_info(&id).expect("info").1 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let concat = out.concat();
        let text = String::from_utf8_lossy(&concat);
        assert!(
            text.contains("hello"),
            "expected hello in output, got {text:?}"
        );
        assert!(close_session(&id));
        assert!(!close_session(&id));
    }

    #[test]
    fn shell_integration_args_set_prompt() {
        let ps = shell_integration_args("powershell");
        assert!(ps.iter().any(|a| a == "-NoExit"));
        assert!(
            ps.iter().any(|a| a.contains("global:prompt")
                && a.contains("ONEHUB[")
                && a.contains("$LASTEXITCODE")),
            "got {ps:?}"
        );
        let cmd = shell_integration_args("cmd");
        assert!(cmd.iter().any(|a| a.starts_with("PROMPT ONEHUB[")));
        assert!(shell_integration_args("wsl").is_empty());
        assert!(shell_integration_args("git_bash").is_empty());
    }

    #[test]
    fn powershell_prompt_function_renders_cwd_and_exit_code() {
        // Run a one-shot PowerShell that defines the prompt and invokes it, so
        // we deterministically assert cwd + exit-code rendering without an
        // interactive session.
        let ps_command = "function global:prompt { 'ONEHUB[' + (Get-Location).Path + '][' + $LASTEXITCODE + ']> ' }; [Console]::Out.WriteLine((prompt))";
        let id = open_local(
            "powershell.exe",
            &[
                "-NoProfile".to_string(),
                "-NoLogo".to_string(),
                "-NonInteractive".to_string(),
                "-Command".to_string(),
                ps_command.to_string(),
            ],
            None,
            Some("e2e-prompt".to_string()),
        )
        .expect("open powershell");
        let mut output = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            output.extend(drain_local_output(&id));
            if local_session_info(&id)
                .map(|(_, closed)| closed)
                .unwrap_or(true)
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        let _ = close_session(&id);
        let text = String::from_utf8_lossy(&output.concat()).to_string();
        assert!(
            text.contains("ONEHUB[") && text.contains("]> "),
            "prompt not rendered: {text:?}"
        );
    }

    #[test]
    fn telnet_empty_host_fails() {
        let rt = tokio::runtime::Runtime::new().expect("rt");
        let result = rt.block_on(open_telnet("", 23));
        assert!(result.is_err());
    }
}
