//! JS<->Rust bridge protocol (T007+): maps WebView2 `postMessage` invoke
//! requests to the PC GUI model and returns reply JSON shaped to match the
//! mXterm frontend contracts (connection_list returns ConnectionProfile[],
//! terminal_connect returns a sessionId string, void commands return null).
//! Pure and headless-testable; the WebView2 wiring lives in `webview2.rs`.

use clients_windows::model::GuiModel;
use serde_json::{json, Value};

/// Window-control actions the shell must apply after handling a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowAction {
    None,
    Minimize,
    Maximize,
    Close,
    StartDrag,
    ToggleMaximize,
}

fn profile_json(model: &GuiModel, index: usize) -> Value {
    let profile = model.profile(index).expect("profile");
    json!({
        "id": format!("session-{index}"),
        "name": profile.name,
        "protocol": "ssh",
        "host": profile.host,
        "port": profile.port,
        "username": profile.user,
        "group": null,
        "credential_mode": "prompt",
        "proxy": { "kind": "none", "host": "", "port": 8080, "username": "", "password": "" },
        "jump": { "kind": "none", "jump_connection_id": "" },
        "advanced": {
            "auth_timeout_ms": 45000,
            "connect_timeout_ms": 30000,
            "keepalive_interval_ms": 20000,
            "terminal_encoding": "utf-8"
        },
        "is_favorite": false,
        "last_connected_at": null,
        "created_at": "host-shell",
        "updated_at": "host-shell",
    })
}

fn sessions_json(model: &GuiModel) -> Vec<Value> {
    (0..model.profile_count())
        .map(|index| profile_json(model, index))
        .collect()
}

fn field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// A registered Tauri-style event listener (used by the UI listen()).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EventListener {
    /// Listener id returned to the UI.
    pub id: u64,
    /// Event name (e.g. "terminal:output").
    pub event: String,
    /// The JS callback id (from transformCallback) to invoke on emit.
    pub handler_id: u64,
}

/// Registry of event listeners with monotonically increasing ids.
#[derive(Debug, Default)]
pub(crate) struct EventRegistry {
    listeners: Vec<EventListener>,
    next_id: u64,
}

impl EventRegistry {
    /// Registers a listener and returns its id.
    pub fn listen(&mut self, event: &str, handler_id: u64) -> u64 {
        self.next_id += 1;
        self.listeners.push(EventListener {
            id: self.next_id,
            event: event.to_string(),
            handler_id,
        });
        self.next_id
    }

    /// Removes a listener by event name + id.
    pub fn unlisten(&mut self, event: &str, id: u64) {
        self.listeners
            .retain(|listener| !(listener.event == event && listener.id == id));
    }

    /// Builds terminal:output event objects for every registered listener.
    /// Each entry is (handler_id, full event object) shaped for the mXterm UI
    /// (TerminalOutputEvent: session_id, request_id, data as number[]).
    pub fn terminal_output_events(&self, session_id: &str, data: &[u8]) -> Vec<(u64, Value)> {
        let bytes: Vec<Value> = data.iter().map(|byte| json!(*byte)).collect();
        self.listeners
            .iter()
            .filter(|listener| listener.event == "terminal:output")
            .map(|listener| {
                let event_obj = json!({
                    "event": "terminal:output",
                    "id": listener.id,
                    "payload": {
                        "session_id": session_id,
                        "request_id": null,
                        "data": bytes,
                    },
                });
                (listener.handler_id, event_obj)
            })
            .collect()
    }

    /// Number of registered listeners (for tests).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.listeners.len()
    }
}

