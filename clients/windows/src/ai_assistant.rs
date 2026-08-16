//! AI assistant (mxterm parity T013).
//!
//! Provider configs and chat sessions persist in the local SQLite store; the
//! API key is stored encrypted via the vault. Chat streaming calls an
//! OpenAI-compatible `/chat/completions` endpoint (synchronous, non-stream for
//! the first pass) and emits AiChatStreamEvent-shaped records. Command
//! assessment uses a local heuristic so it works without an API key.

use std::collections::HashMap;
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

fn now_str() -> String {
    now_ms().to_string()
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}-{}-{:x}", std::process::id(), now_ms())
}

/// A running chat stream registry (stream_id -> (session_id, assistant msg id)).
static STREAMS: Mutex<Option<HashMap<String, (String, String)>>> = Mutex::new(None);

fn streams_map() -> &'static Mutex<Option<HashMap<String, (String, String)>>> {
    &STREAMS
}

fn provider_json(id: &str, request: &Value, now: &str, existing: Option<&Value>) -> Value {
    json!({
        "id": id,
        "name": request.get("name").and_then(Value::as_str).unwrap_or("AI Provider"),
        "provider": request.get("provider").and_then(Value::as_str).unwrap_or("openai"),
        "api_format": request.get("api_format").and_then(Value::as_str).unwrap_or("openai_compatible"),
        "endpoint": request.get("endpoint").and_then(Value::as_str).unwrap_or(""),
        "model": request.get("model").and_then(Value::as_str).unwrap_or(""),
        "api_key_saved": existing
            .and_then(|e| e.get("api_key_saved").and_then(Value::as_bool))
            .unwrap_or(false),
        "created_at": existing.and_then(|e| e.get("created_at").and_then(Value::as_str)).unwrap_or(now),
        "updated_at": now,
    })
}

/// Lists AI provider configs (ai_provider_config_list).
pub fn list_providers(store: &Store) -> Result<Value, String> {
    let items = store.list_ai_providers().map_err(|e| e.to_string())?;
    Ok(json!(items))
}

/// Saves an AI provider config (ai_provider_config_save). The API key, when
/// provided, is stored in the vault (encrypted at rest).
pub fn save_provider(store: &mut Store, request: &Value) -> Result<Value, String> {
    let now = now_str();
    let existing_id = request.get("id").and_then(Value::as_str);
    let existing = existing_id.and_then(|id| store.get_ai_provider(id).ok().flatten());
    let id = existing_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| new_id("ai"));
    let mut provider = provider_json(&id, request, &now, existing.as_ref());

    let api_key = request.get("api_key").and_then(Value::as_str).unwrap_or("");
    let touched = request
        .get("api_key_touched")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if touched && !api_key.is_empty() {
        let key = local_secret_key(store)?;
        let encrypted = encrypt_key(&key, api_key.as_bytes())?;
        provider["api_key_saved"] = json!(true);
        provider["api_key_encrypted"] = json!(encrypted);
    } else if let Some(existing) = &existing {
        if let Some(enc) = existing.get("api_key_encrypted") {
            provider["api_key_encrypted"] = enc.clone();
        }
    }
    store
        .put_ai_provider(&id, &provider)
        .map_err(|e| e.to_string())?;
    let mut out = provider.clone();
    out["api_key_encrypted"] = Value::Null;
    Ok(out)
}

/// Deletes an AI provider config (ai_provider_config_delete).
pub fn delete_provider(store: &mut Store, id: &str) -> Result<Value, String> {
    let _ = store.delete_ai_provider(id);
    Ok(Value::Null)
}

/// Reveals an API key (ai_provider_config_reveal_api_key) from the vault.
pub fn reveal_api_key(store: &mut Store, id: &str) -> Result<Value, String> {
    let provider = store
        .get_ai_provider(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "配置不存在。".to_string())?;
    let enc = provider
        .get("api_key_encrypted")
        .and_then(Value::as_str)
        .ok_or_else(|| "未保存 API key。".to_string())?;
    let key = local_secret_key(store)?;
    let plain = decrypt_key(&key, enc)?;
    Ok(json!({ "api_key": String::from_utf8_lossy(&plain).to_string() }))
}

