//! WebView2 hosting for the PC GUI (T005+).
//!
//! Creates the CoreWebView2 environment and controller, parents the
//! controller to the main window, and exposes the controller + webview for
//! the JS<->Rust bridge (T007). The async COM creation is driven with the
//! webview2-com `wait_for_async_operation` helpers, which pump the message
//! queue until each completion handler fires.

use std::cell::RefCell;
use std::rc::Rc;

use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    ExecuteScriptCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2Controller,
        ICoreWebView2Environment,
    },
    WebMessageReceivedEventHandler,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::RECT;
use windows::Win32::System::Com::CoTaskMemFree;

/// Creates the WebView2 environment + controller parented to `parent`,
/// optionally navigates to `html` (raw HTML via NavigateToString when
/// non-empty, otherwise the caller navigates later), and returns the pair.
pub(crate) unsafe fn init_webview2(
    parent: *mut core::ffi::c_void,
    html: &str,
) -> webview2_com::Result<(ICoreWebView2Controller, ICoreWebView2)> {
    let parent = HWND(parent);

    let environment_slot: Rc<RefCell<Option<ICoreWebView2Environment>>> =
        Rc::new(RefCell::new(None));
    let environment_slot2 = environment_slot.clone();
    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(|handler| unsafe {
            CreateCoreWebView2EnvironmentWithOptions(None, None, None, &handler)?;
            Ok(())
        }),
        Box::new(move |result, environment| {
            result?;
            *environment_slot2.borrow_mut() = environment;
            Ok(())
        }),
    )?;
    let environment = environment_slot
        .borrow()
        .clone()
        .ok_or_else(|| webview2_com::Error::CallbackError("no webview2 environment".to_string()))?;

    let controller_slot: Rc<RefCell<Option<ICoreWebView2Controller>>> = Rc::new(RefCell::new(None));
    let controller_slot2 = controller_slot.clone();
    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            environment.CreateCoreWebView2Controller(parent, &handler)?;
            Ok(())
        }),
        Box::new(move |result, controller| {
            result?;
            *controller_slot2.borrow_mut() = controller;
            Ok(())
        }),
    )?;
    let controller = controller_slot
        .borrow()
        .clone()
        .ok_or_else(|| webview2_com::Error::CallbackError("no webview2 controller".to_string()))?;

    controller.SetParentWindow(parent)?;
    let webview = controller.CoreWebView2()?;
    if !html.is_empty() {
        let wide: Vec<u16> = html.encode_utf16().chain(std::iter::once(0)).collect();
        let pcwstr = windows::core::PCWSTR(wide.as_ptr());
        webview.NavigateToString(pcwstr)?;
    }
    Ok((controller, webview))
}

/// Navigates the WebView2 to a URL.
pub(crate) unsafe fn navigate(webview: &ICoreWebView2, url: &str) -> windows::core::Result<()> {
    let uri = windows::core::HSTRING::from(url);
    webview.Navigate(&uri)
}

/// The JS bridge shim injected into every document before app scripts. It
/// defines window.__TAURI_INTERNALS__ so @tauri-apps/api calls route to the
/// host via chrome.webview.postMessage (mirrors ui/src/bridge/shim.ts).
pub(crate) const SHIM_JS: &str = r#"(function () {
  if (window.__TAURI_INTERNALS__) return;
  window.__probeErrors = [];
  window.addEventListener('error', function (e) { window.__probeErrors.push(String(e.error || e.message)); });
  window.addEventListener('unhandledrejection', function (e) { window.__probeErrors.push('REJ:' + String(e.reason)); });
  var _req = 0;
  var _pending = {};
  var _callbacks = {};
  function _post(m) { if (window.chrome && window.chrome.webview) { window.chrome.webview.postMessage(m); } }
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
    transformCallback: function (cb) { var id = (window.__TAURI_INTERNALS__._cb = (window.__TAURI_INTERNALS__._cb || 0) + 1); if (typeof cb === 'function') { _callbacks[id] = cb; } return id; },
    runCallback: function (id, payload) { if (_callbacks[id]) { _callbacks[id](payload); } },
    unregisterCallback: function (id) { delete _callbacks[id]; },
    convertFileSrc: function (filePath) { return filePath; },
    postMessage: _post,
    invoke: function (cmd, payload, opts) {
      _req += 1; var id = _req;
      return new Promise(function (resolve, reject) {
        _pending[id] = { resolve: resolve, reject: reject };
        _post({ kind: 'invoke', requestId: id, cmd: cmd, payload: payload || {} });
      });
    }
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: function (event, eventId) { return window.__TAURI_INTERNALS__.invoke('plugin:event|unlisten', { event: event, eventId: eventId }); }
  };
  if (window.chrome && window.chrome.webview && window.chrome.webview.addEventListener) {
    window.chrome.webview.addEventListener('message', function (e) {
      var d = (typeof e.data === 'string') ? JSON.parse(e.data) : e.data;
      if (!d) { return; }
      if (d.kind === 'invoke-reply' && _pending[d.requestId]) {
        var p = _pending[d.requestId]; delete _pending[d.requestId];
        if (d.error) { p.reject(new Error(d.error)); } else { p.resolve(d.payload); }
      } else if (d.kind === 'event' && typeof d.handlerId === 'number') {
        window.__TAURI_INTERNALS__.runCallback(d.handlerId, d.payload);
      }
    });
  }
})();"#;

