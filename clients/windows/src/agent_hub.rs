//! Agent Hub basics (T047). A workspace pane that lists live terminal agents
//! (SSH + local shells) side by side with a local project file tree. Agents
//! can be started (local shell) and stopped; the file tree lists directories
//! with traversal protection so the UI can navigate a project.

use serde_json::{json, Value};
use std::path::PathBuf;

/// Available agent kinds (agent_start). Each maps to a local shell launch.
pub const AGENT_KINDS: &[(&str, &str)] = &[
    ("powershell", "PowerShell"),
    ("cmd", "命令提示符"),
    ("wsl", "WSL"),
];

/// Lists live terminal agents: active SSH + local sessions merged with the
/// available local-shell agent kinds (agent_list).
pub fn agent_list() -> Value {
    let mut agents: Vec<Value> = Vec::new();
    for session_id in crate::ssh_terminal::active_session_ids() {
        agents.push(json!({
            "id": session_id,
            "kind": "ssh",
            "name": format!("SSH 会话 {session_id}"),
            "status": "running",
            "terminal": true,
        }));
    }
    for session_id in crate::local_sessions::active_local_session_ids() {
        agents.push(json!({
            "id": session_id,
            "kind": "local",
            "name": format!("本地会话 {session_id}"),
            "status": "running",
            "terminal": true,
        }));
    }
    for (kind, label) in AGENT_KINDS {
        agents.push(json!({
            "id": format!("agent-{kind}"),
            "kind": kind,
            "name": label,
            "status": "available",
            "terminal": false,
        }));
    }
    json!(agents)
}

/// Starts a local shell agent (agent_start). Reuses the local-session
/// infrastructure and returns the new session id.
pub fn agent_start(kind: &str) -> Result<Value, String> {
    let (command, args) = match kind {
        "powershell" => {
            let system_root =
                std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
            let program_files =
                std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
            let pwsh = format!("{program_files}\\PowerShell\\7\\pwsh.exe");
            let winps = format!("{system_root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
            if std::path::Path::new(&pwsh).exists() {
                (pwsh, Vec::new())
            } else {
                (winps, Vec::new())
            }
        }
        "cmd" => {
            let system_root =
                std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
            (format!("{system_root}\\System32\\cmd.exe"), Vec::new())
        }
        "wsl" => {
            let system_root =
                std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
            (format!("{system_root}\\System32\\wsl.exe"), Vec::new())
        }
        other => return Err(format!("未知 Agent 类型：{other}")),
    };
    // Plain shell launches get the T044 shell-integration prompt.
    let mut effective_args = args;
    if effective_args.is_empty() {
        for arg in crate::local_sessions::shell_integration_args(kind) {
            if !effective_args.contains(&arg) {
                effective_args.push(arg);
            }
        }
    }
    let id = crate::local_sessions::open_local(&command, &effective_args, None, None, 80, 24)?;
    Ok(json!({ "id": id, "kind": kind, "status": "running" }))
}

/// Stops an agent session (agent_stop).
pub fn agent_stop(session_id: &str) -> Value {
    let stopped =
        crate::local_sessions::close_session(session_id) || crate::ssh_terminal::close(session_id);
    json!({ "id": session_id, "stopped": stopped })
}

/// Normalizes a path by resolving "." and ".." components.
fn normalize_path(path: &std::path::Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolves the project root: an explicit path, else the current directory,
/// else a sane fallback.
fn project_root(request: &Value) -> Result<PathBuf, String> {
    let explicit = request
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !explicit.is_empty() {
        let path = PathBuf::from(explicit);
        if !path.is_dir() {
            return Err("目录不存在。".to_string());
        }
        return Ok(path);
    }
    if let Ok(cwd) = std::env::current_dir() {
        if cwd.is_dir() {
            return Ok(cwd);
        }
    }
    Err("无法定位项目目录。".to_string())
}

/// Lists a project directory's entries (agent_project_files). Entries are
/// resolved relative to the requested root and never escape it.
pub fn agent_project_files(request: &Value) -> Result<Value, String> {
    let root = project_root(request)?;
    let relative = request
        .get("relative")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let dir = if relative.is_empty() {
        root.clone()
    } else {
        // Normalize components so ".." cannot escape the root lexically.
        let candidate = normalize_path(&root.join(relative));
        if !candidate.starts_with(&root) {
            return Err("非法路径。".to_string());
        }
        candidate
    };
    if !dir.is_dir() {
        return Err("目录不存在。".to_string());
    }
    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(&dir).map_err(|e| format!("读取目录失败：{e}"))?;
    for entry in read_dir {
        let entry = entry.map_err(|e| format!("读取目录失败：{e}"))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("读取类型失败：{e}"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = file_type.is_dir();
        let size = if is_dir {
            0
        } else {
            entry.metadata().map(|m| m.len()).unwrap_or(0)
        };
        let rel = if relative.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", relative.trim_end_matches('/'), name)
        };
        entries.push(json!({
            "name": name,
            "relative": rel,
            "type": if is_dir { "directory" } else { "file" },
            "size": size,
        }));
    }
    entries.sort_by(|a, b| {
        let a_dir = a["type"] == "directory";
        let b_dir = b["type"] == "directory";
        b_dir.cmp(&a_dir).then_with(|| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
        })
    });
    Ok(json!({
        "root": root.to_string_lossy(),
        "relative": relative,
        "entries": entries,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_list_includes_kinds() {
        let agents = agent_list();
        let arr = agents.as_array().expect("array");
        let kinds: Vec<&str> = arr.iter().filter_map(|a| a["kind"].as_str()).collect();
        assert!(kinds.contains(&"powershell"), "got {kinds:?}");
        assert!(kinds.contains(&"cmd"));
        assert!(arr.iter().any(|a| a["status"] == "available"));
    }

    #[test]
    fn agent_start_unknown_kind_fails() {
        let err = agent_start("nosuch").expect_err("unknown kind");
        assert!(err.contains("未知 Agent 类型"), "got {err:?}");
    }

    #[test]
    fn agent_project_files_lists_dir_and_blocks_escape() {
        let dir = std::env::temp_dir().join(format!("onehub-agenthub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create");
        std::fs::write(dir.join("a.txt"), b"hello").expect("write");
        std::fs::create_dir_all(dir.join("sub")).expect("mkdir");

        let listed = agent_project_files(&json!({ "path": dir.to_string_lossy() })).expect("list");
        let entries = listed["entries"].as_array().expect("entries");
        assert!(entries
            .iter()
            .any(|e| e["name"] == "a.txt" && e["type"] == "file"));
        assert!(entries
            .iter()
            .any(|e| e["name"] == "sub" && e["type"] == "directory"));

        let escaped = agent_project_files(&json!({
            "path": dir.to_string_lossy(),
            "relative": "../escape",
        }))
        .expect_err("escape blocked");
        assert!(escaped.contains("非法路径"), "got {escaped:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn agent_start_cmd_roundtrip() {
        let started = agent_start("cmd").expect("start cmd agent");
        let id = started["id"].as_str().unwrap_or("").to_string();
        assert!(id.starts_with("local-"), "got {id:?}");
        let stopped = agent_stop(&id);
        assert_eq!(stopped["stopped"], true);
    }
}
