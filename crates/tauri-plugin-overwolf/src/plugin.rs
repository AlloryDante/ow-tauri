//! The Tauri plugin: [`Builder`] and the event hooks (CONTRACT A.5, A.6;
//! ARCHITECTURE 3).

use std::sync::Arc;

use tauri::plugin::{PluginApi, TauriPlugin};
use tauri::webview::{PageLoadEvent, PageLoadPayload};
use tauri::{AppHandle, Manager, RunEvent, Runtime, Webview, WindowEvent};
use url::Url;

use crate::analytics::Transport;
use crate::config::Config;
use crate::ext::Overwolf;
use crate::host::{Host, SetupOptions};
use crate::packages::PackagesBackend;
use crate::state::log::LogLevel;
use crate::window::{WebviewClass, classify};

pub use crate::commands::list::COMMANDS;

/// Configures and builds the plugin.
///
/// ```no_run
/// # fn manifest() -> &'static str { "{}" }
/// let plugin = tauri_plugin_overwolf::Builder::new()
///     .manifest_json(manifest()) // tauri_plugin_overwolf::embedded_manifest!() in an app
///     .packages_backend(tauri_plugin_overwolf::PackagesBackend::None)
///     .build::<tauri::Wry>();
/// # let _ = plugin;
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
    /// A builder with the defaults of CONTRACT A.1.
    ///
    /// ```rust
    /// let builder = tauri_plugin_overwolf::Builder::new();
    /// # let _ = builder;
    /// ```
    pub fn new() -> Self {
        Builder {
            options: SetupOptions {
                companion_plugins: true,
                runtime_capabilities: true,
                main_webview: true,
                os_queries: true,
                ..SetupOptions::default()
            },
        }
    }

    /// The app manifest, the output of
    /// [`embedded_manifest!`](crate::embedded_manifest). Required.
    ///
    /// ```rust
    /// // In an app: `.manifest_json(tauri_plugin_overwolf::embedded_manifest!())`.
    /// let builder = tauri_plugin_overwolf::Builder::new().manifest_json(r#"{"name":"demo"}"#);
    /// # let _ = builder;
    /// ```
    pub fn manifest_json(mut self, json: &'static str) -> Self {
        self.options.manifest_json = Some(json);
        self
    }

    /// `packagesBackend` (H.2): `None` (default) or the reserved `Native`;
    /// overrides the configuration.
    ///
    /// ```rust
    /// use tauri_plugin_overwolf::{Builder, PackagesBackend};
    /// let builder = Builder::new().packages_backend(PackagesBackend::None);
    /// # let _ = builder;
    /// ```
    pub fn packages_backend(mut self, backend: PackagesBackend) -> Self {
        self.options.packages_backend = Some(backend);
        self
    }

    /// `analytics.hostLabel` and `analytics.hostVersion` (CONTRACT section
    /// 0); `None` keeps the Tauri crate version.
    ///
    /// ```rust
    /// // Reproduce ow-electron's analytics labels exactly.
    /// let builder = tauri_plugin_overwolf::Builder::new()
    ///     .host_label("electron", Some("42.11.4".into()));
    /// # let _ = builder;
    /// ```
    pub fn host_label(mut self, label: impl Into<String>, version: Option<String>) -> Self {
        self.options.host_label = Some((label.into(), version));
        self
    }

    /// Replaces the HTTP client of every host request (analytics and the
    /// consent request), for tests that capture requests (E.3).
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use tauri_plugin_overwolf::analytics::Transport;
    /// fn with_capture(t: Arc<dyn Transport>) -> tauri_plugin_overwolf::Builder {
    ///     tauri_plugin_overwolf::Builder::new().analytics_transport(t)
    /// }
    /// ```
    pub fn analytics_transport(mut self, transport: Arc<dyn Transport>) -> Self {
        self.options.transport = Some(transport);
        self
    }

    /// Forces test ads on or off (the `--test-ad` switch wins when set).
    ///
    /// ```rust
    /// // Test ads in a lab build, whatever the command line says.
    /// let builder = tauri_plugin_overwolf::Builder::new().test_ad(true);
    /// # let _ = builder;
    /// ```
    pub fn test_ad(mut self, enabled: bool) -> Self {
        self.options.test_ad = Some(enabled);
        self
    }

    /// Console-assigned uid; overrides the configuration and the computed
    /// uid (G.2).
    ///
    /// ```rust
    /// let builder = tauri_plugin_overwolf::Builder::new()
    ///     .uid("djpddhibpjddgdpcfkbooljealnjnamkhlihgbab");
    /// # let _ = builder;
    /// ```
    pub fn uid(mut self, uid: impl Into<String>) -> Self {
        self.options.uid = Some(uid.into());
        self
    }

    /// Whether the plugin registers the opener, dialog and global-shortcut
    /// plugins it calls when the app has not (default `true`).
    ///
    /// ```rust
    /// // The app registers opener, dialog and global-shortcut itself.
    /// let builder = tauri_plugin_overwolf::Builder::new().companion_plugins(false);
    /// # let _ = builder;
    /// ```
    pub fn companion_plugins(mut self, enabled: bool) -> Self {
        self.options.companion_plugins = enabled;
        self
    }

    /// Whether the plugin adds its runtime capabilities (default `true`).
    /// Apps that grant `overwolf:main` to `ow-main` in their own
    /// capability files, and test harnesses with an empty ACL, turn it off.
    ///
    /// ```rust
    /// // `capabilities/main.json` grants `overwolf:main` to `ow-main`.
    /// let builder = tauri_plugin_overwolf::Builder::new().runtime_capabilities(false);
    /// # let _ = builder;
    /// ```
    pub fn runtime_capabilities(mut self, enabled: bool) -> Self {
        self.options.runtime_capabilities = enabled;
        self
    }

    /// Whether the plugin creates the hidden main webview `ow-main`
    /// (default `true`). Tests turn it off.
    ///
    /// ```rust
    /// // A mock-runtime test creates `ow-main` itself.
    /// let builder = tauri_plugin_overwolf::Builder::new().main_webview(false);
    /// # let _ = builder;
    /// ```
    pub fn main_webview(mut self, enabled: bool) -> Self {
        self.options.main_webview = enabled;
        self
    }

    /// Overrides the process arguments (tests).
    ///
    /// ```rust
    /// let builder = tauri_plugin_overwolf::Builder::new()
    ///     .argv(vec!["app".into(), "--test-ad".into()]);
    /// # let _ = builder;
    /// ```
    pub fn argv(mut self, argv: Vec<String>) -> Self {
        self.options.argv = Some(argv);
        self
    }

    /// Skips every OS display and cursor query, for Tauri's mock runtime,
    /// which implements none: displays are empty and the cursor is at 0, 0.
    ///
    /// ```rust
    /// let builder = tauri_plugin_overwolf::Builder::new().skip_os_queries();
    /// # let _ = builder;
    /// ```
    #[cfg(feature = "test-util")]
    pub fn skip_os_queries(mut self) -> Self {
        self.options.os_queries = false;
        self
    }

    /// Builds the plugin. The configuration is `plugins.overwolf` in
    /// `tauri.conf.json` (A.1); it may be absent.
    ///
    /// ```no_run
    /// let app = tauri::Builder::default().plugin(
    ///     tauri_plugin_overwolf::Builder::new()
    ///         .manifest_json(manifest()) // `embedded_manifest!()` in an app
    ///         .build(),
    /// );
    /// # let _ = app;
    /// # fn manifest() -> &'static str { "{}" }
    /// ```
    pub fn build<R: Runtime>(self) -> TauriPlugin<R, Option<Config>> {
        let options = self.options;
        tauri::plugin::Builder::<R, Option<Config>>::new(crate::PLUGIN_NAME)
            .invoke_handler(crate::commands::handler())
            .setup(move |app, api| setup(app, &api, options.clone()))
            .on_event(on_event)
            .on_navigation(on_navigation)
            .on_page_load(on_page_load)
            .build()
    }
}

