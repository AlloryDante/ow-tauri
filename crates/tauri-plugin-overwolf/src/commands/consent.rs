//! Consent commands (DESIGN §3.5, §4.7) and the consent windows' own
//! command.

use serde_json::Value;
use tauri::{Runtime, State, Webview};

use super::{core, require_app_webview};
use crate::config::CMP_LABEL_PREFIX;
use crate::error::{Error, Result};
use crate::ext::Overwolf;
use crate::types::CmpWindowOptions;

/// `isCMPRequired()`: never fails for an app webview.
#[tauri::command]
pub(crate) async fn is_cmp_required<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<bool> {
    let core = core(&state);
    require_app_webview(core, &webview)?;
    Ok(core.consent.is_cmp_required())
}

/// Opens the settings window for a JavaScript caller: `cmpURL` must match
/// `consent.allowedCmpOrigins` (SEC-M1); a modal window without a parent
/// is parented to the caller's window.
fn open<R: Runtime>(
    core: &std::sync::Arc<crate::host::Core<R>>,
    webview: &Webview<R>,
    options: Option<CmpWindowOptions>,
) -> Result<()> {
    require_app_webview(core, webview)?;
    let options = options.unwrap_or_default();
    if let Some(url) = &options.cmp_url {
        crate::host::consent::check_js_cmp_url(
            url,
            &core.identity.config.consent.allowed_cmp_origins,
        )?;
    }
    let caller = webview.window().label().to_owned();
    core.consent
        .open_settings_window(core, &options, Some(&caller))
}

/// `openAdPrivacySettingsWindow(options)`.
#[tauri::command]
pub(crate) async fn open_ad_privacy_settings_window<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    options: Option<CmpWindowOptions>,
) -> Result<()> {
    open(core(&state), &webview, options)
}

/// `openCMPWindow(options)`, the deprecated alias.
#[tauri::command]
pub(crate) async fn open_cmp_window<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    options: Option<CmpWindowOptions>,
) -> Result<()> {
    open(core(&state), &webview, options)
}

/// A consent page message (CONTRACT D.6.6). Synchronous; only `ow-cmp*`
/// webviews may call it (runtime capability).
#[tauri::command]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Tauri passes command arguments by value"
)]
pub(crate) fn cmp_event<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    name: String,
    data: Option<Value>,
) -> Result<Value> {
    let label = webview.label();
    if !label.starts_with(CMP_LABEL_PREFIX) {
        return Err(Error::forbidden(format!("{label} is not a consent window")));
    }
    let _ = (core(&state), name, data);
    Err(Error::not_found(format!("no consent window {label}")))
}
