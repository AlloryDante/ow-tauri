//! The run, as ow-electron's harness app runs it (`app/main.cjs`,
//! `app/scenario.cjs`): the same `app.overwolf` snapshots and calls, the
//! same ad window and harness page, the same timed actions, the same quit
//! flow, on plain Tauri windows and the plugin's Rust and JavaScript APIs.
//!
//! Calls an ow-electron app makes in its main process are made here with the
//! plugin's Rust API (`app.overwolf()`); once the ad window's page is up,
//! the scenario's `ow-call` actions go through the JavaScript API
//! (`tauri-plugin-overwolf-api`) in that page, as a Tauri app's page calls
//! it. Every action ow-electron's harness runs that has no Tauri counterpart
//! is recorded as `action-unsupported` (`actions.jsonl`), which
//! `parity-diff.mjs` reports.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use sha2::Digest as _;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, Wry};
use tauri_plugin_overwolf::{CmpWindowOptions, EmailHashes, OverwolfExt};

use crate::harness::{self, lock, wait};
use crate::{observe, windows};

/// How long a call into the harness page may take.
const PAGE_WAIT: Duration = Duration::from_secs(20);

/// The functions of the JavaScript API (`tauri-plugin-overwolf-api`), as
/// the harness page reports them (`harness_page_info`).
static API_SURFACE: Mutex<Option<Value>> = Mutex::new(None);

/// The harness page's user agent (`navigator.userAgent`).
static PAGE_UA: Mutex<Option<String>> = Mutex::new(None);

/// Set when the run quits: the next exit request is not prevented.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// Live ad loads so far, and whether the cap stopped them.
static LIVE_LOADS: AtomicU64 = AtomicU64::new(0);
static LIVE_STOPPED: AtomicBool = AtomicBool::new(false);
static SEEN_GUESTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Windows opened by `open-window` (key -> label), and how many so far.
static EXTRA: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
static EXTRA_COUNT: AtomicU64 = AtomicU64::new(0);

/// Consent windows adopted by a `cmp-open` action (window label -> action label).
static CMP_WINDOWS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// Node's `process.platform` for this OS.
fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// Node's `process.arch` for this CPU.
fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        "x86" => "ia32",
        other => other,
    }
}

/// `{type, value}` as ow-electron's harness describes a data member.
fn member(value: &Value) -> Value {
    let kind = match value {
        Value::Null => return json!({ "type": "undefined", "value": null }),
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) | Value::Object(_) => "object",
    };
    json!({ "type": kind, "value": value })
}

/// The `overwolf.json` snapshot of the Tauri app: the data members ow-electron
/// exposes on `app.overwolf` (`uid`, `muid`, `phasePercent`, `utmParams`)
/// read through the plugin's Rust API (the JavaScript API's functions are
/// the file's `apiSurface`). ow-tauri sets no `OVERWOLF_APP_UID`
/// environment variable.
fn describe_overwolf(app: &AppHandle<Wry>) -> Value {
    let ow = app.overwolf();
    let info = serde_json::to_value(ow.info()).unwrap_or(Value::Null);
    let mut members = Map::new();
    members.insert("muid".into(), member(&json!(ow.muid())));
    for key in ["phasePercent", "uid", "utmParams"] {
        members.insert(key.into(), member(info.get(key).unwrap_or(&Value::Null)));
    }
    json!({
        "present": true,
        "surface": "tauri-plugin-overwolf-api",
        "env": {},
        "members": members,
    })
}

/// The fixed facts of `overwolf.json`.
fn facts(app: &AppHandle<Wry>) -> Map<String, Value> {
    let info = app.package_info();
    let path = app.path();
    let dir = |p: tauri::Result<PathBuf>| p.ok().map(|p| p.display().to_string());
    let mut out = Map::new();
    out.insert("appName".into(), json!(info.name));
    out.insert("appVersion".into(), json!(info.version.to_string()));
    out.insert(
        "versions".into(),
        json!({ "tauri": tauri::VERSION, "webview": tauri::webview_version().ok() }),
    );
    out.insert("platform".into(), json!(node_platform()));
    out.insert("arch".into(), json!(node_arch()));
    out.insert("argv".into(), json!(std::env::args().collect::<Vec<_>>()));
    out.insert("userAgentFallback".into(), json!(lock(&PAGE_UA).clone()));
    // The JavaScript API's functions, once the harness page has loaded.
    out.insert("apiSurface".into(), json!(lock(&API_SURFACE).clone()));
    out.insert(
        "paths".into(),
        json!({
            "home": dir(path.home_dir()),
            "appData": dir(path.data_dir()),
            "userData": dir(path.app_data_dir()),
            "logs": dir(path.app_log_dir()),
            "temp": dir(path.temp_dir()),
        }),
    );
    out
}

/// Adds a snapshot (`overwolf.json`).
pub fn snapshot(app: &AppHandle<Wry>, label: &str) {
    let h = harness::get();
    lock(&h.overwolf).facts = facts(app);
    h.push_snapshot(label, describe_overwolf(app));
}

/// Records one call (`overwolf.json` `calls`).
fn call(label: &str, f: impl FnOnce() -> Result<Value, String>) {
    let h = harness::get();
    let started = Instant::now();
    let t = h.t();
    let result = f();
    let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    h.push_call(match result {
        Ok(result) => json!({ "label": label, "ok": true, "result": result, "ms": ms, "t": t }),
        Err(error) => json!({ "label": label, "ok": false, "error": error, "ms": ms, "t": t }),
    });
}

/// An action ow-electron's harness runs that this app cannot mirror.
fn unsupported(action: &Value, why: &str) {
    harness::get().record(
        "actions.jsonl",
        json!({
            "phase": "action-unsupported",
            "do": action.get("do"),
            "label": action.get("label"),
            "host": "tauri",
            "why": why,
        }),
    );
}