fn host_of<R: Runtime, M: Manager<R>>(manager: &M) -> Option<Arc<Host<R>>> {
    manager
        .try_state::<Overwolf<R>>()
        .map(|s| Arc::clone(&s.inner().0))
}

fn setup<R: Runtime>(
    app: &AppHandle<R>,
    api: &PluginApi<R, Option<Config>>,
    options: SetupOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = api.config().clone().unwrap_or_default();
    let host = crate::host::build_host(app, config, options)?;
    app.manage(Overwolf(Arc::clone(&host)));
    if host.options.runtime_capabilities {
        let capability = crate::capabilities::main_capability()?;
        app.add_capability(capability)?;
        app.add_capability(crate::capabilities::adview_guest_capability()?)?;
        app.add_capability(crate::capabilities::cmp_capability()?)?;
    }
    if host.options.companion_plugins {
        register_companions(app);
    }
    if host.options.main_webview {
        host.create_main_webview(None)?;
    }
    host.spawn_ticker();
    Ok(())
}

/// Registers the opener, dialog and global-shortcut plugins the app has not
/// registered itself. The plugin store is locked while plugins set up and
/// while they receive events, so registration is posted to the event loop
/// from another thread.
fn register_companions<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let handle = app.clone();
        let posted = app.run_on_main_thread(move || {
            let log = |what: &str, r: tauri::Result<()>| {
                if let (Err(err), Some(host)) = (r, host_of(&handle)) {
                    host.log(
                        LogLevel::Error,
                        &format!("registering the {what} plugin failed: {err}"),
                    );
                }
            };
            if handle
                .try_state::<tauri_plugin_opener::Opener<R>>()
                .is_none()
            {
                log(
                    "opener",
                    handle.plugin(
                        tauri_plugin_opener::Builder::new()
                            .open_js_links_on_click(false)
                            .build(),
                    ),
                );
            }
            if handle
                .try_state::<tauri_plugin_dialog::Dialog<R>>()
                .is_none()
            {
                log("dialog", handle.plugin(tauri_plugin_dialog::init()));
            }
            if handle
                .try_state::<tauri_plugin_global_shortcut::GlobalShortcut<R>>()
                .is_none()
            {
                log(
                    "global-shortcut",
                    handle.plugin(tauri_plugin_global_shortcut::Builder::new().build()),
                );
            }
        });
        if let (Err(err), Some(host)) = (posted, host_of(&app)) {
            host.log(
                LogLevel::Error,
                &format!("registering companion plugins failed: {err}"),
            );
        }
    });
}

