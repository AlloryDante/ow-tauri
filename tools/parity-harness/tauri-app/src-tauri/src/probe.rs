//! The lab checks of the ad formats (L1-L3 on macOS, L1-W, L2, L3-W, L5 on
//! Windows) from the ad window's native view tree: `native.rs` (macOS) and
//! `native_win.rs` (Windows). No screen capture, and no input to an ad
//! guest.

use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};
use tauri::{AppHandle, Manager, Wry};

#[cfg(target_os = "macos")]
use crate::native::{self, Point as ProbePoint};
#[cfg(windows)]
use crate::native_win::{self, Point as ProbePoint};

/// How long a probe waits for webviews and the main thread to answer.
const PROBE_WAIT: Duration = Duration::from_secs(5);

/// Parses `[{name, x, y}]`.
#[cfg(any(target_os = "macos", windows))]
fn points(raw: &[Value]) -> Vec<ProbePoint> {
    raw.iter()
        .filter_map(|p| {
            Some(ProbePoint {
                name: p.get("name")?.as_str()?.to_owned(),
                x: p.get("x")?.as_f64()?,
                y: p.get("y")?.as_f64()?,
            })
        })
        .collect()
}

/// The native order of the window `window`'s webviews, the state of each
/// webview (macOS: background; Windows: container region, background
/// colour, mute), which webview a click at each point would reach, then
/// (`snapshot`) the rendering sampled at the points (macOS: each webview's
/// own snapshot; Windows: the composed window, saved as
/// `<capture>-print.bmp` / `<capture>-screen.bmp`), and (`click`, test mode
/// only, decided by the caller) one click at the named point into the app's
/// webview `embedder` when, and only when, the hit test names that webview.
/// Other platforms answer `{unsupported: true}`. Call off the main thread.
///
/// # Errors
///
/// When the window is missing or a webview does not answer in time.
pub fn native_probe(
    app: &AppHandle<Wry>,
    window: &str,
    embedder: &str,
    raw_points: &[Value],
    snapshot: bool,
    click: Option<&str>,
    capture: Option<&str>,
) -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        let _ = capture;
        native_probe_macos(app, window, embedder, raw_points, snapshot, click)
    }
    #[cfg(windows)]
    {
        native_probe_windows(app, window, embedder, raw_points, snapshot, click, capture)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (app, window, embedder, raw_points, snapshot, click, capture);
        Ok(json!({ "unsupported": true }))
    }
}

/// The native window and the webviews of the window `label`
/// (`(ns_window, address -> label)`).
#[cfg(target_os = "macos")]
fn window_views(app: &AppHandle<Wry>, label: &str) -> Result<(usize, native::Views), String> {
    let window = app
        .get_window(label)
        .ok_or_else(|| format!("no window {label}"))?;
    let ns_window = window.ns_window().map_err(|e| e.to_string())? as usize;
    let webviews = window.webviews();
    let (tx, rx) = mpsc::channel();
    for webview in &webviews {
        let tx = tx.clone();
        let label = webview.label().to_owned();
        webview
            .with_webview(move |pw| {
                let _ = tx.send((native::address(pw.inner()), label));
            })
            .map_err(|e| e.to_string())?;
    }
    drop(tx);
    let mut views = native::Views::new();
    while views.len() < webviews.len() {
        let (address, label) = rx.recv_timeout(PROBE_WAIT).map_err(|e| e.to_string())?;
        views.insert(address, label);
    }
    Ok((ns_window, views))
}

#[cfg(target_os = "macos")]
fn native_probe_macos(
    app: &AppHandle<Wry>,
    window: &str,
    embedder: &str,
    raw_points: &[Value],
    snapshot: bool,
    click: Option<&str>,
) -> Result<Value, String> {
    let (ns_window, views) = window_views(app, window)?;
    let pts = points(raw_points);
    let on_main = |f: Box<dyn FnOnce() -> Value + Send>| -> Result<Value, String> {
        let (tx, rx) = mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(f());
        })
        .map_err(|e| e.to_string())?;
        rx.recv_timeout(PROBE_WAIT).map_err(|e| e.to_string())
    };
    let (v, e, p) = (views.clone(), embedder.to_owned(), pts.clone());
    let mut out = on_main(Box::new(move || native::inspect(ns_window, &e, &v, &p)))?;
    if let Some(name) = click {
        out["click"] = match pts.iter().find(|p| p.name == name) {
            None => json!({ "sent": false, "refused": "no such point" }),
            Some(point) => {
                let (v, e, point) = (views.clone(), embedder.to_owned(), point.clone());
                on_main(Box::new(move || native::click(ns_window, &e, &v, &point)))?
            }
        };
    }
    if snapshot {
        let embedder_address = views
            .iter()
            .find(|(_, l)| l.as_str() == embedder)
            .map(|(a, _)| *a);
        let (tx, rx) = mpsc::channel();
        for (address, label) in &views {
            let (tx, label, address, p) = (tx.clone(), label.clone(), *address, pts.clone());
            app.run_on_main_thread(move || {
                native::snapshot(address, embedder_address, p, move |v| {
                    let _ = tx.send((label.clone(), v));
                });
            })
            .map_err(|e| e.to_string())?;
        }
        drop(tx);
        let mut shots = serde_json::Map::new();
        while shots.len() < views.len() {
            match rx.recv_timeout(PROBE_WAIT) {
                Ok((label, v)) => {
                    shots.insert(label, v);
                }
                Err(_) => break,
            }
        }
        out["snapshots"] = Value::Object(shots);
    }
    Ok(out)
}

/// The Windows probe. The webview facts are read on each webview's thread;
/// the hit tests, the capture and the click work on window handles from
/// this thread.
#[cfg(windows)]
fn native_probe_windows(
    app: &AppHandle<Wry>,
    window: &str,
    embedder: &str,
    raw_points: &[Value],
    snapshot: bool,
    click: Option<&str>,
    capture: Option<&str>,
) -> Result<Value, String> {
    let win = app
        .get_window(window)
        .ok_or_else(|| format!("no window {window}"))?;
    let top = win.hwnd().map_err(|e| e.to_string())?.0 as isize;
    let webviews = win.webviews();
    let (tx, rx) = mpsc::channel();
    for webview in &webviews {
        let tx = tx.clone();
        let label = webview.label().to_owned();
        webview
            .with_webview(move |pw| {
                let _ = tx.send(native_win::webview_facts(&label, &pw.controller()));
            })
            .map_err(|e| e.to_string())?;
    }
    drop(tx);
    let mut facts = Vec::new();
    while facts.len() < webviews.len() {
        facts.push(rx.recv_timeout(PROBE_WAIT).map_err(|e| e.to_string())?);
    }
    native_win::keep_on_top(top);
    let pts = points(raw_points);
    let mut out = native_win::inspect(top, embedder, &facts, &pts);
    if let Some(name) = click {
        out["click"] = match pts.iter().find(|p| p.name == name) {
            None => json!({ "sent": false, "refused": "no such point" }),
            Some(point) => native_win::click(top, embedder, &facts, point),
        };
    }
    if snapshot {
        let stem = capture.map(|c| crate::harness::get().run_dir.join(c));
        out["composite"] = native_win::capture(
            top,
            facts.iter().find(|f| f.label == embedder),
            &pts,
            stem.as_deref(),
        );
    }
    Ok(out)
}