/// Before the app runs (ow-electron: the main script's module load).
pub fn setup(app: &AppHandle<Wry>) {
    let h = harness::get();
    snapshot(app, "module-load");
    if h.flag("disableAnalytics") {
        app.overwolf().disable_anonymous_analytics();
        h.record(
            "events.jsonl",
            json!({ "kind": "disableAnonymousAnalytics", "when": "module-load" }),
        );
    }
    observe::start(app);
}

/// The app's run loop events.
pub fn on_event(app: &AppHandle<Wry>, event: &RunEvent) {
    let h = harness::get();
    match event {
        RunEvent::Ready => {
            h.record("events.jsonl", json!({ "kind": "app", "event": "ready" }));
            let app = app.clone();
            std::thread::spawn(move || {
                if h.flag("probeOnly") {
                    probe_only(&app);
                } else {
                    full_run(&app);
                }
            });
        }
        RunEvent::ExitRequested { code, api, .. } => {
            if code.is_some() || QUITTING.load(Ordering::SeqCst) {
                h.record(
                    "events.jsonl",
                    json!({ "kind": "app", "event": "before-quit" }),
                );
                return;
            }
            // The last window closed.
            h.record(
                "events.jsonl",
                json!({ "kind": "app", "event": "window-all-closed" }),
            );
            if h.flag("quitOnAllClosed") {
                h.record(
                    "events.jsonl",
                    json!({ "kind": "quit-on-all-closed", "windows": open_windows(app) }),
                );
                QUITTING.store(true, Ordering::SeqCst);
            } else {
                // Keep running until the timed quit, like an app with a
                // tray icon would.
                api.prevent_exit();
            }
        }
        RunEvent::Exit => {
            h.record("events.jsonl", json!({ "kind": "app", "event": "quit" }));
        }
        _ => {}
    }
}

/// The windows open now (label, URL).
fn open_windows(app: &AppHandle<Wry>) -> Vec<Value> {
    app.windows()
        .into_values()
        .map(|w| json!({ "label": w.label(), "url": windows::known_url(w.label()) }))
        .collect()
}

/// `probeOnly`: two snapshots, then exit.
fn probe_only(app: &AppHandle<Wry>) {
    let h = harness::get();
    snapshot(app, "ready");
    std::thread::sleep(Duration::from_millis(h.num("probeDelayMs").unwrap_or(1500)));
    snapshot(app, "ready+delay");
    QUITTING.store(true, Ordering::SeqCst);
    app.exit(0);
}

/// What the app does to its window right after building it: its Overwolf
/// window name (`windowName`), a new title (`setupTitle`,
/// title-set-in-setup), its close handler (`closeHandler`) and a timed
/// close (`closeMainWindowAtMs`).
fn set_up_main_window(app: &AppHandle<Wry>, window: &tauri::Window<Wry>) {
    let h = harness::get();
    if let Some(name) = h.str_opt("windowName")
        && let Err(error) = app.overwolf().set_window_name(windows::MAIN, name)
    {
        h.record(
            "events.jsonl",
            json!({ "kind": "set-window-name-failed", "error": error.to_string() }),
        );
    }
    if let Some(title) = h.str_opt("setupTitle") {
        let _ = window.set_title(title);
    }
    if let Some(handler) = h.str_opt("closeHandler") {
        install_close_handler(window, handler);
    }
    if let Some(after) = h.num("closeMainWindowAtMs") {
        schedule_main_close(app, after);
    }
}

/// Whether the run is quitting.
pub fn quitting() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

/// `closeHandler`: the app's own handler of a close request on its window,
/// as a tray app writes it (§5.2 #6, #7). Off while the run quits.
///
/// - `tray`: prevent, then hide;
/// - `delay-destroy`: prevent, then destroy 500 ms later;
/// - `confirm-5s`: prevent, then show the window again 5 s later;
/// - `tray-js`: the harness page's `onCloseRequested` does it (`harness.js`).
fn install_close_handler(window: &tauri::Window<Wry>, handler: &str) {
    if handler == "tray-js" {
        return;
    }
    let handler = handler.to_owned();
    let target = window.clone();
    window.on_window_event(move |event| {
        let tauri::WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };
        if quitting() {
            return;
        }
        api.prevent_close();
        harness::get().record(
            "events.jsonl",
            json!({ "kind": "close-handled", "handler": handler }),
        );
        let target = target.clone();
        match handler.as_str() {
            "delay-destroy" => {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(500));
                    let _ = target.destroy();
                });
            }
            "confirm-5s" => {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(5));
                    windows::show_inactive(&target);
                });
            }
            _ => {
                let _ = target.hide();
            }
        }
    });
}

fn full_run(app: &AppHandle<Wry>) {
    let h = harness::get();
    snapshot(app, "ready");
    start_ticks();
    if h.flag("calibrate") {
        // ow-electron's harness proves its JS guard runs before a window can
        // show; this app builds every window hidden and shows it only at
        // alpha 0 (windows::show_inactive).
        h.record(
            "windows.jsonl",
            json!({
                "kind": "calibration",
                "host": "tauri",
                "handlerPrecedesOptions": null,
                "note": "harness windows are built hidden and shown only at alpha 0; plugin windows stay hidden (lab)",
            }),
        );
    }
    if !h.flag("skipStartupCalls") {
        let ow = app.overwolf();
        call("isCMPRequired", || {
            Ok(json!(tauri::async_runtime::block_on(ow.is_cmp_required())))
        });
        // ow-tauri has no package manager (`app.overwolf.packages`).
        let mut names = vec!["packages.hasPendingUpdates"];
        if h.config
            .get("packages")
            .and_then(Value::as_array)
            .is_some_and(|p| !p.is_empty())
        {
            names.extend(["packages.getChannel", "packages.getAvailableChannels"]);
        }
        for name in names {
            unsupported(
                &json!({ "do": "ow-call", "label": name }),
                "ow-tauri has no package manager",
            );
        }
    }
    snapshot(app, "after-calls");
    start_window_and_actions(app);
}

