//! Windows COM probes for W0c-B (B2, B3, B5, B6, B8). Runs on the
//! windows-2025 CI runner only. Loopback fixture guests, never ads.
#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use serde_json::{json, Value};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2_2, ICoreWebView2_4, ICoreWebView2Controller,
    ICoreWebView2Environment, ICoreWebView2Frame2, ICoreWebView2Settings2,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
};
use webview2_com::{
    FrameCreatedEventHandler, FrameNavigationStartingEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, WebResourceRequestedEventHandler, take_pwstr,
};
use windows::core::{Interface, HSTRING, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};

use tauri::Manager;

use crate::Events;

static SEQ: AtomicU64 = AtomicU64::new(0);
fn seq() -> u64 {
    SEQ.fetch_add(1, Ordering::SeqCst)
}

/// A URL whose host is `localhost` or `*.localhost` (Tauri's
/// `http(s)://<scheme>.localhost` app-origin form).
fn is_localhost_host(url: &str) -> bool {
    let after = match url.split_once("://") {
        Some((_, rest)) => rest,
        None => return false,
    };
    let host = after.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    host == "localhost" || host.ends_with(".localhost")
}

/// Installs the guest's COM hooks on its controller thread. `guarded` adds the
/// SEC-B1 local-origin frame guard (top + per-frame NavigationStarting cancel,
/// and a 403 WebResourceRequested filter for `*.localhost`).
pub fn install_guest_hooks(
    label: &str,
    controller: &ICoreWebView2Controller,
    events: Arc<Events>,
    guarded: bool,
) {
    let _ = unsafe { install(label, controller, &events, guarded) };
}

unsafe fn install(
    label: &str,
    controller: &ICoreWebView2Controller,
    events: &Arc<Events>,
    guarded: bool,
) -> windows::core::Result<()> {
    let core = controller.CoreWebView2()?;

    // NewWindowRequested: read IsUserInitiated, record, and block the popup
    // (we never open real windows in the lab).
    {
        let label = label.to_owned();
        let events = Arc::clone(events);
        let mut token = 0_i64;
        core.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    let _ = args.Uri(&raw mut uri);
                    let uri = take_pwstr(uri);
                    let mut ui = windows::core::BOOL::default();
                    let _ = args.IsUserInitiated(&raw mut ui);
                    events.push(
                        &label,
                        "new-window",
                        json!({ "seq": seq(), "uri": uri, "userInitiated": ui.as_bool() }),
                    );
                    let _ = args.SetHandled(true);
                }
                Ok(())
            })),
            &raw mut token,
        )?;
    }

    // Top-level NavigationStarting: record IsUserInitiated; guarded cancels a
    // non-allowed scheme or a localhost host.
    {
        let label = label.to_owned();
        let events = Arc::clone(events);
        let mut token = 0_i64;
        core.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    let _ = args.Uri(&raw mut uri);
                    let uri = take_pwstr(uri);
                    let mut ui = windows::core::BOOL::default();
                    let _ = args.IsUserInitiated(&raw mut ui);
                    let localhost = is_localhost_host(&uri);
                    let cancelled = guarded && localhost;
                    if cancelled {
                        let _ = args.SetCancel(true);
                    }
                    events.push(
                        &label,
                        "nav",
                        json!({ "seq": seq(), "frame": "top", "uri": uri, "userInitiated": ui.as_bool(), "localhost": localhost, "cancelled": cancelled }),
                    );
                }
                Ok(())
            })),
            &raw mut token,
        )?;
    }

    if guarded {
        // Per-frame NavigationStarting via FrameCreated (ICoreWebView2_4).
        if let Ok(core4) = core.cast::<ICoreWebView2_4>() {
            let label = label.to_owned();
            let events = Arc::clone(events);
            let mut token = 0_i64;
            core4.add_FrameCreated(
                &FrameCreatedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let frame = unsafe { args.Frame() }?;
                    if let Ok(frame2) = frame.cast::<ICoreWebView2Frame2>() {
                        let label = label.clone();
                        let events = Arc::clone(&events);
                        let mut t = 0_i64;
                        unsafe {
                            frame2.add_NavigationStarting(
                                &FrameNavigationStartingEventHandler::create(Box::new(move |_, a| {
                                    if let Some(a) = a {
                                        let mut uri = PWSTR::null();
                                        let _ = a.Uri(&raw mut uri);
                                        let uri = take_pwstr(uri);
                                        let localhost = is_localhost_host(&uri);
                                        if localhost {
                                            let _ = a.SetCancel(true);
                                        }
                                        events.push(
                                            &label,
                                            "frame-nav",
                                            json!({ "seq": seq(), "uri": uri, "localhost": localhost, "cancelled": localhost }),
                                        );
                                    }
                                    Ok(())
                                })),
                                &raw mut t,
                            )
                        }?;
                    }
                    Ok(())
                })),
                &raw mut token,
            )?;
        }

        // 403 any *.localhost request; record each hit (and its order).
        let filter = HSTRING::from("*");
        core.AddWebResourceRequestedFilter(&filter, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?;
        let label = label.to_owned();
        let events = Arc::clone(events);
        let env: ICoreWebView2Environment = core.cast::<ICoreWebView2_2>()?.Environment()?;
        let mut token = 0_i64;
        core.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let request = args.Request()?;
                    let mut uri = PWSTR::null();
                    request.Uri(&raw mut uri)?;
                    let uri = take_pwstr(uri);
                    if is_localhost_host(&uri) {
                        let response = env.CreateWebResourceResponse(
                            None::<&windows::Win32::System::Com::IStream>,
                            403,
                            &HSTRING::from("Forbidden"),
                            &HSTRING::from("Content-Type: text/plain"),
                        )?;
                        args.SetResponse(&response)?;
                        events.push(
                            &label,
                            "wrr",
                            json!({ "seq": seq(), "uri": uri, "action": "403" }),
                        );
                    }
                }
                Ok(())
            })),
            &raw mut token,
        )?;
    }
    Ok(())
}

