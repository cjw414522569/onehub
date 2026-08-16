//! ACP (Agent Client Protocol) integration (T050). Talks to external coding
//! agents (Codex / Claude Code / OpenCode) over JSON-RPC with Content-Length
//! framing (the same wire format as LSP). The library implements the framing
//! and the `initialize` handshake; `acp_run_tool` sends a `tools/call`
//! notification and returns any direct response.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// ACP protocol version we advertise in `initialize`.
pub const ACP_PROTOCOL_VERSION: &str = "2025-03-26";
/// ACP protocol name.
pub const ACP_PROTOCOL_NAME: &str = "acp";

/// Well-known agent binaries probed by `acp_detect_agents`.
pub const ACP_AGENTS: &[(&str, &str)] = &[
    ("codex", "Codex"),
    ("claude", "Claude Code"),
    ("opencode", "OpenCode"),
];

/// Frames a JSON-RPC message with LSP-style Content-Length headers.
pub fn acp_frame(message: &str) -> Vec<u8> {
    let body = message.as_bytes();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out
}

/// Parses one complete framed message from a byte buffer, returning the body
/// text and the remaining bytes. `None` means an incomplete frame.
pub fn acp_parse_frame(buffer: &[u8]) -> Option<(String, Vec<u8>)> {
    let header_end = find_subslice(buffer, b"\r\n\r\n")?;
    let header = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let content_length = header.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("Content-Length:")?;
        rest.trim().parse::<usize>().ok()
    })?;
    let body_start = header_end + 4;
    if buffer.len() < body_start + content_length {
        return None;
    }
    let body =
        String::from_utf8_lossy(&buffer[body_start..body_start + content_length]).to_string();
    let remaining = buffer[body_start + content_length..].to_vec();
    Some((body, remaining))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Builds the `initialize` request (ACP handshake).
pub fn acp_build_initialize(request_id: i64) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "initialize",
        "params": {
            "protocolVersion": ACP_PROTOCOL_VERSION,
            "clientCapabilities": { "name": "onehub" },
        },
    })
    .to_string()
}

/// Builds a `tools/call` notification for a tool invocation.
pub fn acp_build_tool_call(call_id: &str, name: &str, arguments: &Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/tools/call",
        "params": {
            "callId": call_id,
            "name": name,
            "arguments": arguments,
        },
    })
    .to_string()
}

/// Detects which ACP agents are installed on PATH (acp_detect_agents).
pub fn acp_detect_agents() -> Value {
    let mut agents = Vec::new();
    for (binary, label) in ACP_AGENTS {
        let found = std::process::Command::new(binary)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        agents.push(json!({
            "binary": binary,
            "label": label,
            "available": found,
        }));
    }
    json!(agents)
}

/// Spawns an ACP agent and performs the `initialize` handshake
/// (acp_handshake). Returns the negotiated protocol version + agent info, or
/// a graceful error when the binary is missing or the handshake times out.
pub fn acp_handshake(binary: &str, timeout_ms: u64) -> Result<Value, String> {
    let mut child = spawn_agent(binary)?;
    let initialize = acp_build_initialize(1);
    let frame = acp_frame(&initialize);
    child
        .stdin
        .as_mut()
        .ok_or_else(|| "Agent 标准输入不可用。".to_string())?
        .write_all(&frame)
        .map_err(|e| format!("写入 Agent 失败：{e}"))?;
    let response = read_frame_with_timeout(&mut child, timeout_ms)
        .ok_or_else(|| "Agent 握手超时或无响应。".to_string())?;
    let parsed: Value =
        serde_json::from_str(&response).map_err(|e| format!("握手响应解析失败：{e}"))?;
    let protocol_version = parsed
        .pointer("/result/protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let agent_name = parsed
        .pointer("/result/agentInfo/name")
        .and_then(Value::as_str)
        .unwrap_or(binary)
        .to_string();
    let _ = child.kill();
    let _ = child.wait();
    Ok(json!({
        "binary": binary,
        "agent": agent_name,
        "protocol_version": protocol_version,
        "handshake": "ok",
    }))
}

/// Runs a tool on an ACP agent (acp_run_tool). Sends tools/call and returns
/// the response if the agent replies, else an acknowledgement.
pub fn acp_run_tool(binary: &str, name: &str, arguments: &Value) -> Result<Value, String> {
    let mut child = spawn_agent(binary)?;
    // Send initialize first (required by ACP), ignore its body.
    let init_frame = acp_frame(&acp_build_initialize(1));
    child
        .stdin
        .as_mut()
        .ok_or_else(|| "Agent 标准输入不可用。".to_string())?
        .write_all(&init_frame)
        .map_err(|e| format!("写入 Agent 失败：{e}"))?;
    let _ = read_frame_with_timeout(&mut child, 5000);
    let call = acp_build_tool_call("onehub-1", name, arguments);
    child
        .stdin
        .as_mut()
        .ok_or_else(|| "Agent 标准输入不可用。".to_string())?
        .write_all(&acp_frame(&call))
        .map_err(|e| format!("写入工具调用失败：{e}"))?;
    let response = read_frame_with_timeout(&mut child, 5000);
    let _ = child.kill();
    let _ = child.wait();
    match response {
        Some(body) => {
            let parsed: Value =
                serde_json::from_str(&body).map_err(|e| format!("工具响应解析失败：{e}"))?;
            Ok(json!({ "binary": binary, "tool": name, "response": parsed }))
        }
        None => Ok(json!({ "binary": binary, "tool": name, "response": null })),
    }
}

fn spawn_agent(binary: &str) -> Result<Child, String> {
    Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("无法启动 Agent {binary}：{e}"))
}

