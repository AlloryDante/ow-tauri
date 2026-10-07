//! The Tauri edition of the parity harness app (see the harness README).
//!
//! All harness logic runs in the plugin's hidden main webview
//! (`web/main.js`, the counterpart of `app/main.cjs`). This shell only:
//!
//! - reads the app manifest under test at run time
//!   (`PARITY_HARNESS_PACKAGE_JSON`), so the identity never enters the build;
//! - keeps the app out of the Dock and the app switcher (macOS accessory
//!   activation policy) before the event loop starts;
//! - gives the harness scripts a few commands that write into the run
//!   directory (`PARITY_HARNESS_CONFIG`'s `runDir`) and act on the plugin's
//!   own windows.
//!
//! On macOS it also answers the lab checks of the ad formats (L1-L3) from
//! the window's native view tree (`native.rs`): no screen capture, and no
//! input to an ad guest.
//!
//! The plugin is built with its `lab` feature: `OW_TAURI_LAB_DIR` turns on its
//! trace and `OW_TAURI_LAB_INVISIBLE=1` makes every window invisible before it
//! can appear. `run.mjs --host tauri` sets both.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tauri::{Manager, Runtime, Webview};
use tauri_plugin_overwolf::OverwolfExt;

#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "Objective-C calls of the native lab probes")]
mod native;

/// The run configuration written by `run.mjs` (`PARITY_HARNESS_CONFIG`).
struct Harness {
    config: Value,
    run_dir: PathBuf,
    started_wall: u128,
    page_events: Mutex<Vec<Value>>,
}

static HARNESS: OnceLock<Harness> = OnceLock::new();

fn wall_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

fn harness() -> Result<&'static Harness, String> {
    HARNESS
        .get()
        .ok_or_else(|| "harness configuration missing".to_owned())
}

/// Only plain file names in the run directory.
fn run_file(name: &str, extension: &str) -> Result<PathBuf, String> {
    let ok = !name.is_empty()
        && name.len() <= 120
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !name.starts_with('.')
        && name.ends_with(extension);
    if !ok {
        return Err(format!("bad run file name {name:?}"));
    }
    Ok(harness()?.run_dir.join(name))
}

fn append(name: &str, line: &str) -> Result<(), String> {
    let path = run_file(name, ".jsonl")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    // One write per line keeps lines whole next to the plugin's trace.
    file.write_all(format!("{line}\n").as_bytes())
        .map_err(|e| e.to_string())
}

/// The run configuration, plus `startedWall` (Unix ms when the process
/// started), the origin of every harness `t`.
#[tauri::command]
fn harness_config() -> Result<Value, String> {
    let h = harness()?;
    let mut config = h.config.clone();
    if let Value::Object(map) = &mut config {
        map.insert(
            "startedWall".into(),
            json!(u64::try_from(h.started_wall).unwrap_or(0)),
        );
        map.insert("host".into(), json!("tauri"));
    }
    Ok(config)
}

/// Appends one line (already JSON) to `<runDir>/<file>` (`*.jsonl`).
#[tauri::command]
fn harness_record(file: String, line: String) -> Result<(), String> {
    append(&file, &line)
}

/// Writes `<runDir>/<file>` (`*.json`).
#[tauri::command]
fn harness_write(file: String, text: String) -> Result<(), String> {
    std::fs::write(run_file(&file, ".json")?, text).map_err(|e| e.to_string())
}

/// A report of the harness page (`page.js`): appended to `page-events.jsonl`
/// and queued for the main script, as ow-electron's harness reads them from
/// the console.
#[tauri::command]
fn harness_page_event<R: Runtime>(webview: Webview<R>, payload: Value) -> Result<(), String> {
    let h = harness()?;
    let mut entry = json!({
        "t": u64::try_from(wall_ms().saturating_sub(h.started_wall)).unwrap_or(0),
        "webContentsId": webview.label(),
    });
    if let (Value::Object(out), Value::Object(fields)) = (&mut entry, &payload) {
        for (k, v) in fields {
            out.insert(k.clone(), v.clone());
        }
    }
    append("page-events.jsonl", &entry.to_string())?;
    h.page_events
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(payload);
    Ok(())
}