/// Tests a provider config (ai_provider_config_test) with a minimal chat
/// completion request.
pub fn test_provider(request: &Value) -> Result<Value, String> {
    let endpoint = request
        .get("endpoint")
        .and_then(Value::as_str)
        .unwrap_or("");
    let api_key = request.get("api_key").and_then(Value::as_str).unwrap_or("");
    if endpoint.is_empty() || api_key.is_empty() {
        return Ok(json!({ "message": "端点或 API key 缺失，无法测试。" }));
    }
    let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));
    let body = json!({
        "model": request.get("model").and_then(Value::as_str).unwrap_or("gpt-4o-mini"),
        "messages": [{ "role": "user", "content": "ping" }],
        "max_tokens": 8,
    });
    match call_openai(&url, api_key, &body) {
        Ok(_) => Ok(json!({ "message": "连接成功。".to_string() })),
        Err(e) => Err(format!("连接失败：{e}")),
    }
}

/// Lists provider models (ai_provider_models_list). Returns a curated list for
/// known providers plus a generic fallback.
pub fn models_list(request: &Value) -> Result<Value, String> {
    let provider = request
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("openai");
    let items: Vec<Value> = match provider {
        "openai" => vec![
            json!({"id": "gpt-4o-mini", "display_name": "GPT-4o mini"}),
            json!({"id": "gpt-4o", "display_name": "GPT-4o"}),
            json!({"id": "gpt-4.1", "display_name": "GPT-4.1"}),
            json!({"id": "o3-mini", "display_name": "o3-mini"}),
        ],
        "claude" => vec![
            json!({"id": "claude-3-5-sonnet-latest", "display_name": "Claude 3.5 Sonnet"}),
            json!({"id": "claude-3-5-haiku-latest", "display_name": "Claude 3.5 Haiku"}),
        ],
        _ => vec![json!({"id": "default", "display_name": "默认模型"})],
    };
    Ok(json!(items))
}

/// Lists chat sessions (ai_chat_session_list).
pub fn list_sessions(store: &Store) -> Result<Value, String> {
    let items = store.list_ai_sessions().map_err(|e| e.to_string())?;
    Ok(json!(items))
}

/// Gets a chat session (ai_chat_session_get).
pub fn get_session(store: &Store, session_id: &str) -> Result<Value, String> {
    let session = store
        .get_ai_session(session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "会话不存在。".to_string())?;
    Ok(session)
}

/// Deletes a chat session (ai_chat_session_delete).
pub fn delete_session(store: &mut Store, session_id: &str) -> Result<Value, String> {
    let _ = store.delete_ai_session(session_id);
    Ok(Value::Null)
}

/// Clears a chat session (ai_chat_session_clear): empties messages.
pub fn clear_session(store: &mut Store, session_id: &str) -> Result<Value, String> {
    let mut session = store
        .get_ai_session(session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "会话不存在。".to_string())?;
    session["messages"] = json!([]);
    session["summary"]["message_count"] = json!(0);
    store
        .put_ai_session(session_id, &session)
        .map_err(|e| e.to_string())?;
    Ok(session)
}

