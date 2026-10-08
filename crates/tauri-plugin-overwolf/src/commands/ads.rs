//! `<owadview>` commands (DESIGN §3.5, §4.4) and the ad guests' own
//! command. Signatures are final; guest hosting arrives in W2, until then
//! `adview_mount` answers `unsupported` and no element is ever mounted.

use serde::Serialize;
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{Runtime, State, Webview};

use super::{core, require_app_webview};
use crate::ads::{
    AdviewCommandName, AdviewMount, AdviewUpdate, ChannelMessage, valid_element_id, valid_geometry,
};
use crate::config::ADVIEW_LABEL_PREFIX;
use crate::error::{Error, Result};
use crate::ext::Overwolf;

/// What `adview_mount` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Mounted {
    /// The guest webview's label (`owad-<n>`).
    pub(crate) guest_label: String,
}

/// The element `element_id` of `webview`, or `not-found` (DESIGN §4.5: an
/// element mounted by another webview is never found).
fn mounted<R: Runtime>(
    core: &crate::host::Core<R>,
    webview: &Webview<R>,
    element_id: &str,
) -> Result<crate::host::ads::Mount> {
    core.ads
        .mount_of(webview.label(), element_id)
        .ok_or_else(|| Error::not_found(format!("no mounted element {element_id}")))
}

/// Mounts an `<owadview>` element: creates its guest.
#[tauri::command]
pub(crate) async fn adview_mount<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    request: AdviewMount,
    on_event: Channel<ChannelMessage>,
) -> Result<Mounted> {
    let core = core(&state);
    require_app_webview(core, &webview)?;
    if !valid_element_id(&request.element_id)
        || !valid_geometry(
            &request.rect,
            request.device_pixel_ratio,
            request.inner_width,
        )
    {
        return Err(Error::invalid_argument("invalid element id or geometry"));
    }
    // 400025 never precedes the launch burst (DESIGN §4.2).
    core.lifecycle.wait_started().await;
    drop(on_event);
    if !core.ads.supported() {
        return Err(Error::unsupported(if cfg!(target_os = "linux") {
            "ads are not available on Linux"
        } else {
            "ads are not available in this build"
        }));
    }
    Err(Error::unsupported(
        "ad guests are not available in this build yet",
    ))
}

/// Updates a mounted element (rectangle, visibility, attributes).
#[tauri::command]
pub(crate) async fn adview_update<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    request: AdviewUpdate,
) -> Result<()> {
    let core = core(&state);
    require_app_webview(core, &webview)?;
    mounted(core, &webview, &request.element_id)?;
    Err(Error::unsupported(
        "ad guests are not available in this build yet",
    ))
}

/// Unmounts an element: destroys its guest.
#[tauri::command]
pub(crate) async fn adview_unmount<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    element_id: String,
) -> Result<()> {
    let core = core(&state);
    require_app_webview(core, &webview)?;
    mounted(core, &webview, &element_id)?;
    Err(Error::unsupported(
        "ad guests are not available in this build yet",
    ))
}

/// An element method (`setAudioMuted`, `reload`, `setPageUrl`,
/// `sendCommand`, CONTRACT B.3.3).
#[tauri::command]
pub(crate) async fn adview_command<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    element_id: String,
    command: AdviewCommandName,
    args: Option<Vec<Value>>,
) -> Result<()> {
    let core = core(&state);
    require_app_webview(core, &webview)?;
    mounted(core, &webview, &element_id)?;
    let _ = (command, args);
    Err(Error::unsupported(
        "ad guests are not available in this build yet",
    ))
}

/// A guest page event (CONTRACT A.2.6). Synchronous, so a guest's events
/// keep their order. Only `owad-*` webviews may call it (and only through
/// the runtime capability the plugin adds for them).
#[tauri::command]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Tauri passes command arguments by value"
)]
pub(crate) fn adview_event<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    slot_id: Option<String>,
    name: String,
    data: Option<Value>,
) -> Result<()> {
    let label = webview.label();
    if !label.starts_with(ADVIEW_LABEL_PREFIX) {
        return Err(Error::forbidden(format!("{label} is not an ad guest")));
    }
    let _ = (core(&state), slot_id, name, data);
    Err(Error::not_found(format!("no ad guest {label}")))
}
