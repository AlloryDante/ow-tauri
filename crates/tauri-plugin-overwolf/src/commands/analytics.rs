//! Analytics switches, the persisted preference, the payment user id and
//! the user switch (DESIGN §3.5).

use serde_json::{Map, Value};
use tauri::{Runtime, State, Webview};

use super::{core, require_app_webview};
use crate::error::Result;
use crate::ext::Overwolf;

/// `disableAnonymousAnalytics()`.
#[tauri::command]
pub(crate) async fn disable_anonymous_analytics<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    state.disable_anonymous_analytics();
    Ok(())
}

/// `disableAdsOptimization()`.
#[tauri::command]
pub(crate) async fn disable_ads_optimization<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    state.disable_ads_optimization();
    Ok(())
}

/// `disableAdsFPD()`.
#[tauri::command]
pub(crate) async fn disable_ads_fpd<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    state.disable_ads_fpd();
    Ok(())
}

/// `setExternalPaymentUserId(options)` (`overwolf:analytics`); the options
/// keep their key order (E.2 #10).
#[tauri::command]
pub(crate) async fn set_external_payment_user_id<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    options: Option<Map<String, Value>>,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    state
        .set_external_payment_user_id(&options.unwrap_or_default())
        .await
}

/// `setAnalyticsUserEnabled(enabled)` (`overwolf:analytics`).
#[tauri::command]
pub(crate) async fn set_analytics_user_enabled<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    enabled: bool,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    state.set_analytics_user_enabled(enabled)
}

/// `setAnonymousAnalyticsPreference(enabled)` (`overwolf:analytics`).
#[tauri::command]
pub(crate) async fn set_anonymous_analytics_preference<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    enabled: bool,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    state.set_anonymous_analytics_preference(enabled)
}