fn start_ticks() {
    let Some(every) = harness::get().num("tickMs").filter(|&ms| ms > 0) else {
        return;
    };
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(every));
            harness::get().record("ticks.jsonl", json!({ "wall": harness::wall_ms() }));
        }
    });
}

/// The query of the harness page (`index.html`), as ow-electron's harness
/// builds it.
fn page_query() -> String {
    let h = harness::get();
    let layouts = h
        .config
        .get("layouts")
        .and_then(Value::as_array)
        .map(|l| {
            l.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(",")
        })
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "none".into());
    let mut pairs = vec![
        ("layouts", layouts),
        ("mode", h.str_opt("mode").unwrap_or("test").to_owned()),
    ];
    for (key, name) in [("elementAttrs", "attrs"), ("elementSpec", "spec")] {
        if let Some(v) = h.config.get(key).filter(|v| !v.is_null()) {
            pairs.push((name, v.to_string()));
        }
    }
    pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={}", encode(&v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// `application/x-www-form-urlencoded` value encoding (`URLSearchParams`).
fn encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                out.push(char::from(b));
            }
            b' ' => out.push('+'),
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// A window position option (`windowPosition`).
fn position() -> Option<(f64, f64)> {
    let p = harness::get().config.get("windowPosition")?.as_array()?;
    Some((p.first()?.as_f64()?, p.get(1)?.as_f64()?))
}

fn start_window_and_actions(app: &AppHandle<Wry>) {
    let h = harness::get();
    let duration = Duration::from_millis(h.num("durationMs").unwrap_or(60_000));
    if h.flag("noWindow") {
        run_actions(app);
        std::thread::sleep(duration);
        quit_flow(app);
        return;
    }
    let size = h.config.get("window").map_or((1000.0, 700.0), |w| {
        (
            w.get("width").and_then(Value::as_f64).unwrap_or(1000.0),
            w.get("height").and_then(Value::as_f64).unwrap_or(700.0),
        )
    });
    let (x, y) = position().unwrap_or((0.0, 0.0));
    let url = WebviewUrl::App(format!("index.html?{}", page_query()).into());
    let spec = windows::Spec {
        label: windows::MAIN,
        url,
        title: h.str_opt("windowTitle"),
        user_agent: h.str_opt("windowUserAgent"),
        size,
        position: (x, y),
    };
    let (window, loads) = match windows::build(app, &spec) {
        Ok(built) => built,
        Err(error) => {
            h.record(
                "events.jsonl",
                json!({ "kind": "uncaught", "message": format!("main window: {error}") }),
            );
            quit_flow(app);
            return;
        }
    };
    h.record(
        "windows.jsonl",
        json!({ "kind": "created", "windowId": 1, "label": windows::MAIN, "state": windows::describe(&window) }),
    );
    window.on_window_event(|event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            harness::get().record(
                "windows.jsonl",
                json!({ "kind": "closed", "windowId": 1, "label": windows::MAIN }),
            );
        }
    });
    set_up_main_window(app, &window);
    if h.str_opt("present") == Some("transparent") {
        windows::show_inactive(&window);
        if let Some((x, y)) = position() {
            let _ = window.set_position(tauri::LogicalPosition::new(x, y));
            h.record(
                "windows.jsonl",
                json!({ "kind": "positioned", "requested": [x, y], "bounds": windows::describe(&window)["bounds"] }),
            );
        }
    }
    h.record(
        "events.jsonl",
        json!({ "kind": "step", "step": "load-file" }),
    );
    if windows::wait_loaded(windows::MAIN, &loads) {
        h.record("events.jsonl", json!({ "kind": "step", "step": "loaded" }));
        if let Some(window) = windows::get(app, windows::MAIN) {
            let mut entry = json!({ "kind": "did-finish-load", "windowId": 1 });
            if let (Value::Object(out), Value::Object(state)) =
                (&mut entry, windows::describe(&window))
            {
                out.extend(state);
            }
            h.record("windows.jsonl", entry);
        }
    }
    run_actions(app);
    for at in [30_000_u64, 120_000, 300_000] {
        if Duration::from_millis(at) < duration {
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(at));
                snapshot(&app, &format!("t+{}s", at / 1000));
            });
        }
    }
    std::thread::sleep(duration);
    quit_flow(app);
}

/// last-window-during-consent: the app's only window closes `after` ms
/// after it was created, whatever the consent window is doing.
fn schedule_main_close(app: &AppHandle<Wry>, after: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(after));
        let Some(window) = windows::get(&app, windows::MAIN) else {
            return;
        };
        let others: Vec<Value> = open_windows(&app)
            .into_iter()
            .filter(|w| w.get("label").and_then(Value::as_str) != Some(windows::MAIN))
            .collect();
        harness::get().record(
            "events.jsonl",
            json!({ "kind": "main-window-close", "afterCreateMs": after, "otherWindows": others }),
        );
        let _ = window.close();
    });
}

