//! Native Windows GUI shell for the PC client (clients/windows).
//!
//! This binary is the "host-shell boundary" named by `contract.json`: the
//! safe, headless-testable UI model lives in `clients_windows::model`, and
//! this file only wires that model to the Win32 message loop (GDI rendering
//! and keyboard input). The Win32 FFI is inherently `unsafe`; every call is
//! confined to small, documented helper functions so the unsafe surface
//! stays reviewable and the architecture boundary is explicit.
//!
//! Usage:
//!   cargo run -p clients-windows             # open the native GUI window
//!   cargo run -p clients-windows -- --check  # headless self-test (CI-safe)

use abi_c::{BatchItem, EventBatch, EVENT_BATCH_VERSION};
use clients_windows::model::{
    GuiCommand, GuiModel, SessionPhase, ACCENT_FG, DEFAULT_BG, DEFAULT_FG, INPUT_BG,
};
use windows_sys::core::w;
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetStockObject,
    GetTextExtentPoint32W, GetTextMetricsW, InvalidateRect, SelectObject, SetBkMode, SetTextColor,
    TextOutW, UpdateWindow, ANSI_CHARSET, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_PITCH,
    FF_MODERN, FW_NORMAL, HFONT, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TEXTMETRICW, TRANSPARENT,
    WHITE_BRUSH,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_BACK, VK_DELETE, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowLongPtrW, KillTimer, LoadCursorW, PostQuitMessage, RegisterClassW, SetTimer,
    SetWindowLongPtrW, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
    GWLP_USERDATA, IDC_ARROW, MSG, SW_SHOWDEFAULT, WM_CHAR, WM_CLOSE, WM_CREATE, WM_DESTROY,
    WM_KEYDOWN, WM_PAINT, WM_SIZE, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

/// Timer id used for the periodic re-render / command drain.
const TIMER_ID: usize = 1;
/// Default window width in pixels.
const WINDOW_WIDTH: i32 = 960;
/// Default window height in pixels.
const WINDOW_HEIGHT: i32 = 640;

/// Per-window state: the model plus cached GDI resources and cell metrics.
struct AppState {
    model: GuiModel,
    font: HFONT,
    cell_w: i32,
    cell_h: i32,
    status_h: i32,
    input_h: i32,
    metrics_ready: bool,
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
        "PC GUI self-check PASS: model, input parsing, phase transitions, command queue, abi-c event batch, and grid rendering all verified headlessly."
    );
}

/// Creates the native window and runs the Win32 message loop.
fn run_gui() {
    // The Win32 FFI below is the documented host-shell boundary; `unsafe` is
    // required to register the class, create the window, and pump messages.
    unsafe {
        // Best-effort per-monitor DPI awareness for crisp GDI text.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let class = WNDCLASSW {
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
        if RegisterClassW(&class) == 0 {
            eprintln!("PC GUI: RegisterClassW failed (error {})", GetLastError());
            std::process::exit(1);
        }

        let hwnd = CreateWindowExW(
            0,
            w!("SshGuiClass"),
            w!("SSH Client — PC GUI (host shell)"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
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

        let mut message = MSG::default();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// The window procedure: the body of an `unsafe extern "system"` function is
/// an implicit unsafe block, which keeps the FFI calls concise here.
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
                    ((height - state.status_h - state.input_h).max(0) / state.cell_h) as usize;
                let cols = (width.max(0) / state.cell_w) as usize;
                state.model.resize(rows.max(1), cols.max(1));
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

/// Stores the per-window state and starts the re-render timer.
unsafe fn wm_create(hwnd: HWND) -> LRESULT {
    let font = CreateFontW(
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
    let state = Box::new(AppState {
        model: GuiModel::new(),
        font,
        cell_w: 8,
        cell_h: 16,
        status_h: 22,
        input_h: 24,
        metrics_ready: false,
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
    if !state.font.is_null() {
        DeleteObject(state.font);
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

/// Repaints the window from the model (status bar, terminal grid, input line).
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
        let previous = SelectObject(hdc, state.font);
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
    let (cell_w, cell_h, status_h, input_h) =
        (state.cell_w, state.cell_h, state.status_h, state.input_h);
    let model = &state.model;

    let mut client = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetClientRect(hwnd, &mut client);
    let width = client.right - client.left;
    let height = client.bottom - client.top;

    // Terminal background.
    let background_brush = CreateSolidBrush(rgb(DEFAULT_BG));
    FillRect(hdc, &client, background_brush);
    DeleteObject(background_brush);

    let previous_font = SelectObject(hdc, state.font);
    SetBkMode(hdc, TRANSPARENT as i32);

    // Status bar (accent background with dark text).
    let status_rect = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: status_h,
    };
    let status_brush = CreateSolidBrush(rgb(ACCENT_FG));
    FillRect(hdc, &status_rect, status_brush);
    DeleteObject(status_brush);
    let status = model.status_line();
    let status_wide = to_wide(&status);
    SetTextColor(hdc, rgb(DEFAULT_BG));
    TextOutW(
        hdc,
        8,
        (status_h - cell_h) / 2,
        status_wide.as_ptr(),
        status_wide.len() as i32 - 1,
    );

    // Terminal rows (per-run foreground colors).
    SetTextColor(hdc, rgb(DEFAULT_FG));
    let rows = model.grid().rows();
    for row in 0..rows {
        let y = status_h + row as i32 * cell_h;
        if y + cell_h > height - input_h {
            break;
        }
        let mut x = 8;
        for (fg, text) in model.grid().row_runs(row) {
            SetTextColor(hdc, rgb(fg));
            let wide = to_wide(&text);
            TextOutW(hdc, x, y, wide.as_ptr(), wide.len() as i32 - 1);
            x += text.chars().count() as i32 * cell_w;
        }
    }

    // Input line.
    let input_rect = RECT {
        left: 0,
        top: height - input_h,
        right: width,
        bottom: height,
    };
    let input_brush = CreateSolidBrush(rgb(INPUT_BG));
    FillRect(hdc, &input_rect, input_brush);
    DeleteObject(input_brush);
    let input = model.input_line();
    let input_wide = to_wide(&input);
    let input_y = height - input_h + (input_h - cell_h) / 2;
    SetTextColor(hdc, rgb(ACCENT_FG));
    TextOutW(
        hdc,
        8,
        input_y,
        input_wide.as_ptr(),
        input_wide.len() as i32 - 1,
    );

    // Caret ("> " prompt is two cells).
    let caret_x = 8 + (model.input_cursor() + 2) as i32 * cell_w;
    let caret_rect = RECT {
        left: caret_x,
        top: input_y,
        right: caret_x + 2,
        bottom: input_y + cell_h,
    };
    let caret_brush = CreateSolidBrush(rgb(ACCENT_FG));
    FillRect(hdc, &caret_rect, caret_brush);
    DeleteObject(caret_brush);

    SelectObject(hdc, previous_font);
    EndPaint(hwnd, &paint_struct);
}

/// Converts RGB to a Windows COLORREF (0x00BBGGRR).
const fn rgb(color: clients_windows::model::Rgb) -> u32 {
    (color.r as u32) | ((color.g as u32) << 8) | ((color.b as u32) << 16)
}

/// Encodes text as NUL-terminated UTF-16 for the wide GDI functions.
fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
