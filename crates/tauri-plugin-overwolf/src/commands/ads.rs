//! Ads, consent, analytics and packages commands (CONTRACT A.2.2, A.2.4,
//! A.2.5, A.2.6, A.2.7).

use serde_json::{Map, Value};
use tauri::ipc::Request;
use tauri::{Runtime, State, Webview};

use super::{body, forbidden, host, require_main, require_ui};
use crate::ads::{ADVIEW_SCOPE, AdviewCommandName, AdviewEvent, AdviewMount, AdviewUpdate};
use crate::config::filter_pending_browser_args;
use crate::consent::{CmpEventData, CmpEventName, CmpWindowOptions};
use crate::error::{Error, Result};
use crate::ext::Overwolf;
use crate::state::log::LogLevel;
use crate::window::{WebviewClass, classify};

/// `adview_mount` result.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Mounted {
    guest_label: String,
}

#[tauri::command]
pub(crate) async fn is_cmp_required<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<bool> {
    require_main(&webview)?;
    Ok(host(&state).is_cmp_required().await)
}

#[tauri::command]
pub(crate) async fn open_cmp_window<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    options: Option<CmpWindowOptions>,
) -> Result<()> {
    require_main(&webview)?;
    host(&state).open_cmp_window(&options.unwrap_or_default())
}

#[tauri::command]
pub(crate) async fn open_ad_privacy_settings_window<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    options: Option<CmpWindowOptions>,
) -> Result<()> {
    require_main(&webview)?;
    host(&state).open_cmp_window(&options.unwrap_or_default())
}

#[tauri::command]
pub(crate) async fn set_user_email_hashes<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    hashes: Option<Map<String, Value>>,
) -> Result<()> {
    require_main(&webview)?;
    host(&state).send_email_hashes(hashes.as_ref());
    Ok(())
}

#[tauri::command]
pub(crate) async fn set_external_payment_user_id<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    options: Option<Map<String, Value>>,
) -> Result<()> {
    require_main(&webview)?;
    let host = host(&state);
    let options = options.unwrap_or_default();
    let user_id_ok = match options.get("userId") {
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(_)) => true,
        _ => false,
    };
    if !user_id_ok {
        let message = "providerName and userId are mandatory";
        return Err(
            Error::invalid_argument(message).with_data(serde_json::json!({ "message": message }))
        );
    }
    if !host.with_core(|c| c.main_ready) {
        return Err(Error::not_ready("ow-electron is not ready yet!"));
    }
    host.analytics_sub_info(options).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn analytics_set_user_enabled<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    enabled: bool,
) -> Result<()> {
    require_main(&webview)?;
    let host = host(&state);
    if !host.info.config.analytics.user_switch {
        return Err(Error::unsupported(
            "analytics_set_user_enabled needs plugins.overwolf.analytics.userSwitch.",
        ));
    }
    host.set_analytics_user_enabled(enabled);
    host.ow_tauri
        .update(|s| s.analytics_user_enabled = Some(enabled))
        .map_err(|e| Error::from_io("ow-tauri.json", &e))
}

#[tauri::command]
pub(crate) async fn app_record_browser_args<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    args: Vec<String>,
) -> Result<()> {
    require_main(&webview)?;
    let host = host(&state);
    let args = filter_pending_browser_args(&args);
    if let Err(err) = host.ow_tauri.update(|s| s.pending_browser_args = args) {
        host.log(
            LogLevel::Warn,
            &format!("could not record browser switches: {}", err.kind()),
        );
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn packages_snapshot<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<Value> {
    require_main(&webview)?;
    Ok(host(&state)
        .with_core(|c| c.state.get("packages").cloned())
        .unwrap_or(Value::Null))
}

#[tauri::command]
pub(crate) async fn packages_relaunch<R: Runtime>(webview: Webview<R>) -> Result<()> {
    require_main(&webview)?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn packages_set_channel<R: Runtime>(
    webview: Webview<R>,
    name: String,
    channel: Option<String>,
) -> Result<()> {
    require_main(&webview)?;
    let _ = channel;
    Err(crate::packages::set_channel_error(&name))
}

#[tauri::command]
pub(crate) async fn packages_get_available_channels<R: Runtime>(
    webview: Webview<R>,
    names: Vec<String>,
) -> Result<Value> {
    require_main(&webview)?;
    crate::packages::get_available_channels(&names)
}

#[tauri::command]
pub(crate) async fn packages_get_channel<R: Runtime>(
    webview: Webview<R>,
    names: Option<Vec<String>>,
) -> Result<Value> {
    require_main(&webview)?;
    Ok(crate::packages::get_channel(&names.unwrap_or_default()))
}

#[tauri::command]
pub(crate) async fn adview_mount<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    request: Request<'_>,
) -> Result<Mounted> {
    let host = host(&state);
    require_ui(&webview, host)?;
    let mount: AdviewMount = body(&request, "adview_mount")?;
    let guest_label = host.mount_guest(&webview, mount)?;
    Ok(Mounted { guest_label })
}

#[tauri::command]
pub(crate) async fn adview_update<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    request: Request<'_>,
) -> Result<()> {
    let host = host(&state);
    require_ui(&webview, host)?;
    let update: AdviewUpdate = body(&request, "adview_update")?;
    host.update_guest(&webview, update)
}

#[tauri::command]
pub(crate) async fn adview_unmount<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    element_id: String,
) -> Result<()> {
    let host = host(&state);
    require_ui(&webview, host)?;
    host.unmount_guest(webview.label(), &element_id);
    Ok(())
}

#[tauri::command]
pub(crate) async fn adview_command<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    element_id: String,
    command: AdviewCommandName,
    args: Option<Vec<Value>>,
) -> Result<()> {
    let host = host(&state);
    require_ui(&webview, host)?;
    host.guest_command(
        webview.label(),
        &element_id,
        command,
        &args.unwrap_or_default(),
    )
}

#[tauri::command]
pub(crate) async fn adview_event<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    request: Request<'_>,
) -> Result<()> {
    let host = host(&state);
    let label = webview.label().to_owned();
    let registered = classify(&label) == WebviewClass::AdviewGuest
        && host.with_core(|c| c.ads.guests.contains_key(&label));
    let in_scope = webview
        .url()
        .is_ok_and(|u| u.scheme() == "https" && u.as_str().starts_with(ADVIEW_SCOPE));
    if !registered || !in_scope {
        return Err(forbidden(&label));
    }
    let event: AdviewEvent = body(&request, "adview_event")?;
    host.guest_event(&label, event)
}

#[tauri::command]
pub(crate) async fn cmp_event<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    name: CmpEventName,
    data: Option<CmpEventData>,
) -> Result<()> {
    let label = webview.label().to_owned();
    if classify(&label) != WebviewClass::Cmp || webview.window().label() != label {
        return Err(forbidden(&label));
    }
    let url = webview.url().ok();
    host(&state).cmp_event(&label, url.as_ref(), name, data)
}