fn quit_flow(app: &AppHandle<Wry>) {
    let h = harness::get();
    h.record("events.jsonl", json!({ "kind": "quit-flow-start" }));
    snapshot(app, "before-quit");
    observe::probe_all(app, "end");
    std::thread::sleep(Duration::from_millis(500));
    QUITTING.store(true, Ordering::SeqCst);
    match h.str_opt("quitStyle") {
        Some("quit" | "exit") => {
            // Quit with the window still open, as a tray app's "Exit" would.
            app.exit(0);
            return;
        }
        #[cfg(target_os = "macos")]
        Some("terminate") => {
            // The app menu's Quit: `[NSApp terminate:]`.
            let _ = app.run_on_main_thread(crate::macos_lab::terminate);
            return;
        }
        _ => {}
    }
    if let Some(window) = windows::get(app, windows::MAIN) {
        let _ = window.close();
    }
    std::thread::sleep(Duration::from_millis(
        h.num("closeToQuitMs").unwrap_or(2000),
    ));
    app.exit(0);
}

/// Runs the scenario's actions, each `at` ms from now on its own thread.
fn run_actions(app: &AppHandle<Wry>) {
    let actions = harness::get()
        .config
        .get("actions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let start = Instant::now();
    for action in actions {
        let app = app.clone();
        std::thread::spawn(move || {
            let at = Duration::from_millis(action.get("at").and_then(Value::as_u64).unwrap_or(0));
            if let Some(left) = at.checked_sub(start.elapsed()) {
                std::thread::sleep(left);
            }
            let h = harness::get();
            let mut entry = json!({ "phase": "start" });
            if let (Value::Object(out), Value::Object(fields)) = (&mut entry, &action) {
                out.extend(fields.clone());
            }
            h.record("actions.jsonl", entry);
            match act(&app, &action) {
                Ok(()) => h.record(
                    "actions.jsonl",
                    json!({ "phase": "done", "do": action.get("do"), "label": action.get("label") }),
                ),
                Err(error) => h.record(
                    "actions.jsonl",
                    json!({ "phase": "error", "do": action.get("do"), "error": error }),
                ),
            }
        });
    }
}

/// A string field of an action.
fn field<'a>(action: &'a Value, key: &str) -> Option<&'a str> {
    action.get(key).and_then(Value::as_str)
}

/// One action (`app/scenario.cjs` `actions`).
fn act(app: &AppHandle<Wry>, action: &Value) -> Result<(), String> {
    let h = harness::get();
    let label = action.get("label").cloned().unwrap_or(Value::Null);
    match field(action, "do").unwrap_or_default() {
        "ow-call" => ow_call(app, action),
        "page-eval" => {
            let code = field(action, "code").unwrap_or_default();
            let result = page_eval(app, code).unwrap_or_else(|e| json!({ "error": e }));
            h.record(
                "events.jsonl",
                json!({ "kind": "page-eval", "label": label, "result": result }),
            );
        }
        "window" => window_action(app, action),
        "crash-guests" => crash_guests(app, field(action, "which").unwrap_or("all")),
        "guest-eval" => {
            let code = format!(
                "JSON.stringify((() => {})())",
                field(action, "code").unwrap_or("undefined")
            );
            for (guest, result) in observe::eval_each(app, observe::is_guest, &code) {
                h.record(
                    "events.jsonl",
                    json!({ "kind": "guest-eval", "label": label, "webContentsId": guest, "result": result }),
                );
            }
        }
        "hook-guest-frames" => {
            for (guest, result) in observe::eval_each(app, observe::is_guest, GUEST_FRAME_HOOK) {
                h.record(
                    "events.jsonl",
                    json!({ "kind": "hook-guest-frames", "label": label, "webContentsId": guest, "result": result }),
                );
            }
            start_frame_drain(app);
        }
        "hit-probe" => hit_probe(app, action),
        "probe-guests" => observe::probe_all(app, field(action, "label").unwrap_or("probe")),
        "state-file" => {
            let mut entry = json!({ "label": label });
            if let (Value::Object(out), Value::Object(state)) = (&mut entry, read_state_file()) {
                out.extend(state);
            }
            h.record("state-file.jsonl", entry);
        }
        "snapshot" => snapshot(app, field(action, "label").unwrap_or("snapshot")),
        "open-window" => open_window(app, action),
        "extra-window" => extra_window(app, action),
        "cmp-open" => cmp_open(app, action),
        "cmp-close" | "cmp-state" => cmp_window(app, action),
        // Never: a screen capture can raise a system permission prompt.
        "screencapture" => {}
        "pkg-call" => unsupported(action, "ow-tauri has no package manager"),
        "introspect" | "listeners" => unsupported(action, "Electron internals"),
        "cookie-set" => unsupported(action, "not used by the compared scenarios"),
        "guest-fixture" | "gesture-case" => unsupported(
            action,
            "the plugin bounces a guest's top-level navigation off Overwolf back to the ad page, so the loopback fixture cannot replace the ad page (plugin lab hook needed)",
        ),
        "heartbeat-pause" => unsupported(
            action,
            "pausing the guest shim's heartbeat needs a plugin lab hook",
        ),
        other => return Err(format!("unknown action {other}")),
    }
    Ok(())
}

/// The harness page's webview (the ad window's), if it is up.
fn page(app: &AppHandle<Wry>) -> Option<tauri::Webview<Wry>> {
    app.get_webview(windows::MAIN)
}

/// Runs `code` in the harness page (`window.__harness.pageEval`: an
/// indirect eval whose value, awaited, comes back through `harness_reply`).
fn page_eval(app: &AppHandle<Wry>, code: &str) -> Result<Value, String> {
    let webview = page(app).ok_or("no harness page")?;
    let h = harness::get();
    let (id, rx) = h.ask();
    let js = format!(
        "window.__harness ? window.__harness.pageEval({id}, {}) : 0",
        Value::String(code.to_owned())
    );
    if let Err(error) = webview.eval(js) {
        h.forget(id);
        return Err(error.to_string());
    }
    let Some(reply) = wait(&rx, PAGE_WAIT) else {
        h.forget(id);
        return Err("the harness page did not answer".into());
    };
    if reply.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(reply.get("value").cloned().unwrap_or(Value::Null))
    } else {
        Err(reply
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("error")
            .to_owned())
    }
}