/// Starts a chat stream (ai_chat_stream_start): appends the user message,
/// calls the provider, and registers a stream record.
pub fn stream_start(store: &mut Store, request: &Value) -> Result<Value, String> {
    let provider_config_id = request
        .get("provider_config_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let content = request.get("content").and_then(Value::as_str).unwrap_or("");
    let provider = store
        .get_ai_provider(provider_config_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "AI 配置不存在。".to_string())?;
    let endpoint = provider
        .get("endpoint")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let model = provider
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let api_key = {
        let enc = provider
            .get("api_key_encrypted")
            .and_then(Value::as_str)
            .unwrap_or("");
        if enc.is_empty() {
            String::new()
        } else {
            let key = local_secret_key(store)?;
            String::from_utf8_lossy(&decrypt_key(&key, enc)?).to_string()
        }
    };

    // Ensure a session exists.
    let session_id = request
        .get("session_id")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .unwrap_or_else(|| new_id("chat"));
    let mut session = store
        .get_ai_session(&session_id)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| {
            json!({
                "summary": {
                    "id": session_id,
                    "title": content.chars().take(40).collect::<String>(),
                    "provider_config_id": provider_config_id,
                    "message_count": 0,
                    "last_message_preview": null,
                    "created_at": now_str(),
                    "updated_at": now_str(),
                },
                "messages": [],
            })
        });

    let user_msg_id = new_id("msg");
    session["messages"]
        .as_array_mut()
        .expect("messages array")
        .push(json!({
            "id": user_msg_id,
            "session_id": session_id,
            "role": "user",
            "content": content,
            "contexts": [],
            "commands": [],
            "status": "complete",
            "created_at": now_str(),
            "updated_at": now_str(),
        }));

    let assistant_msg_id = new_id("msg");
    // Call the provider (best-effort; failure records an error message).
    let assistant_text = if endpoint.is_empty() || api_key.is_empty() {
        "(未配置 API key，无法调用模型。)".to_string()
    } else {
        let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));
        let body = json!({
            "model": model,
            "messages": [{ "role": "user", "content": content }],
        });
        match call_openai(&url, &api_key, &body) {
            Ok(reply) => reply,
            Err(e) => format!("(调用失败：{e})"),
        }
    };
    session["messages"]
        .as_array_mut()
        .expect("messages array")
        .push(json!({
            "id": assistant_msg_id,
            "session_id": session_id,
            "role": "assistant",
            "content": assistant_text,
            "contexts": [],
            "commands": extract_command_suggestions(&assistant_text),
            "status": "complete",
            "created_at": now_str(),
            "updated_at": now_str(),
        }));
    session["summary"]["message_count"] =
        json!(session["messages"].as_array().map(|a| a.len()).unwrap_or(0));
    session["summary"]["last_message_preview"] =
        json!(assistant_text.chars().take(80).collect::<String>());
    session["summary"]["updated_at"] = json!(now_str());
    store
        .put_ai_session(&session_id, &session)
        .map_err(|e| e.to_string())?;

    let stream_id = new_id("stream");
    streams_map()
        .lock()
        .expect("streams lock")
        .get_or_insert_with(HashMap::new)
        .insert(
            stream_id.clone(),
            (session_id.clone(), assistant_msg_id.clone()),
        );

    Ok(json!({
        "stream_id": stream_id,
        "session_id": session_id,
        "user_message_id": user_msg_id,
        "assistant_message_id": assistant_msg_id,
    }))
}

/// Stops a chat stream (ai_chat_stream_stop).
pub fn stream_stop(stream_id: &str) -> Result<Value, String> {
    let _ = streams_map()
        .lock()
        .expect("streams lock")
        .as_mut()
        .and_then(|m| m.remove(stream_id))
        .ok_or_else(|| "流不存在。".to_string())?;
    Ok(Value::Null)
}

/// Assesses a command's risk locally (ai_command_assess).
pub fn assess_command(command: &str) -> Result<Value, String> {
    let (risk, reasons) = assess_command_impl(command);
    Ok(json!({ "command": command, "risk": risk, "reasons": reasons }))
}

