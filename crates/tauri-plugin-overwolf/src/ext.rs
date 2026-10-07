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
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// let uid = app.overwolf().uid();
    /// assert!(!uid.is_empty());
    /// # }
    /// ```
    #[must_use]
    pub fn uid(&self) -> &str {
        &self.0.info.identity.uid
    }

    /// The computed uid, even when an override applies (G.2).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// let ow = app.overwolf();
    /// if ow.uid() != ow.cuid() {
    ///     println!("a console-assigned uid is in use");
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn cuid(&self) -> &str {
        &self.0.info.identity.cuid
    }

    /// The machine/user id used by analytics (E.4).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// println!("muid {}", app.overwolf().muid());
    /// # }
    /// ```
    #[must_use]
    pub fn muid(&self) -> &str {
        &self.0.info.muid
    }

    /// `muidV2` (E.4): equal to [`Overwolf::muid`] except on Windows when the
    /// shared registry values differ.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// assert_eq!(app.overwolf().muid_v2().len(), 36);
    /// # }
    /// ```
    #[must_use]
    pub fn muid_v2(&self) -> &str {
        &self.0.info.muid_v2
    }

    /// The phase percent derived from the muid.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// assert!(app.overwolf().phase_percent() < 100);
    /// # }
    /// ```
    #[must_use]
    pub fn phase_percent(&self) -> u8 {
        self.0.info.phase_percent
    }

    /// `ow-electron.json` `utmParams`, or `None` when there are none.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// if let Some(source) = app.overwolf().utm_params().and_then(|u| u.get("utm_source")) {
    ///     println!("installed from {source}");
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn utm_params(&self) -> Option<&Value> {
        self.0.info.utm_params.as_ref()
    }

    /// The embedded manifest.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// println!("{} {}", app.overwolf().manifest().product_name, app.overwolf().manifest().version);
    /// # }
    /// ```
    #[must_use]
    pub fn manifest(&self) -> &EmbeddedManifest {
        &self.0.info.manifest
    }

    /// The update client (CONTRACT A.5, I): `configure`, `check`,
    /// `download` and `quit_and_install`, with the behaviour of the
    /// `updater_*` commands.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # async fn example(app: &tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
    /// if let Some(result) = app.overwolf().updater().check().await? {
    ///     println!("latest {}", result.update_info.version);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn updater(&self) -> &crate::updater::Updater<R> {
        &self.0.updater_api
    }

    /// The effective configuration (file, builder, environment, switches).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// let config = app.overwolf().config();
    /// println!("main document {}", config.main.url);
    /// # }
    /// ```
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.0.info.config
    }

    /// The per-app state directory `<appData>/ow-electron/<uid>` (F.1).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// let log_dir = app.overwolf().state_dir().join("logs");
    /// println!("{}", log_dir.display());
    /// # }
    /// ```
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        self.0.info.state_dir.root()
    }

    /// The session switches.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// if app.overwolf().flags().ads_fpd_disabled {
    ///     println!("first-party data is off for this session");
    /// }
    /// # }
    /// ```
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
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// app.overwolf().disable_anonymous_analytics();
    /// assert!(app.overwolf().flags().anonymous_analytics_disabled);
    /// # }
    /// ```
    pub fn disable_anonymous_analytics(&self) {
        self.set_flag("anonymousAnalyticsDisabled", |f| {
            f.anonymous_analytics_disabled = true;
        });
        self.0.disable_anonymous_analytics();
    }

    /// `disableAdsOptimization()` (A.2.2).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// app.overwolf().disable_ads_optimization();
    /// # }
    /// ```
    pub fn disable_ads_optimization(&self) {
        self.set_flag("adsOptimizationDisabled", |f| {
            f.ads_optimization_disabled = true;
        });
    }

    /// `disableAdsFPD()` (A.2.2).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// app.overwolf().disable_ads_fpd();
    /// # }
    /// ```
    pub fn disable_ads_fpd(&self) {
        self.set_flag("adsFpdDisabled", |f| f.ads_fpd_disabled = true);
    }

    /// Hashes an email address with the configured encoding (A.2.2).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// let hashes = app.overwolf().generate_user_email_hashes("someone@example.com");
    /// println!("{hashes:?}");
    /// # }
    /// ```
    #[must_use]
    pub fn generate_user_email_hashes(&self, email: &str) -> EmailHashes {
        email_hashes(email, self.0.info.config.email_hashes.encoding)
    }

    /// Fires `app.on('second-instance')` in `ow-main`. Call it from the app's
    /// `tauri-plugin-single-instance` callback (A.5).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// // In the `tauri-plugin-single-instance` callback:
    /// let argv = vec!["app".to_owned(), "--open=settings".to_owned()];
    /// app.overwolf().emit_second_instance(argv, "/".to_owned());
    /// # }
    /// ```
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
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// // From `tauri::Builder::on_web_content_process_terminate` for `ow-main`:
    /// app.overwolf().report_main_webview_crash();
    /// # }
    /// ```
    pub fn report_main_webview_crash(&self) {
        self.0.main_crashed();
    }

    /// Reports that the web content process of a webview ended. Apps forward
    /// every `tauri::Builder::on_web_content_process_terminate` call (macOS)
    /// here: for `ow-main` it is [`Self::report_main_webview_crash`], for an
    /// ad guest (`owad-*`) the crash recovery of D.7, for a consent window
    /// (`ow-cmp*`) its failure path (D.6.1), for a `BrowserWindow` webview
    /// (`bw-*`, `bwr-*`) its `render-process-gone` event (A.3). Other labels
    /// are ignored.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(webview: &tauri::Webview) {
    /// // From `tauri::Builder::on_web_content_process_terminate`:
    /// webview.overwolf().report_web_content_terminated(webview.label());
    /// # }
    /// ```
    pub fn report_web_content_terminated(&self, label: &str) {
        match crate::window::classify(label) {
            crate::window::WebviewClass::Main => self.0.main_crashed(),
            crate::window::WebviewClass::AdviewGuest | crate::window::WebviewClass::Cmp => {
                self.0.web_content_terminated(label);
            }
            crate::window::WebviewClass::Ui(id) | crate::window::WebviewClass::Remote(id) => {
                self.0
                    .window_render_process_gone(id, crate::ads::GoneReason::Crashed, 0);
            }
            crate::window::WebviewClass::Other => {}
        }
    }

    /// Lab mode (feature `lab`, `OW_TAURI_LAB_DIR` set): writes what every
    /// live ad guest's page sees now to `guest-<n>-<phase>.json` in the
    /// trace directory. Does nothing when the trace is off. For the parity
    /// harness only.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// app.overwolf().lab_probe_guests("end");
    /// # }
    /// ```
    #[cfg(feature = "lab")]
    pub fn lab_probe_guests(&self, phase: &str) {
        crate::lab::probe_guests(&self.0.app, phase);
    }

    /// Lab mode (feature `lab`, `OW_TAURI_LAB_DIR` set): appends `entry`, with
    /// the trace's `t` and `wall` fields, as one JSON line to `file` (a plain
    /// file name) in the trace directory, next to the plugin's own trace.
    /// Does nothing when the trace is off or the name is not a plain file
    /// name. For lab drivers only (the example's end-to-end run).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// app.overwolf()
    ///     .lab_record("e2e.jsonl", serde_json::json!({ "step": "start" }));
    /// # }
    /// ```
    #[cfg(feature = "lab")]
    pub fn lab_record(&self, file: &str, entry: Value) {
        crate::lab::record(file, || entry);
    }

    /// Starts the graceful quit sequence (A.6), as `app.quit()` does.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// app.overwolf().quit();
    /// # }
    /// ```
    pub fn quit(&self) {
        self.0.begin_quit(0);
    }

    /// Appends a line to the ow-tauri log (F.4).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// use tauri_plugin_overwolf::LogLevel;
    /// app.overwolf().log(LogLevel::Info, "tray menu opened");
    /// # }
    /// ```
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

    /// Milliseconds since setup on the host clock.
    #[must_use]
    pub fn test_now(&self) -> u64 {
        self.0.now()
    }

    /// As `main_ready`: starts this launch's consent round (D.6.1).
    pub fn test_start_consent(&self) {
        self.0.start_consent();
    }

    /// Whether ad guests may start their first navigation (D.6.5).
    #[must_use]
    pub fn test_consent_gate_open(&self) -> bool {
        self.0.consent_gate_open()
    }

    /// The hidden consent windows that are still open.
    #[must_use]
    pub fn test_hidden_consent_windows(&self) -> Vec<String> {
        self.0
            .with_core(|c| c.consent.hidden.keys().cloned().collect())
    }

    /// `isCMPRequired()` (D.6.2).
    pub async fn test_is_cmp_required(&self) -> bool {
        self.0.is_cmp_required().await
    }

    /// One ads timer step at `now_ms` on the host clock (a fake clock).
    pub fn test_ads_tick(&self, now_ms: u64) {
        self.0.ads_tick(now_ms);
    }

    /// One consent timer step at `now_ms` on the host clock.
    pub fn test_consent_tick(&self, now_ms: u64) {
        self.0.consent_tick(now_ms);
    }

    /// As if the poll saw the window `id` hidden (`false`) or shown again
    /// (`true`): its guests' visibility follows (D.5).
    pub fn test_ads_window_visible(&self, id: u32, visible: bool) {
        if visible {
            self.0.ads_window_shown(id);
        } else {
            self.0.ads_window_hidden(id);
        }
    }

    /// As if the window `id` were about to be destroyed: its guests'
    /// documents become hidden first (D.5).
    pub fn test_ads_window_closing(&self, id: u32) {
        self.0.ads_window_closing(id);
    }

    /// The install-at-exit step of the update client (I.4) without the
    /// exit; returns every install recorded so far (the mock runtime runs
    /// no installer).
    pub async fn test_updater_install_at_exit(&self) -> Vec<Value> {
        self.0.updater_install_at_exit().await;
        self.0.with_core(|c| c.updater.test_installs.clone())
    }

    /// As if window `id` was minimized (`true`) or restored (`false`): the
    /// `minimize` / `restore` window events and its guests' visibility follow.
    pub fn test_window_minimized(&self, id: u32, minimized: bool) {
        let mut state = self
            .0
            .with_core(|c| c.windows.get(id).map(|e| e.state))
            .unwrap_or_default();
        state.minimized = minimized;
        self.0.apply_window_state(id, state);
    }

    /// As if the OS reported the minimize of window `id` started (`done`
    /// false: macOS animates it into the Dock, reporting it neither visible
    /// nor minimized) or ended (`done` true).
    pub fn test_window_minimize_stage(&self, id: u32, done: bool) {
        let stage = if done {
            crate::platform::webview::MinimizeStage::Did
        } else {
            crate::platform::webview::MinimizeStage::Will
        };
        self.0.window_minimize_stage(id, stage);
    }

    /// As if the visibility poll saw window `id` with this OS state; the
    /// window's analytics periods and its guests follow (E.2 #7, D.5).
    pub fn test_poll_window(&self, id: u32, visible: bool, minimized: bool) {
        self.0.apply_poll(&[(id, visible, minimized)]);
    }

    /// The URLs the plugin would have opened in the system browser (a host
    /// without OS queries only records them).
    #[must_use]
    pub fn test_browser_opens(&self) -> Vec<String> {
        self.0.with_core(|c| c.browser_opens.clone())
    }

    /// The state of the ad guest `label`: `{ embedder, elementId, navigated,
    /// ready, domReady, loads, recoveries, visible, embedderHidden, embedderMinimized,
    /// visibilityState, reloadScheduled, passthrough }`, or `None` when it is
    /// gone.
    #[must_use]
    pub fn test_guest(&self, label: &str) -> Option<Value> {
        self.0.with_core(|c| {
            c.ads.guests.get(label).map(|g| {
                serde_json::json!({
                    "embedder": g.embedder,
                    "elementId": g.element_id,
                    "navigated": g.navigated,
                    "ready": g.ready,
                    "domReady": g.dom_ready,
                    "loads": g.loads,
                    "recoveries": g.recoveries,
                    "visible": g.visible,
                    "embedderHidden": g.embedder_hidden,
                    "embedderMinimized": g.embedder_minimized,
                    "visibilityState": if g.sent_visible { "visible" } else { "hidden" },
                    "reloadScheduled": g.reload_at.is_some(),
                    "passthrough": g.passthrough,
                })
            })
        })
    }

    /// What the host sent to its ad guests and did to them natively, in
    /// order, as lab trace records: host messages (`via:
    /// "private-message"`) and the native steps (`kind` `transparent`,
    /// `zorder`, `passthrough`). A host without OS queries records them.
    #[must_use]
    pub fn test_guest_trace(&self) -> Vec<Value> {
        self.0.with_core(|c| c.ads.test_trace.clone())
    }

    /// As if the platform reported a crash of the ad guest `label`.
    pub fn test_guest_crashed(&self, label: &str, reason: crate::ads::GoneReason) {
        self.0.guest_crashed(label, reason, 0);
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
