//! Start and exit on the mock runtime (DESIGN §4.2, §4.3, §7.2), observed
//! through a capturing analytics transport (`Builder::analytics_transport`):
//! the launch burst starts at `RunEvent::Ready` in order, a Rust call in
//! setup suppresses it (R10), `ExitRequested` is never prevented by the
//! plugin, the exit drain runs once across `RunEvent::Exit` and the restart
//! sentinel, and excluded windows never count.
//!
//! `cmp-eu-only` never answers here, so no consent window opens.

#![allow(clippy::unwrap_used, reason = "a test fails on any unexpected error")]

mod common;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tauri::test::MockRuntime;
use tauri::{App, RunEvent, WebviewUrl};
use tauri_plugin_overwolf::{Builder, OverwolfExt};

use common::{Capture, event_name, lock, wait_until};

/// The window-closed Counter.
const WINDOW_CLOSED: &str = "tauri_window_closed";

/// A fixture app sending through `capture`, with the config windows
/// `titles` (their titles feed `window_closed`); the app windows are
/// created by the caller.
fn fixture(
    name: &str,
    capture: &Arc<Capture>,
    extra: &Value,
    titles: &[(&str, &str)],
    plugin: Builder,
) -> (App<MockRuntime>, std::path::PathBuf) {
    let mut config = json!({ "analytics": { "muidStrategy": "per-install" } });
    if let (Some(c), Some(e)) = (config.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            c.insert(k.clone(), v.clone());
        }
    }
    let (context, dir) = common::context(name, &config, titles);
    let app = common::build(context, plugin.analytics_transport(capture.clone()), None);
    (app, dir)
}

/// Every file name under `dir`, recursively.
fn files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(files(&path));
        } else {
            out.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    out
}

/// Waits until a counted window was seen visible (the first-visible
/// heartbeat), then for the 1 s a visible period needs before its end
/// sends `window_closed` (E.2 #7). No event marks that second, so this is
/// the one place a test waits for time itself.
fn visible_for_a_second(capture: &Capture) {
    assert!(
        wait_until(Duration::from_secs(15), || !visible_heartbeats(capture)
            .is_empty()),
        "a window was seen visible"
    );
    std::thread::sleep(Duration::from_millis(1100));
}

/// The heartbeats that report a visible window.
fn visible_heartbeats(capture: &Capture) -> Vec<Value> {
    capture
        .extras("tauri_app_heartbeat")
        .into_iter()
        .filter(|e| e["hasVisibleWindow"] == true)
        .collect()
}

/// The titles of the `window_closed` Counters sent so far, sorted.
fn closed_titles(capture: &Capture) -> Vec<String> {
    let mut titles: Vec<String> = capture
        .extras(WINDOW_CLOSED)
        .iter()
        .map(|e| e["title"].as_str().unwrap_or_default().to_owned())
        .collect();
    titles.sort();
    titles
}

/// DESIGN §4.2: setup writes and sends nothing; at `RunEvent::Ready` the
/// first-launch burst goes out in order (first launch, start, the hidden
/// heartbeat, 400022, 400023), `cmp-eu-only` once beside it, and the first
/// state writes happen.
#[test]
fn the_launch_burst_starts_at_ready_in_order() {
    let capture = Capture::hanging_eu_only();
    let (app, dir) = fixture("burst", &capture, &json!({}), &[], Builder::new());
    common::window(&app, "main", WebviewUrl::default());
    assert!(files(&dir).is_empty(), "setup wrote {:?}", files(&dir));
    assert!(capture.requests().is_empty(), "setup sent a request");
    let worker_capture = Arc::clone(&capture);
    common::run(app, move |_| {
        assert!(
            wait_until(Duration::from_secs(15), || {
                worker_capture.analytics().len() >= 5 && worker_capture.eu_only_count() == 1
            }),
            "the burst went out: {:?}",
            worker_capture
                .analytics()
                .iter()
                .map(event_name)
                .collect::<Vec<_>>()
        );
    });
    let names: Vec<String> = capture.analytics().iter().map(event_name).collect();
    assert_eq!(
        names[..5],
        [
            "tauri_app_first_launch",
            "tauri_app_start",
            "tauri_app_heartbeat",
            "InsertStats 400022",
            "InsertStats 400023",
        ],
        "{names:?}"
    );
    assert_eq!(
        capture.extras("tauri_app_heartbeat")[0]["hasVisibleWindow"],
        false,
        "the burst's heartbeat comes before any window is seen"
    );
    // After the burst: the window analytics only.
    for name in &names[5..] {
        assert!(
            matches!(
                name.as_str(),
                "tauri_app_heartbeat" | "InsertStats 400023" | WINDOW_CLOSED
            ),
            "unexpected request after the burst: {name} in {names:?}"
        );
    }
    assert_eq!(capture.eu_only_count(), 1);
    assert!(
        files(&dir).iter().any(|f| f == "ow-tauri.json"),
        "the first writes happen at Ready: {:?}",
        files(&dir)
    );
}