/// Core heuristic shared by `ai_command_assess` and command-suggestion
/// extraction. Mirrors mxterm's local, offline `assess_command`.
fn assess_command_impl(command: &str) -> (&'static str, Vec<String>) {
    let normalized = command.to_lowercase();
    let mut reasons: Vec<String> = Vec::new();
    if ["rm -rf", "rm -fr", "rm -r -f", "rm -f -r"]
        .iter()
        .any(|p| normalized.contains(p))
    {
        reasons.push("包含递归强制删除。".to_string());
    }
    if ["mkfs", "fdisk", "parted", "wipefs"].iter().any(|item| {
        normalized
            .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
            .any(|part| part == *item)
    }) {
        reasons.push("包含磁盘分区或格式化操作。".to_string());
    }
    if normalized.contains("dd ") && normalized.contains(" of=") {
        reasons.push("包含 dd 写入目标设备或文件。".to_string());
    }
    if (normalized.contains("curl ")
        && (normalized.contains("| sh") || normalized.contains("| bash")))
        || (normalized.contains("wget ")
            && (normalized.contains("| sh") || normalized.contains("| bash")))
    {
        reasons.push("包含下载脚本后直接执行。".to_string());
    }
    if ["iptables", "ufw", "firewall-cmd", "route", "ip route"]
        .iter()
        .any(|item| normalized.contains(item))
    {
        reasons.push("可能修改防火墙或路由。".to_string());
    }
    if ["shutdown", "reboot", "halt", "poweroff"]
        .iter()
        .any(|item| {
            normalized
                .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
                .any(|part| part == *item)
        })
    {
        reasons.push("可能重启或关闭主机。".to_string());
    }
    if normalized.contains("systemctl restart")
        || normalized.contains("systemctl stop")
        || (normalized.contains("service ") && normalized.contains(" stop"))
    {
        reasons.push("可能停止或重启服务。".to_string());
    }
    if normalized.contains("chmod -r 777")
        || normalized.contains("chown -r")
        || normalized.contains("userdel ")
        || normalized.contains("passwd ")
    {
        reasons.push("可能改变权限、用户或认证状态。".to_string());
    }
    if normalized.contains("/etc/ssh") && (normalized.contains('>') || normalized.contains("tee "))
    {
        reasons.push("可能覆盖 SSH 配置。".to_string());
    }
    if contains_sensitive_command_text(&normalized) {
        reasons.push("包含凭据、密钥或 token 明文。".to_string());
    }
    let risk = if reasons.is_empty() {
        "safe"
    } else {
        "dangerous"
    };
    (risk, reasons)
}

/// Detects plaintext credentials/keys/tokens inside a command string.
fn contains_sensitive_command_text(command: &str) -> bool {
    [
        "authorization: bearer",
        "api_key=",
        "apikey=",
        "access_token=",
        "auth_token=",
        "secret_access_key",
        "client_secret",
        "private_key",
        "--password",
        "password=",
        "passwd=",
        "sshpass -p",
        "-----begin",
    ]
    .iter()
    .any(|pattern| command.contains(pattern))
}

