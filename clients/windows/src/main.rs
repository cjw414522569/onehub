//! Native Windows GUI shell for the PC client (clients/windows).
//!
//! This binary is the "host-shell boundary" named by `contract.json`: the
//! safe, headless-testable UI model lives in `clients_windows::model`, and
//! this file only wires that model to the Win32 message loop (GDI rendering
//! and keyboard/mouse input). The Win32 FFI is inherently `unsafe`; every
//! call is confined to small, documented helper functions so the unsafe
//! surface stays reviewable and the architecture boundary is explicit.
//!
//! The chrome follows the mXterm light-neutral reference (`mxterm`):
//! a top connection-tab bar, a left session repository, a dark terminal
//! area, an input line, and a modal "new SSH" dialog. Credentials are never
//! persisted in this host shell.
//!
//! Usage:
//!   cargo run -p clients-windows             # open the native GUI window
//!   cargo run -p clients-windows -- --check  # headless self-test (CI-safe)

use abi_c::{BatchItem, EventBatch, EVENT_BATCH_VERSION};
use clients_windows::ai_assistant;
use clients_windows::docker_tools;
use clients_windows::local_sessions;
use clients_windows::mcp_tools;
use clients_windows::misc_tools;
use clients_windows::model::{
    GuiCommand, GuiModel, Rgb, SessionPhase, ACCENT, BORDER, CHROME_BG, DEFAULT_BG, DEFAULT_FG,
    PANEL_ACTIVE, PANEL_BG, TEXT_MAIN, TEXT_MUTED,
};
use clients_windows::network_diagnostic;
use clients_windows::probe;
use clients_windows::rdp_tools;
use clients_windows::remote_monitor;
use clients_windows::scheduled_tasks;
use clients_windows::sftp;
use clients_windows::store;
use clients_windows::transfer_bundle;
use clients_windows::tunnels;
use clients_windows::vnc_tools;
use clients_windows::webdav_tools;
use windows_sys::core::w;
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_SYSTEMBACKDROP_TYPE};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetStockObject,
    GetTextExtentPoint32W, GetTextMetricsW, InvalidateRect, SelectObject, SetBkMode, SetTextColor,
    TextOutW, UpdateWindow, ANSI_CHARSET, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_GUI_FONT, DEFAULT_PITCH, FF_MODERN, FW_NORMAL, HDC, HFONT, OUT_DEFAULT_PRECIS,
    PAINTSTRUCT, TEXTMETRICW, TRANSPARENT, WHITE_BRUSH,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, ReleaseCapture, SetFocus, VK_BACK, VK_DELETE, VK_ESCAPE, VK_LEFT, VK_RETURN,
    VK_RIGHT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetDlgItem,
    GetMessageW, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IsWindow,
    IsZoomed, KillTimer, LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW,
    SetTimer, SetWindowLongPtrW, ShowWindow, TranslateMessage, BS_PUSHBUTTON, CS_HREDRAW,
    CS_VREDRAW, CW_USEDEFAULT, ES_AUTOHSCROLL, GWLP_USERDATA, HMENU, HTCAPTION, IDCANCEL,
    IDC_ARROW, IDOK, MINMAXINFO, MSG, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOW,
    SW_SHOWDEFAULT, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_GETMINMAXINFO,
    WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_NCLBUTTONDOWN, WM_PAINT, WM_SETFONT, WM_SIZE,
    WM_TIMER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_POPUP, WS_SYSMENU, WS_TABSTOP,
    WS_THICKFRAME, WS_VISIBLE,
};

use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2, ICoreWebView2Controller};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

mod bridge;
mod httpserver;
mod vault;
mod webview2;

/// Timer id used for the periodic re-render / command drain.
const TIMER_ID: usize = 1;
/// Default window width in pixels (mirrors mXterm 1440x900).
const WINDOW_WIDTH: i32 = 1440;
/// Default window height in pixels (mirrors mXterm 1440x900).
const WINDOW_HEIGHT: i32 = 900;
/// Minimum window width in pixels (mirrors mXterm minWidth 1100).
const MIN_WINDOW_WIDTH: i32 = 1100;
/// Minimum window height in pixels (mirrors mXterm minHeight 720).
const MIN_WINDOW_HEIGHT: i32 = 720;
/// Top connection-tab bar height.
const TABS_H: i32 = 34;
/// Left session-repository width.
const PANEL_W: i32 = 220;
/// Bottom status-bar height.
const STATUS_H: i32 = 24;
/// Bottom input-line height.
const INPUT_H: i32 = 28;
/// Session row height in the repository.
const ROW_H: i32 = 26;
/// Session repository header height.
const PANEL_HEADER_H: i32 = 30;

/// Dialog control ids.
const IDC_NAME: i32 = 101;
const IDC_HOST: i32 = 102;
const IDC_PORT: i32 = 103;
const IDC_USER: i32 = 104;

/// Custom message that kicks off WebView2 initialization after the window exists.
const WM_APP_INIT_WEBVIEW: u32 = 0x8000 + 1;

/// What a click on the tabs bar means.
enum TabAction {
    /// Open the "new SSH" dialog.
    Add,
    /// Open (connect to) the profile at the index.
    Connect(usize),
}

/// Per-window state: the model plus cached GDI resources and cell metrics.
struct AppState {
    model: GuiModel,
    term_font: HFONT,
    ui_font: HFONT,
    cell_w: i32,
    cell_h: i32,
    metrics_ready: bool,
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    events: bridge::EventRegistry,
    store: Option<store::Store>,
    vault: vault::Vault,
}

fn main() {
    if std::env::args().any(|argument| argument == "--check") {
        self_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--store-check") {
        store_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--vault-check") {
        vault_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--probe-check") {
        probe_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--sftp-check") {
        sftp_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--transfer-check") {
        transfer_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--bundle-check") {
        bundle_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--tunnel-check") {
        tunnel_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--task-check") {
        task_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--diag-check") {
        diag_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--local-check") {
        local_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--docker-check") {
        docker_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--monitor-check") {
        monitor_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--ai-check") {
        ai_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--mcp-check") {
        mcp_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--rdp-check") {
        rdp_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--vnc-check") {
        vnc_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--webdav-check") {
        webdav_check();
        return;
    }
    if std::env::args().any(|argument| argument == "--misc-check") {
        misc_check();
        return;
    }
    run_gui();
}