/// `{ $undefined: true }` in a scenario stands for `undefined`.
fn is_undefined(v: &Value) -> bool {
    v.get("$undefined").and_then(Value::as_bool) == Some(true)
}

/// `ow-call`: through the JavaScript API in the harness page when it is up,
/// else through the Rust API.
fn ow_call(app: &AppHandle<Wry>, action: &Value) {
    let h = harness::get();
    let name = field(action, "fn").unwrap_or_default().to_owned();
    let args = action
        .get("args")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let generate = action.get("generateFrom").filter(|v| !v.is_null()).cloned();
    let label = field(action, "label").map_or_else(
        || match &generate {
            Some(email) => format!("{name}(generateUserEmailHashes({email}))"),
            None => name.clone(),
        },
        str::to_owned,
    );
    let sync = action.get("sync").and_then(Value::as_bool) == Some(true);
    let via_page = page(app).is_some();
    h.record(
        "events.jsonl",
        json!({ "kind": "ow-call-route", "fn": name, "label": label, "via": if via_page { "js" } else { "rust" } }),
    );
    let run = || -> Result<Value, String> {
        if via_page {
            let code = format!(
                "window.__harness.owCall({}, {}, {})",
                Value::String(name.clone()),
                Value::Array(args.clone()),
                generate.clone().unwrap_or(Value::Null)
            );
            page_eval(app, &code)
        } else {
            rust_call(app, &name, &args, generate.as_ref())
        }
    };
    if sync {
        // Every API call is asynchronous here: a sync throw cannot happen.
        let entry =
            json!({ "kind": "ow-call-sync", "fn": name, "label": label, "returned": "promise" });
        h.record("events.jsonl", entry.clone());
        let settled = match run() {
            Ok(v) => json!({ "settled": "resolved", "value": v }),
            Err(e) => json!({ "settled": "rejected", "error": e }),
        };
        let mut out = entry;
        if let (Value::Object(o), Value::Object(s)) = (&mut out, settled) {
            o.extend(s);
        }
        h.record("events.jsonl", out);
        return;
    }
    call(&label, run);
    snapshot(app, &format!("after {label}"));
}

/// The Rust API counterpart of an `app.overwolf.<name>(...args)` call.
fn rust_call(
    app: &AppHandle<Wry>,
    name: &str,
    args: &[Value],
    generate: Option<&Value>,
) -> Result<Value, String> {
    let ow = app.overwolf();
    let arg = |i: usize| args.get(i).filter(|v| !is_undefined(v));
    let unit =
        |r: tauri_plugin_overwolf::Result<()>| r.map(|()| Value::Null).map_err(|e| e.to_string());
    match name {
        "isCMPRequired" => Ok(json!(tauri::async_runtime::block_on(ow.is_cmp_required()))),
        "generateUserEmailHashes" => {
            let email = arg(0)
                .and_then(Value::as_str)
                .ok_or("email must be a string")?;
            serde_json::to_value(ow.generate_user_email_hashes(email)).map_err(|e| e.to_string())
        }
        "setUserEmailHashes" => {
            let hashes = match generate {
                Some(email) => ow.generate_user_email_hashes(email.as_str().unwrap_or_default()),
                None => match arg(0) {
                    Some(v) => serde_json::from_value::<EmailHashes>(v.clone())
                        .map_err(|e| e.to_string())?,
                    None => {
                        return Err(
                            "the Rust API takes hashes (clear_user_email_hashes clears)".into()
                        );
                    }
                },
            };
            ow.set_user_email_hashes(&hashes);
            Ok(Value::Null)
        }
        "setExternalPaymentUserId" => {
            let options = arg(0)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            unit(tauri::async_runtime::block_on(
                ow.set_external_payment_user_id(&options),
            ))
        }
        "disableAdsFPD" => {
            ow.disable_ads_fpd();
            Ok(Value::Null)
        }
        "disableAdsOptimization" => {
            ow.disable_ads_optimization();
            Ok(Value::Null)
        }
        "disableAnonymousAnalytics" => {
            ow.disable_anonymous_analytics();
            Ok(Value::Null)
        }
        other => Err(format!("TypeError: app.overwolf.{other} is not a function")),
    }
}

/// `window`: an Electron `BrowserWindow` method on the ad window.
fn window_action(app: &AppHandle<Wry>, action: &Value) {
    let h = harness::get();
    let Some(window) = windows::get(app, windows::MAIN) else {
        return;
    };
    let method = field(action, "method").unwrap_or_default();
    let args = action
        .get("args")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if method == "emit" {
        // Electron's `emit('focus')` runs the window's listeners without a
        // focus change; Tauri has no way to raise a window event.
        unsupported(
            &json!({ "do": "window", "label": format!("emit {}", args.first().and_then(Value::as_str).unwrap_or_default()) }),
            "Tauri cannot emit a window event without the window changing",
        );
        return;
    }
    let done = windows::act(&window, method, &args);
    h.record(
        "events.jsonl",
        json!({
            "kind": "window-action",
            "method": method,
            "args": args,
            "done": done,
            "state": windows::describe(&window),
        }),
    );
}

