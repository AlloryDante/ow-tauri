//! The Rust API (DESIGN §3.3): [`Overwolf`] and [`OverwolfExt`].
//!
//! The signatures are frozen (DESIGN §10.0); bodies that need the guests,
//! the consent windows or the update client return `unsupported` until
//! their owners fill them (W2, W3).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use serde_json::{Map, Value};
use tauri::{Manager, Runtime};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::host::{Core, LOG_TARGET};
use crate::identity::{EmailHashes, email_hashes};
use crate::types::{CmpWindowOptions, Info};

/// The warning of a `disableAnonymousAnalytics()` call made after the launch
/// burst was sent (DESIGN §4.8).
const LATE_DISABLE_WARNING: &str = "too late for this launch's burst; use plugins.overwolf.analytics.disableAnonymous, the Builder, or setAnonymousAnalyticsPreference(false)";

/// The plugin's per-app state, reachable from any Tauri manager through
/// [`OverwolfExt::overwolf`] once the plugin's setup has run.
pub struct Overwolf<R: Runtime>(pub(crate) Arc<Core<R>>);

impl<R: Runtime> std::fmt::Debug for Overwolf<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Overwolf")
            .field("uid", &self.uid())
            .finish_non_exhaustive()
    }
}

/// Maps a failed `ow-electron.json` write to [`Error`] (no paths).
fn state_error(err: &crate::state::ow_electron::WriteError) -> Error {
    match err {
        crate::state::ow_electron::WriteError::Io(io) => {
            Error::from_io("Updating ow-electron.json", io)
        }
        crate::state::ow_electron::WriteError::InvalidExisting => {
            Error::backend("ow-electron.json is not valid JSON; it was left untouched")
        }
    }
}

impl<R: Runtime> Overwolf<R> {
    /// What `getInfo()` returns (machine ids are [`Overwolf::muid`] and
    /// [`Overwolf::muid_v2`]).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// let info = app.overwolf().info();
    /// println!("{} {}", info.name, info.version);
    /// # }
    /// ```
    #[must_use]
    pub fn info(&self) -> Info {
        let id = &self.0.identity;
        Info::new(
            &id.app.uid,
            &id.app.cuid,
            id.phase_percent,
            id.utm_params.clone(),
            id.test_ad,
            self.0.ads.supported(),
            &id.app.name,
            &id.app.version,
            id.host.clone(),
        )
    }

    /// The effective app uid (CONTRACT G.2).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// assert!(!app.overwolf().uid().is_empty());
    /// # }
    /// ```
    #[must_use]
    pub fn uid(&self) -> &str {
        &self.0.identity.app.uid
    }