/// The page reports queued since the last call.
#[tauri::command]
fn harness_take_page_events() -> Result<Vec<Value>, String> {
    Ok(std::mem::take(
        &mut *harness()?
            .page_events
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    ))
}

/// Probes every live ad guest (`guest-<n>-<phase>.json`, plugin lab trace).
#[tauri::command]
fn harness_probe_guests<R: Runtime>(app: tauri::AppHandle<R>, phase: String) {
    app.overwolf().lab_probe_guests(&phase);
}

/// How long a lab probe waits for webviews and the main thread to answer.
const PROBE_WAIT: Duration = Duration::from_secs(5);

/// Ad guest webview labels (`owad-<embedder>-<n>`).
fn is_guest(label: &str) -> bool {
    label.starts_with("owad-")
}

/// Evaluates `code` in every live ad guest and returns `[{label, result}]`
/// (`result` is the JSON the expression returned, parsed). The harness's own
/// observation channel (`guest-eval`, `hook-guest-frames`), as the
/// ow-electron harness runs code in its guests; never part of `ipc.jsonl`.
#[tauri::command]
async fn harness_guest_eval<R: Runtime>(
    app: tauri::AppHandle<R>,
    code: String,
) -> Result<Vec<Value>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (tx, rx) = mpsc::channel();
        let mut asked = 0;
        for (label, webview) in app.webviews() {
            if !is_guest(&label) {
                continue;
            }
            let tx = tx.clone();
            let name = label.clone();
            if webview
                .eval_with_callback(&code, move |result| {
                    let value =
                        serde_json::from_str::<Value>(&result).unwrap_or(Value::String(result));
                    let _ = tx.send(json!({ "label": name, "result": value }));
                })
                .is_ok()
            {
                asked += 1;
            }
        }
        drop(tx);
        let mut out = Vec::new();
        while out.len() < asked {
            match rx.recv_timeout(PROBE_WAIT) {
                Ok(v) => out.push(v),
                Err(_) => break,
            }
        }
        out
    })
    .await
    .map_err(|e| e.to_string())
}

/// The native window and the webviews of the window `label`
/// (`(ns_window, address -> label)`).
#[cfg(target_os = "macos")]
fn window_views<R: Runtime>(
    app: &tauri::AppHandle<R>,
    label: &str,
) -> Result<(usize, native::Views), String> {
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

/// Parses `[{name, x, y}]`.
#[cfg(target_os = "macos")]
fn points(raw: &[Value]) -> Vec<native::Point> {
    raw.iter()
        .filter_map(|p| {
            Some(native::Point {
                name: p.get("name")?.as_str()?.to_owned(),
                x: p.get("x")?.as_f64()?,
                y: p.get("y")?.as_f64()?,
            })
        })
        .collect()
}

/// Lab checks L1-L3 (macOS): the native view order of the window `window`,
/// the background state of each webview, which view a click at each point
/// would reach, then (`snapshot`) each webview's own rendering sampled at
/// the points, and (`click`, test mode only) one click at the named point
/// into the app's webview `embedder` when, and only when, the hit test
/// names that webview. Other platforms answer `{unsupported: true}`.
#[tauri::command]
async fn harness_native_probe<R: Runtime>(
    app: tauri::AppHandle<R>,
    window: String,
    embedder: String,
    points: Vec<Value>,
    snapshot: bool,
    click: Option<String>,
) -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        let test_mode = harness()?.config.get("mode").and_then(Value::as_str) == Some("test");
        tauri::async_runtime::spawn_blocking(move || {
            native_probe(
                &app,
                &window,
                &embedder,
                &points,
                snapshot,
                click.as_deref(),
                test_mode,
            )
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, window, embedder, points, snapshot, click);
        Ok(json!({ "unsupported": true }))
    }
}