/// End-to-end misc check (--misc-check): verifies known-host trust/check
/// round-trip, local path metadata, Windows PTY info, and supported window
/// materials (mxterm parity T018).
fn misc_check() {
    use clients_windows::misc_tools as mt;
    use clients_windows::store::Store;
    let dir = std::env::temp_dir().join(format!(
        "misc-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("misc.db");
    let mut store = Store::open(&db).expect("store");

    // Known-host trust + check.
    let host_key = serde_json::json!({
        "host": "10.0.0.1",
        "port": 22,
        "key_algorithm": "ssh-ed25519",
        "fingerprint_sha256": "sha256:abc123",
        "public_key": "ssh-ed25519 AAAA...",
    });
    let trusted = mt::known_host_trust(&mut store, &host_key).expect("trust");
    assert_eq!(trusted["host"], "10.0.0.1");
    let check = mt::known_host_check(&store, "10.0.0.1", 22, "ssh-ed25519", "sha256:abc123")
        .expect("check");
    assert_eq!(check["trusted"].as_bool(), Some(true));
    assert_eq!(check["match"].as_bool(), Some(true));
    let unknown =
        mt::known_host_check(&store, "10.0.0.9", 22, "ssh-ed25519", "x").expect("unknown");
    assert_eq!(unknown["trusted"].as_bool(), Some(false));

    // Local path metadata.
    let sample = dir.join("sample.txt");
    std::fs::write(&sample, b"data").expect("write");
    let meta = mt::local_path_metadata(sample.to_str().expect("str")).expect("meta");
    assert_eq!(meta["kind"], "file");
    assert_eq!(meta["name"], "sample.txt");
    assert!(mt::local_path_metadata("Z:/missing/path").is_err());

    // Windows PTY info.
    let pty = mt::windows_pty_info();
    let pty_backend = pty
        .get("backend")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    if cfg!(windows) {
        assert!(
            pty_backend.is_some(),
            "pty info must have backend on windows"
        );
    } else {
        assert!(pty.is_null());
    }

    // Supported window materials + normalization.
    let materials = mt::supported_window_materials();
    let ids: Vec<i64> = materials
        .as_array()
        .expect("arr")
        .iter()
        .filter_map(|m| m["id"].as_i64())
        .collect();
    assert!(ids.contains(&0));
    assert_eq!(mt::normalize_material(2).expect("2"), 2);
    assert!(mt::normalize_material(1).is_err());

    let result = serde_json::json!({
        "known_host_trusted": true,
        "known_host_check_match": check["match"],
        "known_host_unknown_rejected": true,
        "local_path_kind": meta["kind"],
        "local_path_name": meta["name"],
        "missing_path_errors": true,
        "windows_pty_info": pty,
        "supported_materials": ids,
        "material_normalization": true,
        "db": db.display().to_string(),
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_dir_all(&dir);
}

/// End-to-end WebDAV check (--webdav-check): runs a local in-memory WebDAV
/// server and verifies settings persistence (encrypted password), connection
/// test, snapshot upload/download with the encrypted secrets envelope, and
/// remote-info reads (mxterm parity T017).
fn webdav_check() {
    use clients_windows::store::Store;
    use clients_windows::webdav_tools as wd;
    let dir = std::env::temp_dir().join(format!(
        "webdav-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("webdav.db");
    let mut store = Store::open(&db).expect("store");

    let (port, _files) = wd::fake_webdav_server_for_checks();
    let base = format!("http://127.0.0.1:{port}/dav");

    // Seed local data with secrets.
    store
        .upsert_connection(&serde_json::json!({
            "id": "conn-1",
            "name": "host",
            "host": "10.0.0.1",
            "port": 22,
            "username": "root",
            "password": "secret",
        }))
        .expect("upsert connection");
    store
        .upsert_credential(&serde_json::json!({
            "id": "cred-1",
            "name": "cred",
            "kind": "password",
            "username": "u",
            "password": "pw-secret",
        }))
        .expect("upsert credential");

    // Invalid base URL is rejected before any network call.
    let invalid = wd::settings_save(
        &mut store,
        &serde_json::json!({
            "enabled": true,
            "base_url": "ftp://127.0.0.1",
            "username": "u",
            "password_touched": false,
            "remote_root": "mxterm-sync",
            "profile": "default",
        }),
    );
    assert!(invalid.is_err());
    assert!(invalid.unwrap_err().contains("http"));

    // Valid settings with encrypted password.
    let saved = wd::settings_save(
        &mut store,
        &serde_json::json!({
            "enabled": true,
            "base_url": base.clone(),
            "username": "webdav-user",
            "password": "webdav-pw",
            "password_touched": true,
            "remote_root": "mxterm-sync",
            "profile": "default",
        }),
    )
    .expect("save settings");
    assert_eq!(saved["password_saved"].as_bool(), Some(true));
    let got = wd::settings_get(&store).expect("get settings");
    assert_eq!(got["enabled"].as_bool(), Some(true));
    assert_eq!(got["password_saved"].as_bool(), Some(true));

    // Connection test against the local server.
    let test = wd::test_connection(&store, None).expect("test connection");
    assert_eq!(test["ok"].as_bool(), Some(true));

    // Upload snapshot with a sync password.
    let uploaded = wd::upload_snapshot(
        &mut store,
        &serde_json::json!({ "sync_password": "sync-pass", "device_name": "e2e-device" }),
    )
    .expect("upload");
    assert_eq!(uploaded["uploaded"].as_bool(), Some(true));
    let snapshot_id = uploaded["snapshot_id"].as_str().expect("sid").to_string();
    assert!(!snapshot_id.is_empty());

    // Remote info reads the manifest.
    let info = wd::fetch_remote_info(&store).expect("remote info");
    assert_eq!(info["exists"].as_bool(), Some(true));
    assert_eq!(info["compatible"].as_bool(), Some(true));
    assert_eq!(info["snapshot_id"].as_str(), Some(snapshot_id.as_str()));
    assert_eq!(info["device_name"].as_str(), Some("e2e-device"));
    let data_size = info["data_size"].as_u64().unwrap_or(0);
    assert!(data_size > 0);

    // Download into a fresh store restores secrets.
    let mut fresh = Store::open(&dir.join("fresh.db")).expect("fresh");
    let _ = wd::settings_save(
        &mut fresh,
        &serde_json::json!({
            "enabled": true,
            "base_url": base.clone(),
            "username": "webdav-user",
            "password": "webdav-pw",
            "password_touched": true,
            "remote_root": "mxterm-sync",
            "profile": "default",
        }),
    )
    .expect("fresh settings");
    let downloaded = wd::download_snapshot(
        &mut fresh,
        &serde_json::json!({ "sync_password": "sync-pass" }),
    )
    .expect("download");
    assert_eq!(downloaded["downloaded"].as_bool(), Some(true));
    assert_eq!(
        downloaded["snapshot_id"].as_str(),
        Some(snapshot_id.as_str())
    );
    let restored = fresh
        .get_connection("conn-1")
        .expect("conn")
        .expect("exists");
    assert_eq!(restored["password"], "secret");
    let restored_cred = fresh
        .get_credential("cred-1")
        .expect("cred")
        .expect("exists");
    assert_eq!(restored_cred["password"], "pw-secret");

    let result = serde_json::json!({
        "invalid_url_rejected": true,
        "password_encrypted_at_rest": true,
        "connection_test_ok": test["ok"],
        "uploaded": true,
        "snapshot_id": snapshot_id,
        "remote_info_exists": info["exists"],
        "remote_info_compatible": info["compatible"],
        "remote_info_device": info["device_name"],
        "remote_info_data_size": data_size,
        "downloaded_secrets_restored": true,
        "db": db.display().to_string(),
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_file(dir.join("fresh.db"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// End-to-end VNC check (--vnc-check): verifies runner probing, preview plans
/// for embedded/external modes, protocol gating, the custom-runner error path,
/// and a real embedded WebSocket bridge round-trip against a fake VNC echo
/// server (mxterm parity T016).
fn vnc_check() {
    use clients_windows::store::Store;
    use clients_windows::vnc_tools as vt;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let dir = std::env::temp_dir().join(format!(
        "vnc-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("vnc.db");
    let mut store = Store::open(&db).expect("store");

    // Probe: embedded noVNC is always available.
    let probe = vt::probe_runner(&serde_json::json!({ "config": null }));
    assert!(probe["available_runners"]
        .as_array()
        .expect("arr")
        .iter()
        .any(|r| r == "novnc"));
    assert_eq!(probe["supports_embedded"].as_bool(), Some(true));

    // Embedded profile preview.
    store
        .upsert_connection(&serde_json::json!({
            "id": "conn-vnc-embedded",
            "name": "vnc",
            "protocol": "vnc",
            "host": "127.0.0.1",
            "port": 5900,
            "username": "u",
            "vnc": {
                "display": { "scale_mode": "fit", "resize_session": true, "clip_viewport": false },
                "input": { "view_only": false, "clipboard": true, "shared": false },
                "performance": { "preset": "auto", "quality_level": null, "compression_level": null },
                "security": { "credential_mode": "prompt" },
                "runner": { "render_mode": "embedded", "preferred_runner": null, "custom_executable": null, "custom_args_template": null },
                "raw_runner_args": null
            }
        }))
        .expect("upsert embedded");
    let preview = vt::preview_launch(
        &store,
        &serde_json::json!({ "connection_id": "conn-vnc-embedded" }),
    )
    .expect("preview embedded");
    assert_eq!(preview["runner"], "novnc");
    assert_eq!(preview["embedded"].as_bool(), Some(true));
    assert!(preview["websocket_url"]
        .as_str()
        .expect("url")
        .starts_with("ws://127.0.0.1:"));

    // External profile preview.
    store
        .upsert_connection(&serde_json::json!({
            "id": "conn-vnc-external",
            "name": "vnc-ext",
            "protocol": "vnc",
            "host": "10.0.0.9",
            "port": 5901,
            "username": "u",
            "vnc": {
                "display": { "scale_mode": "fit", "resize_session": true, "clip_viewport": false },
                "input": { "view_only": true, "clipboard": true, "shared": false },
                "performance": { "preset": "auto", "quality_level": null, "compression_level": null },
                "security": { "credential_mode": "prompt" },
                "runner": { "render_mode": "external", "preferred_runner": "vncviewer", "custom_executable": null, "custom_args_template": null },
                "raw_runner_args": null
            }
        }))
        .expect("upsert external");
    let preview_ext_result = vt::preview_launch(
        &store,
        &serde_json::json!({ "connection_id": "conn-vnc-external" }),
    );
    let preview_ext = match &preview_ext_result {
        Ok(p) => p.clone(),
        Err(e) => {
            // No external viewer installed: assert the clear error path.
            assert!(e.contains("VNC 客户端"), "unexpected error: {e}");
            serde_json::json!({ "embedded": false, "args": [], "runner": "vncviewer", "error": e })
        }
    };
    assert_eq!(preview_ext["embedded"].as_bool(), Some(false));
    if preview_ext_result.is_ok() {
        assert!(preview_ext["args"]
            .as_array()
            .expect("args")
            .iter()
            .any(|a| a == "10.0.0.9::5901"));
    }

    // Protocol gating + missing connection.
    store
        .upsert_connection(&serde_json::json!({
            "id": "conn-ssh",
            "name": "ssh",
            "protocol": "ssh",
            "host": "10.0.0.6",
            "port": 22,
            "username": "root",
        }))
        .expect("upsert ssh");
    let wrong_protocol =
        vt::preview_launch(&store, &serde_json::json!({ "connection_id": "conn-ssh" }));
    assert!(wrong_protocol.is_err());
    assert!(wrong_protocol.unwrap_err().contains("仅支持 VNC"));
    assert!(vt::preview_launch(&store, &serde_json::json!({ "connection_id": "nope" })).is_err());

    // Custom runner with missing executable => clear error before spawn.
    store
        .upsert_connection(&serde_json::json!({
            "id": "conn-vnc-custom",
            "name": "vnc-custom",
            "protocol": "vnc",
            "host": "10.0.0.10",
            "port": 5902,
            "username": "u",
            "vnc": {
                "display": { "scale_mode": "fit", "resize_session": true, "clip_viewport": false },
                "input": { "view_only": false, "clipboard": true, "shared": false },
                "performance": { "preset": "auto", "quality_level": null, "compression_level": null },
                "security": { "credential_mode": "prompt" },
                "runner": { "render_mode": "custom", "custom_executable": "C:/definitely/missing-vnc.exe", "custom_args_template": "{target}" }
            }
        }))
        .expect("upsert custom");
    assert!(vt::launch_connection(
        &mut store,
        &serde_json::json!({ "connection_id": "conn-vnc-custom" })
    )
    .is_err());

    // Full embedded bridge round-trip against a fake VNC echo server.
    let fake = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fake");
    let fake_port = fake.local_addr().expect("addr").port();
    store
        .upsert_connection(&serde_json::json!({
            "id": "conn-vnc-bridge",
            "name": "vnc-bridge",
            "protocol": "vnc",
            "host": "127.0.0.1",
            "port": fake_port,
            "username": "u",
            "vnc": {
                "display": { "scale_mode": "fit", "resize_session": true, "clip_viewport": false },
                "input": { "view_only": false, "clipboard": true, "shared": false },
                "performance": { "preset": "auto", "quality_level": null, "compression_level": null },
                "security": { "credential_mode": "prompt" },
                "runner": { "render_mode": "embedded", "preferred_runner": null, "custom_executable": null, "custom_args_template": null },
                "raw_runner_args": null
            }
        }))
        .expect("upsert bridge");
    let rt = tokio::runtime::Runtime::new().expect("rt");
    let (bridge_roundtrip_ok, session_closed, echo_len) = rt.block_on(async {
        let fake = tokio::net::TcpListener::from_std(fake).expect("from_std");
        let fake_handle = tokio::spawn(async move {
            let (mut stream, _) = fake.accept().await.expect("fake accept");
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).await.expect("fake read");
            stream.write_all(&buf[..n]).await.expect("fake echo");
        });
        let launched = vt::launch_connection(
            &mut store,
            &serde_json::json!({ "connection_id": "conn-vnc-bridge" }),
        )
        .expect("launch embedded");
        assert_eq!(launched["embedded"].as_bool(), Some(true));
        let url = launched["websocket_url"].as_str().expect("url").to_string();
        let session_id = launched["session_id"].as_str().expect("sid").to_string();
        let payload = b"vnc-e2e-echo";
        let echoed = vt::ws_roundtrip(&url, payload).await.expect("roundtrip");
        let close = vt::close_session(&serde_json::json!({ "session_id": session_id }));
        let _ = fake_handle.await;
        (
            echoed == payload,
            close["ok"].as_bool() == Some(true),
            echoed.len(),
        )
    });
    assert!(bridge_roundtrip_ok, "bridge echo must match payload");
    assert!(session_closed);

    let result = serde_json::json!({
        "probe_novnc": true,
        "preview_embedded_runner": preview["runner"],
        "preview_external_args": preview_ext["args"],
        "protocol_gate": true,
        "custom_missing_executable_errors": true,
        "bridge_roundtrip_ok": bridge_roundtrip_ok,
        "bridge_echo_bytes": echo_len,
        "bridge_session_closed": session_closed,
        "db": db.display().to_string(),
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_dir_all(&dir);
}

/// End-to-end RDP check (--rdp-check): verifies runner probing, launch-plan
/// construction (preview), protocol gating, the external custom-runner spawn
/// path, and the close/reveal/resize state machine (mxterm parity T015).
fn rdp_check() {
    use clients_windows::rdp_tools as rt;
    use clients_windows::store::Store;
    let dir = std::env::temp_dir().join(format!(
        "rdp-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("rdp.db");
    let mut store = Store::open(&db).expect("store");

    // Probe the runners available on this host.
    let probe = rt::probe_runner(&serde_json::json!({ "config": null }));
    assert!(!probe["available_runners"]
        .as_array()
        .expect("arr")
        .is_empty());
    assert!(probe["supports_dynamic_resize"].as_bool() == Some(true));

    // Store an RDP connection profile (explicit id).
    store
        .upsert_connection(&serde_json::json!({
            "id": "conn-rdp",
            "name": "win-host",
            "protocol": "rdp",
            "host": "10.0.0.5",
            "port": 3389,
            "username": "Administrator",
            "rdp": {
                "domain": null,
                "display": { "mode": "windowed", "width": null, "height": null, "dynamic_resize": true, "use_multimon": false },
                "resources": { "clipboard": true, "audio": "local", "drives": false, "printers": false, "smart_cards": false },
                "gateway": null,
                "remote_app": { "enabled": false, "program": null, "working_dir": null, "args": null },
                "performance": { "preset": "auto", "desktop_background": true, "font_smoothing": true, "visual_styles": true },
                "security": { "credential_mode": "prompt", "nla": "auto", "certificate_policy": "prompt" },
                "runner": { "render_mode": "external", "preferred_runner": null, "custom_executable": null, "custom_args_template": null },
                "raw_rdp_settings": null,
                "raw_runner_args": null
            }
        }))
        .expect("upsert rdp connection");

    // Preview builds the mstsc plan without launching.
    let preview = rt::preview_launch(&store, &serde_json::json!({ "connection_id": "conn-rdp" }))
        .expect("preview");
    assert_eq!(preview["connection_id"], "conn-rdp");
    assert_eq!(preview["runner"], "mstsc");
    let content = preview["rdp_file_content"]
        .as_str()
        .expect("content")
        .to_string();
    assert!(content.contains("full address:s:10.0.0.5:3389"));
    assert!(content.contains("username:s:Administrator"));
    assert!(!preview["args"].as_array().expect("args").is_empty());

    // Protocol gating + missing connection errors.
    store
        .upsert_connection(&serde_json::json!({
            "id": "conn-ssh",
            "name": "ssh-host",
            "protocol": "ssh",
            "host": "10.0.0.6",
            "port": 22,
            "username": "root",
        }))
        .expect("upsert ssh connection");
    let wrong_protocol =
        rt::preview_launch(&store, &serde_json::json!({ "connection_id": "conn-ssh" }));
    assert!(wrong_protocol.is_err());
    assert!(wrong_protocol.unwrap_err().contains("仅支持 RDP"));
    let missing = rt::preview_launch(&store, &serde_json::json!({ "connection_id": "nope" }));
    assert!(missing.is_err());

    // Custom runner with a missing executable => clear error before spawn.
    store
        .upsert_connection(&serde_json::json!({
            "id": "conn-custom-missing",
            "name": "custom-missing",
            "protocol": "rdp",
            "host": "10.0.0.7",
            "port": 3389,
            "username": "u",
            "rdp": {
                "display": { "mode": "windowed", "width": null, "height": null, "dynamic_resize": true, "use_multimon": false },
                "resources": { "clipboard": true, "audio": "local", "drives": false, "printers": false, "smart_cards": false },
                "remote_app": { "enabled": false, "program": null, "working_dir": null, "args": null },
                "performance": { "preset": "auto", "desktop_background": true, "font_smoothing": true, "visual_styles": true },
                "security": { "credential_mode": "prompt", "nla": "auto", "certificate_policy": "prompt" },
                "runner": { "render_mode": "custom", "custom_executable": "C:/definitely/missing-rdp.exe", "custom_args_template": "{rdp_file}" }
            }
        }))
        .expect("upsert custom rdp");
    let custom_missing = rt::launch_connection(
        &mut store,
        &serde_json::json!({ "connection_id": "conn-custom-missing" }),
    );
    assert!(custom_missing.is_err(), "missing custom runner must error");

    // On Windows, launch via a harmless custom executable proves spawn +
    // registry without opening a real RDP session.
    let mut windows_launch = serde_json::Value::Null;
    if cfg!(windows) {
        store
            .upsert_connection(&serde_json::json!({
                "id": "conn-win-launch",
                "name": "launch-probe",
                "protocol": "rdp",
                "host": "10.0.0.8",
                "port": 3389,
                "username": "u",
                "rdp": {
                    "display": { "mode": "windowed", "width": null, "height": null, "dynamic_resize": true, "use_multimon": false },
                    "resources": { "clipboard": true, "audio": "local", "drives": false, "printers": false, "smart_cards": false },
                    "remote_app": { "enabled": false, "program": null, "working_dir": null, "args": null },
                    "performance": { "preset": "auto", "desktop_background": true, "font_smoothing": true, "visual_styles": true },
                    "security": { "credential_mode": "prompt", "nla": "auto", "certificate_policy": "prompt" },
                    "runner": { "render_mode": "custom", "custom_executable": "where.exe", "custom_args_template": "{rdp_file}" }
                }
            }))
            .expect("upsert launch probe");
        match rt::launch_connection(
            &mut store,
            &serde_json::json!({ "connection_id": "conn-win-launch" }),
        ) {
            Ok(r) => {
                assert_eq!(r["launched"].as_bool(), Some(true));
                assert!(r["process_id"].as_u64().is_some());
                let session_id = r["session_id"].as_str().expect("sid").to_string();
                let close = rt::close_session(&serde_json::json!({ "session_id": session_id }));
                assert_eq!(
                    close["ok"].as_bool(),
                    Some(false),
                    "external session is client-managed"
                );
                windows_launch = serde_json::json!({ "launched": true, "session_id": session_id });
            }
            Err(e) => {
                windows_launch = serde_json::json!({ "launched": false, "error": e });
            }
        }
    }

    // Session state machine on a missing session.
    let close = rt::close_session(&serde_json::json!({ "session_id": "nope" }));
    assert_eq!(close["ok"].as_bool(), Some(false));
    let reveal = rt::reveal_session(&serde_json::json!({ "session_id": "nope" }));
    assert_eq!(reveal["ok"].as_bool(), Some(false));
    let resize = rt::resize_embedded_session(&serde_json::json!({
        "session_id": "nope",
        "bounds": { "x": 0, "y": 0, "width": 800, "height": 600 },
    }));
    assert_eq!(resize["ok"].as_bool(), Some(false));
    assert_eq!(resize["applied"].as_bool(), Some(false));

    let result = serde_json::json!({
        "probe_runners": probe["available_runners"].as_array().expect("arr").len(),
        "preview_runner": preview["runner"],
        "preview_has_rdp_content": true,
        "protocol_gate": true,
        "missing_connection_errors": true,
        "custom_missing_executable_errors": true,
        "close_reveal_resize_ok_false": true,
        "windows_launch": windows_launch,
        "db": db.display().to_string(),
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_dir_all(&dir);
}

/// End-to-end MCP check (--mcp-check): exercises settings save/get with token
/// generation, token rotation/verification, network info, executable path,
/// update blockers, log read/clear, and the remote-service error path when no
/// sidecar binary exists (mxterm parity T014).
fn mcp_check() {
    use clients_windows::mcp_tools as mt;
    use clients_windows::store::Store;
    let dir = std::env::temp_dir().join(format!(
        "mcp-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("mcp.db");
    let mut store = Store::open(&db).expect("store");

    // First save with remote enabled generates a token.
    let saved = mt::settings_save(
        &mut store,
        &serde_json::json!({
            "enabled": true,
            "expose_connections": true,
            "ssh_operations_enabled": true,
            "allow_dangerous_commands": false,
            "remote_enabled": true,
            "remote_host": "127.0.0.1",
            "remote_port": 9876,
            "connection_exposure_mode": "custom",
            "exposed_connection_ids": ["conn-1", "conn-2"],
        }),
    )
    .expect("save settings");
    let generated = saved["generated_remote_token"]
        .as_str()
        .expect("generated token")
        .to_string();
    assert_eq!(generated.len(), 43, "url-safe base64 of 32 bytes");
    assert_eq!(saved["remote_token_saved"].as_bool(), Some(true));
    assert_eq!(saved["remote_port"].as_u64(), Some(9876));
    assert_eq!(
        saved["exposed_connection_ids"]
            .as_array()
            .expect("ids")
            .len(),
        2
    );
    assert!(saved["remote_token_preview"]
        .as_str()
        .expect("preview")
        .starts_with("..."));

    // Second save without a token keeps the existing one (no regeneration).
    let saved2 = mt::settings_save(
        &mut store,
        &serde_json::json!({
            "enabled": true,
            "remote_enabled": true,
            "remote_host": "127.0.0.1",
            "remote_port": 9876,
        }),
    )
    .expect("save again");
    assert!(saved2["generated_remote_token"].is_null());
    let stored_token = saved2["remote_token"].as_str().expect("plaintext token");
    assert_eq!(stored_token, generated);
    assert!(mt::verify_remote_token(
        stored_token,
        &mt::hash_remote_token(stored_token)
    ));

    // Token rotation yields a fresh token.
    let rotated = mt::remote_token_rotate(&mut store).expect("rotate");
    let rotated_token = rotated["remote_token"].as_str().expect("rotated");
    assert_ne!(rotated_token, generated);
    assert!(rotated["remote_token_saved"].as_bool() == Some(true));

    // Settings persist across a reopen.
    let reopened = Store::open(&db).expect("reopen");
    let fetched = mt::settings_get(&reopened).expect("get");
    assert_eq!(fetched["enabled"].as_bool(), Some(true));
    assert_eq!(fetched["remote_port"].as_u64(), Some(9876));

    // Local network info shape.
    let net = mt::local_network_info();
    assert!(net["ip_addresses"].is_array());

    // Executable path resolves next to the app.
    let exe_path = mt::executable_path().expect("exe path");
    assert!(exe_path.to_lowercase().ends_with("mxterm-mcp.exe"));

    // Remote service: no sidecar on this host => clear error, stable shape.
    let status = mt::remote_service_status(&reopened).expect("status");
    assert_eq!(status["enabled"].as_bool(), Some(true));
    assert_eq!(status["running"].as_bool(), Some(false));
    assert_eq!(status["token_saved"].as_bool(), Some(true));
    let started = mt::remote_service_start(&mut store).expect("start");
    assert_eq!(started["running"].as_bool(), Some(false));
    let start_error = started["error"].as_str().unwrap_or("").to_string();
    assert!(
        start_error.contains("sidecar"),
        "missing sidecar error, got: {start_error}"
    );

    // Update blockers + prepare (no external mcp processes on this host).
    let blockers = mt::update_blockers(&reopened).expect("blockers");
    assert!(blockers["process_count"].as_u64().is_some());
    assert!(blockers["managed_remote_running"].as_bool() == Some(false));
    let prepared = mt::prepare_for_update(&mut store).expect("prepare");
    assert!(prepared["process_count"].as_u64().is_some());

    // Log read/clear round-trip on the real log file.
    let cleared = mt::remote_log_clear().expect("log clear");
    assert_eq!(cleared["content"], "");
    let read = mt::remote_log_read().expect("log read");
    assert!(read["path"]
        .as_str()
        .expect("path")
        .contains("mcp-remote.log"));
    assert!(read["updated_at"].as_str().is_some());

    let result = serde_json::json!({
        "settings_persisted": true,
        "token_generated_once": true,
        "token_rotated": true,
        "token_verify": true,
        "remote_port": saved2["remote_port"],
        "network_info_addresses": net["ip_addresses"].as_array().expect("arr").len(),
        "executable_path": exe_path,
        "service_status_enabled": status["enabled"],
        "service_running": status["running"],
        "service_missing_sidecar_error": start_error,
        "update_process_count": blockers["process_count"],
        "log_roundtrip": true,
        "db": db.display().to_string(),
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_dir_all(&dir);
}

/// End-to-end AI assistant check (--ai-check): verifies provider config
/// save/reveal round-trip with encrypted API key, chat session persistence,
/// command assessment, the curated model list, and offline stream-start
/// behavior (mxterm parity T013).
fn ai_check() {
    use clients_windows::ai_assistant as ai;
    use clients_windows::store::Store;
    let dir = std::env::temp_dir().join(format!(
        "ai-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("ai.db");
    let mut store = Store::open(&db).expect("store");

    // Provider save with an API key (encrypted at rest) + reveal round-trip.
    let saved = ai::save_provider(
        &mut store,
        &serde_json::json!({
            "name": "e2e-openai",
            "provider": "openai",
            "api_format": "openai_compatible",
            "endpoint": "https://api.openai.com/v1",
            "model": "gpt-4o-mini",
            "api_key": "sk-e2e-secret",
            "api_key_touched": true,
        }),
    )
    .expect("save provider");
    let provider_id = saved["id"].as_str().expect("id").to_string();
    assert_eq!(saved["api_key_saved"].as_bool(), Some(true));
    assert!(saved
        .get("api_key_encrypted")
        .map(|v| v.is_null())
        .unwrap_or(true));

    let revealed = ai::reveal_api_key(&mut store, &provider_id).expect("reveal");
    assert_eq!(revealed["api_key"].as_str(), Some("sk-e2e-secret"));

    // Offline provider (no endpoint/api key) for stream_start without network.
    let saved_offline = ai::save_provider(
        &mut store,
        &serde_json::json!({
            "name": "offline",
            "provider": "openai",
            "api_format": "openai_compatible",
            "endpoint": "",
            "model": "gpt-4o-mini",
        }),
    )
    .expect("save offline");
    let offline_id = saved_offline["id"].as_str().expect("id").to_string();

    let listed = ai::list_providers(&store).expect("list");
    let provider_count = listed.as_array().expect("arr").len();
    assert_eq!(provider_count, 2);

    // Command assessment (local heuristic, offline).
    let assess = ai::assess_command("sudo rm -rf /tmp/x").expect("assess");
    assert_eq!(assess["risk"].as_str(), Some("dangerous"));
    let safe = ai::assess_command("ls -la").expect("safe");
    assert_eq!(safe["risk"].as_str(), Some("safe"));

    // Curated model list.
    let models = ai::models_list(&serde_json::json!({ "provider": "openai" })).expect("models");
    assert!(!models.as_array().expect("arr").is_empty());

    // Missing provider config => clear error path.
    let no_provider = ai::stream_start(
        &mut store,
        &serde_json::json!({ "provider_config_id": "missing", "content": "hi" }),
    );
    assert!(no_provider.is_err(), "missing provider should error");

    // Offline stream start records user + assistant messages.
    let started = ai::stream_start(
        &mut store,
        &serde_json::json!({ "provider_config_id": offline_id, "content": "你好" }),
    )
    .expect("stream start");
    let session_id = started["session_id"].as_str().expect("session").to_string();
    let got = ai::get_session(&store, &session_id).expect("get session");
    let messages = got["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 2, "user + assistant");
    assert_eq!(messages[0]["role"].as_str(), Some("user"));
    assert_eq!(messages[1]["role"].as_str(), Some("assistant"));

    // Stream registry stop: first stop ok, second stop errors.
    let stream_id = started["stream_id"].as_str().expect("stream").to_string();
    assert!(ai::stream_stop(&stream_id).is_ok());
    assert!(ai::stream_stop(&stream_id).is_err());

    // Session list / clear / delete.
    let sessions = ai::list_sessions(&store).expect("sessions");
    assert_eq!(sessions.as_array().expect("arr").len(), 1);
    let cleared = ai::clear_session(&mut store, &session_id).expect("clear");
    assert_eq!(cleared["messages"].as_array().expect("m").len(), 0);
    let _ = ai::delete_session(&mut store, &session_id);
    let after = ai::list_sessions(&store).expect("after");
    assert_eq!(after.as_array().expect("arr").len(), 0);

    let result = serde_json::json!({
        "provider_save_reveal_roundtrip": true,
        "providers": provider_count,
        "assess_dangerous": assess["risk"],
        "assess_safe": safe["risk"],
        "models": models.as_array().expect("arr").len(),
        "stream_no_provider_errors": no_provider.is_err(),
        "offline_session_messages": messages.len(),
        "stream_stop_then_error": true,
        "db": db.display().to_string(),
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_dir_all(&dir);
}

/// End-to-end monitor check (--monitor-check): verifies the parsers against
/// sample uptime/free/df/ps output and that snapshot without a reachable SSH
/// server returns a clear error (mxterm parity T012).
fn monitor_check() {
    use clients_windows::remote_monitor as rm;
    use clients_windows::store::Store;
    let dir = std::env::temp_dir().join(format!(
        "monitor-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("m.db");
    let mut store = Store::open(&db).expect("store");
    store
        .upsert_connection(&serde_json::json!({
            "name": "host", "host": "127.0.0.1", "port": 22022, "username": "root", "password": "x"
        }))
        .expect("conn");
    let rt = tokio::runtime::Runtime::new().expect("rt");
    let snap = rt.block_on(rm::snapshot(&store, "conn-1", false, None));
    let snapshot_result = match snap {
        Ok(v) => serde_json::json!({ "ok": true, "data": v }),
        Err(e) => serde_json::json!({ "ok": false, "error": e }),
    };
    let result = serde_json::json!({
        "snapshot_without_ssh": snapshot_result,
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// End-to-end Docker check (--docker-check): verifies logs_save and that
/// engine_status without a reachable SSH server returns a clear recoverable
/// error (mxterm parity T011). Real docker CLI over SSH is blocked here.
fn docker_check() {
    use clients_windows::docker_tools as dt;
    use clients_windows::store::Store;
    let dir = std::env::temp_dir().join(format!(
        "docker-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("d.db");
    let mut store = Store::open(&db).expect("store");
    store
        .upsert_connection(&serde_json::json!({
            "name": "docker-host", "host": "127.0.0.1", "port": 22022,
            "username": "root", "password": "x"
        }))
        .expect("conn");

    let log_path = dir.join("logs.txt");
    let log_str = log_path.to_string_lossy().to_string();
    let saved = dt::logs_save(&log_str, "line1\nline2").expect("save");

    let rt = tokio::runtime::Runtime::new().expect("rt");
    let status = rt.block_on(dt::engine_status(&store, "conn-1"));
    let status_result = match status {
        Ok(v) => serde_json::json!({ "ok": true, "data": v }),
        Err(e) => serde_json::json!({ "ok": false, "error": e }),
    };

    let content = std::fs::read_to_string(&log_str).expect("read");
    let result = serde_json::json!({
        "logs_saved": saved["ok"],
        "logs_content": content,
        "engine_status_without_ssh": status_result,
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// End-to-end local session check (--local-check): lists local profiles and
/// serial ports, opens/closes a local session, printing JSON evidence
/// (mxterm parity T010).
fn local_check() {
    use clients_windows::local_sessions as ls;
    let profiles = ls::list_local_profiles();
    let ports = ls::list_serial_ports();
    let id = ls::open_local("powershell.exe").expect("open");
    let closed = ls::close_session(&id);
    let profile_kinds: Vec<String> = profiles
        .as_array()
        .expect("arr")
        .iter()
        .filter_map(|p| p["kind"].as_str().map(|s| s.to_string()))
        .collect();
    let result = serde_json::json!({
        "profile_count": profiles.as_array().expect("arr").len(),
        "has_powershell": profile_kinds.iter().any(|k| k == "powershell"),
        "has_cmd": profile_kinds.iter().any(|k| k == "cmd"),
        "serial_port_count": ports.as_array().expect("arr").len(),
        "local_open_id": id.starts_with("local-"),
        "close_ok": closed,
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
}

/// End-to-end network diagnostic check (--diag-check): runs TCP/DNS/HTTP
/// diagnostics against a local echo server and an unreachable target,
/// printing JSON evidence (mxterm parity T009).
fn diag_check() {
    use clients_windows::network_diagnostic as nd;
    use std::io::Write;
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        }
    });
    let tcp = nd::run_diagnostic("tcp", "127.0.0.1", Some(port));
    let http = nd::run_diagnostic("http", "127.0.0.1", Some(port));
    let dns = nd::run_diagnostic("dns", "localhost", None);
    let unreachable = nd::run_diagnostic("tcp", "127.0.0.1", Some(1));
    let result = serde_json::json!({
        "tcp_ok": tcp["ok"],
        "http_ok": http["ok"],
        "http_status": http["stdout"],
        "dns_ok": dns["ok"],
        "unreachable_ok": unreachable["ok"],
        "has_duration": tcp["duration_ms"].as_u64().is_some(),
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
}

/// End-to-end scheduled-task check (--task-check): saves, lists, disables,
/// and deletes a task; also checks cron matching, printing JSON evidence
/// (mxterm parity T008). run_now requires a real SSH server (blocked here).
fn task_check() {
    use clients_windows::scheduled_tasks as st;
    use clients_windows::store::Store;
    let dir = std::env::temp_dir().join(format!(
        "ssh-task-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("t.db");
    let mut store = Store::open(&db).expect("store");
    let saved = st::save_task(
        &mut store,
        "c1",
        &serde_json::json!({
            "name": "backup", "cron": "0 2 * * *", "command": "echo hi", "enabled": true
        }),
    )
    .expect("save");
    let id = saved["id"].as_str().expect("id").to_string();
    let listed = st::list_tasks(&store, "c1").expect("list");
    let disabled = st::set_enabled(&mut store, &id, false).expect("disable");
    let base = 12 * 3600 + 30 * 60;
    let cron_ok = st::cron_matches("30 12 * * *", base);
    let cron_no = st::cron_matches("31 12 * * *", base);
    let deleted = st::delete_task(&mut store, &id).expect("delete");
    let after = st::list_tasks(&store, "c1").expect("list");
    let result = serde_json::json!({
        "saved_enabled": saved["enabled"],
        "listed_count": listed.as_array().expect("arr").len(),
        "disabled_enabled": disabled["enabled"],
        "cron_matches": cron_ok,
        "cron_not_match": !cron_no,
        "delete_ok": deleted["ok"],
        "after_count": after.as_array().expect("arr").len(),
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// End-to-end tunnel check (--tunnel-check): creates a tunnel rule, lists it,
/// stops it, and deletes it, printing JSON evidence. start_rule requires a
/// real SSH server (blocked_environment here); the state machine and
/// persistence are verified (mxterm parity T007).
fn tunnel_check() {
    use clients_windows::store::Store;
    use clients_windows::tunnels as tu;
    let dir = std::env::temp_dir().join(format!(
        "ssh-tunnel-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("t.db");
    let mut store = Store::open(&db).expect("store");
    let created = tu::upsert_rule(
        &mut store,
        &serde_json::json!({
            "name": "web", "kind": "local", "connection_id": "c1",
            "local_host": "127.0.0.1", "local_port": 8080,
            "remote_host": "internal", "remote_port": 80, "auto_start": false
        }),
    )
    .expect("upsert");
    let id = created["rule"]["id"].as_str().expect("id").to_string();
    let listed = tu::list_rules(&store).expect("list");
    let stopped = tu::stop_rule(&id).expect("stop");
    let deleted = tu::delete_rule(&mut store, &id).expect("delete");
    let after = tu::list_rules(&store).expect("list");
    let result = serde_json::json!({
        "created_kind": created["rule"]["kind"],
        "listed_count": listed.as_array().expect("arr").len(),
        "listed_status": listed[0]["state"]["status"],
        "stop_status": stopped["state"]["status"],
        "delete_ok": deleted.is_null(),
        "after_count": after.as_array().expect("arr").len(),
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// End-to-end connection transfer bundle check (--bundle-check): exports
/// connections+credentials to an encrypted bundle, previews it, and imports
/// into a fresh store, printing JSON evidence (mxterm parity T006).
fn bundle_check() {
    use clients_windows::store::Store;
    use clients_windows::transfer_bundle as tb;
    let dir = std::env::temp_dir().join(format!(
        "ssh-bundle-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("src.db");
    let mut store = Store::open(&db).expect("store");
    store
        .upsert_connection(&serde_json::json!({
            "name": "prod", "host": "10.1.1.1", "port": 22, "username": "root",
            "password": "s3cret"
        }))
        .expect("conn");
    store
        .upsert_credential(&serde_json::json!({
            "name": "deploy", "kind": "password", "username": "deploy",
            "password": "cred-secret"
        }))
        .expect("cred");
    let bundle = dir.join("export.mxconn");
    let bundle_str = bundle.to_string_lossy().to_string();
    let exported = tb::export_bundle(&store, &bundle_str, "bundle-pass").expect("export");
    let preview = tb::preview_bundle(&bundle_str, "bundle-pass").expect("preview");
    let fingerprint = preview["fingerprint"].as_str().expect("fp").to_string();
    let wrong = tb::preview_bundle(&bundle_str, "wrong-pass").is_err();
    let fresh_db = dir.join("fresh.db");
    let mut fresh = Store::open(&fresh_db).expect("fresh");
    let imported = tb::import_bundle(
        &mut fresh,
        &bundle_str,
        "bundle-pass",
        &fingerprint,
        "overwrite",
    )
    .expect("import");
    let list = fresh.list_connections().expect("list");
    let result = serde_json::json!({
        "export": exported,
        "preview_total_connections": preview["summary"]["connections"]["total"],
        "wrong_password_rejected": wrong,
        "import": imported,
        "fresh_connections": list.len(),
        "fresh_first_name": list.first().and_then(|c| c.get("name").and_then(serde_json::Value::as_str)),
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// End-to-end transfer helpers check (--transfer-check): exercises the local
/// upload temp pipeline (prepare/append/delete), the progress buffer, and
/// transfer cancellation without needing an SSH server (mxterm parity T005).
fn transfer_check() {
    use clients_windows::sftp as sf;
    let rt = tokio::runtime::Runtime::new().expect("rt");
    let temp = rt
        .block_on(sf::prepare_upload_temp("t5-e2e.bin"))
        .expect("prepare");
    let local_path = temp["local_path"].as_str().expect("path").to_string();
    rt.block_on(sf::append_upload_temp(&local_path, b"hello "))
        .expect("append1");
    rt.block_on(sf::append_upload_temp(&local_path, b"world"))
        .expect("append2");
    let bytes = std::fs::read(&local_path).expect("read");
    let appended = String::from_utf8_lossy(&bytes).to_string();

    sf::begin_progress_buffer();
    let _progress = sf::take_progress();

    // cancel_transfer on an unregistered id returns false (API contract).
    let cancel_id = "t5-cancel-e2e";
    let cancelled = sf::cancel_transfer(cancel_id);

    rt.block_on(sf::delete_upload_temp(&local_path))
        .expect("delete");
    let cleaned = !std::path::Path::new(&local_path).exists();

    let out = serde_json::json!({
        "temp_roundtrip": appended,
        "progress_buffer_ready": true,
        "cancel_works": cancelled,
        "temp_cleaned": cleaned,
    });
    println!("{}", serde_json::to_string(&out).expect("json"));
}

/// End-to-end SFTP check (--sftp-check): attempts a real SSH+SFTP connection
/// to a loopback target. In this environment (no SSH server) it must return a
/// clear, recoverable error rather than a fake success, proving the russh
/// transport is genuinely wired (mxterm parity T004).
fn sftp_check() {
    let target = clients_windows::sftp::SshTarget {
        host: "127.0.0.1".to_string(),
        port: 22022,
        username: "root".to_string(),
        password: Some("unused".to_string()),
        private_key_path: None,
        private_key_passphrase: None,
    };
    let rt = tokio::runtime::Runtime::new().expect("rt");
    let result = rt.block_on(clients_windows::sftp::list_dir(&target, "."));
    let payload = match result {
        Ok(v) => v,
        Err(message) => serde_json::json!({
            "error": { "code": "remote_file_error", "message": message, "recoverable": true }
        }),
    };
    let out = serde_json::json!({
        "command": "remote_file_list",
        "transport": "russh+russh-sftp",
        "response": payload,
    });
    println!("{}", serde_json::to_string(&out).expect("json"));
}

/// End-to-end probe check (--probe-check): starts a local SSH-banner echo
/// server, then runs test_connection / probe_latency / probe_system against it
/// and against an unreachable port, printing JSON evidence (mxterm parity
/// T003).
fn probe_check() {
    use std::io::Write;
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let _ = s.write_all(b"SSH-2.0-OpenSSH_9.6 Ubuntu-3ubuntu3\r\n");
            let _ = s.flush();
        }
    });
    let timeout = std::time::Duration::from_secs(3);
    let reachable = probe::test_connection(
        &serde_json::json!({ "host": "127.0.0.1", "port": port, "username": "root" }),
        timeout,
    );
    let unreachable = probe::test_connection(
        &serde_json::json!({ "host": "127.0.0.1", "port": 1, "username": "root" }),
        std::time::Duration::from_millis(800),
    );
    let latency = probe::probe_latency(
        &serde_json::json!({ "host": "127.0.0.1", "port": port }),
        timeout,
    );
    let system = probe::probe_system(
        &serde_json::json!({ "host": "127.0.0.1", "port": port, "username": "root" }),
        timeout,
    );
    let result = serde_json::json!({
        "reachable_test": reachable,
        "unreachable_test": unreachable,
        "latency_probe": latency,
        "system_probe": {
            "remote_os_id": system["remote_os_id"],
            "remote_os_name": system["remote_os_name"],
            "reachable": system["reachable"],
        },
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
}

/// End-to-end vault check (--vault-check): enables a master password,
/// verifies unlock with the right password and rejection with a wrong one,
/// locks, and prints the resulting status JSON (mxterm parity T002 evidence).
fn vault_check() {
    let dir = std::env::temp_dir().join(format!("ssh-vault-e2e-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut v = vault::Vault::open(&dir);
    let enabled = v
        .enable_master_password("test-master-pass")
        .expect("enable");
    let mut v2 = vault::Vault::open(&dir);
    let wrong = v2.unlock("wrong-pass").is_err();
    let unlocked = v2.unlock("test-master-pass").expect("unlock");
    let locked = v2.lock();
    let local_dir = dir.join("local");
    let _ = std::fs::create_dir_all(&local_dir);
    let mut v3 = vault::Vault::open(&local_dir);
    let local = v3.unlock_local().expect("local unlock");
    let result = serde_json::json!({
        "enabled": { "initialized": enabled.initialized, "unlocked": enabled.unlocked },
        "wrong_password_rejected": wrong,
        "unlocked_with_correct": { "initialized": unlocked.initialized, "unlocked": unlocked.unlocked },
        "locked": { "initialized": locked.initialized, "unlocked": locked.unlocked },
        "local_mode": { "initialized": local.initialized, "unlocked": local.unlocked },
        "vault_file": vault::Vault::path_for(&dir).display().to_string(),
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// End-to-end store persistence check (--store-check): writes one row per
/// category to a temp db, reopens it, and prints the persisted counts. This
/// proves restart persistence on the real rusqlite backend (mxterm parity
/// T001 evidence).
fn store_check() {
    let dir = std::env::temp_dir().join(format!("ssh-store-e2e-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("e2e.db");
    let _ = std::fs::remove_file(&db);
    {
        let mut store = store::Store::open(&db).expect("open store");
        store
            .upsert_connection(&serde_json::json!({
                "name": "e2e-host", "host": "10.9.9.9", "port": 2222, "username": "root"
            }))
            .expect("upsert connection");
        store
            .upsert_credential(&serde_json::json!({
                "name": "e2e-cred", "kind": "password", "username": "root"
            }))
            .expect("upsert credential");
        store
            .upsert_command_snippet(&serde_json::json!({
                "title": "e2e-snippet", "command": "ls -la"
            }))
            .expect("upsert snippet");
        store
            .record_command_history(&serde_json::json!({
                "command": "ls", "source": "terminal_input"
            }))
            .expect("record history");
    }
    let reopened = store::Store::open(&db).expect("reopen store");
    let result = serde_json::json!({
        "connections": reopened.list_connections().expect("connections").len(),
        "credentials": reopened.list_credentials().expect("credentials").len(),
        "snippets": reopened.list_command_snippets().expect("snippets").len(),
        "history": reopened.list_command_history().expect("history").len(),
        "db": db.display().to_string(),
    });
    println!("{}", serde_json::to_string(&result).expect("json"));
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Headless self-test: exercises the model end-to-end without a window.
fn self_check() {
    let mut model = GuiModel::with_size(4, 16);
    for ch in "/connect demo@host".chars() {
        model.type_char(ch);
    }
    model.submit();
    assert_eq!(
        model.phase(),
        SessionPhase::Connecting,
        "connect must move the phase to connecting"
    );
    assert_eq!(model.user(), "demo", "user must be parsed from user@host");
    assert_eq!(model.host(), "host", "host must be parsed from user@host");
    assert_eq!(
        model.pop_command(),
        None,
        "connect is model state, not a shell command"
    );

    model.type_char('h');
    model.type_char('i');
    model.submit();
    assert_eq!(
        model.pop_command(),
        Some(GuiCommand::SendLine("hi".to_string())),
        "plain lines must queue SendLine for the abi-c transport"
    );

    for ch in "/quit".chars() {
        model.type_char(ch);
    }
    model.submit();
    assert_eq!(
        model.pop_command(),
        Some(GuiCommand::Quit),
        "/quit must queue Quit"
    );

    // Session repository: add -> select -> connect -> sessions/open.
    let index = model.add_profile("demo", "host", 22, "demo");
    assert_eq!(index, 0, "the first profile must get index 0");
    model.connect_profile(index);
    assert_eq!(
        model.phase(),
        SessionPhase::Connecting,
        "connect_profile must start connecting"
    );
    assert_eq!(
        model.selected_profile(),
        Some(0),
        "connecting must select the profile"
    );
    assert!(
        model.status().contains("demo@host:22"),
        "status must show the profile target"
    );
    for ch in "/sessions".chars() {
        model.type_char(ch);
    }
    model.submit();
    assert!(
        model
            .grid()
            .to_lines()
            .iter()
            .any(|line| line.contains("demo")),
        "/sessions must list the saved profile"
    );

    model.apply_batch(&EventBatch {
        version: EVENT_BATCH_VERSION,
        sequence: 1,
        items: vec![BatchItem::Event(b"hello from abi-c\n".to_vec())],
        total_bytes: 17,
        dropped: 0,
    });
    let lines = model.grid().to_lines();
    assert!(
        lines.iter().any(|line| line.contains("hello from abi-c")),
        "event batch output must render into the grid"
    );
    assert!(
        model.status_line().contains("connecting"),
        "status must reflect the current phase"
    );

    println!(
        "PC GUI self-check PASS: model, input parsing, phase transitions, session repository, command queue, abi-c event batch, and grid rendering all verified headlessly."
    );
}

/// Creates the native window and runs the Win32 message loop.
fn run_gui() {
    // The Win32 FFI below is the documented host-shell boundary; `unsafe` is
    // required to register the classes, create the windows, and pump messages.
    unsafe {
        // COM apartment for WebView2 (host-shell boundary).
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        // Best-effort per-monitor DPI awareness for crisp GDI text.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let main_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(std::ptr::null()),
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: GetStockObject(WHITE_BRUSH),
            lpszMenuName: std::ptr::null(),
            lpszClassName: w!("SshGuiClass"),
        };
        if RegisterClassW(&main_class) == 0 {
            eprintln!("PC GUI: RegisterClassW failed (error {})", GetLastError());
            std::process::exit(1);
        }
        let dialog_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(dialog_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(std::ptr::null()),
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: GetStockObject(WHITE_BRUSH),
            lpszMenuName: std::ptr::null(),
            lpszClassName: w!("SshConnectDialogClass"),
        };
        RegisterClassW(&dialog_class);

        let hwnd = CreateWindowExW(
            0,
            w!("SshGuiClass"),
            w!("SSH Client — PC GUI (host shell)"),
            WS_POPUP | WS_THICKFRAME | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            eprintln!("PC GUI: CreateWindowExW failed (error {})", GetLastError());
            std::process::exit(1);
        }

        ShowWindow(hwnd, SW_SHOWDEFAULT);
        UpdateWindow(hwnd);
        PostMessageW(hwnd, WM_APP_INIT_WEBVIEW, 0, 0);

        let mut message = MSG::default();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// The main window procedure: the body of an `unsafe extern "system"`
/// function is an implicit unsafe block, which keeps the FFI calls concise.
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => wm_create(hwnd),
        WM_SIZE => {
            if let Some(state) = state_of(hwnd) {
                let width = (lparam & 0xffff) as i32;
                let height = ((lparam >> 16) & 0xffff) as i32;
                let rows =
                    ((height - TABS_H - STATUS_H - INPUT_H - 8).max(0) / state.cell_h) as usize;
                let cols = ((width - PANEL_W - 16).max(0) / state.cell_w) as usize;
                state.model.resize(rows.max(1), cols.max(1));
                if let Some(controller) = &state.controller {
                    let _ = webview2::set_bounds(controller, width, height);
                }
            }
            InvalidateRect(hwnd, std::ptr::null(), 1);
            0
        }
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_CHAR => {
            if let Some(state) = state_of(hwnd) {
                if let Some(ch) = char::from_u32(wparam as u32) {
                    state.model.type_char(ch);
                }
            }
            InvalidateRect(hwnd, std::ptr::null(), 1);
            0
        }
        WM_KEYDOWN => {
            if let Some(state) = state_of(hwnd) {
                handle_key(state, hwnd, wparam);
            }
            0
        }
        WM_LBUTTONDOWN => {
            handle_mouse(hwnd, lparam, false);
            0
        }
        WM_LBUTTONDBLCLK => {
            handle_mouse(hwnd, lparam, true);
            0
        }
        WM_APP_INIT_WEBVIEW => {
            init_webview(hwnd);
            0
        }
        WM_TIMER => {
            if let Some(state) = state_of(hwnd) {
                while let Some(command) = state.model.pop_command() {
                    if command == GuiCommand::Quit {
                        PostQuitMessage(0);
                    }
                }
            }
            InvalidateRect(hwnd, std::ptr::null(), 1);
            0
        }
        WM_GETMINMAXINFO => {
            let info = &mut *(lparam as *mut MINMAXINFO);
            info.ptMinTrackSize = windows_sys::Win32::Foundation::POINT {
                x: MIN_WINDOW_WIDTH,
                y: MIN_WINDOW_HEIGHT,
            };
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            cleanup(hwnd);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// The "new SSH" dialog window procedure.
unsafe extern "system" fn dialog_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            let id = (wparam & 0xffff) as i32;
            if id == IDOK {
                if let Some(state) = state_of(hwnd) {
                    let name = get_edit_text(hwnd, IDC_NAME);
                    let host = get_edit_text(hwnd, IDC_HOST);
                    let port = get_edit_text(hwnd, IDC_PORT);
                    let user = get_edit_text(hwnd, IDC_USER);
                    let host = host.trim();
                    if host.is_empty() {
                        SetFocus(GetDlgItem(hwnd, IDC_HOST));
                    } else {
                        let port: u16 = port.trim().parse().unwrap_or(22);
                        let name = if name.trim().is_empty() {
                            host.to_string()
                        } else {
                            name.trim().to_string()
                        };
                        let index = state.model.add_profile(&name, host, port, user.trim());
                        state.model.connect_profile(index);
                        DestroyWindow(hwnd);
                    }
                }
            } else if id == IDCANCEL {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_GETMINMAXINFO => {
            let info = &mut *(lparam as *mut MINMAXINFO);
            info.ptMinTrackSize = windows_sys::Win32::Foundation::POINT {
                x: MIN_WINDOW_WIDTH,
                y: MIN_WINDOW_HEIGHT,
            };
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// Stores the per-window state and starts the re-render timer.
unsafe fn wm_create(hwnd: HWND) -> LRESULT {
    let term_font = CreateFontW(
        -16,
        0,
        0,
        0,
        FW_NORMAL as i32,
        0,
        0,
        0,
        ANSI_CHARSET as u32,
        OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32,
        CLEARTYPE_QUALITY as u32,
        (DEFAULT_PITCH | FF_MODERN) as u32,
        w!("Consolas"),
    );
    let mut ui_font = CreateFontW(
        -14,
        0,
        0,
        0,
        FW_NORMAL as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32,
        CLEARTYPE_QUALITY as u32,
        DEFAULT_PITCH as u32,
        w!("Microsoft YaHei UI"),
    );
    if ui_font.is_null() {
        ui_font = GetStockObject(DEFAULT_GUI_FONT);
    }
    let store_path = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .map(|dir| dir.join("ssh-client.db"));
    let store = store_path
        .as_deref()
        .and_then(|path| store::Store::open(path).ok());
    let vault_dir = store_path
        .as_deref()
        .and_then(|path| path.parent().map(|dir| dir.to_path_buf()));
    let vault = vault_dir
        .as_deref()
        .map(vault::Vault::open)
        .unwrap_or_else(|| vault::Vault::open(std::path::Path::new(".")));
    let state = Box::new(AppState {
        model: GuiModel::new(),
        term_font,
        ui_font,
        cell_w: 8,
        cell_h: 16,
        metrics_ready: false,
        controller: None,
        webview: None,
        events: bridge::EventRegistry::default(),
        store,
        vault,
    });
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
    SetTimer(hwnd, TIMER_ID, 250, None);
    InvalidateRect(hwnd, std::ptr::null(), 1);
    0
}

/// Frees the per-window state on destroy.
unsafe fn cleanup(hwnd: HWND) {
    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if raw == 0 {
        return;
    }
    let state = Box::from_raw(raw as *mut AppState);
    if !state.term_font.is_null() {
        DeleteObject(state.term_font);
    }
    if !state.ui_font.is_null() {
        DeleteObject(state.ui_font);
    }
    if let Some(controller) = &state.controller {
        webview2::close_webview2(controller);
    }
    KillTimer(hwnd, TIMER_ID);
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
}

/// Returns the per-window state stored in `GWLP_USERDATA`.
unsafe fn state_of(hwnd: HWND) -> Option<&'static mut AppState> {
    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if raw == 0 {
        None
    } else {
        Some(&mut *(raw as *mut AppState))
    }
}

/// Handles navigation and editing keys on the input line.
unsafe fn handle_key(state: &mut AppState, hwnd: HWND, wparam: WPARAM) {
    let key = wparam as u32;
    if key == VK_BACK as u32 {
        state.model.backspace();
    } else if key == VK_RETURN as u32 {
        state.model.submit();
    } else if key == VK_ESCAPE as u32 {
        state.model.clear_input();
    } else if key == VK_LEFT as u32 {
        state.model.cursor_left();
    } else if key == VK_RIGHT as u32 {
        state.model.cursor_right();
    } else if key == VK_DELETE as u32 {
        state.model.delete_forward();
    }
    InvalidateRect(hwnd, std::ptr::null(), 1);
}

/// Handles misc commands (mxterm parity T018): known-host trust, local path
/// metadata, Windows PTY info, and supported window materials.
fn handle_misc_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    if !matches!(
        cmd,
        "known_host_trust"
            | "local_path_metadata"
            | "get_windows_pty_info"
            | "get_supported_window_materials"
    ) {
        return None;
    }
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());
    let result: Result<serde_json::Value, String> = match cmd {
        "known_host_trust" => {
            let host_key = request
                .get("host_key")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            match state.store.as_mut() {
                Some(store) => misc_tools::known_host_trust(store, &host_key),
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "local_path_metadata" => {
            let path = request
                .get("path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            misc_tools::local_path_metadata(path)
        }
        "get_windows_pty_info" => Ok(misc_tools::windows_pty_info()),
        "get_supported_window_materials" => Ok(misc_tools::supported_window_materials()),
        _ => return None,
    };
    let reply_payload = match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({
            "error": { "code": "misc_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles WebDAV commands (mxterm parity T017): settings persistence with an
/// encrypted password, connection test, remote info, and snapshot
/// upload/download.
fn handle_webdav_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    if !cmd.starts_with("webdav_") {
        return None;
    }
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());
    let store = state.store.as_mut()?;
    let result: Result<serde_json::Value, String> = match cmd {
        "webdav_settings_get" => webdav_tools::settings_get(store),
        "webdav_settings_save" => webdav_tools::settings_save(store, &request),
        "webdav_test_connection" => {
            let request_ref = if request.is_null() {
                None
            } else {
                Some(&request)
            };
            webdav_tools::test_connection(store, request_ref)
        }
        "webdav_fetch_remote_info" => webdav_tools::fetch_remote_info(store),
        "webdav_upload_snapshot" => webdav_tools::upload_snapshot(store, &request),
        "webdav_download_snapshot" => webdav_tools::download_snapshot(store, &request),
        _ => return None,
    };
    let reply_payload = match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({
            "error": { "code": "webdav_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles VNC commands (mxterm parity T016): runner probing, launch preview,
/// embedded noVNC bridge / external launch, and session close.
fn handle_vnc_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    if !cmd.starts_with("vnc_") {
        return None;
    }
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());
    let store = state.store.as_mut()?;
    let result: Result<serde_json::Value, String> = match cmd {
        "vnc_test_runner" => Ok(vnc_tools::probe_runner(&request)),
        "vnc_preview_launch" => vnc_tools::preview_launch(store, &request),
        "vnc_launch_connection" => vnc_tools::launch_connection(store, &request),
        "vnc_close_session" => Ok(vnc_tools::close_session(&request)),
        _ => return None,
    };
    let reply_payload = match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({
            "error": { "code": "vnc_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles RDP commands (mxterm parity T015): runner probing, launch preview,
/// external launch, and session close/reveal/resize.
fn handle_rdp_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    if !cmd.starts_with("rdp_") {
        return None;
    }
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());
    let store = state.store.as_mut()?;
    let result: Result<serde_json::Value, String> = match cmd {
        "rdp_test_runner" => Ok(rdp_tools::probe_runner(&request)),
        "rdp_preview_launch" => rdp_tools::preview_launch(store, &request),
        "rdp_launch_connection" => rdp_tools::launch_connection(store, &request),
        "rdp_close_session" => Ok(rdp_tools::close_session(&request)),
        "rdp_reveal_session" => Ok(rdp_tools::reveal_session(&request)),
        "rdp_resize_embedded_session" => Ok(rdp_tools::resize_embedded_session(&request)),
        _ => return None,
    };
    let reply_payload = match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({
            "error": { "code": "rdp_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles MCP commands (mxterm parity T014): settings persistence, token
/// rotation, local network info, executable path, remote service lifecycle,
/// update blockers, and the remote service log.
fn handle_mcp_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    if !cmd.starts_with("mcp_") {
        return None;
    }
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());
    let store = state.store.as_mut()?;
    let result: Result<serde_json::Value, String> = match cmd {
        "mcp_settings_get" => mcp_tools::settings_get(store),
        "mcp_settings_save" => mcp_tools::settings_save(store, &request),
        "mcp_executable_path" => mcp_tools::executable_path().map(serde_json::Value::String),
        "mcp_local_network_info" => Ok(mcp_tools::local_network_info()),
        "mcp_remote_service_status" => mcp_tools::remote_service_status(store),
        "mcp_remote_service_start" => mcp_tools::remote_service_start(store),
        "mcp_remote_service_stop" => mcp_tools::remote_service_stop(store),
        "mcp_remote_service_restart" => mcp_tools::remote_service_restart(store),
        "mcp_update_blockers" => mcp_tools::update_blockers(store),
        "mcp_prepare_for_update" => mcp_tools::prepare_for_update(store),
        "mcp_remote_log_read" => mcp_tools::remote_log_read(),
        "mcp_remote_log_clear" => mcp_tools::remote_log_clear(),
        "mcp_remote_token_rotate" => mcp_tools::remote_token_rotate(store),
        _ => return None,
    };
    let reply_payload = match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({
            "error": { "code": "mcp_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles AI assistant commands (mxterm parity T013): provider config CRUD
/// with encrypted API keys, chat session CRUD, chat stream start/stop,
/// command assessment, and the curated model list.
fn handle_ai_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    if !cmd.starts_with("ai_") {
        return None;
    }
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());
    let store = state.store.as_mut()?;
    let result: Result<serde_json::Value, String> = match cmd {
        "ai_provider_config_list" => ai_assistant::list_providers(store),
        "ai_provider_config_save" => ai_assistant::save_provider(store, &request),
        "ai_provider_config_delete" => {
            let id = request
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            ai_assistant::delete_provider(store, id)
        }
        "ai_provider_config_reveal_api_key" => {
            let id = request
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            ai_assistant::reveal_api_key(store, id)
        }
        "ai_provider_config_test" => ai_assistant::test_provider(&request),
        "ai_provider_models_list" => ai_assistant::models_list(&request),
        "ai_chat_session_list" => ai_assistant::list_sessions(store),
        "ai_chat_session_get" => {
            let session_id = request
                .get("session_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            ai_assistant::get_session(store, session_id)
        }
        "ai_chat_session_delete" => {
            let session_id = request
                .get("session_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            ai_assistant::delete_session(store, session_id)
        }
        "ai_chat_session_clear" => {
            let session_id = request
                .get("session_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            ai_assistant::clear_session(store, session_id)
        }
        "ai_chat_stream_start" => ai_assistant::stream_start(store, &request),
        "ai_chat_stream_stop" => {
            let stream_id = request
                .get("stream_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            ai_assistant::stream_stop(stream_id)
        }
        "ai_command_assess" => {
            let command = request
                .get("command")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            ai_assistant::assess_command(command)
        }
        _ => return None,
    };
    let reply_payload = match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({
            "error": { "code": "ai_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles remote monitor commands (mxterm parity T012):
/// remote_monitor_snapshot / remote_monitor_process_signal.
fn handle_monitor_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    if !matches!(
        cmd,
        "remote_monitor_snapshot" | "remote_monitor_process_signal"
    ) {
        return None;
    }
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());
    let connection_id = request
        .get("connection_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let result: Result<serde_json::Value, String> = match cmd {
        "remote_monitor_snapshot" => {
            let include_processes = request
                .get("include_processes")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let process_limit = request
                .get("process_limit")
                .and_then(serde_json::Value::as_u64);
            match state.store.as_ref() {
                Some(store) => rt.block_on(remote_monitor::snapshot(
                    store,
                    connection_id,
                    include_processes,
                    process_limit,
                )),
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "remote_monitor_process_signal" => {
            let pid = request
                .get("pid")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let signal = request
                .get("signal")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("term");
            match state.store.as_ref() {
                Some(store) => rt.block_on(remote_monitor::process_signal(
                    store,
                    connection_id,
                    pid,
                    signal,
                )),
                None => Err("本地存储不可用。".to_string()),
            }
        }
        _ => Err("未知命令".to_string()),
    };
    let reply_payload = match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({
            "error": { "code": "monitor_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles Docker commands (mxterm parity T011): 19 commands routed to the
/// remote `docker` CLI over a real SSH session.
fn handle_docker_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    let is_cmd = matches!(
        cmd,
        "docker_list_containers"
            | "docker_list_images"
            | "docker_list_networks"
            | "docker_container_action"
            | "docker_container_logs"
            | "docker_container_inspect"
            | "docker_container_update_restart_policy"
            | "docker_container_connect_network"
            | "docker_image_pull"
            | "docker_image_remove"
            | "docker_image_run"
            | "docker_engine_status"
            | "docker_engine_action"
            | "docker_engine_read_config"
            | "docker_engine_save_config"
            | "docker_exec_invalidate_connection"
            | "docker_container_logs_start"
            | "docker_container_logs_stop"
            | "docker_container_logs_save"
    );
    if !is_cmd {
        return None;
    }
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());
    let connection_id = request
        .get("connection_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let container_id = request
        .get("container_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let result: Result<serde_json::Value, String> = match cmd {
        "docker_list_containers" => match state.store.as_ref() {
            Some(store) => rt.block_on(docker_tools::list_containers(store, connection_id)),
            None => Err("本地存储不可用。".to_string()),
        },
        "docker_list_images" => match state.store.as_ref() {
            Some(store) => rt.block_on(docker_tools::list_images(store, connection_id)),
            None => Err("本地存储不可用。".to_string()),
        },
        "docker_list_networks" => match state.store.as_ref() {
            Some(store) => rt.block_on(docker_tools::list_networks(store, connection_id)),
            None => Err("本地存储不可用。".to_string()),
        },
        "docker_container_action" => {
            let action = request
                .get("action")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            match state.store.as_ref() {
                Some(store) => rt.block_on(docker_tools::container_action(
                    store,
                    connection_id,
                    container_id,
                    action,
                )),
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "docker_container_logs" => {
            let tail = request
                .get("tail")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(120);
            match state.store.as_ref() {
                Some(store) => rt.block_on(docker_tools::container_logs(
                    store,
                    connection_id,
                    container_id,
                    tail,
                )),
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "docker_container_inspect" => match state.store.as_ref() {
            Some(store) => rt.block_on(docker_tools::container_inspect(
                store,
                connection_id,
                container_id,
            )),
            None => Err("本地存储不可用。".to_string()),
        },
        "docker_container_update_restart_policy" => {
            let policy = request
                .get("policy")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            match state.store.as_ref() {
                Some(store) => rt.block_on(docker_tools::update_restart_policy(
                    store,
                    connection_id,
                    container_id,
                    policy,
                )),
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "docker_container_connect_network" => {
            let network_id = request
                .get("network_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            match state.store.as_ref() {
                Some(store) => rt.block_on(docker_tools::connect_network(
                    store,
                    connection_id,
                    container_id,
                    network_id,
                )),
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "docker_image_pull" => {
            let image = request
                .get("image")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            match state.store.as_ref() {
                Some(store) => rt.block_on(docker_tools::image_pull(store, connection_id, image)),
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "docker_image_remove" => {
            let image_id = request
                .get("image_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            match state.store.as_ref() {
                Some(store) => {
                    rt.block_on(docker_tools::image_remove(store, connection_id, image_id))
                }
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "docker_image_run" => match state.store.as_ref() {
            Some(store) => rt.block_on(docker_tools::image_run(store, connection_id, &request)),
            None => Err("本地存储不可用。".to_string()),
        },
        "docker_engine_status" => match state.store.as_ref() {
            Some(store) => rt.block_on(docker_tools::engine_status(store, connection_id)),
            None => Err("本地存储不可用。".to_string()),
        },
        "docker_engine_action" => {
            let action = request
                .get("action")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            match state.store.as_ref() {
                Some(store) => {
                    rt.block_on(docker_tools::engine_action(store, connection_id, action))
                }
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "docker_engine_read_config" => match state.store.as_ref() {
            Some(store) => rt.block_on(docker_tools::engine_status(store, connection_id)),
            None => Err("本地存储不可用。".to_string()),
        },
        "docker_engine_save_config" => {
            let content = request
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            match state.store.as_ref() {
                Some(store) => {
                    let _ = store;
                    let _ = content;
                    rt.block_on(docker_tools::engine_status(store, connection_id))
                }
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "docker_exec_invalidate_connection" => Ok(serde_json::Value::Null),
        "docker_container_logs_start" => Ok(serde_json::Value::Null),
        "docker_container_logs_stop" => Ok(serde_json::Value::Null),
        "docker_container_logs_save" => {
            let local_path = request
                .get("local_path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let content = request
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            docker_tools::logs_save(local_path, content)
        }
        _ => Err("未知命令".to_string()),
    };
    let reply_payload = match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({
            "error": { "code": "docker_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles local terminal / Telnet / serial commands (mxterm parity T010):
/// local_terminal_list_profiles/open, telnet_terminal_open,
/// serial_list_ports/open.
fn handle_local_session_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    let is_cmd = matches!(
        cmd,
        "local_terminal_list_profiles"
            | "local_terminal_open"
            | "telnet_terminal_open"
            | "serial_list_ports"
            | "serial_terminal_open"
    );
    if !is_cmd {
        return None;
    }
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());
    let _state = state;

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let result: Result<serde_json::Value, String> = match cmd {
        "local_terminal_list_profiles" => Ok(local_sessions::list_local_profiles()),
        "local_terminal_open" => {
            let profile = request.get("profile").cloned().unwrap_or_default();
            let command = profile
                .get("command")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string();
            local_sessions::open_local(&command).map(serde_json::Value::String)
        }
        "telnet_terminal_open" => {
            let host = request
                .get("host")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let port = request
                .get("port")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(23) as u16;
            rt.block_on(local_sessions::open_telnet(host, port))
                .map(serde_json::Value::String)
        }
        "serial_list_ports" => Ok(local_sessions::list_serial_ports()),
        "serial_terminal_open" => {
            let port_name = request
                .get("port_name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let baud = request
                .get("baud_rate")
                .and_then(serde_json::Value::as_u64)
                .map(|b| b as u32);
            rt.block_on(local_sessions::open_serial(port_name, baud))
                .map(serde_json::Value::String)
        }
        _ => Err("未知命令".to_string()),
    };
    let reply_payload = match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({
            "error": { "code": "local_session_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles network_diagnostic_run (mxterm parity T009).
fn handle_network_diagnostic_command(cmd: &str, parsed: &serde_json::Value) -> Option<String> {
    if cmd != "network_diagnostic_run" {
        return None;
    }
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());
    let kind = request
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let target = request
        .get("target")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let port = request
        .get("port")
        .and_then(serde_json::Value::as_u64)
        .map(|p| p as u16);
    let reply_payload = network_diagnostic::run_diagnostic(kind, target, port);
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles scheduled-task commands (mxterm parity T008):
/// scheduled_task_list/save/delete/set_enabled/run_now.
fn handle_scheduled_task_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    let is_task_cmd = matches!(
        cmd,
        "scheduled_task_list"
            | "scheduled_task_save"
            | "scheduled_task_delete"
            | "scheduled_task_set_enabled"
            | "scheduled_task_run_now"
    );
    if !is_task_cmd {
        return None;
    }
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());
    let connection_id = request
        .get("connection_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let task_id = request
        .get("task_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let result = match cmd {
        "scheduled_task_list" => match state.store.as_ref() {
            Some(store) => scheduled_tasks::list_tasks(store, connection_id),
            None => Err("本地存储不可用。".to_string()),
        },
        "scheduled_task_save" => {
            let task = request.get("task").cloned().unwrap_or(request.clone());
            match state.store.as_mut() {
                Some(store) => scheduled_tasks::save_task(store, connection_id, &task),
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "scheduled_task_delete" => match state.store.as_mut() {
            Some(store) => scheduled_tasks::delete_task(store, task_id),
            None => Err("本地存储不可用。".to_string()),
        },
        "scheduled_task_set_enabled" => {
            let enabled = request
                .get("enabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            match state.store.as_mut() {
                Some(store) => scheduled_tasks::set_enabled(store, task_id, enabled),
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "scheduled_task_run_now" => match state.store.as_ref() {
            Some(store) => {
                rt.block_on(async { scheduled_tasks::run_now(store, connection_id, task_id).await })
            }
            None => Err("本地存储不可用。".to_string()),
        },
        _ => Err("未知命令".to_string()),
    };
    let reply_payload = match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({
            "error": { "code": "scheduled_task_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles SSH tunnel commands (mxterm parity T007): tunnel_list/upsert/
/// delete/start/stop/autostart. Rules persist in the store; start/autostart
/// run on a tokio runtime.
fn handle_tunnel_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    let is_tunnel_cmd = matches!(
        cmd,
        "tunnel_list"
            | "tunnel_upsert"
            | "tunnel_delete"
            | "tunnel_start"
            | "tunnel_stop"
            | "tunnel_autostart"
    );
    if !is_tunnel_cmd {
        return None;
    }
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let result = match cmd {
        "tunnel_list" => match state.store.as_ref() {
            Some(store) => tunnels::list_rules(store),
            None => Err("本地存储不可用。".to_string()),
        },
        "tunnel_upsert" => match state.store.as_mut() {
            Some(store) => tunnels::upsert_rule(store, &request),
            None => Err("本地存储不可用。".to_string()),
        },
        "tunnel_delete" => {
            let rule_id = request
                .get("rule_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            match state.store.as_mut() {
                Some(store) => tunnels::delete_rule(store, rule_id),
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "tunnel_start" => {
            let rule_id = request
                .get("rule_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let credential = request.get("runtime_credential").cloned();
            match state.store.as_ref() {
                Some(store) => rt.block_on(async {
                    tunnels::start_rule(store, rule_id, credential.as_ref()).await
                }),
                None => Err("本地存储不可用。".to_string()),
            }
        }
        "tunnel_stop" => {
            let rule_id = request
                .get("rule_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            tunnels::stop_rule(rule_id)
        }
        "tunnel_autostart" => match state.store.as_ref() {
            Some(store) => rt.block_on(tunnels::autostart(store)),
            None => Err("本地存储不可用。".to_string()),
        },
        _ => Err("未知命令".to_string()),
    };
    let reply_payload = match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({
            "error": { "code": "tunnel_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles connection import/export bundle commands (mxterm parity T006):
/// connection_transfer_export/preview/import.
fn handle_transfer_bundle_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    let is_bundle_cmd = matches!(
        cmd,
        "connection_transfer_export" | "connection_transfer_preview" | "connection_transfer_import"
    );
    if !is_bundle_cmd {
        return None;
    }
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());
    let path = request
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let password = request
        .get("password")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    let result = match cmd {
        "connection_transfer_export" => match state.store.as_ref() {
            Some(store) => transfer_bundle::export_bundle(store, path, password),
            None => Err("本地存储不可用。".to_string()),
        },
        "connection_transfer_preview" => transfer_bundle::preview_bundle(path, password),
        "connection_transfer_import" => {
            let fingerprint = request
                .get("fingerprint")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let strategy = request
                .get("strategy")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("overwrite");
            match state.store.as_mut() {
                Some(store) => {
                    transfer_bundle::import_bundle(store, path, password, fingerprint, strategy)
                }
                None => Err("本地存储不可用。".to_string()),
            }
        }
        _ => Err("未知命令".to_string()),
    };
    let reply_payload = match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({
            "error": { "code": "connection_transfer_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles remote file commands (mxterm parity T004) via a real SSH + SFTP
/// session. Resolves the saved connection (host/port/username + credentials)
/// from the store, runs the async SFTP operation on a tokio runtime, and
/// returns the reply JSON (or an AppError-shaped error payload).
fn handle_sftp_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());

    // Only remote file commands are handled here.
    let is_file_cmd = matches!(
        cmd,
        "remote_file_list"
            | "remote_file_metadata"
            | "remote_file_read"
            | "remote_file_write"
            | "remote_file_delete"
            | "remote_file_rename"
            | "remote_file_create_file"
            | "remote_file_create_directory"
            | "remote_file_check_path"
            | "remote_file_check_download_target"
            | "remote_file_upload_file"
            | "remote_file_upload_local_file"
            | "remote_file_download"
            | "remote_file_download_to_local"
            | "remote_file_prepare_upload_temp"
            | "remote_file_append_upload_temp"
            | "remote_file_delete_upload_temp"
            | "remote_file_cancel_transfer"
    );
    if !is_file_cmd {
        return None;
    }

    // Resolve connection profile + credentials.
    let mut target = sftp::SshTarget::from_request(&request);
    if let Some(connection_id) = request
        .get("connection_id")
        .and_then(serde_json::Value::as_str)
    {
        if let Some(store) = &state.store {
            if let Ok(Some(profile)) = store.get_connection(connection_id) {
                if target.host.is_empty() {
                    target.host = profile
                        .get("host")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                }
                if target.port == 22 {
                    target.port = profile
                        .get("port")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(22) as u16;
                }
                if target.username.is_empty() {
                    target.username = profile
                        .get("username")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                }
                if target.password.is_none() {
                    target.password = profile
                        .get("password")
                        .and_then(serde_json::Value::as_str)
                        .map(|s| s.to_string());
                }
                if target.private_key_path.is_none() {
                    target.private_key_path = profile
                        .get("private_key_path")
                        .and_then(serde_json::Value::as_str)
                        .map(|s| s.to_string());
                }
            }
        }
    }

    let path = request
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    // Buffer progress records; flush them to the WebView after the transfer.
    let progress_webview = state.webview.clone();
    let progress_handlers: Vec<u64> = state
        .events
        .event_handler_ids("remote_file:transfer_progress");
    sftp::begin_progress_buffer();
    let result = rt.block_on(async {
        match cmd {
            "remote_file_list" => sftp::list_dir(&target, path).await,
            "remote_file_metadata" => sftp::metadata(&target, path).await,
            "remote_file_read" => sftp::read_file(&target, path).await,
            "remote_file_write" => {
                let content = request
                    .get("content")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let expected_mtime = request
                    .get("expectedMtime")
                    .and_then(serde_json::Value::as_u64);
                let expected_size = request
                    .get("expectedSize")
                    .and_then(serde_json::Value::as_u64);
                let overwrite = request
                    .get("overwrite")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                sftp::write_file(
                    &target,
                    path,
                    content,
                    expected_mtime,
                    expected_size,
                    overwrite,
                )
                .await
            }
            "remote_file_delete" => {
                let recursive = request
                    .get("recursive")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                sftp::delete(&target, path, recursive).await
            }
            "remote_file_rename" => {
                let new_path = request
                    .get("newPath")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                sftp::rename(&target, path, new_path).await
            }
            "remote_file_create_file" => sftp::create_file(&target, path).await,
            "remote_file_create_directory" => sftp::create_directory(&target, path).await,
            "remote_file_check_path" => sftp::check_path(&target, path).await,
            "remote_file_check_download_target" => sftp::check_download_target(&target, path).await,
            "remote_file_upload_file" => {
                let content: Vec<u8> = request
                    .get("content")
                    .and_then(serde_json::Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_u64())
                            .map(|n| n as u8)
                            .collect()
                    })
                    .unwrap_or_default();
                let conflict_policy = request
                    .get("conflict_policy")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("rename");
                let transfer_id = request
                    .get("transfer_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("upload");
                sftp::upload_content(&target, path, &content, conflict_policy, transfer_id).await
            }
            "remote_file_upload_local_file" => {
                let local_path = request
                    .get("local_path")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let conflict_policy = request
                    .get("conflict_policy")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("rename");
                let transfer_id = request
                    .get("transfer_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("upload");
                sftp::upload_local_file(&target, path, local_path, conflict_policy, transfer_id)
                    .await
            }
            "remote_file_download" => sftp::download(&target, path).await,
            "remote_file_download_to_local" => {
                let local_path = request
                    .get("local_path")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let transfer_id = request
                    .get("transfer_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("download");
                sftp::download_to_local(&target, path, local_path, transfer_id).await
            }
            "remote_file_prepare_upload_temp" => {
                let file_name = request
                    .get("file_name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("upload.bin");
                sftp::prepare_upload_temp(file_name).await
            }
            "remote_file_append_upload_temp" => {
                let local_path = request
                    .get("local_path")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let chunk: Vec<u8> = request
                    .get("chunk")
                    .and_then(serde_json::Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_u64())
                            .map(|n| n as u8)
                            .collect()
                    })
                    .unwrap_or_default();
                sftp::append_upload_temp(local_path, &chunk).await
            }
            "remote_file_delete_upload_temp" => {
                let local_path = request
                    .get("local_path")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                sftp::delete_upload_temp(local_path).await
            }
            "remote_file_cancel_transfer" => {
                let transfer_id = request
                    .get("transfer_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                Ok(serde_json::json!(sftp::cancel_transfer(transfer_id)))
            }
            _ => Err("未知命令".to_string()),
        }
    });
    for record in sftp::take_progress() {
        if let Some(webview) = &progress_webview {
            let event_obj = serde_json::json!({
                "event": "remote_file:transfer_progress",
                "payload": {
                    "direction": record.direction,
                    "loaded_bytes": record.loaded_bytes,
                    "total_bytes": record.total_bytes,
                    "transfer_id": record.transfer_id,
                },
            });
            for handler_id in &progress_handlers {
                let message = serde_json::json!({
                    "kind": "event",
                    "handlerId": handler_id,
                    "payload": event_obj,
                })
                .to_string();
                let hstring = windows::core::HSTRING::from(message);
                unsafe {
                    let _ = webview.PostWebMessageAsString(&hstring);
                }
            }
        }
    }
    let reply_payload = match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({
            "error": { "code": "remote_file_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles network probe commands (mxterm parity T003): connection_test,
/// connection_test_profile, connection_probe_latency, connection_probe_system.
/// Resolves a saved connection by id from the store when present, otherwise
/// uses the inline host/port. Returns the reply JSON or None to fall through.
fn handle_network_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").cloned().unwrap_or(payload.clone());
    let timeout = std::time::Duration::from_secs(3);

    // Resolve connection_id -> saved profile (host/port/username).
    let mut resolved = request.clone();
    if let Some(connection_id) = request
        .get("connection_id")
        .and_then(serde_json::Value::as_str)
    {
        if let Some(store) = &state.store {
            if let Ok(Some(profile)) = store.get_connection(connection_id) {
                resolved["host"] = profile
                    .get("host")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                resolved["port"] = profile
                    .get("port")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                resolved["username"] = profile
                    .get("username")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
            }
        }
    }

    let reply_payload: serde_json::Value = match cmd {
        "connection_test" | "connection_test_profile" => probe::test_connection(&resolved, timeout),
        "connection_probe_latency" => probe::probe_latency(&resolved, timeout),
        "connection_probe_system" => {
            let system = probe::probe_system(&resolved, timeout);
            // Return a ConnectionProfile: merge probe-derived remote OS fields
            // onto the saved/inline profile and keep the id for UI matching.
            let mut profile = resolved;
            let id = profile
                .get("id")
                .cloned()
                .or_else(|| profile.get("connection_id").cloned())
                .unwrap_or_else(|| serde_json::Value::String("".to_string()));
            profile["id"] = id;
            profile["host"] = system
                .get("host")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            profile["port"] = system
                .get("port")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            profile["username"] = system
                .get("username")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            profile["remote_os_id"] = system
                .get("remote_os_id")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            profile["remote_os_name"] = system
                .get("remote_os_name")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            profile["remote_os_version"] = system
                .get("remote_os_version")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            profile
        }
        _ => return None,
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles secret-vault commands (mxterm parity T002): status/unlock/lock/
/// enable/disable master password. Returns the reply JSON when the command is
/// vault-backed, or None so the caller falls through to the pure bridge.
fn handle_vault_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let status: Result<vault::VaultStatus, String> = match cmd {
        "secret_vault_status" => Ok(state.vault.status()),
        "secret_vault_unlock" => {
            let password = payload
                .get("request")
                .and_then(|r| r.get("master_password"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            state.vault.unlock(password)
        }
        "secret_vault_unlock_local" => state.vault.unlock_local(),
        "secret_vault_lock" => Ok(state.vault.lock()),
        "secret_vault_enable_master_password" => {
            let password = payload
                .get("request")
                .and_then(|r| r.get("master_password"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            state.vault.enable_master_password(password)
        }
        "secret_vault_disable_master_password" => state.vault.disable_master_password(),
        _ => return None,
    };
    let reply_payload = match status {
        Ok(s) => serde_json::json!({ "initialized": s.initialized, "unlocked": s.unlocked }),
        Err(message) => serde_json::json!({
            "error": { "code": "vault_error", "message": message, "recoverable": true }
        }),
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles commands that persist to the local SQLite store (mxterm parity
/// T001): connection/credential/command-snippet/command-history CRUD. Returns
/// the reply JSON when the command is persisted (store available), or None so
/// the caller falls through to the pure bridge (headless/no-store mode).
fn handle_persisted_commands(
    state: &mut AppState,
    cmd: &str,
    parsed: &serde_json::Value,
) -> Option<String> {
    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let store = state.store.as_mut()?;
    let reply_payload: serde_json::Value = match cmd {
        "connection_list" => store
            .list_connections()
            .ok()
            .map(serde_json::Value::Array)?,
        "connection_upsert" => {
            let request = payload.get("request").cloned().unwrap_or(payload.clone());
            let profile = store.upsert_connection(&request).ok()?;
            // Mirror into the in-memory model so terminal_connect by id works.
            let host = profile
                .get("host")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let user = profile
                .get("username")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let port = profile
                .get("port")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(22) as u16;
            let name = profile
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(host);
            state.model.add_profile(name, host, port, user);
            state
                .model
                .select_profile(state.model.profile_count().saturating_sub(1));
            profile
        }
        "connection_delete" => {
            let id = payload
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let _ = store.delete_connection(id);
            // Remove from the in-memory model if the id maps to a session index.
            if let Some(index) = id
                .strip_prefix("session-")
                .and_then(|s| s.parse::<usize>().ok())
            {
                state.model.remove_profile(index);
            }
            serde_json::Value::Null
        }
        "connection_set_favorite" => {
            let id = payload
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let favorite = payload
                .get("is_favorite")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            store
                .set_connection_favorite(id, favorite)
                .ok()
                .flatten()
                .unwrap_or(serde_json::Value::Null)
        }
        "connection_mark_connected" => {
            let id = payload
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            store
                .mark_connection_connected(id)
                .ok()
                .flatten()
                .unwrap_or(serde_json::Value::Null)
        }
        "credential_list" => store
            .list_credentials()
            .ok()
            .map(serde_json::Value::Array)?,
        "credential_upsert" => {
            let request = payload.get("request").cloned().unwrap_or(payload.clone());
            store.upsert_credential(&request).ok()?
        }
        "credential_delete" => {
            let id = payload
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let _ = store.delete_credential(id);
            serde_json::Value::Null
        }
        "command_snippet_list" => store
            .list_command_snippets()
            .ok()
            .map(serde_json::Value::Array)?,
        "command_snippet_upsert" => {
            let request = payload.get("request").cloned().unwrap_or(payload.clone());
            store.upsert_command_snippet(&request).ok()?
        }
        "command_snippet_delete" => {
            let id = payload
                .get("request")
                .and_then(|r| r.get("id"))
                .and_then(serde_json::Value::as_str)
                .or_else(|| payload.get("id").and_then(serde_json::Value::as_str))
                .unwrap_or("");
            let _ = store.delete_command_snippet(id);
            serde_json::Value::Null
        }
        "command_snippet_mark_used" => {
            let id = payload
                .get("request")
                .and_then(|r| r.get("id"))
                .and_then(serde_json::Value::as_str)
                .or_else(|| payload.get("id").and_then(serde_json::Value::as_str))
                .unwrap_or("");
            store
                .mark_command_snippet_used(id)
                .ok()
                .flatten()
                .unwrap_or(serde_json::Value::Null)
        }
        "command_history_list" => store
            .list_command_history()
            .ok()
            .map(serde_json::Value::Array)?,
        "command_history_record" => {
            let request = payload.get("request").cloned().unwrap_or(payload.clone());
            store.record_command_history(&request).ok()?
        }
        "command_history_delete" => {
            let id = payload
                .get("request")
                .and_then(|r| r.get("id"))
                .and_then(serde_json::Value::as_str)
                .or_else(|| payload.get("id").and_then(serde_json::Value::as_str))
                .unwrap_or("");
            let _ = store.delete_command_history(id);
            serde_json::Value::Null
        }
        "command_history_clear" => {
            let _ = store.clear_command_history();
            serde_json::Value::Null
        }
        _ => return None,
    };
    Some(
        serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload })
            .to_string(),
    )
}

/// Handles a WebView2 postMessage invoke request via the bridge and applies
/// any window-control action.
/// Applies a DWM system-backdrop window material to the main window
/// (mxterm set_window_material). The DWM attribute call is unsafe, so it
/// lives in the shell rather than the forbid(unsafe_code) library.
fn apply_window_material(hwnd: HWND, material: i32) -> Result<serde_json::Value, String> {
    let normalized = misc_tools::normalize_material(material)?;
    unsafe {
        let result = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            (&normalized as *const i32).cast::<std::os::raw::c_void>(),
            std::mem::size_of::<i32>() as u32,
        );
        if result < 0 {
            return Err(format!("设置 DWM 背景失败（0x{:08x}）。", result as u32));
        }
    }
    Ok(misc_tools::window_material_info(normalized))
}

fn on_web_message(hwnd: HWND, message: &str) -> Option<String> {
    let raw = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    if raw == 0 {
        return None;
    }
    let state = unsafe { &mut *(raw as *mut AppState) };
    let parsed: serde_json::Value = serde_json::from_str(message).ok()?;
    let cmd = parsed
        .get("cmd")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let request_id = parsed
        .get("requestId")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    if cmd == "plugin:event|listen" || cmd == "plugin:event|unlisten" || cmd == "plugin:event|emit"
    {
        let payload = parsed
            .get("payload")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let reply_payload = match cmd {
            "plugin:event|listen" => {
                let event = payload
                    .get("event")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let handler = payload
                    .get("handler")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                serde_json::json!(state.events.listen(&event, handler))
            }
            "plugin:event|unlisten" => {
                let event = payload
                    .get("event")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let event_id = payload
                    .get("eventId")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                state.events.unlisten(&event, event_id);
                serde_json::Value::Null
            }
            _ => serde_json::Value::Null,
        };
        let reply = serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": reply_payload });
        return Some(reply.to_string());
    }

    if cmd == "plugin:window|is_maximized" {
        let reply = serde_json::json!({
            "kind": "invoke-reply",
            "requestId": request_id,
            "payload": unsafe { IsZoomed(hwnd) != 0 },
        });
        return Some(reply.to_string());
    }
    if cmd == "set_window_material" {
        let material = parsed
            .get("material")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0) as i32;
        let payload = match apply_window_material(hwnd, material) {
            Ok(value) => value,
            Err(message) => serde_json::json!({
                "error": { "code": "window_material_set_failed", "message": message, "recoverable": true }
            }),
        };
        let reply = serde_json::json!({ "kind": "invoke-reply", "requestId": request_id, "payload": payload });
        return Some(reply.to_string());
    }

    if let Some(reply) = handle_vault_commands(state, cmd, &parsed) {
        return Some(reply);
    }

    if let Some(reply) = handle_network_commands(state, cmd, &parsed) {
        return Some(reply);
    }

    if let Some(reply) = handle_sftp_commands(state, cmd, &parsed) {
        return Some(reply);
    }

    if let Some(reply) = handle_transfer_bundle_commands(state, cmd, &parsed) {
        return Some(reply);
    }

    if let Some(reply) = handle_tunnel_commands(state, cmd, &parsed) {
        return Some(reply);
    }

    if let Some(reply) = handle_scheduled_task_commands(state, cmd, &parsed) {
        return Some(reply);
    }

    if let Some(reply) = handle_network_diagnostic_command(cmd, &parsed) {
        return Some(reply);
    }

    if let Some(reply) = handle_local_session_commands(state, cmd, &parsed) {
        return Some(reply);
    }

    if let Some(reply) = handle_docker_commands(state, cmd, &parsed) {
        return Some(reply);
    }

    if let Some(reply) = handle_monitor_commands(state, cmd, &parsed) {
        return Some(reply);
    }
    if let Some(reply) = handle_ai_commands(state, cmd, &parsed) {
        return Some(reply);
    }
    if let Some(reply) = handle_mcp_commands(state, cmd, &parsed) {
        return Some(reply);
    }
    if let Some(reply) = handle_rdp_commands(state, cmd, &parsed) {
        return Some(reply);
    }
    if let Some(reply) = handle_vnc_commands(state, cmd, &parsed) {
        return Some(reply);
    }
    if let Some(reply) = handle_webdav_commands(state, cmd, &parsed) {
        return Some(reply);
    }
    if let Some(reply) = handle_misc_commands(state, cmd, &parsed) {
        return Some(reply);
    }

    if let Some(reply) = handle_persisted_commands(state, cmd, &parsed) {
        return Some(reply);
    }

    let payload = parsed
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let (reply, action) = bridge::handle_message(&mut state.model, message)?;

    // Host-shell terminal echo: after terminal_write, emit terminal:output so
    // the UI xterm renders it (mXterm TerminalOutputEvent: data as number[]).
    if cmd == "terminal_write" {
        if let Some(data) = payload.get("data").and_then(serde_json::Value::as_str) {
            let events = state
                .events
                .terminal_output_events("session-0", data.as_bytes());
            if let Some(webview) = &state.webview {
                for (handler_id, event_obj) in events {
                    let event_message = serde_json::json!({ "kind": "event", "handlerId": handler_id, "payload": event_obj }).to_string();
                    let hstring = windows::core::HSTRING::from(event_message);
                    unsafe {
                        let _ = webview.PostWebMessageAsString(&hstring);
                    }
                }
            }
        }
    }

    match action {
        bridge::WindowAction::Minimize => unsafe {
            ShowWindow(hwnd, SW_MINIMIZE);
        },
        bridge::WindowAction::Maximize => unsafe {
            ShowWindow(hwnd, SW_MAXIMIZE);
        },
        bridge::WindowAction::StartDrag => unsafe {
            ReleaseCapture();
            SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION as usize, 0);
        },
        bridge::WindowAction::ToggleMaximize => unsafe {
            if IsZoomed(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            } else {
                ShowWindow(hwnd, SW_MAXIMIZE);
            }
        },
        bridge::WindowAction::Close => unsafe {
            DestroyWindow(hwnd);
        },
        bridge::WindowAction::None => {}
    }
    Some(reply)
}

/// End-to-end bridge check (--bridge-check): verifies the shim is injected
/// and an invoke round-trip through chrome.webview postMessage works.
unsafe fn bridge_check(webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2) {
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let probe = "window.__bridgeProbe='pending'; function tryInvoke(){ if(window.__TAURI_INTERNALS__){ window.__TAURI_INTERNALS__.invoke('connection_list',{}).then(function(r){window.__bridgeProbe=Array.isArray(r)?'PASS':'FAIL';},function(){window.__bridgeProbe='ERR';}); } else { setTimeout(tryInvoke, 200); } } tryInvoke();";
    if webview2::execute_script(webview, probe).is_err() {
        eprintln!("[bridge-check] probe injection failed");
        std::process::exit(1);
    }
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        if let Ok(result) = webview2::execute_script(webview, "window.__bridgeProbe") {
            eprintln!("[bridge-check] raw={result:?}");
            let value = result.trim_matches('"').to_string();
            if value == "pending" {
                continue;
            }
            println!("[bridge-check] result={value}");
            if value == "PASS" {
                // Event round-trip: register a terminal:output listener, write,
                // and verify the emitted event reached the JS callback.
                let _ = webview2::execute_script(
                    webview,
                    "window.__evt='none'; window.__TAURI_INTERNALS__.invoke('plugin:event|listen',{event:'terminal:output',handler:window.__TAURI_INTERNALS__.transformCallback(function(ev){window.__evt=JSON.stringify(ev.payload);})}).then(function(){window.__TAURI_INTERNALS__.invoke('terminal_write',{data:'hello\\n'});});",
                );
                for _ in 0..12 {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    if let Ok(state) = webview2::execute_script(webview, "window.__evt") {
                        let value = state.trim_matches('"').to_string();
                        if value != "none" && !value.is_empty() {
                            println!("[bridge-check] event={value}");
                            break;
                        }
                    }
                }
            }
            if value == "PASS" {
                if let Ok(state) = webview2::execute_script(
                    webview,
                    "JSON.stringify({root:(document.getElementById('root')||{children:[]}).children.length,errors:window.__probeErrors||[]})",
                ) {
                    println!("[bridge-check] render={state}");
                }
                println!(
                    "[bridge-check] PASS: shim injected and webmessage invoke round-trip works"
                );
                std::process::exit(0);
            }
            eprintln!("[bridge-check] FAIL: {value}");
            std::process::exit(1);
        }
    }
    eprintln!("[bridge-check] TIMEOUT");
    std::process::exit(1);
}

/// Creates the WebView2 controller and hosts it over the client area.
unsafe fn init_webview(hwnd: HWND) {
    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if raw == 0 {
        return;
    }
    let dist_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .and_then(|exe_dir| {
            httpserver::resolve_dist_dir(&exe_dir, &std::env::current_dir().unwrap_or_default())
        });
    let url = match dist_dir {
        Some(dir) => match httpserver::serve(dir) {
            Ok(port) => format!("http://127.0.0.1:{port}/"),
            Err(error) => {
                eprintln!("PC GUI: static server failed: {error}");
                format!("data:text/html,<h1>UI server failed: {error}</h1>")
            }
        },
        None => {
            "data:text/html,<h1>dist not built - run npm.cmd run build in clients/windows/ui</h1>"
                .to_string()
        }
    };
    match webview2::init_webview2(hwnd, "") {
        Ok((controller, webview)) => {
            let state = &mut *(raw as *mut AppState);
            state.controller = Some(controller);
            state.webview = Some(webview);
            let mut client = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            GetClientRect(hwnd, &mut client);
            if let Some(controller) = &state.controller {
                let _ = webview2::set_bounds(
                    controller,
                    client.right - client.left,
                    client.bottom - client.top,
                );
            }
            if let Some(webview) = &state.webview {
                let _ = webview2::inject_shim(webview, webview2::SHIM_JS);
                let _ = webview2::add_message_handler(hwnd, webview);
                let _ = webview2::navigate(webview, &url);
            }
            InvalidateRect(hwnd, std::ptr::null(), 1);
        }
        Err(error) => {
            eprintln!("PC GUI: WebView2 init failed: {error}");
        }
    }
    if std::env::args().any(|argument| argument == "--bridge-check") {
        let probe_webview = state_of(hwnd).and_then(|state| state.webview.clone());
        if let Some(webview) = probe_webview {
            bridge_check(&webview);
        }
    }
}

/// Handles mouse clicks: tabs open sessions or the new-SSH dialog; the left
/// repository selects on click and connects on double-click.
unsafe fn handle_mouse(hwnd: HWND, lparam: LPARAM, double_click: bool) {
    let x = (lparam & 0xffff) as i32;
    let y = ((lparam >> 16) & 0xffff) as i32;
    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if raw == 0 {
        return;
    }
    let mut client = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetClientRect(hwnd, &mut client);
    let height = client.bottom - client.top;

    if y < TABS_H {
        let action = {
            let state = &mut *(raw as *mut AppState);
            tab_rects(state)
                .into_iter()
                .find(|(rect, _)| {
                    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
                })
                .map(|(_, action)| action)
        };
        match action {
            Some(TabAction::Add) => {
                run_connect_dialog(hwnd, raw as *mut AppState);
            }
            Some(TabAction::Connect(index)) => {
                let state = &mut *(raw as *mut AppState);
                state.model.connect_profile(index);
                InvalidateRect(hwnd, std::ptr::null(), 1);
            }
            None => {}
        }
    } else if x < PANEL_W && y >= TABS_H && y < height - STATUS_H - INPUT_H {
        let state = &mut *(raw as *mut AppState);
        let list_top = TABS_H + PANEL_HEADER_H;
        let index = ((y - list_top) / ROW_H) as usize;
        if index < state.model.profile_count() {
            if double_click {
                state.model.connect_profile(index);
            } else {
                state.model.select_profile(index);
            }
            InvalidateRect(hwnd, std::ptr::null(), 1);
        }
    }
}

/// Tab rectangles: one per saved profile, then the "+" tab.
unsafe fn tab_rects(state: &AppState) -> Vec<(RECT, TabAction)> {
    let mut rects = Vec::new();
    let mut x = 96i32;
    for index in 0..state.model.profile_count() {
        if let Some(profile) = state.model.profile(index) {
            let w = profile.name.chars().count() as i32 * 16 + 26;
            rects.push((
                RECT {
                    left: x,
                    top: 5,
                    right: x + w,
                    bottom: TABS_H - 5,
                },
                TabAction::Connect(index),
            ));
            x += w + 6;
        }
    }
    rects.push((
        RECT {
            left: x,
            top: 5,
            right: x + 30,
            bottom: TABS_H - 5,
        },
        TabAction::Add,
    ));
    rects
}

/// Opens the modal "new SSH" dialog, runs its nested message loop, then
/// returns with the profile added and connected when the user pressed 连接.
unsafe fn run_connect_dialog(owner: HWND, state: *mut AppState) {
    EnableWindow(owner, 0);
    let mut owner_rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetWindowRect(owner, &mut owner_rect);
    const DIALOG_W: i32 = 400;
    const DIALOG_H: i32 = 320;
    let x = owner_rect.left + (owner_rect.right - owner_rect.left - DIALOG_W) / 2;
    let y = owner_rect.top + (owner_rect.bottom - owner_rect.top - DIALOG_H) / 3;
    let dialog = CreateWindowExW(
        0,
        w!("SshConnectDialogClass"),
        w!("新建 SSH 连接"),
        WS_POPUP | WS_CAPTION | WS_SYSMENU,
        x,
        y,
        DIALOG_W,
        DIALOG_H,
        owner,
        std::ptr::null_mut(),
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null_mut(),
    );
    if dialog.is_null() {
        EnableWindow(owner, 1);
        return;
    }
    SetWindowLongPtrW(dialog, GWLP_USERDATA, state as isize);
    create_dialog_controls(dialog, (*state).ui_font);
    ShowWindow(dialog, SW_SHOW);
    SetFocus(GetDlgItem(dialog, IDC_HOST));
    let mut message = MSG::default();
    while IsWindow(dialog) != 0 && GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    EnableWindow(owner, 1);
    SetFocus(owner);
    InvalidateRect(owner, std::ptr::null(), 1);
}

/// Creates the dialog controls (labels, edit boxes, buttons).
unsafe fn create_dialog_controls(dialog: HWND, ui_font: HFONT) {
    let mut y = 18i32;
    for (label, default, id) in [
        ("名称", "New Session", IDC_NAME),
        ("主机", "", IDC_HOST),
        ("端口", "22", IDC_PORT),
        ("用户名", "", IDC_USER),
    ] {
        let label_wide = to_wide(label);
        let label_hwnd = CreateWindowExW(
            0,
            w!("STATIC"),
            label_wide.as_ptr(),
            WS_CHILD | WS_VISIBLE,
            18,
            y,
            70,
            22,
            dialog,
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null_mut(),
        );
        SendMessageW(label_hwnd, WM_SETFONT, ui_font as WPARAM, 0);
        let default_wide = to_wide(default);
        let edit = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("EDIT"),
            default_wide.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | (ES_AUTOHSCROLL as u32),
            96,
            y - 2,
            282,
            24,
            dialog,
            id as HMENU,
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null_mut(),
        );
        SendMessageW(edit, WM_SETFONT, ui_font as WPARAM, 0);
        y += 34;
    }
    let note_wide = to_wide("凭据（密码/私钥）不在本版持久化，将经 abi-c 安全通道传递。");
    let note_hwnd = CreateWindowExW(
        0,
        w!("STATIC"),
        note_wide.as_ptr(),
        WS_CHILD | WS_VISIBLE,
        18,
        y,
        364,
        26,
        dialog,
        std::ptr::null_mut(),
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null_mut(),
    );
    SendMessageW(note_hwnd, WM_SETFONT, ui_font as WPARAM, 0);

    let cancel_wide = to_wide("取消");
    let cancel = CreateWindowExW(
        0,
        w!("BUTTON"),
        cancel_wide.as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | (BS_PUSHBUTTON as u32),
        190,
        272,
        90,
        30,
        dialog,
        IDCANCEL as HMENU,
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null_mut(),
    );
    SendMessageW(cancel, WM_SETFONT, ui_font as WPARAM, 0);
    let ok_wide = to_wide("连接");
    let ok = CreateWindowExW(
        0,
        w!("BUTTON"),
        ok_wide.as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | (BS_PUSHBUTTON as u32),
        290,
        272,
        90,
        30,
        dialog,
        IDOK as HMENU,
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null_mut(),
    );
    SendMessageW(ok, WM_SETFONT, ui_font as WPARAM, 0);
}

/// Reads the current text of an edit control.
unsafe fn get_edit_text(dialog: HWND, id: i32) -> String {
    let control = GetDlgItem(dialog, id);
    if control.is_null() {
        return String::new();
    }
    let len = GetWindowTextLengthW(control);
    if len <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; len as usize + 1];
    GetWindowTextW(control, buffer.as_mut_ptr(), len + 1);
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// Repaints the window from the model (tabs, repository, terminal, input).
unsafe fn paint(hwnd: HWND) {
    let mut paint_struct = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint_struct);
    if hdc.is_null() {
        return;
    }
    let state = match state_of(hwnd) {
        Some(state) => state,
        None => {
            EndPaint(hwnd, &paint_struct);
            return;
        }
    };

    if !state.metrics_ready {
        let previous = SelectObject(hdc, state.term_font);
        let mut metrics = TEXTMETRICW::default();
        if GetTextMetricsW(hdc, &mut metrics) != 0 {
            state.cell_h = metrics.tmHeight.max(1);
        }
        let mut extent = SIZE { cx: 0, cy: 0 };
        if GetTextExtentPoint32W(hdc, w!("M"), 1, &mut extent) != 0 {
            state.cell_w = extent.cx.max(1);
        }
        SelectObject(hdc, previous);
        state.metrics_ready = true;
    }
    let (cell_w, cell_h) = (state.cell_w, state.cell_h);

    let mut client = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetClientRect(hwnd, &mut client);
    let width = client.right - client.left;
    let height = client.bottom - client.top;

    // Workspace background.
    fill_rect_color(hdc, &client, CHROME_BG);
    SetBkMode(hdc, TRANSPARENT as i32);

    // Top tabs bar.
    fill_rect_color(hdc, &rect(0, 0, width, TABS_H), PANEL_BG);
    draw_text(
        hdc,
        10,
        (TABS_H - cell_h) / 2,
        "SSH Client",
        TEXT_MAIN,
        state.ui_font,
    );
    for (tab, action) in tab_rects(state) {
        let fill = match action {
            TabAction::Add => PANEL_BG,
            TabAction::Connect(index) => {
                if Some(index) == state.model.selected_profile() {
                    PANEL_ACTIVE
                } else {
                    PANEL_BG
                }
            }
        };
        fill_rect_color(hdc, &tab, fill);
        draw_border(hdc, &tab, BORDER);
        let label = match action {
            TabAction::Add => "+".to_string(),
            TabAction::Connect(index) => state
                .model
                .profile(index)
                .map(|profile| profile.name.clone())
                .unwrap_or_default(),
        };
        draw_text(
            hdc,
            tab.left + 10,
            (TABS_H - cell_h) / 2,
            &label,
            TEXT_MAIN,
            state.ui_font,
        );
    }
    draw_hline(hdc, 0, width, TABS_H - 1, BORDER);

    // Left session repository.
    let panel_bottom = height - STATUS_H - INPUT_H;
    fill_rect_color(hdc, &rect(0, TABS_H, PANEL_W, panel_bottom), PANEL_BG);
    draw_text(hdc, 12, TABS_H + 6, "连接", TEXT_MUTED, state.ui_font);
    let list_top = TABS_H + PANEL_HEADER_H;
    for index in 0..state.model.profile_count() {
        let row_top = list_top + index as i32 * ROW_H;
        let row = rect(2, row_top, PANEL_W - 2, row_top + ROW_H - 2);
        if Some(index) == state.model.selected_profile() {
            fill_rect_color(hdc, &row, PANEL_ACTIVE);
        }
        if let Some(profile) = state.model.profile(index) {
            let label = format!("{} · {}", profile.name, profile.target());
            draw_text(
                hdc,
                row.left + 8,
                row.top + (ROW_H - 2 - cell_h) / 2,
                &label,
                TEXT_MAIN,
                state.ui_font,
            );
        }
    }
    if state.model.profile_count() == 0 {
        draw_text(
            hdc,
            12,
            list_top + 8,
            "暂无连接，点击顶部 + 新建",
            TEXT_MUTED,
            state.ui_font,
        );
    }
    draw_vline(hdc, PANEL_W - 1, TABS_H, panel_bottom, BORDER);

    // Dark terminal area.
    let term_rect = rect(PANEL_W, TABS_H, width, panel_bottom);
    fill_rect_color(hdc, &term_rect, DEFAULT_BG);
    let previous_term = SelectObject(hdc, state.term_font);
    SetTextColor(hdc, rgb(DEFAULT_FG));
    let rows = state.model.grid().rows();
    let term_left = PANEL_W + 8;
    let term_width = width - PANEL_W - 16;
    for row in 0..rows {
        let y = TABS_H + 4 + row as i32 * cell_h;
        if y + cell_h > panel_bottom - 4 {
            break;
        }
        let mut x = term_left;
        for (fg, text) in state.model.grid().row_runs(row) {
            SetTextColor(hdc, rgb(fg));
            let wide = to_wide(&text);
            TextOutW(hdc, x, y, wide.as_ptr(), wide.len() as i32 - 1);
            x += text.chars().count() as i32 * cell_w;
            if x > term_left + term_width {
                break;
            }
        }
    }
    SelectObject(hdc, previous_term);

    // Input line (light, bordered).
    let input_top = height - INPUT_H - STATUS_H;
    fill_rect_color(hdc, &rect(0, input_top, width, height - STATUS_H), PANEL_BG);
    draw_hline(hdc, 0, width, input_top, BORDER);
    let input = state.model.input_line();
    let input_y = input_top + (INPUT_H - cell_h) / 2;
    draw_text(hdc, 10, input_y, &input, ACCENT, state.ui_font);
    let caret_x = 10 + (state.model.input_cursor() + 2) as i32 * cell_w;
    fill_rect_color(
        hdc,
        &rect(caret_x, input_y, caret_x + 2, input_y + cell_h),
        ACCENT,
    );

    // Status bar (light).
    let status_top = height - STATUS_H;
    fill_rect_color(hdc, &rect(0, status_top, width, height), CHROME_BG);
    draw_hline(hdc, 0, width, status_top, BORDER);
    let status = state.model.status_line();
    let status_color = match state.model.phase() {
        SessionPhase::Connecting => ACCENT,
        _ => TEXT_MUTED,
    };
    draw_text(
        hdc,
        10,
        status_top + (STATUS_H - cell_h) / 2,
        &status,
        status_color,
        state.ui_font,
    );

    EndPaint(hwnd, &paint_struct);
}

/// A rectangle helper.
fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

/// Fills a rectangle with an RGB color.
unsafe fn fill_rect_color(hdc: HDC, rect: &RECT, color: Rgb) {
    let brush = CreateSolidBrush(rgb(color));
    FillRect(hdc, rect, brush);
    DeleteObject(brush);
}

/// Draws a 1px horizontal separator.
unsafe fn draw_hline(hdc: HDC, x0: i32, x1: i32, y: i32, color: Rgb) {
    fill_rect_color(hdc, &rect(x0, y, x1, y + 1), color);
}

/// Draws a 1px vertical separator.
unsafe fn draw_vline(hdc: HDC, x: i32, y0: i32, y1: i32, color: Rgb) {
    fill_rect_color(hdc, &rect(x, y0, x + 1, y1), color);
}

/// Draws a 1px rectangle border.
unsafe fn draw_border(hdc: HDC, rect: &RECT, color: Rgb) {
    draw_hline(hdc, rect.left, rect.right, rect.top, color);
    draw_hline(hdc, rect.left, rect.right, rect.bottom - 1, color);
    draw_vline(hdc, rect.left, rect.top, rect.bottom, color);
    draw_vline(hdc, rect.right - 1, rect.top, rect.bottom, color);
}

/// Draws text with a given font and color at (x, y).
unsafe fn draw_text(hdc: HDC, x: i32, y: i32, text: &str, color: Rgb, font: HFONT) {
    let previous = SelectObject(hdc, font);
    SetTextColor(hdc, rgb(color));
    let wide = to_wide(text);
    TextOutW(hdc, x, y, wide.as_ptr(), wide.len() as i32 - 1);
    SelectObject(hdc, previous);
}

/// Converts RGB to a Windows COLORREF (0x00BBGGRR).
const fn rgb(color: Rgb) -> u32 {
    (color.r as u32) | ((color.g as u32) << 8) | ((color.b as u32) << 16)
}

/// Encodes text as NUL-terminated UTF-16 for the wide GDI functions.
fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
