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
use clients_windows::model::{
    GuiCommand, GuiModel, Rgb, SessionPhase, ACCENT, BORDER, CHROME_BG, DEFAULT_BG, DEFAULT_FG,
    PANEL_ACTIVE, PANEL_BG, TEXT_MAIN, TEXT_MUTED,
};
use windows_sys::core::w;
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
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
    WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_NCLBUTTONDOWN, WM_PAINT, WM_SETFONT,
    WM_SIZE, WM_TIMER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE,
    WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_THICKFRAME, WS_VISIBLE,
};

use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2, ICoreWebView2Controller};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

mod bridge;
mod httpserver;
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
}

fn main() {
    if std::env::args().any(|argument| argument == "--check") {
        self_check();
        return;
    }
    run_gui();
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

/// Handles a WebView2 postMessage invoke request via the bridge and applies
/// any window-control action.
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