/// Extracts shell command suggestions from assistant text: fenced shell
/// blocks plus inline lines that begin with a known shell binary. Mirrors
/// mxterm's `extract_command_suggestions`.
fn extract_command_suggestions(content: &str) -> Vec<Value> {
    let mut commands: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut in_fence = false;
    let mut fence_lang = String::new();
    let mut fence_lines: Vec<String> = Vec::new();

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if let Some(lang) = line.strip_prefix("```") {
            if in_fence {
                if is_shell_fence(&fence_lang) {
                    let command = fence_lines
                        .iter()
                        .map(String::as_str)
                        .filter(|item| {
                            !item.trim().is_empty() && !item.trim_start().starts_with('#')
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    push_command_suggestion(&mut commands, &mut seen, &command);
                }
                fence_lines.clear();
                fence_lang.clear();
                in_fence = false;
            } else {
                in_fence = true;
                fence_lang = lang.trim().to_lowercase();
            }
            continue;
        }
        if in_fence {
            fence_lines.push(line.to_string());
            continue;
        }
        if let Some(command) = shell_like_command(line) {
            push_command_suggestion(&mut commands, &mut seen, &command);
        }
    }
    if in_fence && is_shell_fence(&fence_lang) {
        let command = fence_lines
            .iter()
            .map(String::as_str)
            .filter(|item| !item.trim().is_empty() && !item.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        push_command_suggestion(&mut commands, &mut seen, &command);
    }
    commands
}

fn push_command_suggestion(commands: &mut Vec<Value>, seen: &mut Vec<String>, command: &str) {
    let command = command.trim();
    if command.is_empty() || command.len() > 4000 {
        return;
    }
    if seen.iter().any(|item| item == command) {
        return;
    }
    let (risk, reasons) = assess_command_impl(command);
    seen.push(command.to_string());
    commands.push(json!({
        "command": command,
        "risk": risk,
        "reasons": reasons,
    }));
}

/// Returns a shell command when the line starts with a known shell binary.
fn shell_like_command(line: &str) -> Option<String> {
    let mut command = line.trim();
    if let Some(rest) = command.strip_prefix('$') {
        command = rest.trim_start();
    } else if command.starts_with("# ") {
        return None;
    }
    let first = command
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(|ch: char| ch == '`' || ch == '"' || ch == '\'');
    let known = [
        "apt",
        "brew",
        "cargo",
        "cat",
        "cd",
        "chmod",
        "chown",
        "cp",
        "curl",
        "dd",
        "df",
        "dig",
        "docker",
        "du",
        "find",
        "fdisk",
        "firewall-cmd",
        "git",
        "grep",
        "halt",
        "ip",
        "iptables",
        "journalctl",
        "kubectl",
        "less",
        "ls",
        "mkdir",
        "mkfs",
        "mv",
        "netstat",
        "node",
        "npm",
        "pnpm",
        "poweroff",
        "ps",
        "python",
        "python3",
        "reboot",
        "rm",
        "route",
        "scp",
        "sed",
        "service",
        "shutdown",
        "ss",
        "ssh",
        "sudo",
        "systemctl",
        "tail",
        "tar",
        "traceroute",
        "ufw",
        "unzip",
        "userdel",
        "vim",
        "wget",
        "wipefs",
        "yarn",
    ];
    (known.iter().any(|item| item.eq_ignore_ascii_case(first))
        || first.to_lowercase().starts_with("mkfs."))
    .then(|| command.trim_matches('`').to_string())
}

/// Recognizes code-fence languages that contain shell commands.
fn is_shell_fence(lang: &str) -> bool {
    matches!(
        lang,
        "bash" | "sh" | "shell" | "zsh" | "powershell" | "ps1" | "cmd" | "bat" | "console" | ""
    )
}

/// Calls an OpenAI-compatible chat completions endpoint and returns the
/// assistant text.
fn call_openai(url: &str, api_key: &str, body: &Value) -> Result<String, String> {
    let body_text = serde_json::to_string(body).map_err(|e| e.to_string())?;
    let response = ureq::post(url)
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {api_key}"))
        .send_string(&body_text)
        .map_err(|e| e.to_string())?;
    let text = response.into_string().map_err(|e| e.to_string())?;
    let json: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    json.pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| "响应缺少 content。".to_string())
}

/// Returns (and lazily persists) the local AI key-encryption secret.
fn local_secret_key(store: &mut Store) -> Result<[u8; 32], String> {
    if let Some(existing) = store
        .get_app_secret("ai_key_enc")
        .map_err(|e| e.to_string())?
    {
        let bytes = base64_decode(&existing).ok_or_else(|| "密钥无效。".to_string())?;
        if bytes.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
    }
    let mut key = [0u8; 32];
    getrandom::getrandom(&mut key).map_err(|e| e.to_string())?;
    store
        .put_app_secret("ai_key_enc", &base64_encode(&key))
        .map_err(|e| e.to_string())?;
    Ok(key)
}

fn encrypt_key(key: &[u8; 32], plaintext: &[u8]) -> Result<String, String> {
    use aes_gcm::aead::{Aead, KeyInit, Nonce};
    use aes_gcm::{Aes256Gcm, Key};
    use getrandom::getrandom;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce = [0u8; 12];
    getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let enc = cipher
        .encrypt(Nonce::<Aes256Gcm>::from_slice(&nonce), plaintext)
        .map_err(|e| e.to_string())?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&enc);
    Ok(base64_encode(&out))
}

