//! The update client commands (CONTRACT A.2.8, I).

use serde::Deserialize;
use tauri::ipc::Request;
use tauri::{Runtime, State, Webview};

use super::{body, host, require_main};
use crate::error::Result;
use crate::ext::Overwolf;
use crate::updater::{UpdateCheckResult, UpdaterConfig};

#[tauri::command]
pub(crate) async fn updater_configure<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    request: Request<'_>,
) -> Result<()> {
    require_main(&webview)?;
    let config: UpdaterConfig = body(&request, "updater_configure")?;
    host(&state).updater_configure(&config)
}

#[tauri::command]
pub(crate) async fn updater_check<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<Option<UpdateCheckResult>> {
    require_main(&webview)?;
    host(&state).updater_check().await
}

#[tauri::command]
pub(crate) async fn updater_download<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<Vec<String>> {
    require_main(&webview)?;
    let files = host(&state).updater_download().await?;
    Ok(files
        .into_iter()
        .map(|f| f.to_string_lossy().into_owned())
        .collect())
}

/// `updater_quit_and_install` arguments.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuitAndInstall {
    #[serde(default)]
    is_silent: Option<bool>,
    #[serde(default)]
    is_force_run_after: Option<bool>,
}

#[tauri::command]
pub(crate) async fn updater_quit_and_install<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    request: Request<'_>,
) -> Result<()> {
    require_main(&webview)?;
    let args: QuitAndInstall = match request.body() {
        tauri::ipc::InvokeBody::Json(serde_json::Value::Null) => QuitAndInstall::default(),
        _ => body(&request, "updater_quit_and_install")?,
    };
    host(&state).updater_quit_and_install(
        args.is_silent.unwrap_or(false),
        args.is_force_run_after.unwrap_or(false),
    )
}
