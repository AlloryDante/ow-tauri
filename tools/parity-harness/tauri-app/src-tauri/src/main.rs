//! The Tauri edition of the parity harness app (see the harness README): a
//! plain Tauri app on `tauri-plugin-overwolf`, built with the plugin's `lab`
//! feature, that runs the scenarios ow-electron's harness app
//! (`app/main.cjs`) runs and writes the same capture files.
//!
//! - `harness.rs`: the run configuration (`PARITY_HARNESS_CONFIG`) and the
//!   capture files;
//! - `driver.rs`: the run itself (snapshots, calls, the ad window, the
//!   timed actions, the quit flow);
//! - `windows.rs`: plain Tauri windows, built hidden and shown only
//!   invisible in the lab;
//! - `observe.rs`: the ad guests', consent pages' and cookies' probes;
//! - `probe.rs`, `native.rs`, `native_win.rs`: the native lab checks;
//! - `macos_lab.rs`: the macOS invisible-lab guards.
//!
//! The ad window's page (`web/`) is the shared harness page
//! (`app/page.js`) with the plugin's `<owadview>` runtime and JavaScript API
//! (`tauri-plugin-overwolf-api`). The identity under test is read at run
//! time (`PARITY_HARNESS_PACKAGE_JSON`) into the plugin's configuration, so
//! it never enters the build.
//!
//! `OW_TAURI_LAB_DIR` turns on the plugin's lab trace and
//! `OW_TAURI_LAB_INVISIBLE=1` keeps every window invisible; `run.mjs --host
//! tauri` sets both.

use serde_json::{Value, json};
use tauri::{AppHandle, Webview, Wry};

mod driver;
mod harness;
#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "Objective-C calls of the macOS lab guards")]
mod macos_lab;
#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "Objective-C calls of the native lab probes")]
mod native;
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Win32 and WebView2 calls of the native lab probes"
)]
mod native_win;
mod observe;
mod probe;
mod windows;

/// The run configuration, plus `host` and `startedWall` (Unix ms when the
/// process started, the origin of every harness `t`).
#[tauri::command]
fn harness_config() -> Value {
    let h = harness::get();
    let mut config = h.config.clone();
    if let Value::Object(map) = &mut config {
        map.insert("startedWall".into(), json!(h.started_wall));
        map.insert("host".into(), json!("tauri"));
    }
    config
}

/// A report of the harness page (`page.js`), appended to
/// `page-events.jsonl` as ow-electron's harness writes the page's console
/// reports.
#[tauri::command]
fn harness_page_event(app: AppHandle<Wry>, webview: Webview<Wry>, payload: Value) {
    let h = harness::get();
    let mut entry = json!({ "webContentsId": webview.label() });
    if let (Value::Object(out), Value::Object(fields)) = (&mut entry, &payload) {
        out.extend(fields.clone());
    }
    h.record("page-events.jsonl", entry);
    driver::page_event(&app, &payload);
}

/// The answer of the harness page to a request of the driver.
#[tauri::command]
fn harness_reply(id: u64, reply: Value) {
    harness::get().answer(id, reply);
}

/// What the harness page reports about itself once loaded (user agent,
/// the JavaScript API's functions).
#[tauri::command]
fn harness_page_info(info: Value) {
    driver::page_info(&info);
}

/// The identity under test: the app's name and version (`package_info`,
/// what `getInfo()` and the analytics report), the configuration's
/// `productName` and `version`, and `plugins.overwolf.{name, author, uid}`.
fn configure(context: &mut tauri::Context<Wry>) -> Result<(), String> {
    let id = harness::identity()?;
    let info = context.package_info_mut();
    info.name.clone_from(&id.product_name);
    if let Ok(version) = semver::Version::parse(&id.version) {
        info.version = version;
    }
    let config = context.config_mut();
    config.product_name = Some(id.product_name.clone());
    config.version = Some(id.version.clone());
    let mut overwolf = serde_json::Map::new();
    overwolf.insert("name".into(), json!(id.product_name));
    if let Some(author) = id.author {
        overwolf.insert("author".into(), json!(author));
    }
    if let Some(uid) = id.uid {
        overwolf.insert("uid".into(), json!(uid));
    }
    config
        .plugins
        .0
        .insert("overwolf".into(), Value::Object(overwolf));
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let h = harness::install()?;
    // Before any window or webview exists: the invisible lab app never
    // activates and no window becomes key.
    #[cfg(target_os = "macos")]
    if tauri_plugin_overwolf::lab_invisible() {
        macos_lab::hold_app_back();
    }
    let mut context = tauri::generate_context!();
    configure(&mut context)?;

    let builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_overwolf::Builder::new()
                .test_ad(h.test_mode())
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            harness_config,
            harness_page_event,
            harness_reply,
            harness_page_info,
        ])
        .setup(|app| {
            driver::setup(app.handle());
            Ok(())
        });

    // The invisible lab app never comes to the front: activated at launch,
    // it would take the keyboard from the app the user is typing in.
    #[cfg(target_os = "macos")]
    let builder = builder
        .activate_ignoring_other_apps(!tauri_plugin_overwolf::lab_invisible())
        // macOS reports a crashed web content process only through this hook.
        .on_web_content_process_terminate(
            tauri_plugin_overwolf::web_content_process_terminate_hook(),
        );

    #[allow(unused_mut, reason = "mutated on macOS only")]
    let mut app = builder.build(context)?;
    // No Dock icon, no app switcher entry, before the event loop starts.
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    app.run(|app, event| driver::on_event(app, &event));
    Ok(())
}
