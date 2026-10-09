//! The harness's app windows: plain Tauri windows, built hidden and, when a
//! run shows them, made invisible first (macOS invisible lab: alpha 0,
//! click-through, never key; see `macos_lab.rs`), as ow-electron's harness
//! shows its windows inactive at opacity 0.
//!
//! A window is sized as an Electron `BrowserWindow` of the same options:
//! `width` / `height` are the frame, `x` / `y` its top-left corner.

use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};
use tauri::webview::PageLoadEvent;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder, Window, Wry,
};

use crate::harness;

/// The label of the harness's ad window (the counterpart of ow-electron's
/// main `BrowserWindow`).
pub const MAIN: &str = "main";

/// How long a window waits for its first page.
const LOAD_WAIT: Duration = Duration::from_secs(30);

/// The URL each harness window last loaded (label -> URL). Read instead of
/// `Webview::url`, which panics in wry while a `WKWebView` has no URL yet
/// (a window opened on `about:blank`).
static URLS: std::sync::Mutex<std::collections::BTreeMap<String, String>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// The URL window `label` last loaded, if known.
pub fn known_url(label: &str) -> Option<String> {
    harness::lock(&URLS).get(label).cloned()
}

/// What to build.
pub struct Spec<'a> {
    /// The window label.
    pub label: &'a str,
    /// What the window loads.
    pub url: WebviewUrl,
    /// The title; `None` keeps Tauri's default (the plugin maps it to the
    /// app name, as ow-electron titles an untitled window).
    pub title: Option<&'a str>,
    /// The webview's user agent; `None` keeps the default (custom-ua).
    pub user_agent: Option<&'a str>,
    /// Frame size in logical pixels.
    pub size: (f64, f64),
    /// Frame position in logical pixels.
    pub position: (f64, f64),
}

/// Builds a hidden, unfocusable window. Returns the window and the
/// receiver of its page loads (the URL of each finished load); see
/// [`wait_loaded`].
///
/// # Errors
///
/// When Tauri cannot build the window.
pub fn build(
    app: &AppHandle<Wry>,
    spec: &Spec<'_>,
) -> tauri::Result<(Window<Wry>, mpsc::Receiver<String>)> {
    let (tx, rx) = mpsc::channel();
    if let WebviewUrl::External(url) = &spec.url {
        harness::lock(&URLS).insert(spec.label.to_owned(), url.to_string());
    }
    let label = spec.label.to_owned();
    let mut builder = WebviewWindowBuilder::new(app, spec.label, spec.url.clone())
        .inner_size(spec.size.0, spec.size.1)
        .position(spec.position.0, spec.position.1)
        .visible(false)
        .focused(false)
        .focusable(false)
        .skip_taskbar(true)
        .on_page_load(move |_, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let url = payload.url().to_string();
                harness::lock(&URLS).insert(label.clone(), url.clone());
                let _ = tx.send(url);
            }
        });
    if let Some(title) = spec.title {
        builder = builder.title(title);
    }
    if let Some(user_agent) = spec.user_agent {
        builder = builder.user_agent(user_agent);
    }
    let window = builder.build()?.as_ref().window();
    set_frame_size(&window, spec.size.0, spec.size.1);
    Ok((window, rx))
}

/// Waits (bounded) for the first page load of window `label`; records a
/// timeout. Returns whether the page loaded.
pub fn wait_loaded(label: &str, loads: &mpsc::Receiver<String>) -> bool {
    let loaded = loads.recv_timeout(LOAD_WAIT).is_ok();
    if !loaded {
        harness::get().record(
            "events.jsonl",
            json!({ "kind": "window-load-timeout", "label": label }),
        );
    }
    loaded
}

/// Sizes the window so that its frame is `width` x `height` (logical), as
/// an Electron `BrowserWindow` is sized.
pub fn set_frame_size(window: &Window<Wry>, width: f64, height: f64) {
    let _ = window.set_size(LogicalSize::new(width, height));
    let (Ok(scale), Ok(outer), Ok(inner)) = (
        window.scale_factor(),
        window.outer_size(),
        window.inner_size(),
    ) else {
        return;
    };
    let dw = f64::from(outer.width.saturating_sub(inner.width)) / scale;
    let dh = f64::from(outer.height.saturating_sub(inner.height)) / scale;
    if dw > 0.0 || dh > 0.0 {
        let _ = window.set_size(LogicalSize::new(
            (width - dw).max(1.0),
            (height - dh).max(1.0),
        ));
    }
}

