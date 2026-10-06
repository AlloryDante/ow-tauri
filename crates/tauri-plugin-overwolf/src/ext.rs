//! The Rust API (CONTRACT A.5): [`Overwolf`] and [`OverwolfExt`].

use std::path::Path;
use std::sync::Arc;

use serde_json::{Map, Value};
use tauri::{Manager, Runtime};

use crate::config::Config;
use crate::host::Host;
use crate::identity::{EmailHashes, email_hashes};
use crate::ipc::messages::HostMessage;
use crate::manifest::EmbeddedManifest;
use crate::snapshot::Flags;
use crate::state::log::LogLevel;

/// The plugin's per-app state, reachable from any Tauri manager through
/// [`OverwolfExt::overwolf`] once the plugin's setup has run.
pub struct Overwolf<R: Runtime>(pub(crate) Arc<Host<R>>);

impl<R: Runtime> std::fmt::Debug for Overwolf<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Overwolf")
            .field("uid", &self.uid())
            .finish_non_exhaustive()
    }
}

impl<R: Runtime> Overwolf<R> {
    /// The effective app uid (G.2).
    #[must_use]
    pub fn uid(&self) -> &str {
        &self.0.info.identity.uid
    }

    /// The computed uid, even when an override applies (G.2).
    #[must_use]
    pub fn cuid(&self) -> &str {
        &self.0.info.identity.cuid
    }

    /// The machine/user id used by analytics (E.4).
    #[must_use]
    pub fn muid(&self) -> &str {
        &self.0.info.muid
    }

    /// Equals [`Overwolf::muid`] (OQ-02).
    #[must_use]
    pub fn muid_v2(&self) -> &str {
        &self.0.info.muid
    }

    /// The phase percent derived from the muid.
    #[must_use]
    pub fn phase_percent(&self) -> u8 {
        self.0.info.phase_percent
    }

    /// `ow-electron.json` `utmParams`, or `null`.
    #[must_use]
    pub fn utm_params(&self) -> &Value {
        &self.0.info.utm_params
    }

    /// The embedded manifest.
    #[must_use]
    pub fn manifest(&self) -> &EmbeddedManifest {
        &self.0.info.manifest
    }

    /// The effective configuration (file, builder, environment, switches).
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.0.info.config
    }

    /// The per-app state directory `<appData>/ow-electron/<uid>` (F.1).
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        self.0.info.state_dir.root()
    }

    /// The session switches.
    #[must_use]
    pub fn flags(&self) -> Flags {
        self.0.with_core(|c| c.flags)
    }

    fn set_flag(&self, path: &str, edit: impl FnOnce(&mut Flags)) {
        self.0.with_core(|c| {
            edit(&mut c.flags);
            c.patch(vec![(format!("flags.{path}"), Value::Bool(true))]);
        });
    }

    /// `app.overwolf.disableAnonymousAnalytics()` (A.2.2).
    pub fn disable_anonymous_analytics(&self) {
        self.set_flag("anonymousAnalyticsDisabled", |f| {
            f.anonymous_analytics_disabled = true;
        });
    }

    /// `disableAdsOptimization()` (A.2.2).
    pub fn disable_ads_optimization(&self) {
        self.set_flag("adsOptimizationDisabled", |f| {
            f.ads_optimization_disabled = true;
        });
    }

    /// `disableAdsFPD()` (A.2.2).
    pub fn disable_ads_fpd(&self) {
        self.set_flag("adsFpdDisabled", |f| f.ads_fpd_disabled = true);
    }

    /// Hashes an email address with the configured encoding (A.2.2).
    #[must_use]
    pub fn generate_user_email_hashes(&self, email: &str) -> EmailHashes {
        email_hashes(email, self.0.info.config.email_hashes.encoding)
    }

    /// Fires `app.on('second-instance')` in `ow-main`. Call it from the app's
    /// `tauri-plugin-single-instance` callback (A.5).
    pub fn emit_second_instance(&self, argv: Vec<String>, cwd: String) {
        let mut extra = Map::new();
        extra.insert("argv".into(), Value::from(argv));
        extra.insert("cwd".into(), Value::from(cwd));
        self.0.send_main(HostMessage::Lifecycle {
            event: "second-instance".into(),
            request_id: None,
            exit_code: None,
            extra,
        });
    }

    /// Reports that the `ow-main` render process died (A.6). Apps forward
    /// `tauri::Builder::on_web_content_process_terminate` (macOS) here for
    /// the webview labelled `ow-main`.
    pub fn report_main_webview_crash(&self) {
        self.0.main_crashed();
    }

    /// Starts the graceful quit sequence (A.6), as `app.quit()` does.
    pub fn quit(&self) {
        self.0.begin_quit(0);
    }

    /// Appends a line to the ow-tauri log (F.4).
    pub fn log(&self, level: LogLevel, message: &str) {
        self.0.log(level, message);
    }
}

/// Drives the plugin's Tauri event handlers directly, for tests on Tauri's
/// mock runtime, which emits no window, navigation or page-load events. Not
/// part of the stable API.
#[cfg(feature = "test-util")]
#[doc(hidden)]
impl<R: Runtime> Overwolf<R> {
    /// As if the window `label` sent `WindowEvent::Destroyed`.
    pub fn test_window_destroyed(&self, label: &str) {
        crate::plugin::window_event(&self.0, label, &tauri::WindowEvent::Destroyed);
    }

    /// As if the webview `label` asked to navigate to `url`; returns whether
    /// the navigation may proceed.
    #[must_use]
    pub fn test_navigation(&self, label: &str, url: &url::Url) -> bool {
        crate::plugin::navigation(&self.0, label, url)
    }

    /// As if the webview `label` reported a page load of `url`.
    pub fn test_page_load(&self, label: &str, url: &url::Url, finished: bool) {
        let event = if finished {
            tauri::webview::PageLoadEvent::Finished
        } else {
            tauri::webview::PageLoadEvent::Started
        };
        crate::plugin::page_load(&self.0, label, event, url);
    }

    /// As if the OS asked the app to exit (`ExitRequested` without a code,
    /// after the plugin prevented it).
    pub fn test_exit_requested(&self) {
        self.0.exit_requested();
    }

    /// Whether a soft restart of `ow-main` is in progress.
    #[must_use]
    pub fn test_soft_restart_pending(&self) -> bool {
        self.0.with_core(|c| c.soft_restart.is_some())
    }

    /// Whether the app is exiting through the plugin.
    #[must_use]
    pub fn test_exiting(&self) -> bool {
        self.0.with_core(|c| c.exiting)
    }
}

/// Access to [`Overwolf`] from `App`, `AppHandle`, `Window`, `Webview` and
/// `WebviewWindow`.
///
/// ```no_run
/// use tauri_plugin_overwolf::OverwolfExt;
/// fn show_uid<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
///     println!("uid {}", app.overwolf().uid());
/// }
/// ```
pub trait OverwolfExt<R: Runtime> {
    /// The plugin state.
    ///
    /// # Panics
    ///
    /// When called before the plugin's setup has run (the state is not
    /// registered yet).
    fn overwolf(&self) -> &Overwolf<R>;
}

impl<R: Runtime, T: Manager<R>> OverwolfExt<R> for T {
    fn overwolf(&self) -> &Overwolf<R> {
        self.state::<Overwolf<R>>().inner()
    }
}