/// Reads one framed message from the agent's stdout with a timeout.
fn read_frame_with_timeout(child: &mut Child, timeout_ms: u64) -> Option<String> {
    let stdout = child.stdout.as_mut()?;
    let mut reader = BufReader::new(stdout);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut buffer = Vec::new();
    loop {
        if Instant::now() > deadline {
            return None;
        }
        let _ = reader.read_until(b'\n', &mut buffer).map_err(|_| ()).ok();
        if let Some((body, _remaining)) = acp_parse_frame(&buffer) {
            return Some(body);
        }
        if buffer.len() > 4 * 1024 * 1024 {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acp_frame_roundtrip() {
        let message = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let frame = acp_frame(message);
        let header = String::from_utf8_lossy(&frame[..40]).to_string();
        assert!(header.contains("Content-Length:"));
        let (body, remaining) = acp_parse_frame(&frame).expect("parse");
        assert_eq!(body, message);
        assert!(remaining.is_empty());
    }

    #[test]
    fn acp_frame_incomplete_returns_none() {
        let frame = acp_frame("{}");
        assert!(acp_parse_frame(&frame[..frame.len() - 3]).is_none());
    }

    #[test]
    fn acp_initialize_and_tool_call_messages() {
        let initialize = acp_build_initialize(1);
        let parsed: Value = serde_json::from_str(&initialize).expect("json");
        assert_eq!(parsed["method"], "initialize");
        assert_eq!(parsed["params"]["protocolVersion"], ACP_PROTOCOL_VERSION);
        assert_eq!(parsed["params"]["clientCapabilities"]["name"], "onehub");

        let call = acp_build_tool_call("c1", "read_file", &json!({ "path": "/tmp/a" }));
        let parsed_call: Value = serde_json::from_str(&call).expect("json");
        assert_eq!(parsed_call["method"], "notifications/tools/call");
        assert_eq!(parsed_call["params"]["name"], "read_file");
        assert_eq!(parsed_call["params"]["callId"], "c1");
    }

    #[test]
    fn acp_detect_agents_returns_array() {
        let agents = acp_detect_agents();
        let arr = agents.as_array().expect("array");
        assert!(arr.len() >= 3);
        for agent in arr {
            assert!(agent["binary"].as_str().is_some());
            assert!(agent["available"].as_bool().is_some());
        }
    }

    #[test]
    fn acp_handshake_missing_binary_is_graceful() {
        let err = acp_handshake("onehub-no-such-agent-binary-xyz", 800).expect_err("missing");
        assert!(err.contains("无法启动"), "got {err:?}");
        let err =
            acp_run_tool("onehub-no-such-agent-binary-xyz", "x", &json!({})).expect_err("missing");
        assert!(err.contains("无法启动"), "got {err:?}");
    }
}
