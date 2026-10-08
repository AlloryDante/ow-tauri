//! `<owadview>` commands (DESIGN §3.5, §4.4) and the ad guests' own
//! command. The guests themselves live in `host::ads`.

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
    let guest_label = crate::host::ads::mount(core, &webview, request, on_event).await?;
    Ok(Mounted { guest_label })
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
    crate::host::ads::update(core, &webview, request)
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
    crate::host::ads::unmount(core, &webview, &element_id);
    Ok(())
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
    crate::host::ads::command(
        core,
        &webview,
        &element_id,
        command,
        args.as_deref().unwrap_or_default(),
    )
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
    // The guest capability admits the ad page's scope only; checked again
    // here (§7.3): a guest webview off the ad page is no live guest.
    if !webview
        .url()
        .is_ok_and(|url| crate::host::ads::guest_url_allowed(&url))
    {
        return Err(Error::not_found(format!("{label} is not on the ad page")));
    }
    crate::host::ads::guest_event(core(&state), label, slot_id.as_deref(), &name, data)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tauri::Manager;

    use super::*;
    use crate::error::ErrorCode;
    use crate::host::windows::tests::{Capture, mock_app, window};

    /// §7.3: only an `owad-*` webview on the ad page may send guest events;
    /// the label, not the claimed slot id, names the guest.
    #[test]
    fn guest_events_come_only_from_the_ad_page() {
        let (app, dir, _core) = mock_app(
            "commands-adview-event",
            &json!({}),
            &[],
            Capture::answering("{}"),
        );
        window(&app, "main");
        window(&app, "owad-7");
        let send = |label: &str| {
            let webview = crate::compat::webview(&app, label).unwrap();
            adview_event(
                webview,
                app.state::<Overwolf<tauri::test::MockRuntime>>(),
                Some("owad-1".into()),
                "impression".into(),
                None,
            )
        };
        let code = |r: Result<()>| r.unwrap_err().code();
        assert_eq!(code(send("main")), ErrorCode::Forbidden, "not an ad guest");
        let off_page = send("owad-7").unwrap_err();
        assert_eq!(off_page.code(), ErrorCode::NotFound);
        assert!(off_page.to_string().contains("not on the ad page"));
        let guest = crate::compat::webview(&app, "owad-7").unwrap();
        guest
            .navigate(url::Url::parse(crate::ads::ADVIEW_URL).unwrap())
            .unwrap();
        let on_page = send("owad-7").unwrap_err();
        assert_eq!(on_page.code(), ErrorCode::NotFound);
        assert!(
            on_page.to_string().contains("no ad guest owad-7"),
            "{on_page}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
