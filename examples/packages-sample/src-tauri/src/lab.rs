//! The end-to-end lab (Cargo feature `lab`, off by default; see
//! `e2e/README.md`). It never ships: `e2e/run.mjs` builds the app with it,
//! and nothing happens unless the environment asks for it.
//!
//! - `OW_TAURI_LAB_INVISIBLE=1` (the plugin's lab mode): every window is
//!   invisible before it can appear, dialogs and the file manager do not
//!   open, and this shell keeps the app out of the Dock and the app switcher.
//! - `OW_TAURI_LAB_PACKAGE_JSON=<file>`: the app manifest to run with instead
//!   of the embedded one, so a lab identity never enters the build or the
//!   repository.
//! - `OW_TAURI_LAB_DIR=<dir>`: the plugin's trace, plus the driver's records
//!   (`e2e.jsonl`) through [`e2e_record`].
//! - `OW_SAMPLE_E2E_CONFIG=<json>`: the driver's run configuration
//!   ([`e2e_config`]); without it the driver stays inert.

use serde_json::Value;
use tauri::{Runtime, Webview};
use tauri_plugin_overwolf::OverwolfExt;

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

/// Whether the app must stay out of the Dock and the app switcher.
#[must_use]
pub fn invisible() -> bool {
    tauri_plugin_overwolf::lab_invisible()
}

/// The driver's run configuration (`OW_SAMPLE_E2E_CONFIG`), or `null`
/// when the runner did not launch the app. Main webview only.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri passes command arguments by value"
)]
pub fn e2e_config<R: Runtime>(webview: Webview<R>) -> Result<Value, String> {
    if webview.label() != "ow-main" {
        return Err("e2e_config is for the main webview only".to_owned());
    }
    match std::env::var("OW_SAMPLE_E2E_CONFIG") {
        Ok(text) if !text.is_empty() => {
            serde_json::from_str(&text).map_err(|e| format!("OW_SAMPLE_E2E_CONFIG: {e}"))
        }
        _ => Ok(Value::Null),
    }
}

/// Appends one record of the end-to-end driver to `e2e.jsonl` in the lab
/// directory. Only the main webview (`ow-main`), where the driver runs, may
/// call it.
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
