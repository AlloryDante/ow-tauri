//! The invisible lab (Cargo feature `lab`, off by default; see
//! `e2e/README.md`). It never ships: `e2e/run.mjs` builds a debug app with
//! it under the lab bundle id, and nothing happens unless the environment
//! asks for it.
//!
//! - `OW_TAURI_LAB_INVISIBLE=1` (the plugin's lab mode): the showcase window
//!   is on screen at alpha 0, ignores the mouse and never becomes key; the
//!   app never activates and stays out of the Dock and the app switcher.
//! - `OW_TAURI_LAB_DIR=<dir>`: the plugin's trace, plus the driver's records
//!   (`e2e.jsonl`) through [`e2e_record`].
//! - `OW_SHOWCASE_LAB_SINK=<http://127.0.0.1:port>`: where the plugin's
//!   analytics and the consent experiment go. Without it they go to a
//!   closed loopback port: a lab build never reports to Overwolf.
//! - `OW_SHOWCASE_E2E_CONFIG=<json>`: the driver's run configuration
//!   ([`e2e_config`]); without it the driver stays inert. Its `stillsDir`
//!   is where [`e2e_still`] writes; its `mode` gates [`e2e_native_probe`]'s
//!   click (test mode only).
//!
//! On macOS the driver also gets the window's native hit test and an
//! in-process still of the window ([`native`]): no screen capture, and no
//! input to an ad guest.

use std::io::Write as _;
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::time::Duration;

use serde_json::{Value, json};
use tauri::{AppHandle, Manager, Runtime, Webview, Window};

use crate::showcase::MAIN;

#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "Objective-C calls of the native lab probes")]
mod native;

/// How long a native probe or snapshot may take.
#[cfg(target_os = "macos")]
const PROBE_WAIT: Duration = Duration::from_secs(10);
/// Where the plugin's host requests go without `OW_SHOWCASE_LAB_SINK`: the
/// discard port on loopback (nothing listens, nothing leaves the machine).
const CLOSED_SINK: &str = "http://127.0.0.1:9";

/// Whether the app must stay invisible (`OW_TAURI_LAB_INVISIBLE=1`).
#[must_use]
pub fn invisible() -> bool {
    tauri_plugin_overwolf::lab_invisible()
}

/// The loopback base URL the plugin's analytics and consent experiment go
/// to: `OW_SHOWCASE_LAB_SINK` when it is a loopback `http` URL, else
/// [`CLOSED_SINK`].
fn sink(value: Option<&str>) -> String {
    let loopback = |v: &&str| {
        ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"]
            .iter()
            .any(|p| v.starts_with(p))
    };
    value.map(str::trim).filter(loopback).map_or_else(
        || CLOSED_SINK.to_owned(),
        |v| v.trim_end_matches('/').to_owned(),
    )
}

/// The plugin's test endpoints for a sink base URL.
fn endpoints(base: &str) -> tauri_plugin_overwolf::analytics::TestEndpoints {
    tauri_plugin_overwolf::analytics::TestEndpoints {
        counter: Some(format!("{base}/analytics/Counter")),
        insert_stats: Some(format!("{base}/tracking/InsertStats")),
        cmp_eu_only: Some(format!("{base}/experiments/cmp-eu-only")),
        update_feed: None,
    }
}

/// The lab's plugin builder: test ads follow `--test-ad` as usual; analytics
/// and the consent experiment go to the loopback sink.
pub fn overwolf_builder(builder: tauri_plugin_overwolf::Builder) -> tauri_plugin_overwolf::Builder {
    let base = sink(std::env::var("OW_SHOWCASE_LAB_SINK").ok().as_deref());
    builder.endpoints(endpoints(&base))
}

/// Before the app is built: in the invisible lab nothing may activate the
/// app or make a window key (macOS; `WebKit` asks for both).
pub fn hold_app_back() {
    #[cfg(target_os = "macos")]
    if invisible() {
        native::hold_app_back();
    }
}

/// After the window is built, before it is shown: alpha 0 and
/// click-through in the invisible lab.
pub fn prepare_window<R: Runtime>(window: &Window<R>) {
    if !invisible() {
        return;
    }
    let _ = window.set_ignore_cursor_events(true);
    let _ = window.set_focusable(false);
    #[cfg(target_os = "macos")]
    if let Ok(ns_window) = window.ns_window() {
        native::prepare_invisible(ns_window as usize);
    }
}