/// `crash-guests`: ends the web content process of each ad guest (macOS;
/// `kill -9` of the `WKWebView`'s process, as ow-electron's harness crashes
/// a guest renderer).
fn crash_guests(app: &AppHandle<Wry>, which: &str) {
    let h = harness::get();
    let mut guests: Vec<(String, tauri::Webview<Wry>)> = app
        .webviews()
        .into_iter()
        .filter(|(l, _)| observe::is_guest(l))
        .collect();
    guests.sort_by(|a, b| a.0.cmp(&b.0));
    if which == "first" {
        guests.truncate(1);
    }
    for (label, webview) in guests {
        let url = webview.url().ok().map(|u| u.to_string());
        let pid = web_process_pid(app, &webview);
        let killed = pid.is_some_and(|pid| {
            std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .status()
                .is_ok_and(|s| s.success())
        });
        h.record(
            "events.jsonl",
            json!({ "kind": "crash-guest", "webContentsId": label, "url": url, "pid": pid, "killed": killed }),
        );
    }
}

/// The pid of a webview's web content process (macOS).
#[cfg(target_os = "macos")]
fn web_process_pid(app: &AppHandle<Wry>, webview: &tauri::Webview<Wry>) -> Option<i32> {
    let (tx, rx) = std::sync::mpsc::channel();
    webview
        .with_webview(move |pw| {
            let _ = tx.send(crate::native::address(pw.inner()));
        })
        .ok()?;
    let address = rx.recv_timeout(Duration::from_secs(5)).ok()?;
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(crate::macos_lab::web_process_pid(address));
    })
    .ok()?;
    rx.recv_timeout(Duration::from_secs(5)).ok()?
}

/// The pid of a webview's web content process (not read off macOS).
#[cfg(not(target_os = "macos"))]
fn web_process_pid(_app: &AppHandle<Wry>, _webview: &tauri::Webview<Wry>) -> Option<i32> {
    None
}

/// `<stateDir>/ow-electron.json` now (`app/scenario.cjs` `readStateFile`).
fn read_state_file() -> Value {
    let Some(dir) = harness::get().str_opt("stateDir").map(PathBuf::from) else {
        return json!({ "error": "no stateDir in config" });
    };
    let siblings = std::fs::read_dir(&dir).ok().map(|d| {
        let mut names: Vec<String> = d
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect();
        names.sort();
        names
    });
    let file = dir.join("ow-electron.json");
    let Ok(bytes) = std::fs::read(&file) else {
        return json!({ "exists": false, "siblings": siblings });
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let (keys, parse_error) = match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) => (json!(m.keys().collect::<Vec<_>>()), Value::Null),
        Ok(Value::Array(_) | Value::Null) => (json!("object"), Value::Null),
        Ok(Value::String(_)) => (json!("string"), Value::Null),
        Ok(Value::Number(_)) => (json!("number"), Value::Null),
        Ok(Value::Bool(_)) => (json!("boolean"), Value::Null),
        Err(e) => (Value::Null, json!(e.to_string())),
    };
    let digest = sha2::Sha256::digest(&bytes);
    let sha256 = digest.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    });
    json!({
        "exists": true,
        "size": bytes.len(),
        "sha256": sha256,
        "keys": keys,
        "parseError": parse_error,
        "text": harness::truncate(&text, 20_000),
        "siblings": siblings,
    })
}

/// `open-window`: a plain Tauri window as ow-electron's harness builds a
/// `BrowserWindow` (400x300 at 0,0 unless the options say otherwise), an
/// app file or a URL, shown inactive at alpha 0 unless `show: false`.
fn open_window(app: &AppHandle<Wry>, action: &Value) {
    let h = harness::get();
    let key = field(action, "key").unwrap_or("window").to_owned();
    let options = action.get("options").cloned().unwrap_or_else(|| json!({}));
    let opt = |k: &str| options.get(k).and_then(Value::as_f64);
    let n = EXTRA_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
    let label = format!("x-{n}");
    let file = field(action, "file").unwrap_or("blank.html");
    let url = if let Some(u) = field(action, "url") {
        match tauri::Url::parse(u) {
            Ok(u) => WebviewUrl::External(u),
            Err(e) => {
                h.record(
                    "events.jsonl",
                    json!({ "kind": "open-window-failed", "key": key, "error": e.to_string() }),
                );
                return;
            }
        }
    } else {
        let path = match field(action, "query") {
            Some(q) => format!("{file}?{q}"),
            None => file.to_owned(),
        };
        WebviewUrl::App(path.into())
    };
    let spec = windows::Spec {
        label: &label,
        url,
        title: options.get("title").and_then(Value::as_str),
        user_agent: None,
        size: (
            opt("width").unwrap_or(400.0),
            opt("height").unwrap_or(300.0),
        ),
        position: (opt("x").unwrap_or(0.0), opt("y").unwrap_or(0.0)),
    };
    let (window, loads) = match windows::build(app, &spec) {
        Ok(built) => built,
        Err(e) => {
            h.record(
                "events.jsonl",
                json!({ "kind": "open-window-failed", "key": key, "error": e.to_string() }),
            );
            return;
        }
    };
    lock(&EXTRA).insert(key.clone(), label.clone());
    let id = n + 1;
    h.record(
        "windows.jsonl",
        json!({ "kind": "created", "windowId": id, "label": label, "key": key }),
    );
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            harness::get().record("windows.jsonl", json!({ "kind": "closed", "windowId": id }));
        }
    });
    if let Some(name) = options.get("name").and_then(Value::as_str)
        && let Err(e) = app.overwolf().set_window_name(&label, name)
    {
        h.record(
            "events.jsonl",
            json!({ "kind": "set-window-name-failed", "key": key, "name": name, "error": e.to_string() }),
        );
    }
    // `about:` pages report no page load (Electron's loadURL resolves at once).
    if !field(action, "url").is_some_and(|u| u.starts_with("about:")) {
        windows::wait_loaded(&label, &loads);
    }
    if action.get("show").and_then(Value::as_bool) != Some(false) {
        windows::show_inactive(&window);
    }
    h.record(
        "events.jsonl",
        json!({
            "kind": "open-window",
            "key": key,
            "options": options,
            "file": if field(action, "url").is_some() { Value::Null } else { json!(file) },
            "url": field(action, "url"),
            "state": windows::describe(&window),
        }),
    );
}