/// B2: read the UA with `ICoreWebView2Settings2::get_UserAgent`.
pub fn read_ua(app: &tauri::AppHandle, label: &str) -> Result<Value, String> {
    on_controller(app, label, |controller| unsafe {
        let core = controller.CoreWebView2()?;
        let settings2 = core.Settings()?.cast::<ICoreWebView2Settings2>()?;
        let mut ua = PWSTR::null();
        settings2.UserAgent(&raw mut ua)?;
        Ok(json!({ "userAgent": take_pwstr(ua) }))
    })
}

/// B6: set the controller's `ZoomFactor`.
pub fn set_zoom(app: &tauri::AppHandle, label: &str, zoom: f64) -> Value {
    match on_controller(app, label, move |controller| unsafe {
        controller.SetZoomFactor(zoom)?;
        let mut z = 0.0;
        controller.ZoomFactor(&raw mut z)?;
        Ok(json!({ "ok": true, "zoomFactor": z }))
    }) {
        Ok(v) => v,
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

/// Runs `f` on the webview's own thread with its controller.
fn on_controller<F>(app: &tauri::AppHandle, label: &str, f: F) -> Result<Value, String>
where
    F: FnOnce(&ICoreWebView2Controller) -> windows::core::Result<Value> + Send + 'static,
{
    let webview = app
        .webviews()
        .get(label)
        .cloned()
        .ok_or_else(|| format!("no webview {label}"))?;
    let (tx, rx) = mpsc::channel();
    webview
        .with_webview(move |pw| {
            let r = f(&pw.controller()).map_err(|e| e.to_string());
            let _ = tx.send(r);
        })
        .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|e| e.to_string())?
}

/// B3: a real OS click on the guest's button via `SendInput` (so WebView2
/// marks the resulting popup/navigation user-initiated). The guest sits at
/// logical (480,0) in the main window; the button is near its top-left.
pub fn click_guest(app: &tauri::AppHandle, main_label: &str, _guest_label: &str) -> Value {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
        MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, MOUSE_EVENT_FLAGS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SetForegroundWindow, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
        SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };
    // `get_window`, not `get_webview_window`: once a child webview is attached
    // the window is no longer a "webview window" and `get_webview_window`
    // returns None (SPA F3). `Window` carries hwnd/scale/inner_position.
    let Some(win) = app.get_window(main_label) else {
        return json!({ "error": "no main window" });
    };
    let hwnd = match win.hwnd() {
        Ok(h) => h.0 as isize,
        Err(e) => return json!({ "error": e.to_string() }),
    };
    let scale = win.scale_factor().unwrap_or(1.0);
    let Ok(origin) = win.inner_position() else {
        return json!({ "error": "no inner position" });
    };
    // guest logical (480,0) + button ~(30,30) → window logical (510,30).
    let sx = origin.x + ((510.0_f64) * scale).round() as i32;
    let sy = origin.y + ((30.0_f64) * scale).round() as i32;
    unsafe {
        let _ = SetForegroundWindow(HWND(hwnd as *mut c_void));
        let (vx, vy, vw, vh) = (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2),
        );
        let norm = |v: i32, o: i32, s: i32| ((v - o) * 65535) / (s - 1);
        let mouse = |flags: MOUSE_EVENT_FLAGS| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: norm(sx, vx, vw),
                    dy: norm(sy, vy, vh),
                    mouseData: 0,
                    dwFlags: flags | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [
            mouse(MOUSEEVENTF_MOVE),
            mouse(MOUSEEVENTF_LEFTDOWN),
            mouse(MOUSEEVENTF_LEFTUP),
        ];
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        json!({ "screen": [sx, sy], "scale": scale, "sent": sent })
    }
}

/// B8 lives in its own crate (`../b8-rewrap`) because its link question (two
/// webview2-com-sys versions) must not break B1-B6. The CI job builds it
/// separately and records whether the cross-minor re-wrap compiles and links.
#[allow(dead_code)]
fn unused(_: WPARAM, _: LPARAM, _: *const c_void) {}