/// Puts the (alpha 0) window on screen without making it key or activating
/// the app, so the plugin sees it visible and the user sees nothing.
pub fn order_front<R: Runtime>(window: &Window<R>) {
    #[cfg(target_os = "macos")]
    {
        let Ok(ns_window) = window.ns_window() else {
            return;
        };
        let address = ns_window as usize;
        let _ = window.run_on_main_thread(move || native::order_front(address));
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Elsewhere the lab keeps windows hidden (macOS is the lab host).
        let _ = window;
    }
}

/// The run configuration, or `Null`.
fn config() -> Value {
    std::env::var("OW_SHOWCASE_E2E_CONFIG")
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

/// Only the showcase webview, where the driver runs, may call the lab
/// commands (an ad guest cannot reach app commands at all).
fn main_only<R: Runtime>(webview: &Webview<R>, command: &str) -> Result<(), String> {
    if webview.label() == MAIN {
        Ok(())
    } else {
        Err(format!("{command} is for the showcase webview only"))
    }
}

/// The driver's run configuration (`OW_SHOWCASE_E2E_CONFIG`) with this
/// process's `pid` added (the restart check tells the two processes apart
/// by it), or `null` when the runner did not launch the app.
///
/// # Errors
///
/// Called from another webview, or the variable is not valid JSON.
#[tauri::command]
pub fn e2e_config<R: Runtime>(webview: Webview<R>) -> Result<Value, String> {
    main_only(&webview, "e2e_config")?;
    match std::env::var("OW_SHOWCASE_E2E_CONFIG") {
        Ok(text) if !text.is_empty() => {
            let mut config: Value =
                serde_json::from_str(&text).map_err(|e| format!("OW_SHOWCASE_E2E_CONFIG: {e}"))?;
            if let Some(object) = config.as_object_mut() {
                object.insert("pid".to_owned(), json!(std::process::id()));
            }
            Ok(config)
        }
        _ => Ok(Value::Null),
    }
}

/// Appends one record of the driver to `<OW_TAURI_LAB_DIR>/e2e.jsonl`
/// (nothing without a lab directory).
///
/// # Errors
///
/// Called from another webview, or the file cannot be written.
#[tauri::command]
pub fn e2e_record<R: Runtime>(webview: Webview<R>, entry: Value) -> Result<(), String> {
    main_only(&webview, "e2e_record")?;
    let Some(dir) = std::env::var_os("OW_TAURI_LAB_DIR").filter(|d| !d.is_empty()) else {
        return Ok(());
    };
    let mut line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(PathBuf::from(dir).join("e2e.jsonl"))
        .map_err(|e| e.to_string())?;
    file.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

/// The app's windows: label, visibility (as the window reports it; in the
/// invisible lab a shown window is on screen at alpha 0) and its webviews'
/// URLs with the app origin as `<app>`.
///
/// # Errors
///
/// Called from another webview.
#[tauri::command]
pub fn e2e_windows<R: Runtime>(webview: Webview<R>) -> Result<Value, String> {
    main_only(&webview, "e2e_windows")?;
    let windows: Vec<Value> = webview
        .app_handle()
        .windows()
        .into_iter()
        .map(|(label, window)| {
            let webviews: Vec<Value> = window
                .webviews()
                .iter()
                .map(|w| {
                    let url = w
                        .url()
                        .map(|u| app_relative(u.as_str()))
                        .unwrap_or_default();
                    json!({ "label": w.label(), "url": url })
                })
                .collect();
            json!({
                "label": label,
                "visible": window.is_visible().unwrap_or(false),
                "webviews": webviews,
            })
        })
        .collect();
    Ok(Value::Array(windows))
}

/// An app page URL with its origin as `<app>`; other URLs as they are.
fn app_relative(url: &str) -> String {
    for origin in ["tauri://localhost/", "http://tauri.localhost/"] {
        if let Some(rest) = url.strip_prefix(origin) {
            return format!("<app>/{rest}");
        }
    }
    url.to_owned()
}

/// Quits the app as a user would (the plugin drains its analytics at exit).
///
/// # Errors
///
/// Called from another webview.
#[tauri::command]
pub fn e2e_quit<R: Runtime>(webview: Webview<R>) -> Result<(), String> {
    main_only(&webview, "e2e_quit")?;
    webview.app_handle().exit(0);
    Ok(())
}

/// The showcase window and the label of its page's webview.
#[cfg(target_os = "macos")]
fn showcase_window<R: Runtime>(app: &AppHandle<R>) -> Result<(Window<R>, String), String> {
    let window = app.get_window(MAIN).ok_or("no showcase window")?;
    Ok((window, MAIN.to_owned()))
}

/// The native window and its webviews (`address -> label`).
#[cfg(target_os = "macos")]
fn window_views<R: Runtime>(window: &Window<R>) -> Result<(usize, native::Views), String> {
    let ns_window = window.ns_window().map_err(|e| e.to_string())? as usize;
    let webviews = window.webviews();
    let (tx, rx) = std::sync::mpsc::channel();
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

/// Runs `f` on the main thread and waits for its result.
#[cfg(target_os = "macos")]
fn on_main<R: Runtime, T: Send + 'static>(
    app: &AppHandle<R>,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(PROBE_WAIT).map_err(|e| e.to_string())
}

/// The lab's hit test (macOS): for each point (`[{name, x, y}]`, page CSS
/// pixels of the showcase webview) the view a click there would reach (the
/// app's webview, an ad guest, or another view). With `click` (test mode
/// only) it then sends one click at that point into the app's webview, and
/// only when the hit test names the app's webview. Other platforms answer
/// `{unsupported: true}`.
///
/// # Errors
///
/// Called from another webview, or no showcase window.
#[tauri::command]
pub async fn e2e_native_probe<R: Runtime>(
    webview: Webview<R>,
    points: Vec<Value>,
    click: Option<String>,
) -> Result<Value, String> {
    main_only(&webview, "e2e_native_probe")?;
    #[cfg(target_os = "macos")]
    {
        let app = webview.app_handle().clone();
        tauri::async_runtime::spawn_blocking(move || native_probe(&app, &points, click.as_deref()))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (points, click);
        Ok(json!({ "unsupported": true }))
    }
}

#[cfg(target_os = "macos")]
fn native_probe<R: Runtime>(
    app: &AppHandle<R>,
    raw: &[Value],
    click: Option<&str>,
) -> Result<Value, String> {
    let (window, embedder) = showcase_window(app)?;
    let (ns_window, views) = window_views(&window)?;
    let points: Vec<native::Point> = raw
        .iter()
        .filter_map(|p| {
            Some(native::Point {
                name: p.get("name")?.as_str()?.to_owned(),
                x: p.get("x")?.as_f64()?,
                y: p.get("y")?.as_f64()?,
            })
        })
        .collect();
    let (v, e, p) = (views.clone(), embedder.clone(), points.clone());
    let mut out = on_main(app, move || native::inspect(ns_window, &e, &v, &p))?;
    out["embedder"] = json!(embedder);
    if let Some(name) = click {
        let test_mode = config().get("mode").and_then(Value::as_str) == Some("test");
        out["click"] = match points.iter().find(|p| p.name == name) {
            // Synthetic input only in test mode, only into the app (lab rule).
            _ if !test_mode => json!({ "sent": false, "refused": "not in test mode" }),
            None => json!({ "sent": false, "refused": "no such point" }),
            Some(point) => {
                let point = point.clone();
                on_main(app, move || {
                    native::click(ns_window, &embedder, &views, &point)
                })?
            }
        };
    }
    Ok(out)
}

/// Writes a still of the showcase window (macOS): every webview of the
/// window renders its own content (`WKWebView` snapshots, no screen
/// capture), drawn bottom to top at its frame into one PNG,
/// `<stillsDir>/<name>.png`. Answers the path and each webview's frame.
/// Other platforms answer `{unsupported: true}`.
///
/// # Errors
///
/// Called from another webview, no `stillsDir` in the run configuration,
/// a name that is not plain, or the snapshot failed.
#[tauri::command]
pub async fn e2e_still<R: Runtime>(webview: Webview<R>, name: String) -> Result<Value, String> {
    main_only(&webview, "e2e_still")?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("name must be letters, digits and '-'".to_owned());
    }
    let dir: PathBuf = config()
        .get("stillsDir")
        .and_then(Value::as_str)
        .ok_or("no stillsDir in the run configuration")?
        .into();
    #[cfg(target_os = "macos")]
    {
        let app = webview.app_handle().clone();
        tauri::async_runtime::spawn_blocking(move || still(&app, &dir, &name))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = dir;
        Ok(json!({ "unsupported": true }))
    }
}

#[cfg(target_os = "macos")]
fn still<R: Runtime>(
    app: &AppHandle<R>,
    dir: &std::path::Path,
    name: &str,
) -> Result<Value, String> {
    let (window, _) = showcase_window(app)?;
    let (ns_window, views) = window_views(&window)?;
    let (tx, rx) = std::sync::mpsc::channel();
    for (address, label) in &views {
        let (tx, address, label) = (tx.clone(), *address, label.clone());
        app.run_on_main_thread(move || {
            native::shot(ns_window, address, label, move |s| {
                let _ = tx.send(s);
            });
        })
        .map_err(|e| e.to_string())?;
    }
    drop(tx);
    let mut shots = Vec::new();
    while shots.len() < views.len() {
        shots.push(rx.recv_timeout(PROBE_WAIT).map_err(|e| e.to_string())?);
    }
    let frames: Vec<Value> = shots
        .iter()
        .map(|s| {
            json!({
                "label": s.label, "rect": s.rect, "z": s.z, "hidden": s.hidden,
                "insetTop": s.inset_top,
                "bytes": s.png.len(), "error": s.error,
            })
        })
        .collect();
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // With OW_SHOWCASE_STILL_PARTS set, each webview's own snapshot is kept
    // next to the composite (`<name>.<label>.png`), to check the composite.
    if std::env::var_os("OW_SHOWCASE_STILL_PARTS").is_some() {
        for s in shots.iter().filter(|s| !s.png.is_empty()) {
            std::fs::write(dir.join(format!("{name}.{}.png", s.label)), &s.png)
                .map_err(|e| e.to_string())?;
        }
    }
    let png = on_main(app, move || {
        native::composite(ns_window, &shots, native::scale(ns_window))
    })??;
    let path = dir.join(format!("{name}.png"));
    std::fs::write(&path, png).map_err(|e| e.to_string())?;
    Ok(json!({ "path": path, "webviews": frames }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sink_is_loopback_or_closed() {
        assert_eq!(sink(None), CLOSED_SINK);
        assert_eq!(sink(Some("https://example.com")), CLOSED_SINK);
        assert_eq!(sink(Some("http://127.0.0.1.example.com")), CLOSED_SINK);
        assert_eq!(
            sink(Some("http://127.0.0.1:4100/")),
            "http://127.0.0.1:4100"
        );
        assert_eq!(sink(Some(" http://[::1]:5 ")), "http://[::1]:5");
    }

    #[test]
    fn every_host_request_goes_to_the_sink() {
        let e = endpoints("http://127.0.0.1:4100");
        let counter = e.rewrite(&format!(
            "{}?a=1",
            tauri_plugin_overwolf::analytics::COUNTER_URL
        ));
        assert_eq!(counter, "http://127.0.0.1:4100/analytics/Counter?a=1");
        for url in [
            tauri_plugin_overwolf::analytics::INSERT_STATS_URL,
            tauri_plugin_overwolf::analytics::CMP_EU_ONLY_URL,
        ] {
            assert!(
                e.rewrite(url).starts_with("http://127.0.0.1:4100/"),
                "{url}"
            );
        }
    }

    #[test]
    fn app_urls_hide_the_origin() {
        assert_eq!(
            app_relative("tauri://localhost/renderer/index.html#sizes"),
            "<app>/renderer/index.html#sizes"
        );
        assert_eq!(
            app_relative("http://tauri.localhost/renderer/index.html"),
            "<app>/renderer/index.html"
        );
        assert_eq!(app_relative("https://example.com/"), "https://example.com/");
    }
}
