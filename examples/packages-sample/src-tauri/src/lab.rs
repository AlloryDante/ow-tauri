//! The invisible lab (Cargo feature `lab`, off by default; see
//! `e2e/README.md`). It never ships: `e2e/run.mjs` builds a debug app with
//! it under the lab bundle id, and nothing happens unless the environment
//! asks for it.
//!
//! - `OW_TAURI_LAB_INVISIBLE=1` (the plugin's lab mode): the window is on
//!   screen at alpha 0, ignores the mouse and never becomes key; the app
//!   never activates and stays out of the Dock and the app switcher.
//! - `OW_TAURI_LAB_DIR=<dir>`: the plugin's trace, plus the driver's records
//!   (`e2e.jsonl`) through [`e2e_record`].
//! - `OW_SAMPLE_LAB_SINK=<http://127.0.0.1:port>`: where the plugin's
//!   analytics and the consent experiment go. Without it they go to a
//!   closed loopback port: a lab build never reports to Overwolf.
//! - `OW_SAMPLE_E2E_CONFIG=<json>`: the driver's run configuration
//!   ([`e2e_config`]); without it the driver stays inert. Its `stillsDir` is
//!   where [`e2e_still`] writes.
//! - `OW_SAMPLE_LAB_APPEARANCE=dark|light`: the system appearance of the
//!   app's pages (macOS), so the light and dark themes can both be recorded.
//! - `OW_SAMPLE_LAB_WINDOW=<width>x<height>`: the window's size (logical).
//! - `OW_SAMPLE_LAB_STILL=<file.png>`: page-host mode, for a page without the
//!   driver (the quickstart's, built into this shell by `e2e/run.mjs --page
//!   quickstart`): `OW_SAMPLE_LAB_STILL_AFTER_MS` (default 15000) after the
//!   page has loaded, a still of the window goes to that file, a `done`
//!   record to `e2e.jsonl`, and the app quits.
//!
//! On macOS the driver also gets in-process stills of a window
//! ([`e2e_still`]; no screen capture) and can put a consent window the
//! plugin keeps hidden in the lab on screen at alpha 0 ([`e2e_reveal`]), so
//! its page renders for a still.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use tauri::{AppHandle, Manager, Runtime, Webview, Window};

use crate::sample::MAIN;

#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "Objective-C calls of the invisible lab")]
mod native;

/// How long a snapshot of the window may take.
#[cfg(target_os = "macos")]
const PROBE_WAIT: Duration = Duration::from_secs(10);
/// How long page-host mode waits after the page has loaded, by default.
const STILL_AFTER: Duration = Duration::from_secs(15);

/// Where the plugin's host requests go without `OW_SAMPLE_LAB_SINK`: the
/// discard port on loopback (nothing listens, nothing leaves the machine).
const CLOSED_SINK: &str = "http://127.0.0.1:9";

/// Whether the app must stay invisible (`OW_TAURI_LAB_INVISIBLE=1`).
#[must_use]
pub fn invisible() -> bool {
    tauri_plugin_overwolf::lab_invisible()
}

/// The loopback base URL the plugin's analytics and consent experiment go
/// to: `OW_SAMPLE_LAB_SINK` when it is a loopback `http` URL, else
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
        update_feed: Some(format!("{base}/updates")),
    }
}