fn decrypt_key(key: &[u8; 32], blob: &str) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::{Aead, KeyInit, Nonce};
    use aes_gcm::{Aes256Gcm, Key};
    let data = base64_decode(blob).ok_or_else(|| "密钥 blob 无效。".to_string())?;
    if data.len() < 12 {
        return Err("密钥 blob 过短。".to_string());
    }
    let (nonce, ciphertext) = data.split_at(12);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher
        .decrypt(Nonce::<Aes256Gcm>::from_slice(nonce), ciphertext)
        .map_err(|_| "密钥解密失败。".to_string())
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

/// Builds an "explain this SQL" prompt for the AI (ai_explain_prompt).
pub fn ai_build_explain_prompt(sql: &str, dialect: &str) -> Result<Value, String> {
    if sql.trim().is_empty() {
        return Err("SQL 为空。".to_string());
    }
    let dialect = if dialect.trim().is_empty() {
        "通用 SQL"
    } else {
        dialect
    };
    Ok(json!({
        "role": "user",
        "prompt": format!(
            "请用中文解释下面这段 {dialect} SQL 的作用、执行逻辑与潜在风险，分点说明：\n\n```sql\n{sql}\n```"
        ),
    }))
}

/// Builds a "generate SQL" prompt from natural language + schema (ai_generate_sql_prompt).
pub fn ai_build_generate_sql_prompt(
    natural_language: &str,
    dialect: &str,
    schema: &str,
) -> Result<Value, String> {
    if natural_language.trim().is_empty() {
        return Err("描述为空。".to_string());
    }
    let dialect = if dialect.trim().is_empty() {
        "通用 SQL"
    } else {
        dialect
    };
    let schema_block = if schema.trim().is_empty() {
        "（未提供表结构）".to_string()
    } else {
        format!("可用表结构：\n{schema}")
    };
    Ok(json!({
        "role": "user",
        "prompt": format!(
            "请根据下面的自然语言需求，生成 {dialect} 语句，只输出 SQL，不要额外解释。\n需求：{natural_language}\n{schema_block}"
        ),
    }))
}

/// Analyzes a query result (columns + rows) and produces a chart spec for the
/// UI renderer (ai_chart_spec). Picks line/bar/pie heuristically.
pub fn ai_chart_spec(columns: &[String], rows: &[Vec<Value>]) -> Value {
    if columns.is_empty() {
        return json!({ "chart_type": "none", "reason": "无列" });
    }
    if rows.is_empty() {
        return json!({ "chart_type": "none", "reason": "无数据" });
    }
    // First non-numeric column is the category axis; numeric columns become series.
    let numeric: Vec<usize> = (0..columns.len())
        .filter(|&index| {
            rows.iter()
                .all(|row| row.get(index).map(is_numeric_value).unwrap_or(false))
        })
        .collect();
    let category = if numeric.len() == columns.len() {
        None
    } else {
        (0..columns.len()).find(|index| !numeric.contains(index))
    };
    let series: Vec<Value> = numeric
        .iter()
        .map(|&index| {
            json!({
                "name": columns[index],
                "values": rows
                    .iter()
                    .map(|row| row.get(index).and_then(Value::as_f64).unwrap_or(0.0))
                    .collect::<Vec<f64>>(),
            })
        })
        .collect();
    if series.is_empty() {
        return json!({ "chart_type": "none", "reason": "没有数值列" });
    }
    let chart_type = if rows.len() <= 1 {
        "pie"
    } else if series.len() >= 2 {
        "line"
    } else {
        "bar"
    };
    json!({
        "chart_type": chart_type,
        "category": category.map(|index| columns[index].clone()).unwrap_or_default(),
        "labels": rows
            .iter()
            .map(|row| {
                category
                    .and_then(|index| row.get(index).map(cell_text))
                    .unwrap_or_else(|| row[0].to_string())
            })
            .collect::<Vec<String>>(),
        "series": series,
    })
}