    /// The computed uid, even when `uid` overrides it (CONTRACT G.2).
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
        &self.0.identity.app.cuid
    }

    /// The machine id the analytics send as `muid` (CONTRACT E.4).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// assert_eq!(app.overwolf().muid().len(), 36);
    /// # }
    /// ```
    #[must_use]
    pub fn muid(&self) -> &str {
        &self.0.identity.machine.muid
    }

    /// `muidV2` (CONTRACT E.4): equal to [`Overwolf::muid`] except on
    /// Windows when the shared registry values differ.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// assert_eq!(app.overwolf().muid_v2().len(), 36);
    /// # }
    /// ```
    #[must_use]
    pub fn muid_v2(&self) -> &str {
        &self.0.identity.machine.muid_v2
    }

    /// The phase bucket of this machine, 0 to 99.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// assert!(app.overwolf().phase_percent() < 100);
    /// # }
    /// ```
    #[must_use]
    pub fn phase_percent(&self) -> u8 {
        self.0.identity.phase_percent
    }

    /// The UTM parameters stored at install (`ow-electron.json`), if any.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// if let Some(utm) = app.overwolf().utm_params() {
    ///     println!("installed from {utm}");
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn utm_params(&self) -> Option<&Value> {
        self.0.identity.utm_params.as_ref()
    }

    /// Whether test ads are on (configuration, builder, `--test-ad` or
    /// `OW_TAURI_TEST_AD=1`).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// println!("test ads: {}", app.overwolf().is_test_ad());
    /// # }
    /// ```
    #[must_use]
    pub fn is_test_ad(&self) -> bool {
        self.0.identity.test_ad
    }

    /// The validated `plugins.overwolf` configuration, with the builder's
    /// overrides applied.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// println!("{:?}", app.overwolf().config().analytics.host_label);
    /// # }
    /// ```
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.0.identity.config
    }

    /// `<appData>/ow-electron/<uid>`, the directory of `ow-electron.json`
    /// and `ow-tauri.json`.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// assert!(app.overwolf().state_dir().ends_with(app.overwolf().uid()));
    /// # }
    /// ```
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        self.0.identity.state_dir.root()
    }

    /// `isCMPRequired()` (CONTRACT D.6.2): never fails; `true` when the
    /// answer is not known.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # async fn example(app: tauri::AppHandle) {
    /// if app.overwolf().is_cmp_required().await {
    ///     println!("show the privacy settings entry");
    /// }
    /// # }
    /// ```
    pub async fn is_cmp_required(&self) -> bool {
        self.0.consent.is_cmp_required(&self.0).await
    }

    /// Opens the ad privacy settings window (CONTRACT D.6.4). A Rust caller
    /// may use any `https:` `cmp_url`.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::{CmpTab, CmpWindowOptions, OverwolfExt};
    /// # async fn example(app: tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
    /// app.overwolf()
    ///     .open_ad_privacy_settings_window(CmpWindowOptions::new().tab(CmpTab::Vendors))
    ///     .await
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// `invalid-argument` for a bad option, `not-found` for an unknown
    /// parent window, `unsupported` where the window cannot open.
    #[allow(
        unknown_lints,
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "frozen async interface; the consent host (W2) awaits"
    )]
    pub async fn open_ad_privacy_settings_window(&self, options: CmpWindowOptions) -> Result<()> {
        if let Some(url) = &options.cmp_url
            && !crate::config::is_https_url(url)
        {
            return Err(Error::invalid_argument("cmpURL must be an https: URL"));
        }
        self.0.consent.open_settings_window(&self.0, &options, None)
    }

    /// `openCMPWindow`: the deprecated alias of
    /// [`Overwolf::open_ad_privacy_settings_window`].
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::{CmpWindowOptions, OverwolfExt};
    /// # async fn example(app: tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
    /// app.overwolf().open_cmp_window(CmpWindowOptions::new()).await
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// As [`Overwolf::open_ad_privacy_settings_window`].
    pub async fn open_cmp_window(&self, options: CmpWindowOptions) -> Result<()> {
        self.open_ad_privacy_settings_window(options).await
    }

    /// `generateUserEmailHashes(email)` (CONTRACT A.2.2): hashes the
    /// normalised address in `emailHashes.encoding`, and also sends and
    /// stores the hashes as [`Overwolf::set_user_email_hashes`] does.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// let hashes = app.overwolf().generate_user_email_hashes("user@example.com");
    /// assert!(hashes.sha256.is_some());
    /// # }
    /// ```
    #[must_use]
    pub fn generate_user_email_hashes(&self, email: &str) -> EmailHashes {
        let hashes = email_hashes(email, self.0.identity.config.email_hashes.encoding);
        self.set_user_email_hashes(&hashes);
        hashes
    }

    /// `setUserEmailHashes(hashes)` (CONTRACT A.2.2): the hashes go to every
    /// ad guest and are stored as `eHashes` in `ow-electron.json`, as
    /// ow-electron does. Empty hashes are ignored; so is every call after
    /// [`Overwolf::disable_ads_fpd`] (one warning).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::{EmailHashes, OverwolfExt};
    /// # fn example(app: &tauri::AppHandle) {
    /// let hashes = EmailHashes { sha256: Some("ab12".into()), ..EmailHashes::default() };
    /// app.overwolf().set_user_email_hashes(&hashes);
    /// # }
    /// ```
    pub fn set_user_email_hashes(&self, hashes: &EmailHashes) {
        if hashes.is_empty() {
            return;
        }
        if self.0.flags.ads_fpd_disabled.load(Ordering::SeqCst) {
            log::warn!(target: LOG_TARGET, "setUserEmailHashes() after disableAdsFPD() is ignored");
            return;
        }
        let get = |h: &Option<String>| h.clone().unwrap_or_default();
        let (sha1, md5, sha256) = (get(&hashes.sha1), get(&hashes.md5), get(&hashes.sha256));
        if let Err(err) = self
            .0
            .state
            .ow_electron
            .write_e_hashes(&sha1, &md5, &sha256)
        {
            log::warn!(target: LOG_TARGET, "eHashes not stored: {err}");
        }
        self.0.ads.set_email_hashes(Some(hashes.clone()));
    }

    /// `clearUserEmailHashes()` (SEC-M9): forgets the hashes and removes
    /// `eHashes` from `ow-electron.json`.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
    /// app.overwolf().clear_user_email_hashes()
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// `io` or `backend` when `ow-electron.json` could not be updated.
    pub fn clear_user_email_hashes(&self) -> Result<()> {
        self.0.ads.set_email_hashes(None);
        self.0
            .state
            .ow_electron
            .clear_e_hashes()
            .map_err(|e| state_error(&e))
    }

    /// `disableAnonymousAnalytics()` (CONTRACT E.3): only the mandatory
    /// events are sent from now on. Called before `RunEvent::Ready` (for
    /// example in the app's setup closure) it applies to the launch burst;
    /// later, one warning says the burst was already sent (R10).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// app.overwolf().disable_anonymous_analytics();
    /// # }
    /// ```
    pub fn disable_anonymous_analytics(&self) {
        let late = self.0.analytics.disable_anonymous();
        if late
            && !self
                .0
                .flags
                .late_disable_warned
                .swap(true, Ordering::SeqCst)
        {
            log::warn!(target: LOG_TARGET, "{LATE_DISABLE_WARNING}");
        }
    }

    /// `disableAdsOptimization()` (CONTRACT D.2).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// app.overwolf().disable_ads_optimization();
    /// # }
    /// ```
    pub fn disable_ads_optimization(&self) {
        self.0
            .flags
            .ads_optimization_disabled
            .store(true, Ordering::SeqCst);
    }

    /// `disableAdsFPD()` (CONTRACT D.2): no first-party data reaches the
    /// guests from now on.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// app.overwolf().disable_ads_fpd();
    /// # }
    /// ```
    pub fn disable_ads_fpd(&self) {
        self.0.flags.ads_fpd_disabled.store(true, Ordering::SeqCst);
    }

    /// Persists the user's anonymous-analytics choice in `ow-tauri.json`;
    /// `false` applies from the next launch's burst, as
    /// [`Overwolf::disable_anonymous_analytics`] before Ready (R10).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
    /// app.overwolf().set_anonymous_analytics_preference(false)
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// `io` when `ow-tauri.json` could not be written.
    pub fn set_anonymous_analytics_preference(&self, enabled: bool) -> Result<()> {
        self.0
            .state
            .ow_tauri
            .update(|s| s.anonymous_analytics = Some(enabled))
            .map_err(|e| Error::from_io("Writing ow-tauri.json", &e))
    }

    /// `setExternalPaymentUserId(options)` (CONTRACT E.2 #10): one
    /// `<label>_sub_info` request with the options in their key order.
    /// Resolves after the response or the failure.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::{OverwolfExt, PaymentUserIdOptions};
    /// # async fn example(app: tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
    /// let options = PaymentUserIdOptions::new("user-1").provider("tebex").into_map();
    /// app.overwolf().set_external_payment_user_id(&options).await
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// `invalid-argument` (ow-electron's message) when `userId` is missing
    /// or empty.
    pub async fn set_external_payment_user_id(&self, options: &Map<String, Value>) -> Result<()> {
        let user_id_ok = match options.get("userId") {
            Some(Value::String(s)) => !s.is_empty(),
            Some(Value::Number(_)) => true,
            _ => false,
        };
        if !user_id_ok {
            return Err(Error::invalid_argument(
                "providerName and userId are mandatory",
            ));
        }
        self.0.analytics.sub_info(options).await;
        Ok(())
    }

    /// The app-level analytics switch (`analytics.userSwitch` only),
    /// persisted in `ow-tauri.json`.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
    /// app.overwolf().set_analytics_user_enabled(false)
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// `unsupported` without `analytics.userSwitch`; `io` when
    /// `ow-tauri.json` could not be written.
    pub fn set_analytics_user_enabled(&self, enabled: bool) -> Result<()> {
        if !self.0.identity.config.analytics.user_switch {
            return Err(Error::unsupported(
                "setAnalyticsUserEnabled needs plugins.overwolf.analytics.userSwitch",
            ));
        }
        self.0.analytics.set_user_enabled(enabled);
        self.0
            .state
            .ow_tauri
            .update(|s| s.analytics_user_enabled = Some(enabled))
            .map_err(|e| Error::from_io("Writing ow-tauri.json", &e))
    }

    /// Names window `window_label` in the analytics (`x-ow-window`, the
    /// `name` of `window_closed`, D4).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
    /// app.overwolf().set_window_name("main", "home")
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// `invalid-argument` unless `name` is 1 to 128 printable ASCII
    /// characters; `forbidden` for a plugin window; `not-found` for an
    /// unknown window.
    pub fn set_window_name(&self, window_label: &str, name: &str) -> Result<()> {
        if !crate::host::windows::valid_window_name(name) {
            return Err(Error::invalid_argument(
                "a window name is 1 to 128 printable ASCII characters",
            ));
        }
        if crate::config::is_reserved_label(window_label) {
            return Err(Error::forbidden("plugin windows cannot be renamed"));
        }
        if crate::compat::window(&self.0.app, window_label).is_none() {
            return Err(Error::not_found(format!("no window {window_label}")));
        }
        self.0.windows.set_name(window_label, name);
        Ok(())
    }

    /// Ends the visible periods and drains the analytics requests now (at
    /// most 1.5 s), as every exit and restart does by itself (the restart
    /// sentinel, DESIGN §4.2). Idempotent; kept for explicit callers.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) {
    /// app.overwolf().prepare_for_restart();
    /// app.restart();
    /// # }
    /// ```
    pub fn prepare_for_restart(&self) {
        crate::host::lifecycle::on_exit(&self.0);
    }

    /// The update client with the configured feed (CONTRACT I).
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # async fn example(app: tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
    /// if let Some(update) = app.overwolf().updater()?.check().await? {
    ///     println!("version {} is available", update.version);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// As [`UpdaterBuilder::build`](crate::updater::UpdaterBuilder::build).
    #[cfg(all(feature = "updater", windows))]
    pub fn updater(&self) -> Result<crate::updater::Updater<R>> {
        self.updater_builder().build()
    }

    /// An update client builder with the configured defaults.
    ///
    /// ```no_run
    /// use tauri_plugin_overwolf::OverwolfExt;
    /// # fn example(app: &tauri::AppHandle) -> tauri_plugin_overwolf::Result<()> {
    /// let updater = app.overwolf().updater_builder().channel("beta").build()?;
    /// # let _ = updater;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(all(feature = "updater", windows))]
    pub fn updater_builder(&self) -> crate::updater::UpdaterBuilder<R> {
        crate::updater::UpdaterBuilder::new(self.0.app.clone())
    }
}

/// Access to [`Overwolf`] from any Tauri manager (`App`, `AppHandle`,
/// `Window`, `Webview`, ...).
///
/// ```no_run
/// use tauri_plugin_overwolf::OverwolfExt;
/// # fn example(app: &tauri::AppHandle) {
/// println!("uid {}", app.overwolf().uid());
/// # }
/// ```
pub trait OverwolfExt<R: Runtime> {
    /// The plugin's state.
    ///
    /// # Panics
    ///
    /// When the plugin is not registered.
    fn overwolf(&self) -> &Overwolf<R>;
}

impl<R: Runtime, T: Manager<R>> OverwolfExt<R> for T {
    fn overwolf(&self) -> &Overwolf<R> {
        self.state::<Overwolf<R>>().inner()
    }
}

#[cfg(test)]
mod late_warning_tests {
    #[test]
    fn the_late_disable_warning_is_the_design_text() {
        assert_eq!(
            super::LATE_DISABLE_WARNING,
            "too late for this launch's burst; use plugins.overwolf.analytics.disableAnonymous, the Builder, or setAnonymousAnalyticsPreference(false)"
        );
    }
}
