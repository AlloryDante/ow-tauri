//! The Tauri plugin: [`Builder`], [`init`] and the hooks (DESIGN §3.3,
//! §4.2). Every hook forwards to `host::dispatch` and nothing else (frozen
//! in W1).

use std::sync::Arc;

use tauri::plugin::{PluginApi, TauriPlugin};
use tauri::{AppHandle, Manager, Runtime};

use crate::config::Config;
use crate::ext::Overwolf;
use crate::host::{SetupOptions, core_of, dispatch, lifecycle};

/// The plugin with the defaults: `Builder::new().build()`.
///
/// ```no_run
/// # fn example(context: tauri::Context) {
/// tauri::Builder::default()
///     .plugin(tauri_plugin_overwolf::init())
///     .run(context)
///     .expect("error while running the app");
/// # }
/// ```
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R, Option<Config>> {
    Builder::new().build()
}

/// Configures the plugin. Everything else comes from `plugins.overwolf` in
/// `tauri.conf.json` (DESIGN §3.2); the uid is configuration-only, so the
/// build step that writes the installer records sees the same one.
///
/// ```
/// let builder = tauri_plugin_overwolf::Builder::new()
///     .test_ad(cfg!(debug_assertions))
///     .exclude_windows(["tray-*"]);
/// # let _ = builder;
/// ```
#[derive(Debug, Clone)]
#[must_use]
pub struct Builder {
    options: SetupOptions,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    /// A builder with the defaults.
    pub fn new() -> Self {
        Builder {
            options: SetupOptions {
                os_queries: true,
                runtime_capabilities: true,
                ..SetupOptions::default()
            },
        }
    }

    /// Turns test ads on. Test ads are on when this, `ads.testAd`, the
    /// argument `--test-ad` or `OW_TAURI_TEST_AD=1` says so.
    ///
    /// ```
    /// let builder = tauri_plugin_overwolf::Builder::new().test_ad(true);
    /// # let _ = builder;
    /// ```
    pub fn test_ad(mut self, enabled: bool) -> Self {
        self.options.test_ad = enabled;
        self
    }

    /// `disableAnonymousAnalytics()` before this launch's first analytics
    /// requests (D1): only the mandatory set is sent.
    ///
    /// ```
    /// let builder = tauri_plugin_overwolf::Builder::new().disable_anonymous_analytics();
    /// # let _ = builder;
    /// ```
    pub fn disable_anonymous_analytics(mut self) -> Self {
        self.options.disable_anonymous_analytics = true;
        self
    }

    /// `disableAdsOptimization()` from the start.
    ///
    /// ```
    /// let builder = tauri_plugin_overwolf::Builder::new().disable_ads_optimization();
    /// # let _ = builder;
    /// ```
    pub fn disable_ads_optimization(mut self) -> Self {
        self.options.disable_ads_optimization = true;
        self
    }

    /// `disableAdsFPD()` from the start.
    ///
    /// ```
    /// let builder = tauri_plugin_overwolf::Builder::new().disable_ads_fpd();
    /// # let _ = builder;
    /// ```
    pub fn disable_ads_fpd(mut self) -> Self {
        self.options.disable_ads_fpd = true;
        self
    }

    /// The host label and version the analytics and the guests report
    /// (`analytics.hostLabel`, `analytics.hostVersion`); `None` keeps the
    /// Tauri version.
    ///
    /// ```
    /// let builder = tauri_plugin_overwolf::Builder::new().host_label("tauri", None);
    /// # let _ = builder;
    /// ```
    pub fn host_label(mut self, label: impl Into<String>, version: Option<String>) -> Self {
        self.options.host_label = Some((label.into(), version));
        self
    }

    /// Window label globs (`*`, `?`) that never count as visible app
    /// windows for the analytics (D6), added to `analytics.excludeWindows`.
    ///
    /// ```
    /// let builder = tauri_plugin_overwolf::Builder::new().exclude_windows(["tray-*", "splash"]);
    /// # let _ = builder;
    /// ```
    pub fn exclude_windows<I, S>(mut self, globs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.options
            .exclude_windows
            .extend(globs.into_iter().map(Into::into));
        self
    }