#[cfg(target_os = "macos")]
fn native_probe<R: Runtime>(
    app: &tauri::AppHandle<R>,
    window: &str,
    embedder: &str,
    raw_points: &[Value],
    snapshot: bool,
    click: Option<&str>,
    test_mode: bool,
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
            // Synthetic input only in test mode (lab rule).
            _ if !test_mode => json!({ "sent": false, "refused": "not in test mode" }),
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

/// Describes, or closes, one of the plugin's own windows (consent windows),
/// as ow-electron's harness does with the windows ow-electron opens.
#[tauri::command]
fn harness_window<R: Runtime>(
    app: tauri::AppHandle<R>,
    label: String,
    action: String,
) -> Result<Value, String> {
    let Some(window) = app.get_webview_window(&label) else {
        return Ok(json!({ "label": label, "exists": false }));
    };
    let state = json!({
        "label": label,
        "exists": true,
        "title": window.title().ok(),
        "visible": window.is_visible().ok(),
        "focused": window.is_focused().ok(),
        "position": window.outer_position().ok().map(|p| [p.x, p.y]),
        "size": window.outer_size().ok().map(|s| [s.width, s.height]),
        "url": window.url().ok().map(|u| u.to_string()),
    });
    match action.as_str() {
        "state" => Ok(state),
        "close" => {
            window.close().map_err(|e| e.to_string())?;
            Ok(state)
        }
        other => Err(format!("unknown action {other}")),
    }
}

fn load_manifest() -> Result<&'static str, String> {
    let path = std::env::var_os("PARITY_HARNESS_PACKAGE_JSON")
        .ok_or("PARITY_HARNESS_PACKAGE_JSON is not set")?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("package.json: {e}"))?;
    let parsed = tauri_plugin_overwolf::manifest::parse_package_json(&text)
        .map_err(|e| format!("package.json: {e}"))?;
    let json = serde_json::to_string(&parsed.manifest).map_err(|e| e.to_string())?;
    Ok(Box::leak(json.into_boxed_str()))
}

fn load_config() -> Result<Harness, String> {
    let path =
        std::env::var_os("PARITY_HARNESS_CONFIG").ok_or("PARITY_HARNESS_CONFIG is not set")?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("config: {e}"))?;
    let config: Value = serde_json::from_str(&text).map_err(|e| format!("config: {e}"))?;
    let run_dir = config
        .get("runDir")
        .and_then(Value::as_str)
        .ok_or("config: runDir missing")?
        .into();
    Ok(Harness {
        config,
        run_dir,
        started_wall: wall_ms(),
        page_events: Mutex::new(Vec::new()),
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let harness = load_config()?;
    let _ = HARNESS.set(harness);
    let manifest = load_manifest()?;

    let builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_overwolf::Builder::new()
                .manifest_json(manifest)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            harness_config,
            harness_record,
            harness_write,
            harness_page_event,
            harness_take_page_events,
            harness_probe_guests,
            harness_window,
            harness_guest_eval,
            harness_native_probe,
        ]);

    // The invisible lab app never comes to the front: activated at launch,
    // it would take the keyboard from the app the user is typing in.
    #[cfg(target_os = "macos")]
    let builder = builder.activate_ignoring_other_apps(!tauri_plugin_overwolf::lab_invisible());

    // macOS reports a crashed web content process only through this hook.
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(|webview| {
        webview
            .overwolf()
            .report_web_content_terminated(webview.label());
    });

    #[allow(unused_mut, reason = "mutated on macOS only")]
    let mut app = builder.build(tauri::generate_context!())?;
    // No Dock icon, no app switcher entry, before the event loop starts.
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    app.run(|_, _| {});
    Ok(())
}
