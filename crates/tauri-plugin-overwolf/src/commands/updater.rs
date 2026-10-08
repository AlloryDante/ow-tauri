//! The update client commands (DESIGN §3.5, D18; Windows with the
//! `updater` feature (R6)). The update engine arrives in W3; until then, and
//! off Windows or without the feature, every command answers `unsupported`.
#![allow(
    dead_code,
    reason = "the update engine (W3) builds the remaining values"
)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{ResourceId, Runtime, State, Webview};

use super::{core, require_app_webview};
use crate::error::{Error, Result};
use crate::ext::Overwolf;

/// `CheckOptions` of `check()`. No headers from JavaScript (SEC-M10).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CheckOptions {
    /// The channel (`latest`, `beta`, ...).
    pub(crate) channel: Option<String>,
    /// Allow a lower version (only with `updater.allowJsDowngrade`, SEC-m4).
    pub(crate) allow_downgrade: Option<bool>,
    /// Allow pre-release versions.
    pub(crate) allow_prerelease: Option<bool>,
    /// Request timeout in milliseconds.
    pub(crate) timeout: Option<u64>,
}

/// What `updater_check` returns for an available update.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateMetadata {
    /// The `Update` resource.
    pub(crate) rid: ResourceId,
    /// The new version.
    pub(crate) version: String,
    /// The running version.
    pub(crate) current_version: String,
    /// The release date.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) date: Option<String>,
    /// The release notes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) body: Option<String>,
    /// The feed entry as received.
    pub(crate) raw: Value,
}

/// A download progress event (`tauri-plugin-updater`'s shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", content = "data")]
pub(crate) enum DownloadEvent {
    /// The download started.
    #[serde(rename_all = "camelCase")]
    Started {
        /// The body length, when known.
        content_length: Option<u64>,
    },
    /// A chunk arrived.
    #[serde(rename_all = "camelCase")]
    Progress {
        /// Its length.
        chunk_length: usize,
    },
    /// The download finished.
    Finished,
}

fn unavailable() -> Error {
    Error::unsupported(if cfg!(all(feature = "updater", windows)) {
        "the update client is not available in this build yet"
    } else {
        "the update client needs Windows and the `updater` feature"
    })
}

/// `check(options)`.
#[tauri::command]
pub(crate) async fn updater_check<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    options: Option<CheckOptions>,
) -> Result<Option<UpdateMetadata>> {
    require_app_webview(core(&state), &webview)?;
    let _ = options;
    Err(unavailable())
}

/// `Update.download(onEvent)`.
#[tauri::command]
pub(crate) async fn updater_download<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    rid: ResourceId,
    on_event: Channel<DownloadEvent>,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    let _ = (rid, on_event);
    Err(unavailable())
}

/// `Update.install()`.
#[tauri::command]
pub(crate) async fn updater_install<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    rid: ResourceId,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    let _ = rid;
    Err(unavailable())
}

/// `Update.downloadAndInstall(onEvent)`.
#[tauri::command]
pub(crate) async fn updater_download_and_install<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    rid: ResourceId,
    on_event: Channel<DownloadEvent>,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    let _ = (rid, on_event);
    Err(unavailable())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_events_match_the_official_updater() {
        let json = |e: DownloadEvent| serde_json::to_value(e).unwrap();
        assert_eq!(
            json(DownloadEvent::Started {
                content_length: Some(3)
            }),
            serde_json::json!({ "event": "Started", "data": { "contentLength": 3 } })
        );
        assert_eq!(
            json(DownloadEvent::Progress { chunk_length: 2 }),
            serde_json::json!({ "event": "Progress", "data": { "chunkLength": 2 } })
        );
        assert_eq!(
            json(DownloadEvent::Finished),
            serde_json::json!({ "event": "Finished" })
        );
        assert!(
            serde_json::from_value::<CheckOptions>(serde_json::json!({ "headers": {} })).is_err()
        );
    }
}
