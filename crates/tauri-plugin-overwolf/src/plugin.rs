//! The Tauri plugin: [`Builder`] and the event hooks (CONTRACT A.5, A.6;
//! ARCHITECTURE 3).

use std::sync::Arc;

use tauri::plugin::{PluginApi, TauriPlugin};
use tauri::webview::PageLoadPayload;
use tauri::{AppHandle, Manager, RunEvent, Runtime, Webview, WindowEvent};
use url::Url;

use crate::config::Config;
use crate::ext::Overwolf;
use crate::host::{Host, SetupOptions};
use crate::packages::{PackageRuntime, PackagesBackend};
use crate::state::log::LogLevel;
use crate::window::{WebviewClass, classify};

pub use crate::commands::list::COMMANDS;

/// Configures and builds the plugin.
///
/// ```no_run
/// # fn manifest() -> &'static str { "{}" }
/// let plugin = tauri_plugin_overwolf::Builder::new()
///     .manifest_json(manifest()) // tauri_plugin_overwolf::embedded_manifest!() in an app
///     .packages_backend(tauri_plugin_overwolf::PackagesBackend::Auto)
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
    pub fn manifest_json(mut self, json: &'static str) -> Self {
        self.options.manifest_json = Some(json);
        self
    }

    /// Which package runtime to use (H.1); overrides `packagesBackend` in
    /// the configuration.
    pub fn packages_backend(mut self, backend: PackagesBackend) -> Self {
        self.options.packages_backend = Some(backend);
        self
    }

    /// Registers a native package runtime (H.2).
    pub fn package_runtime(mut self, runtime: Arc<dyn PackageRuntime>) -> Self {
        self.options.package_runtime = Some(runtime);
        self
    }

    /// Forces test ads on or off (the `--test-ad` switch wins when set).
    pub fn test_ad(mut self, enabled: bool) -> Self {
        self.options.test_ad = Some(enabled);
        self
    }

    /// Console-assigned uid; overrides the configuration and the computed
    /// uid (G.2).
    pub fn uid(mut self, uid: impl Into<String>) -> Self {
        self.options.uid = Some(uid.into());
        self
    }

    /// Whether the plugin registers the opener, dialog and global-shortcut
    /// plugins it calls when the app has not (default `true`).
    pub fn companion_plugins(mut self, enabled: bool) -> Self {
        self.options.companion_plugins = enabled;
        self
    }

    /// Whether the plugin adds its runtime capabilities (default `true`).
    /// Apps that grant `overwolf:main` to `ow-main` in their own
    /// capability files, and test harnesses with an empty ACL, turn it off.
    pub fn runtime_capabilities(mut self, enabled: bool) -> Self {
        self.options.runtime_capabilities = enabled;
        self
    }

    /// Whether the plugin creates the hidden main webview `ow-main`
    /// (default `true`). Tests turn it off.
    pub fn main_webview(mut self, enabled: bool) -> Self {
        self.options.main_webview = enabled;
        self
    }

    /// Overrides the process arguments (tests).
    pub fn argv(mut self, argv: Vec<String>) -> Self {
        self.options.argv = Some(argv);
        self
    }

    /// Skips every OS display and cursor query, for Tauri's mock runtime,
    /// which implements none: displays are empty and the cursor is at 0, 0.
    #[cfg(feature = "test-util")]
    pub fn skip_os_queries(mut self) -> Self {
        self.options.os_queries = false;
        self
    }

    /// Builds the plugin. The configuration is `plugins.overwolf` in
    /// `tauri.conf.json` (A.1); it may be absent.
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
            // window closing or from the OS (A.6): the plugin runs the quit
            // sequence instead. During a soft restart or after a main webview
            // crash the plugin is already handling the exit.
            if code.is_none() {
                api.prevent_exit();
                if !host.is_exiting() && !host.soft_restart_pending() && host.has_main_webview() {
                    host.begin_quit(0);
                }
            }
        }
        RunEvent::WindowEvent { label, event, .. } => window_event(&host, label, event),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            host.send_main(crate::ipc::messages::HostMessage::lifecycle(
                "activate", None, None,
            ));
        }
        _ => {}
    }
}

fn window_event<R: Runtime>(host: &Arc<Host<R>>, label: &str, event: &WindowEvent) {
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
        WebviewClass::Ui(id) => host.window_event(id, event),
        _ => {}
    }
}

fn on_navigation<R: Runtime>(webview: &Webview<R>, url: &Url) -> bool {
    let Some(host) = host_of(webview) else {
        return true;
    };
    match classify(webview.label()) {
        WebviewClass::Main => host.main_navigation(url),
        WebviewClass::Ui(_) => host.ui_navigation(url),
        _ => true,
    }
}

fn on_page_load<R: Runtime>(webview: &Webview<R>, payload: &PageLoadPayload<'_>) {
    let Some(host) = host_of(webview) else { return };
    let label = webview.label();
    match classify(label) {
        WebviewClass::Main => host.main_page_load(payload.event()),
        WebviewClass::Ui(id) => host.window_page_load(id, true, label, payload.event()),
        WebviewClass::Remote(id) => host.window_page_load(id, false, label, payload.event()),
        _ => {}
    }
}