/// `extra-window`: a `BrowserWindow` method on a window `open-window` opened.
fn extra_window(app: &AppHandle<Wry>, action: &Value) {
    let h = harness::get();
    let key = field(action, "key").unwrap_or_default();
    let Some(label) = lock(&EXTRA).get(key).cloned() else {
        return;
    };
    let Some(window) = windows::get(app, &label) else {
        return;
    };
    let method = field(action, "method").unwrap_or_default();
    let args = action
        .get("args")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let done = windows::act(&window, method, &args);
    h.record(
        "events.jsonl",
        json!({ "kind": "extra-window", "key": key, "method": method, "args": args, "done": done }),
    );
}

/// Consent windows (`ow-cmp*`) open now.
fn cmp_labels(app: &AppHandle<Wry>) -> Vec<String> {
    app.windows()
        .into_keys()
        .filter(|l| l.starts_with("ow-cmp"))
        .collect()
}

/// `cmp-open`: `openAdPrivacySettingsWindow` / `openCMPWindow` through the
/// Rust API; the consent windows that appear within 2 s belong to the action.
fn cmp_open(app: &AppHandle<Wry>, action: &Value) {
    let h = harness::get();
    let name = field(action, "fn").unwrap_or_default().to_owned();
    let label = field(action, "label").unwrap_or_default().to_owned();
    let options = action.get("options").cloned();
    let entry =
        json!({ "kind": "cmp-open", "fn": name, "label": label, "options": options, "t": h.t() });
    let parsed = match options.clone() {
        None => Ok(CmpWindowOptions::default()),
        Some(v) => serde_json::from_value::<CmpWindowOptions>(v),
    };
    let parsed = match parsed {
        Ok(p) => p,
        Err(e) => {
            let mut out = entry;
            out["threw"] = json!(e.to_string());
            h.record("events.jsonl", out);
            return;
        }
    };
    let mut out = entry;
    out["returned"] = json!("promise");
    h.record("events.jsonl", out);
    let before = cmp_labels(app);
    let started = Instant::now();
    let settle = app.clone();
    let settle_label = label.clone();
    std::thread::spawn(move || {
        let ow = settle.overwolf();
        let result = tauri::async_runtime::block_on(async {
            if name == "openCMPWindow" {
                ow.open_cmp_window(parsed).await
            } else {
                ow.open_ad_privacy_settings_window(parsed).await
            }
        });
        let after_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let open: Vec<String> = lock(&CMP_WINDOWS)
            .iter()
            .filter(|(w, l)| **l == settle_label && settle.get_window(w).is_some())
            .map(|(w, _)| w.clone())
            .collect();
        harness::get().record(
            "events.jsonl",
            match result {
                Ok(()) => json!({ "kind": "cmp-settled", "label": settle_label, "settled": "resolved", "value": null, "afterMs": after_ms, "openWindows": open }),
                Err(e) => json!({ "kind": "cmp-settled", "label": settle_label, "settled": "rejected", "error": e.to_string(), "afterMs": after_ms }),
            },
        );
    });
    // Adopt the consent windows that appear within 2 s.
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        for w in cmp_labels(app) {
            if before.contains(&w) || lock(&CMP_WINDOWS).contains_key(&w) {
                continue;
            }
            lock(&CMP_WINDOWS).insert(w.clone(), label.clone());
            if let Some(window) = windows::get(app, &w) {
                let mut rec = json!({ "kind": "cmp-window", "label": label });
                if let (Value::Object(o), Value::Object(s)) = (&mut rec, windows::describe(&window))
                {
                    o.extend(s);
                }
                h.record("windows.jsonl", rec);
                let action_label = label.clone();
                window.on_window_event(move |event| {
                    if matches!(event, tauri::WindowEvent::Destroyed) {
                        harness::get().record(
                            "windows.jsonl",
                            json!({ "kind": "cmp-window-closed", "label": action_label }),
                        );
                    }
                });
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `cmp-close` / `cmp-state` on the consent windows of an action.
fn cmp_window(app: &AppHandle<Wry>, action: &Value) {
    let h = harness::get();
    let label = field(action, "label").unwrap_or_default();
    let close = field(action, "do") == Some("cmp-close");
    let mine: Vec<String> = lock(&CMP_WINDOWS)
        .iter()
        .filter(|(_, l)| l.as_str() == label)
        .map(|(w, _)| w.clone())
        .collect();
    for w in mine {
        let Some(window) = windows::get(app, &w) else {
            continue;
        };
        let kind = if close {
            "cmp-window-before-close"
        } else {
            "cmp-window-state"
        };
        let mut rec = json!({ "kind": kind, "label": label });
        if let (Value::Object(o), Value::Object(s)) = (&mut rec, windows::describe(&window)) {
            o.extend(s);
        }
        h.record("windows.jsonl", rec);
        if close {
            let _ = window.close();
        }
    }
}

/// `app/scenario.cjs` `GUEST_FRAME_HOOK`, with the guest's console replaced
/// by a queue the harness drains (Tauri cannot read a guest's console).
const GUEST_FRAME_HOOK: &str = r"JSON.stringify((() => {
  const top = window;
  top.__parityFrameLog = top.__parityFrameLog || [];
  const summarize = (d) => { try { return typeof d === 'string' ? d.slice(0, 1500) : JSON.stringify(d).slice(0, 1500); } catch (e) { return String(d).slice(0, 200); } };
  const hook = (w, path) => {
    try {
      if (w.__parityFrameHooked) return 0;
      w.__parityFrameHooked = true;
      w.addEventListener('message', (e) => {
        try { if (top.__parityFrameLog.length < 5000) top.__parityFrameLog.push('__PARITYF__' + JSON.stringify({ path, href: w.location.href.slice(0, 200), origin: e.origin, fromParent: e.source === w.parent, data: summarize(e.data) })); } catch (err) {}
      }, true);
      return 1;
    } catch (e) { return 0; }
  };
  const walk = (w, path) => {
    let n = 0;
    for (let i = 0; i < w.frames.length; i++) {
      const f = w.frames[i];
      try { void f.location.href; } catch (e) { continue; }
      n += hook(f, path + '/' + i) + walk(f, path + '/' + i);
    }
    return n;
  };
  if (!top.__parityFrameTimer) top.__parityFrameTimer = setInterval(() => walk(top, ''), 1000);
  return walk(top, '');
})())";