/// Shows the window without focusing it. In the invisible lab it is made
/// alpha 0 and click-through first, on the main thread, and ordered front
/// without becoming key (`macos_lab::hold_app_back`).
pub fn show_inactive(window: &Window<Wry>) {
    if tauri_plugin_overwolf::lab_invisible() {
        let _ = window.set_ignore_cursor_events(true);
        #[cfg(target_os = "macos")]
        {
            let Ok(ns_window) = window.ns_window() else {
                return;
            };
            let ns_window = ns_window as usize;
            let (tx, rx) = mpsc::channel();
            if window
                .run_on_main_thread(move || {
                    let _ = tx.send(crate::macos_lab::prepare_invisible(ns_window));
                })
                .is_err()
            {
                return;
            }
            // Never show a window whose alpha is not 0.
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Some(alpha)) if alpha < 0.01 => {}
                _ => {
                    harness::get().record(
                        "events.jsonl",
                        json!({ "kind": "show-refused", "label": window.label(), "why": "alpha not 0" }),
                    );
                    return;
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Off macOS the invisible lab never shows a window.
            harness::get().record(
                "events.jsonl",
                json!({ "kind": "show-refused", "label": window.label(), "why": "invisible lab off macOS" }),
            );
            return;
        }
    }
    let _ = window.show();
}

/// Electron-like rectangle (logical pixels).
fn rect(x: f64, y: f64, w: f64, h: f64) -> Value {
    json!({ "x": x.round(), "y": y.round(), "width": w.round(), "height": h.round() })
}

/// The window's state in the shape of ow-electron's harness
/// `describeWindow` (bounds in logical pixels).
pub fn describe(window: &Window<Wry>) -> Value {
    let scale = window.scale_factor().unwrap_or(1.0);
    let bounds = match (window.outer_position(), window.outer_size()) {
        (Ok(p), Ok(s)) => rect(
            f64::from(p.x) / scale,
            f64::from(p.y) / scale,
            f64::from(s.width) / scale,
            f64::from(s.height) / scale,
        ),
        _ => Value::Null,
    };
    let content = match (window.inner_position(), window.inner_size()) {
        (Ok(p), Ok(s)) => rect(
            f64::from(p.x) / scale,
            f64::from(p.y) / scale,
            f64::from(s.width) / scale,
            f64::from(s.height) / scale,
        ),
        _ => Value::Null,
    };
    json!({
        "label": window.label(),
        "title": window.title().ok(),
        "bounds": bounds,
        "contentBounds": content,
        "visible": window.is_visible().ok(),
        "minimized": window.is_minimized().ok(),
        "focused": window.is_focused().ok(),
        "resizable": window.is_resizable().ok(),
        "minimizable": window.is_minimizable().ok(),
        "maximizable": window.is_maximizable().ok(),
        "closable": window.is_closable().ok(),
        "alwaysOnTop": window.is_always_on_top().ok(),
        "url": known_url(window.label()),
    })
}

/// A window action of the scenarios (`window`, `extra-window`): Electron
/// `BrowserWindow` method names mapped onto Tauri's window. `show` and
/// `focus` show inactive (the lab never focuses a window). Returns `false`
/// for a method Tauri has no counterpart for.
pub fn act(window: &Window<Wry>, method: &str, args: &[Value]) -> bool {
    let num = |i: usize| args.get(i).and_then(Value::as_f64);
    match method {
        "minimize" => drop(window.minimize()),
        "restore" => drop(window.unminimize()),
        "hide" => drop(window.hide()),
        "show" | "showInactive" | "focus" => show_inactive(window),
        "close" => drop(window.close()),
        "destroy" => drop(window.destroy()),
        "setTitle" => {
            let title = args.first().and_then(Value::as_str).unwrap_or_default();
            drop(window.set_title(title));
        }
        "setSize" => {
            if let (Some(w), Some(h)) = (num(0), num(1)) {
                set_frame_size(window, w, h);
            }
        }
        "setPosition" => {
            if let (Some(x), Some(y)) = (num(0), num(1)) {
                drop(window.set_position(LogicalPosition::new(x, y)));
            }
        }
        "loadURL" => {
            let Some(url) = args
                .first()
                .and_then(Value::as_str)
                .and_then(|u| tauri::Url::parse(u).ok())
            else {
                return false;
            };
            let Some(webview) = window
                .webviews()
                .into_iter()
                .find(|w| w.label() == window.label())
            else {
                return false;
            };
            harness::lock(&URLS).insert(window.label().to_owned(), url.to_string());
            drop(webview.navigate(url));
        }
        _ => return false,
    }
    true
}

/// The window `label`, if it exists (`get_window`: Tauri's webview-window
/// lookup does not find a window once it hosts an ad guest).
pub fn get(app: &AppHandle<Wry>, label: &str) -> Option<Window<Wry>> {
    app.get_window(label)
}