/// R10: `disableAnonymousAnalytics()` called from Rust before
/// `RunEvent::Ready` (the app's setup) leaves only the mandatory set in the
/// burst: no `app_start`, no 400022.
#[test]
fn a_rust_call_before_ready_suppresses_the_launch_burst() {
    let capture = Capture::hanging_eu_only();
    let (app, _dir) = fixture("burst-r10", &capture, &json!({}), &[], Builder::new());
    common::window(&app, "main", WebviewUrl::default());
    app.overwolf().disable_anonymous_analytics();
    let worker_capture = Arc::clone(&capture);
    common::run(app, move |_| {
        // The first-visible heartbeat (mandatory) follows the burst.
        assert!(
            wait_until(Duration::from_secs(15), || !visible_heartbeats(
                &worker_capture
            )
            .is_empty()),
            "the first-visible heartbeat went out"
        );
    });
    let names: Vec<String> = capture.analytics().iter().map(event_name).collect();
    assert_eq!(
        names[..3],
        [
            "tauri_app_first_launch",
            "tauri_app_heartbeat",
            "InsertStats 400023",
        ],
        "{names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|n| n == "tauri_app_start" || n == "InsertStats 400022" || n == WINDOW_CLOSED),
        "only the mandatory set: {names:?}"
    );
}

/// D2: the plugin never prevents `ExitRequested` and never exits the app.
/// A tray app prevents the exit when its last window closes and keeps
/// running (a window opened later works); the exit drain runs once, at the
/// real exit.
#[test]
fn exit_requested_is_never_prevented_and_a_tray_app_keeps_running() {
    let capture = Capture::hanging_eu_only();
    let (app, _dir) = fixture(
        "tray",
        &capture,
        &json!({}),
        &[("main", "Main"), ("later", "Later")],
        Builder::new(),
    );
    common::window(&app, "main", WebviewUrl::default());
    let exit_requests = Arc::new(AtomicUsize::new(0));
    let cleared_after_prevent = Arc::new(AtomicUsize::new(0));
    let closed_while_running = Arc::new(Mutex::new(Vec::new()));
    let (requests, cleared, closed, watched) = (
        Arc::clone(&exit_requests),
        Arc::clone(&cleared_after_prevent),
        Arc::clone(&closed_while_running),
        Arc::clone(&capture),
    );
    let worker_capture = Arc::clone(&capture);
    common::run_with(
        app,
        move |_| visible_for_a_second(&worker_capture),
        move |handle, event| match event {
            RunEvent::ExitRequested { api, .. } if requests.fetch_add(1, Ordering::SeqCst) == 0 => {
                // The last window closed; the tray app stays.
                api.prevent_exit();
                lock(&closed).push(watched.extras(WINDOW_CLOSED).len());
                common::window(handle, "later", WebviewUrl::default());
            }
            RunEvent::MainEventsCleared if requests.load(Ordering::SeqCst) == 1 => {
                lock(&closed).push(watched.extras(WINDOW_CLOSED).len());
                if cleared.fetch_add(1, Ordering::SeqCst) == 0 {
                    common::close_all(handle);
                }
            }
            _ => {}
        },
    );
    assert_eq!(
        exit_requests.load(Ordering::SeqCst),
        2,
        "the second exit request ended the app"
    );
    assert!(
        cleared_after_prevent.load(Ordering::SeqCst) >= 1,
        "the app kept running"
    );
    assert!(
        lock(&closed_while_running).iter().all(|n| *n == 0),
        "no exit drain while the app kept running: {:?}",
        lock(&closed_while_running)
    );
    assert_eq!(
        closed_titles(&capture)
            .iter()
            .filter(|t| *t == "Main")
            .count(),
        1,
        "one drain at the real exit: {:?}",
        closed_titles(&capture)
    );
}

/// PAR-M7: `AppHandle::restart()` skips `RunEvent::Exit` but clears the
/// app's resources first; the plugin's sentinel there runs the exit drain.
/// It runs once: a second clear, `prepare_for_restart()` and the final
/// `RunEvent::Exit` send nothing more.
#[test]
fn the_restart_sentinel_drains_once() {
    let capture = Capture::hanging_eu_only();
    let (app, _dir) = fixture(
        "sentinel",
        &capture,
        &json!({}),
        &[("main", "Main")],
        Builder::new(),
    );
    common::window(&app, "main", WebviewUrl::default());
    let worker_capture = Arc::clone(&capture);
    let counts = common::run(app, move |handle| {
        visible_for_a_second(&worker_capture);
        let before = worker_capture.extras(WINDOW_CLOSED).len();
        // What `AppHandle::restart()` runs before it restarts.
        handle.cleanup_before_exit();
        let drained = worker_capture.extras(WINDOW_CLOSED).len();
        handle.cleanup_before_exit();
        handle.overwolf().prepare_for_restart();
        let again = worker_capture.extras(WINDOW_CLOSED).len();
        (before, drained, again)
    });
    assert_eq!(
        counts,
        (0, 1, 1),
        "(before, after the sentinel, after a second exit path)"
    );
    assert_eq!(
        closed_titles(&capture),
        ["Main"],
        "RunEvent::Exit did not drain again"
    );
}

/// D6, §4.3.2: windows matching `analytics.excludeWindows` or
/// `Builder::exclude_windows` are tracked but never counted: their visible
/// periods send no `window_closed`, the others do (at the exit drain).
#[test]
fn excluded_windows_never_count() {
    let capture = Capture::hanging_eu_only();
    let (app, _dir) = fixture(
        "excluded",
        &capture,
        &json!({ "analytics": { "muidStrategy": "per-install", "excludeWindows": ["tray-*"] } }),
        &[
            ("main", "Main"),
            ("settings/panel", "Settings"),
            ("overlay:hud", "Overlay"),
            ("tray-menu", "Tray"),
        ],
        Builder::new().exclude_windows(["overlay:*"]),
    );
    for label in ["main", "settings/panel", "overlay:hud", "tray-menu"] {
        common::window(&app, label, WebviewUrl::default());
    }
    let worker_capture = Arc::clone(&capture);
    common::run(app, move |_| visible_for_a_second(&worker_capture));
    assert_eq!(closed_titles(&capture), ["Main", "Settings"]);
}
