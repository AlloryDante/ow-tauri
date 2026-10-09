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
//!   ([`e2e_config`]); without it the driver stays inert.

use std::io::Write as _;
use std::path::PathBuf;

use serde_json::{Value, json};
use tauri::{Manager, Runtime, Webview, Window};

use crate::sample::MAIN;

#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "Objective-C calls of the invisible lab")]
mod native;

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

/// Only the sample webview, where the driver runs, may call the lab
/// commands (an ad guest cannot reach app commands at all).
fn main_only(label: &str, command: &str) -> Result<(), String> {
    if label == MAIN {
        Ok(())
    } else {
        Err(format!("{command} is for the sample webview only"))
    }
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
