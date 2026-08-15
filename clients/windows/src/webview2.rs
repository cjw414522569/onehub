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
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2Controller,
        ICoreWebView2Environment,
    },
};
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::RECT;

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