    /// The app composes its own `on_web_content_process_terminate` hook and
    /// calls [`handle_web_content_process_terminate`](crate::handle_web_content_process_terminate)
    /// from it, so the plugin does not warn that the hook is missing.
    ///
    /// ```
    /// let builder = tauri_plugin_overwolf::Builder::new().forwards_web_content_process_terminate();
    /// # let _ = builder;
    /// ```
    #[cfg(target_os = "macos")]
    pub fn forwards_web_content_process_terminate(mut self) -> Self {
        self.options.forwards_terminate = true;
        self
    }

    /// The embedded `dev-app-update.yml` a debug build reads instead of the
    /// configured feed (CONTRACT I.1). Ignored in release builds.
    ///
    /// ```
    /// let builder = tauri_plugin_overwolf::Builder::new()
    ///     .dev_update_config("provider: generic\nurl: http://127.0.0.1:8080/\n");
    /// # let _ = builder;
    /// ```
    #[cfg(all(feature = "updater", windows))]
    pub fn dev_update_config(mut self, yaml: &'static str) -> Self {
        self.options.dev_update_config = Some(yaml);
        self
    }

    /// **Tests only.** Replaces the HTTP client of every host request, to
    /// capture requests. Not a stable API.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    pub fn analytics_transport(mut self, transport: Arc<dyn crate::analytics::Transport>) -> Self {
        self.options.transport = Some(transport);
        self
    }

    /// **Tests only.** Points host requests at other endpoints (failure
    /// injection). Not a stable API.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    pub fn endpoints(mut self, endpoints: crate::analytics::TestEndpoints) -> Self {
        self.options.endpoints = Some(endpoints);
        self
    }

    /// Builds the plugin.
    #[must_use]
    pub fn build<R: Runtime>(self) -> TauriPlugin<R, Option<Config>> {
        let options = self.options;
        tauri::plugin::Builder::<R, Option<Config>>::new(crate::PLUGIN_NAME)
            .invoke_handler(crate::commands::handler())
            .setup(move |app, api| setup(app, &api, options.clone()))
            .on_window_ready(|window| {
                if let Some(core) = core_of(&window) {
                    dispatch::on_window_ready(&core, &window);
                }
            })
            .on_webview_ready(|webview| {
                if let Some(core) = core_of(&webview) {
                    dispatch::on_webview_ready(&core, &webview);
                }
            })
            .on_page_load(|webview, payload| {
                if let Some(core) = core_of(webview) {
                    dispatch::on_page_load(&core, webview, payload);
                }
            })
            .on_navigation(|webview, url| {
                core_of(webview).is_none_or(|core| dispatch::on_navigation(&core, webview, url))
            })
            .on_event(|app, event| {
                if let Some(core) = core_of(app) {
                    dispatch::on_event(&core, event);
                }
            })
            .build()
    }

    /// The setup options (tests adjust them).
    #[cfg(test)]
    pub(crate) fn options_mut(&mut self) -> &mut SetupOptions {
        &mut self.options
    }
}

/// The plugin's setup: reads the configuration and the state, registers the
/// guest and consent runtime capabilities and the restart sentinel. Writes
/// nothing (DESIGN §4.2).
fn setup<R: Runtime>(
    app: &AppHandle<R>,
    api: &PluginApi<R, Option<Config>>,
    options: SetupOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let capabilities = options.runtime_capabilities;
    let core = crate::host::setup::setup(app, api.config().clone(), options)?;
    app.manage(Overwolf(Arc::clone(&core)));
    if capabilities {
        app.add_capability(crate::capabilities::adview_guest_capability()?)?;
        app.add_capability(crate::capabilities::cmp_capability()?)?;
    }
    lifecycle::install_sentinel(&core);
    #[cfg(target_os = "macos")]
    crate::platform::terminate::warn_if_unwired(core.options.forwards_terminate);
    Ok(())
}
