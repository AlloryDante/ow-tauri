//! Identity, window naming and email hash commands (DESIGN §3.5).

use serde_json::Value;
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

/// The argument of `set_user_email_hashes`: `{ value }`, where a missing
/// `value` is JavaScript `undefined` and `null` stays `null` (L1: the two
/// differ in ow-electron).
#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct SetEmailHashes {
    /// The app's argument; `None` when it passed `undefined` or nothing.
    #[serde(default, deserialize_with = "present")]
    value: Option<Value>,
}

/// Deserialises a present field, `null` included, as `Some`.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<Value>, D::Error> {
    <Value as serde::Deserialize>::deserialize(d).map(Some)
}

/// `setUserEmailHashes(hashes)` (`overwolf:email-hashes`): see
/// [`Overwolf::set_user_email_hashes_value`].
#[tauri::command]
pub(crate) async fn set_user_email_hashes<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    hashes: SetEmailHashes,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    state.set_user_email_hashes_value(hashes.value);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// L1: a missing `value` is `undefined` (removes `eHashes`); `null` is
    /// stored as `null`.
    #[test]
    fn set_email_hashes_tells_undefined_from_null() {
        let parse = |json: &str| serde_json::from_str::<SetEmailHashes>(json).unwrap().value;
        assert_eq!(parse("{}"), None);
        assert_eq!(parse(r#"{"value":null}"#), Some(Value::Null));
        assert_eq!(parse(r#"{"value":""}"#), Some(Value::String(String::new())));
        assert_eq!(
            parse(r#"{"value":{"sha256":"a","x":1}}"#),
            Some(serde_json::json!({"sha256":"a","x":1}))
        );
    }
}