fn on_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    let Some(host) = host_of(app) else { return };
    match event {
        RunEvent::ExitRequested { code, api, .. } => {
            // `app.exit(code)` from the plugin's own exit path carries a code
            // and proceeds. A request without a code comes from the last
            // window closing or from the OS (A.6): it is always prevented and
            // the host runs the quit sequence (or exits when it cannot).
            if code.is_none() {
                api.prevent_exit();
                host.exit_requested();
            }
        }
        RunEvent::Exit => host.on_exit(),
        // D.6.1, D.6.2: the consent request and startup window of the launch.
        RunEvent::Ready => host.start_consent(),
        RunEvent::WindowEvent { label, event, .. } => window_event(&host, label, event),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows,
            ..
        } => {
            let mut message = crate::ipc::messages::HostMessage::lifecycle("activate", None, None);
            if let crate::ipc::messages::HostMessage::Lifecycle { extra, .. } = &mut message {
                extra.insert(
                    "hasVisibleWindows".into(),
                    serde_json::Value::Bool(*has_visible_windows),
                );
            }
            host.send_main(message);
        }
        _ => {}
    }
}

pub(crate) fn window_event<R: Runtime>(host: &Arc<Host<R>>, label: &str, event: &WindowEvent) {
    match classify(label) {
        WebviewClass::Main => match event {
            WindowEvent::CloseRequested { api, .. } => {
                // `ow-main` lives as long as the app; closing it would end the
                // app without the quit sequence.
                api.prevent_close();
            }
            WindowEvent::Destroyed => host.main_destroyed(),
            _ => {}
        },
        WebviewClass::Ui(id) => {
            if let WindowEvent::Focused(focused) = event {
                host.ads_window_focus(id, *focused);
            }
            host.window_event(id, event);
        }
        WebviewClass::Cmp => {
            if matches!(event, WindowEvent::Destroyed) {
                host.consent_window_gone(label);
            }
        }
        _ => {}
    }
}

fn on_navigation<R: Runtime>(webview: &Webview<R>, url: &Url) -> bool {
    host_of(webview).is_none_or(|host| navigation(&host, webview.label(), url))
}

/// The navigation policy of the webview `label` (A.2.3.1, A.6).
pub(crate) fn navigation<R: Runtime>(host: &Arc<Host<R>>, label: &str, url: &Url) -> bool {
    match classify(label) {
        WebviewClass::Main => host.main_navigation(url),
        WebviewClass::Ui(id) => host.ui_navigation(id, url),
        WebviewClass::AdviewGuest => host.guest_navigation(label, url),
        WebviewClass::Cmp => host.cmp_navigation(label, url),
        _ => true,
    }
}

fn on_page_load<R: Runtime>(webview: &Webview<R>, payload: &PageLoadPayload<'_>) {
    if let Some(host) = host_of(webview) {
        page_load(&host, webview.label(), payload.event(), payload.url());
    }
}