/// Handles one `{kind:"invoke", requestId, cmd, payload}` message and returns
/// the reply JSON (payload shaped for the mXterm UI) plus a window action.
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
        "connection_list" | "list_sessions" => json!(sessions_json(model)),
        "get_status" => json!({
            "phase": model.phase().as_str(),
            "status": model.status(),
            "host": model.host(),
            "user": model.user(),
            "selected": model.selected_profile(),
            "sessions": sessions_json(model),
        }),
        "connection_upsert" => {
            let request = payload.get("request").cloned().unwrap_or(payload.clone());
            let id = field(&request, "id");
            let host = field(&request, "host");
            let port = request.get("port").and_then(Value::as_u64).unwrap_or(22) as u16;
            let username = field(&request, "username");
            let name = field(&request, "name");
            let display = if name.is_empty() { host.clone() } else { name };
            let index = if let Some(existing) = id
                .strip_prefix("session-")
                .and_then(|s| s.parse::<usize>().ok())
            {
                model.remove_profile(existing);
                model.add_profile(&display, &host, port, &username)
            } else {
                model.add_profile(&display, &host, port, &username)
            };
            model.select_profile(index);
            json!(profile_json(model, index))
        }
        "connection_delete" => {
            let id = field(&payload, "id");
            if let Some(index) = id
                .strip_prefix("session-")
                .and_then(|s| s.parse::<usize>().ok())
            {
                model.remove_profile(index);
            }
            Value::Null
        }
        "terminal_connect" => {
            let request = payload.get("request").cloned().unwrap_or(payload.clone());
            let index = match field(&request, "connection_id")
                .strip_prefix("session-")
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|index| *index < model.profile_count())
            {
                Some(index) => index,
                None => {
                    let host = field(&request, "host");
                    let port = request.get("port").and_then(Value::as_u64).unwrap_or(22) as u16;
                    let username = field(&request, "username");
                    model.add_profile(&host, &host, port, &username)
                }
            };
            model.connect_profile(index);
            json!(format!("session-{index}"))
        }
        "terminal_close" => {
            model.disconnect("user requested");
            Value::Null
        }
        "terminal_resize" => {
            let rows = payload.get("rows").and_then(Value::as_u64).unwrap_or(24) as usize;
            let cols = payload.get("cols").and_then(Value::as_u64).unwrap_or(80) as usize;
            model.resize(rows, cols);
            Value::Null
        }
        "terminal_write" => {
            if let Some(data) = payload.get("data").and_then(Value::as_str) {
                model.append_output(data);
            }
            Value::Null
        }
        "connection_test" | "connection_test_profile" => {
            json!({ "ok": true, "message": "host shell: transport via abi-c not yet wired" })
        }
        "connection_probe_latency" => json!({ "latency_ms": null, "reachable": true }),
        "connection_probe_system" => {
            let index = model
                .selected_profile()
                .unwrap_or(0)
                .min(model.profile_count().saturating_sub(1));
            json!(profile_json(model, index))
        }
        "connection_mark_connected" | "connection_set_favorite" => {
            let index = field(&payload, "id")
                .strip_prefix("session-")
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|i| *i < model.profile_count())
                .unwrap_or(0);
            json!(profile_json(model, index))
        }
        "disconnect" => {
            model.disconnect("user requested");
            Value::Null
        }
        "quit" => {
            action = WindowAction::Close;
            Value::Null
        }
        "window_minimize" => {
            action = WindowAction::Minimize;
            Value::Null
        }
        "window_maximize" => {
            action = WindowAction::Maximize;
            Value::Null
        }
        "window_close" => {
            action = WindowAction::Close;
            Value::Null
        }
        "get_app_runtime_info" => json!({
            "platform": "windows",
            "family": "windows",
            "arch": std::env::consts::ARCH,
            "version": env!("CARGO_PKG_VERSION"),
        }),
        "get_windows_pty_info" => Value::Null,
        "get_supported_window_materials" => json!([{ "id": 0, "name": "auto" }]),
        "secret_vault_status" | "secret_vault_unlock" | "secret_vault_unlock_local" => {
            json!({ "initialized": true, "unlocked": true })
        }
        "secret_vault_lock"
        | "secret_vault_enable_master_password"
        | "secret_vault_disable_master_password" => {
            json!({ "initialized": true, "unlocked": false })
        }
        "command_snippet_list" => json!([]),
        "credential_list" => json!([]),
        "command_history_list" => json!([]),
        "local_terminal_list_profiles" => json!([]),
        "serial_list_ports" => json!([]),
        // File / SFTP / transfer surface: interface-only in the host shell
        // (no live SFTP session yet). list resolves to [] so the panel renders
        // empty; mutations resolve null so nothing crashes. Real SFTP wiring
        // goes through the abi-c bridge in a later row.
        "remote_file_list" => json!([]),
        "remote_file_metadata"
        | "remote_file_read"
        | "remote_file_write"
        | "remote_file_create_file"
        | "remote_file_create_directory"
        | "remote_file_delete"
        | "remote_file_rename"
        | "remote_file_check_path"
        | "remote_file_check_download_target"
        | "remote_file_upload_file"
        | "remote_file_upload_local_file"
        | "remote_file_upload_archive"
        | "remote_file_upload_local_archive"
        | "remote_file_download"
        | "remote_file_download_to_local"
        | "remote_file_prepare_upload_temp"
        | "remote_file_append_upload_temp"
        | "remote_file_delete_upload_temp"
        | "remote_file_cancel_transfer"
        | "connection_transfer_export"
        | "connection_transfer_import"
        | "connection_transfer_preview" => Value::Null,
        // Tauri plugin IPC the copied UI needs at bootstrap (window state,
        // event listeners). Benign values keep the UI rendering; unknown
        // plugin commands resolve to null so the UI degrades gracefully.
        "plugin:window|outer_position" | "plugin:window|inner_position" => {
            json!({ "x": 0, "y": 0 })
        }
        "plugin:window|outer_size" | "plugin:window|inner_size" => {
            json!({ "width": 1440, "height": 900 })
        }
        "plugin:window|minimize" => {
            action = WindowAction::Minimize;
            Value::Null
        }
        "plugin:window|toggle_maximize" => {
            action = WindowAction::ToggleMaximize;
            Value::Null
        }
        "plugin:window|close" => {
            action = WindowAction::Close;
            Value::Null
        }
        "plugin:window|start_dragging" => {
            action = WindowAction::StartDrag;
            Value::Null
        }
        "plugin:window|is_maximized" => json!(false),
        "plugin:window|is_minimized" | "plugin:window|is_fullscreen" => json!(false),
        "plugin:window|is_visible" => json!(true),
        "plugin:window|scale_factor" => json!(1.0),
        "plugin:window|current_monitor" => json!({
            "position": { "x": 0, "y": 0 },
            "size": { "width": 1920, "height": 1080 },
            "scaleFactor": 1,
        }),
        "plugin:window|available_monitors" => json!([{
            "position": { "x": 0, "y": 0 },
            "size": { "width": 1920, "height": 1080 },
            "scaleFactor": 1,
        }]),
        "plugin:event|unlisten" | "plugin:event|emit" | "plugin:event|emit_to" => Value::Null,
        "plugin:opener|open_url"
        | "plugin:opener|open_path"
        | "plugin:opener|reveal_item_in_dir" => Value::Null,
        "plugin:process|relaunch" | "plugin:process|exit" => Value::Null,
        "plugin:updater|check" => Value::Null,
        "plugin:clipboard-manager|read_text" => json!(""),
        "plugin:clipboard-manager|write_text" => Value::Null,
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
    fn connection_list_returns_ui_shaped_array() {
        let mut model = GuiModel::with_size(4, 24);
        model.add_profile("dev", "10.0.0.1", 22, "root");
        model.add_profile("prod", "10.0.0.2", 2222, "ops");
        let body = invoke(&mut model, "connection_list", json!({}));
        let sessions = body.as_array().expect("array");
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0]["name"], "dev");
        assert_eq!(sessions[0]["host"], "10.0.0.1");
        assert_eq!(sessions[0]["port"], 22);
        assert_eq!(sessions[0]["username"], "root");
        assert_eq!(sessions[0]["id"], "session-0");
        assert_eq!(sessions[1]["name"], "prod");
    }

    #[test]
    fn connection_upsert_returns_profile_and_selects() {
        let mut model = GuiModel::with_size(4, 24);
        let body = invoke(
            &mut model,
            "connection_upsert",
            json!({ "request": { "name": "dev", "host": "10.0.0.1", "port": 2222, "username": "root" } }),
        );
        assert_eq!(body["name"], "dev");
        assert_eq!(body["port"], 2222);
        assert_eq!(model.profile_count(), 1);
        assert_eq!(model.selected_profile(), Some(0));
    }

    #[test]
    fn connection_upsert_edit_replaces_profile() {
        let mut model = GuiModel::with_size(4, 24);
        model.add_profile("dev", "10.0.0.1", 22, "root");
        let body = invoke(
            &mut model,
            "connection_upsert",
            json!({ "request": { "id": "session-0", "name": "dev2", "host": "10.0.0.9", "port": 22, "username": "ops" } }),
        );
        assert_eq!(body["host"], "10.0.0.9");
        assert_eq!(body["username"], "ops");
        assert_eq!(model.profile_count(), 1);
    }

    #[test]
    fn terminal_connect_returns_session_id_string() {
        let mut model = GuiModel::with_size(4, 24);
        let body = invoke(
            &mut model,
            "terminal_connect",
            json!({ "request": { "host": "10.0.0.1", "port": 22, "username": "root", "cols": 80, "rows": 24 } }),
        );
        assert_eq!(body, "session-0");
        assert_eq!(model.phase().as_str(), "connecting");
        invoke(&mut model, "terminal_close", json!({}));
        assert_eq!(model.phase().as_str(), "disconnected");
    }

    #[test]
    fn terminal_connect_by_connection_id_reuses_profile() {
        let mut model = GuiModel::with_size(4, 24);
        model.add_profile("dev", "10.0.0.1", 22, "root");
        let body = invoke(
            &mut model,
            "terminal_connect",
            json!({ "request": { "connection_id": "session-0", "cols": 80, "rows": 24 } }),
        );
        assert_eq!(body, "session-0");
        assert_eq!(model.profile_count(), 1);
        assert_eq!(model.phase().as_str(), "connecting");
    }

    #[test]
    fn connection_test_returns_step_result() {
        let mut model = GuiModel::with_size(4, 24);
        let body = invoke(&mut model, "connection_test", json!({ "request": {} }));
        assert_eq!(body["ok"], true);
        assert!(body["message"].as_str().is_some());
    }

    #[test]
    fn void_commands_return_null() {
        let mut model = GuiModel::with_size(4, 24);
        model.add_profile("dev", "10.0.0.1", 22, "root");
        invoke(
            &mut model,
            "terminal_resize",
            json!({ "rows": 10, "cols": 40 }),
        );
        assert_eq!(model.grid().rows(), 10);
        assert!(invoke(&mut model, "terminal_write", json!({ "data": "hi\n" })).is_null());
        assert!(invoke(
            &mut model,
            "connection_delete",
            json!({ "id": "session-0" })
        )
        .is_null());
        assert_eq!(model.profile_count(), 0);
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
    fn window_plugin_commands_return_benign_values() {
        let mut model = GuiModel::with_size(4, 24);
        let pos = invoke(&mut model, "plugin:window|outer_position", json!({}));
        assert_eq!(pos["x"], 0);
        let monitors = invoke(&mut model, "plugin:window|available_monitors", json!({}));
        assert!(monitors.as_array().is_some());
        // plugin:event|listen is handled by on_web_message (the WebView2 layer).
    }

    #[test]
    fn plugin_window_actions_map_to_window_actions() {
        let mut model = GuiModel::with_size(4, 24);
        for (cmd, expected) in [
            ("plugin:window|minimize", WindowAction::Minimize),
            (
                "plugin:window|toggle_maximize",
                WindowAction::ToggleMaximize,
            ),
            ("plugin:window|close", WindowAction::Close),
            ("plugin:window|start_dragging", WindowAction::StartDrag),
        ] {
            let msg =
                json!({ "kind": "invoke", "requestId": 1, "cmd": cmd, "payload": {} }).to_string();
            let (_, action) = handle_message(&mut model, &msg).expect("reply");
            assert_eq!(action, expected, "{cmd} should map to {expected:?}");
        }
    }

    #[test]
    fn unknown_command_resolves_null() {
        let mut model = GuiModel::with_size(4, 24);
        let body = invoke(&mut model, "docker_list_containers", json!({}));
        assert!(body.is_null());
    }

    #[test]
    fn event_registry_listen_unlisten_and_output() {
        let mut registry = EventRegistry::default();
        let id = registry.listen("terminal:output", 42);
        assert_eq!(id, 1);
        assert_eq!(registry.len(), 1);
        let events = registry.terminal_output_events("session-0", b"hi\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, 42);
        assert_eq!(events[0].1["payload"]["session_id"], "session-0");
        assert_eq!(events[0].1["payload"]["data"][0], 104);
        registry.unlisten("terminal:output", 1);
        assert_eq!(registry.len(), 0);
        assert!(registry
            .terminal_output_events("session-0", b"x")
            .is_empty());
    }

    #[test]
    fn file_transfer_surface_is_interface_only() {
        let mut model = GuiModel::with_size(4, 24);
        assert!(invoke(&mut model, "remote_file_list", json!({})).is_array());
        assert!(invoke(&mut model, "remote_file_read", json!({})).is_null());
        assert!(invoke(&mut model, "remote_file_write", json!({})).is_null());
        assert!(invoke(&mut model, "remote_file_upload_file", json!({})).is_null());
        assert!(invoke(&mut model, "connection_transfer_export", json!({})).is_null());
    }

    #[test]
    fn get_status_reports_model() {
        let mut model = GuiModel::with_size(4, 24);
        model.add_profile("dev", "10.0.0.1", 22, "root");
        model.connect_profile(0);
        let body = invoke(&mut model, "get_status", json!({}));
        assert_eq!(body["phase"], "connecting");
        assert_eq!(body["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(body["sessions"][0]["id"], "session-0");
    }
}
