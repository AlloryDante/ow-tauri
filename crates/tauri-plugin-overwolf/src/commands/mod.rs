//! The plugin's commands (DESIGN §3.5): 25, listed in [`list::COMMANDS`].
//!
//! Every app command first passes [`require_app_webview`] (DESIGN §4.5), so
//! a mis-scoped capability cannot hand a plugin webview or a remote page the
//! app's commands; the two guest commands check their own label class.
//! Mobile builds register the same names, all answering `unsupported`
//! (`crate::mobile`).

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod ads;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod analytics;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod consent;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod info;
pub(crate) mod list;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod updater;

use std::sync::Arc;

use tauri::{Runtime, State, Webview};
use url::Url;

use crate::error::{Error, Result};
use crate::ext::Overwolf;
use crate::host::Core;

/// The [`Core`] behind the managed state.
#[cfg_attr(
    any(target_os = "android", target_os = "ios"),
    allow(dead_code, reason = "mobile builds register no commands")
)]
pub(crate) fn core<'a, R: Runtime>(state: &'a State<'_, Overwolf<R>>) -> &'a Arc<Core<R>> {
    &state.inner().0
}

/// The origin of `url` as `scheme://host[:port]`, also for schemes whose
/// WHATWG origin is opaque (`tauri://localhost`).
pub(crate) fn origin_of(url: &Url) -> String {
    match url.scheme() {
        "http" | "https" => url.origin().ascii_serialization(),
        scheme => match (url.host_str(), url.port()) {
            (Some(host), Some(port)) => format!("{scheme}://{host}:{port}"),
            (Some(host), None) => format!("{scheme}://{host}"),
            (None, _) => format!("{scheme}:"),
        },
    }
}

/// The origins Tauri serves app assets from, on every OS (custom protocol,
/// with and without `useHttpsScheme`).
pub(crate) const APP_ORIGINS: [&str; 3] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// Whether a page at `url` is a local app page (DESIGN §4.5): an app
/// origin, `build.devUrl`'s origin in a debug build, or one of
/// `ads.allowedEmbedderOrigins`.
pub(crate) fn is_app_page(url: &Url, dev_origin: Option<&str>, allowed: &[String]) -> bool {
    let origin = origin_of(url);
    APP_ORIGINS.contains(&origin.as_str())
        || (cfg!(debug_assertions) && dev_origin.is_some_and(|d| d.trim_end_matches('/') == origin))
        || allowed.iter().any(|o| o.trim_end_matches('/') == origin)
}

/// The caller gate of every app command (DESIGN §4.5): the webview is not a
/// plugin webview (`owad-*`, `ow-cmp*`) and shows a local app page.
///
/// # Errors
///
/// `forbidden` otherwise.
#[cfg_attr(
    any(target_os = "android", target_os = "ios"),
    allow(dead_code, reason = "mobile builds register no commands")
)]
pub(crate) fn require_app_webview<R: Runtime>(core: &Core<R>, webview: &Webview<R>) -> Result<()> {
    let label = webview.label();
    let refused = || {
        Error::forbidden(format!(
            "tauri-plugin-overwolf: {label} is not a local app webview"
        ))
    };
    if crate::config::is_reserved_label(label) {
        return Err(refused());
    }
    let url = webview.url().map_err(|_| refused())?;
    let id = &core.identity;
    if is_app_page(
        &url,
        id.dev_origin.as_deref(),
        &id.config.ads.allowed_embedder_origins,
    ) {
        Ok(())
    } else {
        Err(refused())
    }
}

/// The invoke handler with every command of [`list::COMMANDS`].
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static
{
    tauri::generate_handler![
        ads::adview_mount,
        ads::adview_update,
        ads::adview_unmount,
        ads::adview_command,
        info::set_window_name,
        info::get_info,
        info::get_machine_ids,
        consent::is_cmp_required,
        consent::open_ad_privacy_settings_window,
        consent::open_cmp_window,
        analytics::disable_anonymous_analytics,
        analytics::disable_ads_optimization,
        analytics::disable_ads_fpd,
        info::generate_user_email_hashes,
        info::set_user_email_hashes,
        info::clear_user_email_hashes,
        analytics::set_external_payment_user_id,
        analytics::set_analytics_user_enabled,
        analytics::set_anonymous_analytics_preference,
        updater::updater_check,
        updater::updater_download,
        updater::updater_install,
        updater::updater_download_and_install,
        ads::adview_event,
        consent::cmp_event,
    ]
}

/// The invoke handler on mobile: every command answers `unsupported`.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static
{
    crate::mobile::handler()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_pages() {
        let url = |s: &str| Url::parse(s).unwrap();
        assert!(is_app_page(&url("tauri://localhost/index.html"), None, &[]));
        assert!(is_app_page(&url("http://tauri.localhost/"), None, &[]));
        assert!(is_app_page(&url("https://tauri.localhost/a?b"), None, &[]));
        assert!(!is_app_page(&url("https://example.com/"), None, &[]));
        assert!(!is_app_page(&url("tauri://localhost.evil/"), None, &[]));
        assert!(!is_app_page(
            &url("http://tauri.localhost:8080/"),
            None,
            &[]
        ));
        let allowed = vec!["http://localhost:9527/".to_owned()];
        assert!(is_app_page(&url("http://localhost:9527/x"), None, &allowed));
        assert_eq!(
            is_app_page(
                &url("http://localhost:1420/"),
                Some("http://localhost:1420"),
                &[]
            ),
            cfg!(debug_assertions),
        );
        assert_eq!(origin_of(&url("tauri://localhost/x")), "tauri://localhost");
    }

    /// The handler, the command list and the permission files agree.
    #[test]
    fn commands_permissions_and_handler_agree() {
        assert_eq!(list::COMMANDS.len(), 25);
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let generated = manifest.join("permissions/autogenerated/commands");
        let mut files: Vec<String> = std::fs::read_dir(&generated)
            .unwrap()
            .map(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .trim_end_matches(".toml")
                    .to_owned()
            })
            .collect();
        files.sort();
        let mut listed: Vec<String> = list::COMMANDS.iter().map(|c| (*c).to_owned()).collect();
        listed.sort();
        assert_eq!(files, listed);
        let default = std::fs::read_to_string(manifest.join("permissions/default.toml")).unwrap();
        let granted = default.matches("\"allow-").count();
        assert_eq!(granted, 12, "overwolf:default grants exactly 12 commands");
        // Every command name in the handler macro is in the list: the
        // source of the handler names them all.
        let source = include_str!("mod.rs");
        let handler = source
            .split("generate_handler![")
            .nth(1)
            .and_then(|s| s.split(']').next())
            .unwrap();
        let mut names: Vec<String> = handler
            .split(',')
            .filter_map(|s| s.trim().rsplit("::").next().map(str::to_owned))
            .filter(|s| !s.is_empty())
            .collect();
        names.sort();
        assert_eq!(names, listed);
    }
}
