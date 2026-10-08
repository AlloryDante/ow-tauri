//! The invisible lab (Cargo feature `lab`, off by default; see
//! `e2e/README.md`). It never ships: `e2e/run.mjs` builds the app with it,
//! and nothing happens unless the environment asks for it.
//!
//! - `OW_TAURI_LAB_INVISIBLE=1` (the plugin's lab mode): every window is
//!   invisible before it can appear, dialogs and the file manager do not
//!   open, and this shell keeps the app out of the Dock, the app switcher
//!   and the front.
//! - `OW_TAURI_LAB_PACKAGE_JSON=<file>`: the app manifest to run with instead
//!   of the embedded one.
//! - `OW_TAURI_LAB_DIR=<dir>`: the plugin's trace, plus the driver's records
//!   (`e2e.jsonl`) through [`e2e_record`].
//! - `OW_SHOWCASE_E2E_CONFIG=<json>`: the driver's run configuration
//!   ([`e2e_config`]); without it the driver stays inert. Its `stillsDir`
//!   is where [`e2e_still`] writes; its `mode` gates [`e2e_native_probe`]'s
//!   click (test mode only).
//!
//! On macOS the driver also gets the window's native hit test and an
//! in-process still of the window ([`native`]): no screen capture, and no
//! input to an ad guest.

use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::time::Duration;

use serde_json::{Value, json};
#[cfg(target_os = "macos")]
use tauri::Manager;
use tauri::{Runtime, Webview};
use tauri_plugin_overwolf::OverwolfExt;

#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "Objective-C calls of the native lab probes")]
mod native;

/// How long a native probe or snapshot may take.
#[cfg(target_os = "macos")]
const PROBE_WAIT: Duration = Duration::from_secs(10);

/// The manifest JSON to run with: the `package.json` named by
/// `OW_TAURI_LAB_PACKAGE_JSON` when it is set, else `embedded`.
///
/// # Errors
///
/// The file cannot be read or is not a valid app manifest.
pub fn manifest(embedded: &'static str) -> Result<&'static str, String> {
    let Some(path) = std::env::var_os("OW_TAURI_LAB_PACKAGE_JSON").filter(|p| !p.is_empty()) else {
        return Ok(embedded);
    };
    let text = std::fs::read_to_string(&path).map_err(|e| format!("lab package.json: {e}"))?;
    let parsed = tauri_plugin_overwolf::manifest::parse_package_json(&text)
        .map_err(|e| format!("lab package.json: {e}"))?;
    let json = serde_json::to_string(&parsed.manifest).map_err(|e| e.to_string())?;
    // Read once per process; the plugin keeps the manifest for its lifetime.
    Ok(Box::leak(json.into_boxed_str()))
}

/// Whether the app must stay out of the Dock, the app switcher and the front
/// (macOS: the shell then makes it an accessory app that is not activated at
/// launch; elsewhere the plugin keeps the lab windows hidden).
#[cfg(target_os = "macos")]
#[must_use]
pub fn invisible() -> bool {
    tauri_plugin_overwolf::lab_invisible()
}

/// The driver's run configuration (`OW_SHOWCASE_E2E_CONFIG`), or `null`
/// when the runner did not launch the app. Main webview only.
///
/// # Errors
///
/// Called from another webview, or the variable is not valid JSON.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri passes command arguments by value"
)]
pub fn e2e_config<R: Runtime>(webview: Webview<R>) -> Result<Value, String> {
    if webview.label() != "ow-main" {
        return Err("e2e_config is for the main webview only".to_owned());
    }
    match std::env::var("OW_SHOWCASE_E2E_CONFIG") {
        Ok(text) if !text.is_empty() => {
            serde_json::from_str(&text).map_err(|e| format!("OW_SHOWCASE_E2E_CONFIG: {e}"))
        }
        _ => Ok(Value::Null),
    }
}

/// Appends one record of the driver to `e2e.jsonl` in the lab directory.
/// Only the main webview (`ow-main`), where the driver runs, may call it.
///
/// # Errors
///
/// Called from another webview.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri passes command arguments by value"
)]
pub fn e2e_record<R: Runtime>(webview: Webview<R>, entry: Value) -> Result<(), String> {
    if webview.label() != "ow-main" {
        return Err("e2e_record is for the main webview only".to_owned());
    }
    webview.overwolf().lab_record("e2e.jsonl", entry);
    Ok(())
}

/// Only the main webview (`ow-main`), where the driver runs, may call the
/// lab commands.
fn main_only<R: Runtime>(webview: &Webview<R>, command: &str) -> Result<(), String> {
    if webview.label() == "ow-main" {
        Ok(())
    } else {
        Err(format!("{command} is for the main webview only"))
    }
}

/// The run configuration, or `Null`.
fn config() -> Value {
    std::env::var("OW_SHOWCASE_E2E_CONFIG")
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

/// Asks the plugin's lab to read every ad guest's page now
/// (`guest-<n>-<phase>.json` in the lab directory: what the page sees and the
/// URLs it and its same-origin frames requested, for the live fill check).
///
/// # Errors
///
/// Called from another webview, or `phase` is not a plain name.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri passes command arguments by value"
)]
pub fn e2e_probe_guests<R: Runtime>(webview: Webview<R>, phase: String) -> Result<(), String> {
    main_only(&webview, "e2e_probe_guests")?;
    if phase.is_empty() || !phase.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("phase must be letters, digits and '-'".to_owned());
    }
    webview.overwolf().lab_probe_guests(&phase);
    Ok(())
}

/// The showcase window: the one whose webview shows `renderer/index.html`.
#[cfg(target_os = "macos")]
fn showcase_window<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(tauri::Window<R>, String), String> {
    for webview in app.webviews().into_values() {
        let shows = webview
            .url()
            .is_ok_and(|u| u.path().ends_with("/renderer/index.html"));
        if shows {
            return Ok((webview.window(), webview.label().to_owned()));
        }
    }
    Err("no showcase window".to_owned())
}

/// The native window and its webviews (`address -> label`).
#[cfg(target_os = "macos")]
fn window_views<R: Runtime>(window: &tauri::Window<R>) -> Result<(usize, native::Views), String> {
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
    app: &tauri::AppHandle<R>,
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
    app: &tauri::AppHandle<R>,
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
    app: &tauri::AppHandle<R>,
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
