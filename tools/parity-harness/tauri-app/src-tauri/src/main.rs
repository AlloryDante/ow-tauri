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
//! The plugin is built with its `lab` feature: `OW_TAURI_LAB_DIR` turns on its
//! trace and `OW_TAURI_LAB_INVISIBLE=1` makes every window invisible before it
//! can appear. `run.mjs --host tauri` sets both.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tauri::{Manager, Runtime, Webview};
use tauri_plugin_overwolf::OverwolfExt;

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