/// A page load of the webview `label` (top-level documents only).
pub(crate) fn page_load<R: Runtime>(
    host: &Arc<Host<R>>,
    label: &str,
    event: PageLoadEvent,
    url: &Url,
) {
    match classify(label) {
        WebviewClass::Main => host.main_page_load(event, url),
        WebviewClass::Ui(id) => host.window_page_load(id, true, label, event, url),
        WebviewClass::Remote(id) => host.window_page_load(id, false, label, event, url),
        WebviewClass::AdviewGuest => host.guest_page_load(label, event, url),
        WebviewClass::Cmp => host.consent_page_load(label, event, url),
        WebviewClass::Other => {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
    use tauri::{App, Manager, WebviewUrl, WebviewWindowBuilder};

    use super::{Builder, host_of};
    use crate::ipc::messages::{HostMessage, WindowEventName};
    use crate::ipc::router::MAIN_LABEL;
    use crate::manifest::EmbeddedManifest;

    fn app(name: &str, probe: tauri::plugin::TauriPlugin<MockRuntime>) -> App<MockRuntime> {
        let dir =
            std::env::temp_dir().join(format!("ow-tauri-plugin-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let manifest = serde_json::to_string(&EmbeddedManifest::minimal(
            "Plugin Test",
            "Example Studio",
            "1.0.0",
        ))
        .unwrap();
        let mut context = mock_context(noop_assets());
        context.config_mut().plugins.0.insert(
            "overwolf".into(),
            serde_json::json!({ "state": { "appDataDir": dir } }),
        );
        let mut builder = Builder::new()
            .manifest_json(Box::leak(manifest.into_boxed_str()))
            .companion_plugins(false)
            .runtime_capabilities(false)
            .main_webview(false)
            .argv(vec!["plugin-test".into()]);
        builder.options.os_queries = false;
        mock_builder()
            .plugin(builder.build())
            .plugin(probe)
            .build(context)
            .unwrap()
    }

    fn wait(what: &str, done: impl Fn() -> bool) {
        let start = Instant::now();
        while !done() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "timed out: {what}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The new `ow-main` of a soft restart is created while Tauri holds its
    /// plugin-store lock (the `Destroyed` window event reaches the plugin's
    /// `on_event` under that lock). Creating it inline would take the lock
    /// again and deadlock. The probe plugin's `on_webview_ready` hook runs
    /// under the same lock and reports the destruction from there.
    #[test]
    fn soft_restart_recreates_main_from_a_hook_without_deadlock() {
        let fired = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&fired);
        let probe = tauri::plugin::Builder::<MockRuntime>::new("probe")
            .on_webview_ready(move |webview| {
                if webview.label() == "probe" {
                    let host = host_of(webview.app_handle()).unwrap();
                    host.main_destroyed();
                    seen.store(true, Ordering::SeqCst);
                }
            })
            .build();
        let app = app("soft-restart", probe);
        let host = host_of(app.handle()).unwrap();
        let url = tauri::Url::parse("tauri://localhost/index.html").unwrap();
        host.with_core(|c| {
            c.soft_restart = Some(url.clone());
            c.restart_stale_windows.insert(5);
        });
        // An exit request during the restart waits for the new main.
        host.exit_requested();
        assert!(host.with_core(|c| c.quit_after_restart && !c.quit.is_running()));
        // Late events of a window the restart closed are dropped.
        host.with_core(|c| {
            c.queue_main(HostMessage::window(5, WindowEventName::Closed, None));
            assert_eq!(c.router.queued(MAIN_LABEL), 0);
        });

        let handle = app.handle().clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let built = WebviewWindowBuilder::new(&handle, "probe", WebviewUrl::App("x".into()))
                .build()
                .is_ok();
            let _ = tx.send(built);
        });
        let built = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the hook returned (no deadlock)");
        assert!(built);
        assert!(fired.load(Ordering::SeqCst));
        wait("the new ow-main", || {
            app.get_webview_window(MAIN_LABEL).is_some()
        });
        assert!(
            host.with_core(|c| c.soft_restart.is_some()),
            "the restart lasts until the new main has loaded"
        );
        // A second `Destroyed` report does not create another one.
        host.main_destroyed();

        host.main_page_load(tauri::webview::PageLoadEvent::Finished, &url);
        assert!(host.with_core(|c| c.soft_restart.is_none() && !c.quit_after_restart));
        assert!(
            host.with_core(|c| c.quit.is_running()),
            "the deferred quit started"
        );
    }

    #[test]
    fn crash_reports_are_counted_once() {
        let probe = tauri::plugin::Builder::<MockRuntime>::new("probe").build();
        let app = app("crash-once", probe);
        let host = host_of(app.handle()).unwrap();
        // The app's process-termination hook and the window's `Destroyed`
        // both report the same crash.
        host.with_core(|c| c.exiting = true);
        host.main_crashed();
        let history = host.ow_tauri.get().extra.get("mainCrashes").cloned();
        assert!(
            history.is_none(),
            "an exit already under way records nothing"
        );
    }
}
