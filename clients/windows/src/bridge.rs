//! JS<->Rust bridge protocol (T007): maps WebView2 `postMessage` invoke
//! requests to the PC GUI model and returns reply JSON. Pure and
//! headless-testable; the WebView2 wiring lives in `webview2.rs`.

use clients_windows::model::GuiModel;
use serde_json::{json, Value};

/// Window-control actions the shell must apply after handling a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowAction {
    None,
    Minimize,
    Maximize,
    Close,
}

fn sessions_json(model: &GuiModel) -> Vec<Value> {
    (0..model.profile_count())
        .map(|index| {
            let profile = model.profile(index).expect("profile");
            json!({
                "id": format!("session-{index}"),
                "name": profile.name,
                "host": profile.host,
                "port": profile.port,
                "username": profile.user,
                "protocol": "ssh",
                "group": null,
                "credential_mode": "prompt",
                "proxy": null,
                "jump": null,
                "advanced": null,
                "is_favorite": false,
                "created_at": null,
                "updated_at": null,
                "selected": model.selected_profile() == Some(index),
            })
        })
        .collect()
}

/// Handles one `{kind:"invoke", requestId, cmd, payload}` message and returns
/// the reply JSON plus any window action to apply.
pub(crate) fn handle_message(
    model: &mut GuiModel,
    message: &str,
) -> Option<(String, WindowAction)> {
    let parsed: Value = serde_json::from_str(message).ok()?;
    let request_id = parsed.get("requestId").cloned().unwrap_or(Value::Null);
    let cmd = parsed.get("cmd").and_then(Value::as_str).unwrap_or("");
    let payload = parsed.get("payload").cloned().unwrap_or(Value::Null);
    let mut action = WindowAction::None;

    let body: Value = match cmd {
        "connection_list" | "list_sessions" => {
            json!({ "ok": true, "sessions": sessions_json(model) })
        }
        "get_status" => json!({
            "ok": true,
            "phase": model.phase().as_str(),
            "status": model.status(),
            "host": model.host(),
            "user": model.user(),
            "selected": model.selected_profile(),
            "sessions": sessions_json(model),
        }),
        "connection_upsert" => {
            let request = payload.get("request").cloned().unwrap_or(Value::Null);
            let name = request
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let host = request
                .get("host")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let port = request.get("port").and_then(Value::as_u64).unwrap_or(22) as u16;
            let username = request
                .get("username")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if host.is_empty() {
                json!({ "ok": false, "error": "host required" })
            } else {
                let display = if name.is_empty() {
                    host.as_str()
                } else {
                    name.as_str()
                };
                let index = model.add_profile(display, &host, port, &username);
                model.select_profile(index);
                json!({ "ok": true, "id": format!("session-{index}") })
            }
        }
        "connection_delete" => {
            let id = payload.get("id").and_then(Value::as_str).unwrap_or("");
            if let Some(index) = id
                .strip_prefix("session-")
                .and_then(|s| s.parse::<usize>().ok())
            {
                model.remove_profile(index);
                json!({ "ok": true })
            } else {
                json!({ "ok": false, "error": "bad id" })
            }
        }
        "terminal_connect" => {
            let request = payload.get("request").cloned().unwrap_or(payload.clone());
            let host = request
                .get("host")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let port = request.get("port").and_then(Value::as_u64).unwrap_or(22) as u16;
            let username = request
                .get("username")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if host.is_empty() {
                json!({ "ok": false, "error": "host required" })
            } else {
                let index = model.add_profile(&host, &host, port, &username);
                model.connect_profile(index);
                json!({ "ok": true, "sessionId": format!("session-{index}"), "phase": "connecting" })
            }
        }
        "terminal_close" => {
            model.disconnect("user requested");
            json!({ "ok": true, "phase": "disconnected" })
        }
        "terminal_resize" => {
            let rows = payload.get("rows").and_then(Value::as_u64).unwrap_or(24) as usize;
            let cols = payload.get("cols").and_then(Value::as_u64).unwrap_or(80) as usize;
            model.resize(rows, cols);
            json!({ "ok": true })
        }
        "terminal_write" => {
            if let Some(data) = payload.get("data").and_then(Value::as_str) {
                model.append_output(data);
            }
            json!({ "ok": true })
        }
        "disconnect" => {
            model.disconnect("user requested");
            json!({ "ok": true })
        }
        "quit" => {
            action = WindowAction::Close;
            json!({ "ok": true })
        }
        "window_minimize" => {
            action = WindowAction::Minimize;
            json!({ "ok": true })
        }
        "window_maximize" => {
            action = WindowAction::Maximize;
            json!({ "ok": true })
        }
        "window_close" => {
            action = WindowAction::Close;
            json!({ "ok": true })
        }
        "get_app_runtime_info" => json!({
            "platform": "windows",
            "family": "windows",
            "arch": std::env::consts::ARCH,
            "version": env!("CARGO_PKG_VERSION"),
        }),
        "get_windows_pty_info" => Value::Null,
        "get_supported_window_materials" => json!([]),
        "secret_vault_status" => json!({ "enabled": false, "locked": false }),
        "command_snippet_list" => json!([]),
        "credential_list" => json!([]),
        "command_history_list" => json!([]),
        "local_terminal_list_profiles" => json!([]),
        "serial_list_ports" => json!([]),
        // Anything the host shell does not implement resolves to null so the
        // copied UI degrades gracefully instead of rejecting.
        _ => Value::Null,
    };

    let reply = json!({ "kind": "invoke-reply", "requestId": request_id, "payload": body });
    Some((reply.to_string(), action))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clients_windows::model::GuiModel;

    fn invoke(model: &mut GuiModel, cmd: &str, payload: serde_json::Value) -> Value {
        let msg =
            json!({ "kind": "invoke", "requestId": 7, "cmd": cmd, "payload": payload }).to_string();
        let (reply, _action) = handle_message(model, &msg).expect("reply");
        let parsed: Value = serde_json::from_str(&reply).expect("json");
        assert_eq!(parsed["requestId"], 7);
        parsed["payload"].clone()
    }

    #[test]
    fn connection_list_round_trips_sessions() {
        let mut model = GuiModel::with_size(4, 24);
        model.add_profile("dev", "10.0.0.1", 22, "root");
        model.add_profile("prod", "10.0.0.2", 2222, "ops");
        let body = invoke(&mut model, "connection_list", json!({}));
        assert_eq!(body["ok"], true);
        let sessions = body["sessions"].as_array().expect("sessions");
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0]["name"], "dev");
        assert_eq!(sessions[0]["host"], "10.0.0.1");
        assert_eq!(sessions[0]["port"], 22);
        assert_eq!(sessions[0]["username"], "root");
        assert_eq!(sessions[1]["name"], "prod");
    }

    #[test]
    fn connection_upsert_adds_and_selects() {
        let mut model = GuiModel::with_size(4, 24);
        let body = invoke(
            &mut model,
            "connection_upsert",
            json!({ "request": { "name": "dev", "host": "10.0.0.1", "port": 2222, "username": "root" } }),
        );
        assert_eq!(body["ok"], true);
        assert_eq!(model.profile_count(), 1);
        assert_eq!(model.selected_profile(), Some(0));
        assert_eq!(model.profile(0).unwrap().port, 2222);
    }

    #[test]
    fn terminal_connect_and_close() {
        let mut model = GuiModel::with_size(4, 24);
        let body = invoke(
            &mut model,
            "terminal_connect",
            json!({ "request": { "host": "10.0.0.1", "port": 22, "username": "root" } }),
        );
        assert_eq!(body["phase"], "connecting");
        assert_eq!(model.phase().as_str(), "connecting");
        invoke(&mut model, "terminal_close", json!({}));
        assert_eq!(model.phase().as_str(), "disconnected");
    }

    #[test]
    fn terminal_resize_and_write() {
        let mut model = GuiModel::with_size(4, 24);
        invoke(
            &mut model,
            "terminal_resize",
            json!({ "rows": 10, "cols": 40 }),
        );
        assert_eq!(model.grid().rows(), 10);
        assert_eq!(model.grid().cols(), 40);
        invoke(&mut model, "terminal_write", json!({ "data": "hello\n" }));
        assert!(model.grid().to_lines()[0].contains("hello"));
    }

    #[test]
    fn window_close_action() {
        let mut model = GuiModel::with_size(4, 24);
        let msg = json!({ "kind": "invoke", "requestId": 1, "cmd": "window_close", "payload": {} })
            .to_string();
        let (_, action) = handle_message(&mut model, &msg).expect("reply");
        assert_eq!(action, WindowAction::Close);
    }

    #[test]
    fn unknown_command_resolves_null() {
        let mut model = GuiModel::with_size(4, 24);
        let body = invoke(&mut model, "docker_list_containers", json!({}));
        assert!(body.is_null());
    }

    #[test]
    fn get_status_reports_model() {
        let mut model = GuiModel::with_size(4, 24);
        model.add_profile("dev", "10.0.0.1", 22, "root");
        model.connect_profile(0);
        let body = invoke(&mut model, "get_status", json!({}));
        assert_eq!(body["phase"], "connecting");
        assert_eq!(body["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(body["sessions"][0]["selected"], true);
    }
}