fn is_numeric_value(value: &Value) -> bool {
    value.is_number()
}

fn cell_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        _ => value.to_string(),
    }
}

/// Runs a query against a database session and returns the chart spec
/// (ai_chart_spec_from_query).
pub fn ai_chart_spec_from_query(session_id: &str, sql: &str) -> Result<Value, String> {
    let outcome = crate::db::query_session(session_id, sql)?;
    Ok(ai_chart_spec(&outcome.columns, &outcome.rows))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assess_dangerous_command() {
        let r = assess_command("sudo rm -rf /").expect("assess");
        assert_eq!(r["risk"], "dangerous");
        assert!(!r["reasons"].as_array().expect("arr").is_empty());
    }

    #[test]
    fn assess_safe_command() {
        let r = assess_command("ls -la").expect("assess");
        assert_eq!(r["risk"], "safe");
    }

    #[test]
    fn models_curated() {
        let r = models_list(&json!({ "provider": "openai" })).expect("models");
        assert!(!r.as_array().expect("arr").is_empty());
    }

    #[test]
    fn suggestions_extracted_from_fence_and_inline() {
        let content = "运行以下命令：\n\n```bash\nls -la\nsudo rm -rf /tmp/x\n```\n\n$ curl -s https://example.com/x.sh | sh";
        let suggestions = extract_command_suggestions(content);
        assert_eq!(suggestions.len(), 2);
        let fenced = &suggestions[0];
        assert!(fenced["command"].as_str().expect("cmd").contains("rm -rf"));
        assert_eq!(fenced["risk"], "dangerous");
        let inline = &suggestions[1];
        assert!(inline["command"].as_str().expect("cmd").starts_with("curl"));
        assert_eq!(inline["risk"], "dangerous");
    }

    #[test]
    fn explain_and_generate_sql_prompts() {
        let explain =
            ai_build_explain_prompt("SELECT * FROM users WHERE id = 1", "mysql").expect("explain");
        let prompt = explain["prompt"].as_str().unwrap_or("");
        assert!(prompt.contains("mysql"));
        assert!(prompt.contains("SELECT * FROM users"));
        let empty = ai_build_explain_prompt("  ", "mysql").expect_err("empty");
        assert!(empty.contains("SQL 为空"));

        let generate =
            ai_build_generate_sql_prompt("查所有用户的邮箱", "postgresql", "users(id, email)")
                .expect("generate");
        let gen_prompt = generate["prompt"].as_str().unwrap_or("");
        assert!(gen_prompt.contains("postgresql"));
        assert!(gen_prompt.contains("users(id, email)"));
        let empty_gen = ai_build_generate_sql_prompt("", "pg", "").expect_err("empty");
        assert!(empty_gen.contains("描述为空"));
    }

    #[test]
    fn chart_spec_picks_type_and_series() {
        let spec = ai_chart_spec(
            &["month".to_string(), "sales".to_string(), "cost".to_string()],
            &[
                vec![json!("一月"), json!(100), json!(60)],
                vec![json!("二月"), json!(120), json!(70)],
            ],
        );
        assert_eq!(spec["chart_type"], "line", "got {spec:?}");
        assert_eq!(spec["category"], "month");
        assert_eq!(spec["series"].as_array().map(|a| a.len()).unwrap_or(0), 2);
        assert_eq!(spec["labels"][0], "一月");

        let single = ai_chart_spec(
            &["name".to_string(), "count".to_string()],
            &[vec![json!("a"), json!(5)]],
        );
        assert_eq!(single["chart_type"], "pie");

        let no_data = ai_chart_spec(&["a".to_string()], &[]);
        assert_eq!(no_data["chart_type"], "none");
    }

    #[test]
    fn key_roundtrip() {
        let key = [7u8; 32];
        let blob = encrypt_key(&key, b"sk-secret").expect("enc");
        let plain = decrypt_key(&key, &blob).expect("dec");
        assert_eq!(plain, b"sk-secret");
    }
}
