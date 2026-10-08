//! Identity, window naming and email hash commands (DESIGN §3.5).

use tauri::{Runtime, State, Webview};

use super::{core, require_app_webview};
use crate::error::Result;
use crate::ext::Overwolf;
use crate::identity::EmailHashes;
use crate::types::{Info, MachineIds};

/// `setWindowName(name)`: names the caller's own window (SEC-m1).
#[tauri::command]
pub(crate) async fn set_window_name<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    name: String,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    state.set_window_name(webview.window().label(), &name)
}

/// `getInfo()`: no machine ids (R7).
#[tauri::command]
pub(crate) async fn get_info<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<Info> {
    require_app_webview(core(&state), &webview)?;
    Ok(state.info())
}

/// `getMachineIds()` (`overwolf:machine-id`).
#[tauri::command]
pub(crate) async fn get_machine_ids<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<MachineIds> {
    require_app_webview(core(&state), &webview)?;
    Ok(MachineIds::new(state.muid(), state.muid_v2()))
}

/// `generateUserEmailHashes(email)` (`overwolf:email-hashes`).
#[tauri::command]
pub(crate) async fn generate_user_email_hashes<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    email: String,
) -> Result<EmailHashes> {
    require_app_webview(core(&state), &webview)?;
    Ok(state.generate_user_email_hashes(&email))
}

/// `setUserEmailHashes(hashes)` (`overwolf:email-hashes`).
#[tauri::command]
pub(crate) async fn set_user_email_hashes<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    hashes: Option<EmailHashes>,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    if let Some(hashes) = hashes {
        state.set_user_email_hashes(&hashes);
    }
    Ok(())
}

/// `clearUserEmailHashes()` (`overwolf:email-hashes`).
#[tauri::command]
pub(crate) async fn clear_user_email_hashes<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    state.clear_user_email_hashes()
}