/// The lab's plugin builder: test ads follow `--test-ad` as usual; analytics,
/// the consent experiment and the update feed go to the loopback sink.
pub fn overwolf_builder(builder: tauri_plugin_overwolf::Builder) -> tauri_plugin_overwolf::Builder {
    let base = sink(std::env::var("OW_SAMPLE_LAB_SINK").ok().as_deref());
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

/// After the window is built, before it is shown (main thread): the
/// appearance the run asks for, and in the invisible lab alpha 0,
/// click-through and above other apps' windows.
pub fn prepare_window<R: Runtime>(window: &Window<R>) {
    #[cfg(target_os = "macos")]
    if let Ok(theme) = std::env::var("OW_SAMPLE_LAB_APPEARANCE") {
        native::set_appearance(theme.trim());
    }
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

/// A window size `<width>x<height>` (logical pixels, each 200 to 4000).
fn parse_size(text: &str) -> Option<(f64, f64)> {
    let (w, h) = text.trim().split_once('x')?;
    let (w, h): (u16, u16) = (w.parse().ok()?, h.parse().ok()?);
    let ok = |v: u16| (200..=4000).contains(&v);
    (ok(w) && ok(h)).then(|| (f64::from(w), f64::from(h)))
}

/// The window size of `OW_SAMPLE_LAB_WINDOW`, if set and valid.
#[must_use]
pub fn window_size() -> Option<(f64, f64)> {
    parse_size(&std::env::var("OW_SAMPLE_LAB_WINDOW").ok()?)
}

/// Page-host mode's wait (`OW_SAMPLE_LAB_STILL_AFTER_MS`, else
/// [`STILL_AFTER`]).
fn still_after(value: Option<&str>) -> Duration {
    value
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map_or(STILL_AFTER, Duration::from_millis)
}

/// Once the sample window's page has loaded: in page-host mode
/// (`OW_SAMPLE_LAB_STILL`), wait, write a still of the window, record
/// `done` and quit.
pub fn page_shown<R: Runtime>(app: &AppHandle<R>) {
    let Some(path) = std::env::var_os("OW_SAMPLE_LAB_STILL").filter(|p| !p.is_empty()) else {
        return;
    };
    let wait = still_after(
        std::env::var("OW_SAMPLE_LAB_STILL_AFTER_MS")
            .ok()
            .as_deref(),
    );
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        let path = PathBuf::from(path);
        let entry = match still(&app, MAIN, &path) {
            Ok(out) => json!({ "kind": "done", "still": out }),
            Err(error) => json!({ "kind": "fatal", "text": format!("still: {error}") }),
        };
        let _ = record(&entry);
        app.exit(0);
    });
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

/// Only the sample webview, where the driver runs, may call the lab
/// commands (an ad guest cannot reach app commands at all).
fn main_only(label: &str, command: &str) -> Result<(), String> {
    if label == MAIN {
        Ok(())
    } else {
        Err(format!("{command} is for the sample webview only"))
    }
}

/// The run configuration, or `Null`.
fn config() -> Value {
    std::env::var("OW_SAMPLE_E2E_CONFIG")
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

/// The run configuration `text` with `pid` added, or `null` for none.
fn with_pid(text: Option<&str>, pid: u32) -> Result<Value, String> {
    match text {
        Some(text) if !text.is_empty() => {
            let mut config: Value =
                serde_json::from_str(text).map_err(|e| format!("OW_SAMPLE_E2E_CONFIG: {e}"))?;
            if let Some(object) = config.as_object_mut() {
                object.insert("pid".to_owned(), json!(pid));
            }
            Ok(config)
        }
        _ => Ok(Value::Null),
    }
}

/// The driver's run configuration (`OW_SAMPLE_E2E_CONFIG`) with this
/// process's `pid` added, or `null` when the runner did not launch the app.
///
/// # Errors
///
/// Called from another webview, or the variable is not valid JSON.
#[tauri::command]
pub fn e2e_config<R: Runtime>(webview: Webview<R>) -> Result<Value, String> {
    main_only(webview.label(), "e2e_config")?;
    let text = std::env::var("OW_SAMPLE_E2E_CONFIG").ok();
    with_pid(text.as_deref(), std::process::id())
}

/// Appends one record of the driver to `<OW_TAURI_LAB_DIR>/e2e.jsonl`
/// (nothing without a lab directory).
///
/// # Errors
///
/// Called from another webview, or the file cannot be written.
#[tauri::command]
pub fn e2e_record<R: Runtime>(webview: Webview<R>, entry: Value) -> Result<(), String> {
    main_only(webview.label(), "e2e_record")?;
    record(&entry)
}

/// Appends `entry` to `<OW_TAURI_LAB_DIR>/e2e.jsonl` (nothing without a lab
/// directory).
fn record(entry: &Value) -> Result<(), String> {
    let Some(dir) = std::env::var_os("OW_TAURI_LAB_DIR").filter(|d| !d.is_empty()) else {
        return Ok(());
    };
    let mut line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(PathBuf::from(dir).join("e2e.jsonl"))
        .map_err(|e| e.to_string())?;
    file.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

/// The state of the sample window as the OS reports it (the plugin polls
/// the same to tell the ad guests whether they are visible).
///
/// # Errors
///
/// Called from another webview.
#[tauri::command]
pub fn e2e_window<R: Runtime>(webview: Webview<R>) -> Result<Value, String> {
    main_only(webview.label(), "e2e_window")?;
    let window = webview.window();
    Ok(json!({
        "visible": window.is_visible().unwrap_or(false),
        "minimized": window.is_minimized().unwrap_or(false),
        "focused": window.is_focused().unwrap_or(false),
    }))
}

/// Quits the app as a user would (the plugin drains its analytics at exit).
///
/// # Errors
///
/// Called from another webview.
#[tauri::command]
pub fn e2e_quit<R: Runtime>(webview: Webview<R>) -> Result<(), String> {
    main_only(webview.label(), "e2e_quit")?;
    webview.app_handle().exit(0);
    Ok(())
}

/// A still name: 1 to 60 of letters, digits and `-`.
fn plain_name(name: &str) -> bool {
    (1..=60).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A consent window of the plugin (`ow-cmp`, `ow-cmp-startup-2`, …): the
/// only windows [`e2e_reveal`] and [`e2e_still`] take besides the sample's.
fn consent_window(label: &str) -> bool {
    label == "ow-cmp" || label.starts_with("ow-cmp-")
}

/// Writes a still of window `window` (default the sample window; else a
/// consent window) to `<stillsDir>/<name>.png` (macOS): every webview of the
/// window renders its own content (`WKWebView` snapshots, no screen
/// capture), drawn bottom to top at its frame into one PNG. Answers the path
/// and each webview's frame. Other platforms answer `{unsupported: true}`.
///
/// # Errors
///
/// Called from another webview, no `stillsDir` in the run configuration, a
/// name that is not plain, a window that is not the sample's or a consent
/// window, or the snapshot failed.
#[tauri::command]
pub async fn e2e_still<R: Runtime>(
    webview: Webview<R>,
    name: String,
    window: Option<String>,
) -> Result<Value, String> {
    main_only(webview.label(), "e2e_still")?;
    if !plain_name(&name) {
        return Err("name must be 1 to 60 letters, digits and '-'".to_owned());
    }
    let label = window.unwrap_or_else(|| MAIN.to_owned());
    if label != MAIN && !consent_window(&label) {
        return Err("only the sample window or a consent window".to_owned());
    }
    let dir: PathBuf = config()
        .get("stillsDir")
        .and_then(Value::as_str)
        .ok_or("no stillsDir in the run configuration")?
        .into();
    let app = webview.app_handle().clone();
    let path = dir.join(format!("{name}.png"));
    tauri::async_runtime::spawn_blocking(move || still(&app, &label, &path))
        .await
        .map_err(|e| e.to_string())?
}

/// Puts the consent window `label`, which the plugin keeps hidden in the
/// lab, on screen at alpha 0 like the sample window (never key, the app not
/// activated), so its page renders for [`e2e_still`]. Answers its page URL
/// without the query (which carries ids).
///
/// # Errors
///
/// Called from another webview, not a consent window, or no such window
/// yet.
#[tauri::command]
pub fn e2e_reveal<R: Runtime>(webview: Webview<R>, label: String) -> Result<Value, String> {
    main_only(webview.label(), "e2e_reveal")?;
    if !consent_window(&label) {
        return Err("only a consent window".to_owned());
    }
    let window = webview
        .app_handle()
        .get_window(&label)
        .ok_or_else(|| format!("no window {label}"))?;
    let url = window
        .webviews()
        .first()
        .and_then(|w| w.url().ok())
        .map(|mut u| {
            u.set_query(None);
            u.to_string()
        });
    if invisible() {
        #[cfg(target_os = "macos")]
        if let Ok(ns_window) = window.ns_window() {
            let address = ns_window as usize;
            window
                .run_on_main_thread(move || {
                    native::prepare_invisible(address);
                    native::order_front(address);
                })
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(json!({ "label": label, "url": url }))
}

/// The still of window `label` written to `path` (see [`e2e_still`]).
#[cfg(target_os = "macos")]
fn still<R: Runtime>(app: &AppHandle<R>, label: &str, path: &Path) -> Result<Value, String> {
    let window = app
        .get_window(label)
        .ok_or_else(|| format!("no window {label}"))?;
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
        shots.push(
            rx.recv_timeout(PROBE_WAIT)
                .map_err(|_| "a webview snapshot did not finish in time".to_owned())?,
        );
    }
    let frames: Vec<Value> = shots
        .iter()
        .map(|s| {
            json!({
                "label": s.label, "rect": s.rect, "z": s.z, "hidden": s.hidden,
                "bytes": s.png.len(), "error": s.error,
            })
        })
        .collect();
    if !shots.iter().any(|s| !s.png.is_empty()) {
        return Err(format!("no webview of {label} rendered: {frames:?}"));
    }
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(native::composite(
            ns_window,
            &shots,
            native::scale(ns_window),
        ));
    })
    .map_err(|e| e.to_string())?;
    let png = rx.recv_timeout(PROBE_WAIT).map_err(|e| e.to_string())??;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, png).map_err(|e| e.to_string())?;
    Ok(json!({ "path": path, "webviews": frames }))
}

/// Stills are macOS only.
#[cfg(not(target_os = "macos"))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the same signature as the macOS still"
)]
fn still<R: Runtime>(app: &AppHandle<R>, label: &str, path: &Path) -> Result<Value, String> {
    let _ = (app, label, path);
    Ok(json!({ "unsupported": true }))
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
        assert_eq!(sink(Some("http://localhost:8/")), "http://localhost:8");
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
        assert_eq!(
            e.update_feed.as_deref(),
            Some("http://127.0.0.1:4100/updates")
        );
    }

    #[test]
    fn lab_commands_are_for_the_sample_webview_only() {
        assert!(main_only(MAIN, "e2e_quit").is_ok());
        assert_eq!(
            main_only("owad-1", "e2e_quit"),
            Err("e2e_quit is for the sample webview only".to_owned())
        );
    }

    #[test]
    fn window_sizes_are_width_by_height_in_range() {
        assert_eq!(parse_size("1200x800"), Some((1200.0, 800.0)));
        assert_eq!(parse_size(" 800x600 "), Some((800.0, 600.0)));
        for bad in [
            "",
            "1200",
            "x800",
            "1200x",
            "100x800",
            "1200x5000",
            "a x b",
            "-1x800",
        ] {
            assert_eq!(parse_size(bad), None, "{bad}");
        }
    }

    #[test]
    fn page_host_waits_15_s_unless_told() {
        assert_eq!(still_after(None), STILL_AFTER);
        assert_eq!(still_after(Some("2500")), Duration::from_millis(2500));
        assert_eq!(still_after(Some("soon")), STILL_AFTER);
    }

    #[test]
    fn stills_take_plain_names_of_the_sample_or_consent_windows() {
        assert!(plain_name("settings-dark"));
        assert!(plain_name("p1"));
        for bad in ["", "../x", "a/b", "a.png", "a b", &"x".repeat(61)] {
            assert!(!plain_name(bad), "{bad}");
        }
        for ok in [
            "ow-cmp",
            "ow-cmp-startup",
            "ow-cmp-startup-2",
            "ow-cmp-default",
        ] {
            assert!(consent_window(ok), "{ok}");
        }
        for bad in ["main", "owad-1", "ow-cmpx", "settings"] {
            assert!(!consent_window(bad), "{bad}");
        }
    }

    #[test]
    fn the_run_configuration_carries_the_pid() {
        assert_eq!(with_pid(None, 7), Ok(Value::Null));
        assert_eq!(with_pid(Some(""), 7), Ok(Value::Null));
        assert_eq!(
            with_pid(Some(r#"{"adWaitMs":5}"#), 7),
            Ok(json!({ "adWaitMs": 5, "pid": 7 }))
        );
        assert!(
            with_pid(Some("{"), 7).is_err_and(|e| e.starts_with("OW_SAMPLE_E2E_CONFIG: ")),
            "invalid JSON is refused"
        );
    }
}