/// Takes the queued frame messages of a guest.
const FRAME_DRAIN: &str = "JSON.stringify((() => { const l = window.__parityFrameLog || []; window.__parityFrameLog = []; return l; })())";

/// Moves the queued frame messages of every guest into `console.jsonl`,
/// every second (once started).
fn start_frame_drain(app: &AppHandle<Wry>) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            for (guest, lines) in observe::eval_each(&app, observe::is_guest, FRAME_DRAIN) {
                for message in lines.as_array().into_iter().flatten() {
                    harness::get().record(
                        "console.jsonl",
                        json!({ "type": "owadview", "webContentsId": guest, "level": 0, "message": message }),
                    );
                }
            }
        }
    });
}

/// `hit-probe`: lab checks L1-L3 (what the page hits at each point, which
/// native view a click there reaches, each webview's own rendering) and,
/// in test mode, one click into the app's own webview.
fn hit_probe(app: &AppHandle<Wry>, action: &Value) {
    let h = harness::get();
    let points = action.get("points").cloned().unwrap_or_else(|| json!([]));
    let dom = page_eval(app, &format!("window.__parityHit({points})"))
        .unwrap_or_else(|e| json!({ "error": e }));
    // The page resolved selector points to CSS px; the native probe uses those.
    let resolved: Vec<Value> = dom
        .get("points")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|p| json!({ "name": p.get("name"), "x": p.get("x"), "y": p.get("y") }))
        .collect();
    let label = field(action, "label").unwrap_or("hit").to_owned();
    let click = if h.test_mode() {
        field(action, "click").map(str::to_owned)
    } else {
        None
    };
    let snapshot = action.get("snapshot").and_then(Value::as_bool) == Some(true);
    let capture: String = format!(
        "hit-{}",
        label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            })
            .collect::<String>()
    );
    let native = crate::probe::native_probe(
        app,
        windows::MAIN,
        windows::MAIN,
        &resolved,
        snapshot,
        click.as_deref(),
        Some(&capture),
    )
    .unwrap_or_else(|e| json!({ "error": e }));
    h.record(
        "events.jsonl",
        json!({ "kind": "hit-probe", "label": label, "host": "tauri", "dom": dom, "native": native }),
    );
}

/// A report of the harness page (`page.js`): counts live ad loads as
/// ow-electron's harness does from the console.
pub fn page_event(app: &AppHandle<Wry>, payload: &Value) {
    if payload.get("kind").and_then(Value::as_str) != Some("owadview-event") {
        return;
    }
    let event = payload
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let cid = payload.get("cid").cloned().unwrap_or(Value::Null);
    if event == "dom-ready" {
        let key = cid.to_string();
        let mut seen = lock(&SEEN_GUESTS);
        let reload = seen.contains(&key);
        if !reload {
            seen.push(key);
        }
        drop(seen);
        count_live_load(
            app,
            if reload { "guest-reload" } else { "guest-load" },
            &cid,
        );
    }
    if matches!(event, "impression" | "display_ad_loaded") {
        count_live_load(app, &format!("event:{event}"), &cid);
    }
}

/// Logs a live ad load (or a fill event of one) and enforces the cap.
fn count_live_load(app: &AppHandle<Wry>, reason: &str, cid: &Value) {
    let h = harness::get();
    if h.str_opt("mode") != Some("live") {
        return;
    }
    let fill = reason.starts_with("event:");
    let n = if fill {
        LIVE_LOADS.load(Ordering::SeqCst)
    } else {
        LIVE_LOADS.fetch_add(1, Ordering::SeqCst) + 1
    };
    let mut entry =
        json!({ "n": n, "reason": reason, "detail": { "cid": cid }, "at": harness::wall_ms() });
    if fill {
        entry["fill"] = json!(true);
    }
    h.record("live-loads.jsonl", entry);
    if n > h.num("maxLiveLoads").unwrap_or(0) && !LIVE_STOPPED.swap(true, Ordering::SeqCst) {
        h.record(
            "events.jsonl",
            json!({ "kind": "live-cap-reached", "liveLoads": n }),
        );
        if let Some(webview) = page(app) {
            let _ =
                webview.eval("document.querySelectorAll('owadview').forEach((el) => el.remove())");
        }
    }
}

/// What the harness page reports about itself once loaded.
pub fn page_info(info: &Value) {
    if let Some(ua) = info.get("userAgent").and_then(Value::as_str) {
        *lock(&PAGE_UA) = Some(ua.to_owned());
    }
    if let Some(api) = info.get("api") {
        *lock(&API_SURFACE) = Some(api.clone());
    }
}