/// Injects the bridge shim into every document created in this WebView2.
pub(crate) unsafe fn inject_shim(
    webview: &ICoreWebView2,
    script: &str,
) -> webview2_com::Result<()> {
    let wide: Vec<u16> = script.encode_utf16().chain(std::iter::once(0)).collect();
    let webview = webview.clone();
    AddScriptToExecuteOnDocumentCreatedCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            let pcwstr = windows::core::PCWSTR(wide.as_ptr());
            webview.AddScriptToExecuteOnDocumentCreated(pcwstr, &handler)?;
            Ok(())
        }),
        Box::new(|result, _script_id| {
            result?;
            Ok(())
        }),
    )?;
    Ok(())
}

/// Registers the WebMessageReceived handler; hwnd is the main window whose
/// AppState the bridge mutates. Returns the event token.
pub(crate) unsafe fn add_message_handler(
    hwnd: *mut core::ffi::c_void,
    webview: &ICoreWebView2,
) -> windows::core::Result<i64> {
    let handler = WebMessageReceivedEventHandler::create(Box::new(move |sender, args| {
        let sender = sender.expect("webmessage sender");
        let args = args.expect("webmessage args");
        let mut json_pwstr = windows::core::PWSTR::null();
        args.WebMessageAsJson(&mut json_pwstr)?;
        let message = json_pwstr.to_string()?;
        if !json_pwstr.is_null() {
            CoTaskMemFree(Some(json_pwstr.0 as *const core::ffi::c_void));
        }
        eprintln!(
            "[bridge] <- {}",
            message.chars().take(140).collect::<String>()
        );
        if let Some(reply) = crate::on_web_message(hwnd, &message) {
            eprintln!(
                "[bridge] -> {}",
                reply.chars().take(140).collect::<String>()
            );
            let hstring = windows::core::HSTRING::from(reply);
            sender.PostWebMessageAsString(&hstring)?;
        }
        Ok(())
    }));
    let mut token: i64 = 0;
    webview.add_WebMessageReceived(&handler, &mut token)?;
    Ok(token)
}

/// Runs a JS expression in the page and returns the JSON-encoded result.
pub(crate) unsafe fn execute_script(
    webview: &ICoreWebView2,
    script: &str,
) -> webview2_com::Result<String> {
    let wide: Vec<u16> = script.encode_utf16().chain(std::iter::once(0)).collect();
    let webview = webview.clone();
    let output_slot: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let output_slot2 = output_slot.clone();
    ExecuteScriptCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            let pcwstr = windows::core::PCWSTR(wide.as_ptr());
            webview.ExecuteScript(pcwstr, &handler)?;
            Ok(())
        }),
        Box::new(move |result, output| {
            result?;
            *output_slot2.borrow_mut() = Some(output);
            Ok(())
        }),
    )?;
    let output = output_slot.borrow().clone().unwrap_or_default();
    Ok(output)
}

/// Updates the WebView2 controller bounds (call on WM_SIZE).
pub(crate) unsafe fn set_bounds(
    controller: &ICoreWebView2Controller,
    width: i32,
    height: i32,
) -> windows::core::Result<()> {
    controller.SetBounds(RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    })
}

/// Closes the WebView2 controller (call on window destroy).
pub(crate) unsafe fn close_webview2(controller: &ICoreWebView2Controller) {
    let _ = controller.Close();
}
