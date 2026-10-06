//! The hidden main webview `ow-main`: creation, liveness, navigation policy,
//! soft restart and crash handling (CONTRACT A.6, B.1.6).

use std::sync::Arc;

use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use url::Url;

use super::Host;
use crate::ipc::router::MAIN_LABEL;
use crate::lifecycle::{CrashDecision, record_crash};
use crate::screen::{ElectronDisplay, MonitorInfo};
use crate::state::log::LogLevel;
use crate::window::options::{MainNavigation, NAVIGATION_HOOK_IS_TOP_LEVEL_ONLY, main_navigation};

/// The runtime bundle embedded into `ow-main` and every `bw-*` webview.
pub(crate) const BOOTSTRAP_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/bootstrap.js"));

fn monitor_info(m: &tauri::Monitor) -> MonitorInfo {
    let work = m.work_area();
    MonitorInfo {
        name: m.name().cloned().unwrap_or_default(),
        x: m.position().x,
        y: m.position().y,
        width: m.size().width,
        height: m.size().height,
        work_x: work.position.x,
        work_y: work.position.y,
        work_width: work.size.width,
        work_height: work.size.height,
        scale_factor: m.scale_factor(),
    }
}

/// Current monitors as Tauri reports them.
/// Empty when `os_queries` is off.
pub(crate) fn monitors<R: Runtime>(
    app: &AppHandle<R>,
    os_queries: bool,
) -> (Vec<MonitorInfo>, Option<MonitorInfo>) {
    if !os_queries {
        return (Vec::new(), None);
    }
    let all = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(monitor_info)
        .collect();
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .as_ref()
        .map(monitor_info);
    (all, primary)
}

/// Displays in Electron's shape and the primary display id.
pub(crate) fn read_displays<R: Runtime>(
    app: &AppHandle<R>,
    os_queries: bool,
) -> (Vec<ElectronDisplay>, u32) {
    let (all, primary) = monitors(app, os_queries);
    super::setup::displays_of(&all, primary.as_ref())
}

/// `window.__OW_TAURI_BOOTSTRAP__ = <json>;` followed by the runtime bundle,
/// guarded so it does nothing outside the app origin (A.2.3.1): the snapshot
/// carries the app's identity.
pub(crate) fn main_init_script(origin: &str, snapshot: &serde_json::Value) -> String {
    format!(
        "if (location.origin === {origin}) {{\nwindow.__OW_TAURI_BOOTSTRAP__ = {snapshot};\n{BOOTSTRAP_JS}\n}}",
        origin = serde_json::Value::String(origin.to_owned())
    )
}

/// What to do once `ow-main`'s window is gone.
enum AfterDestroyed {
    /// Nothing (the app is exiting, or the restart already recreates it).
    Nothing,
    /// Create the new `ow-main` of a soft restart.
    Recreate(Url),
    /// It disappeared on its own: treat it as a crash.
    Crashed,
}

/// What to do after a page load of `ow-main`.
enum AfterMainLoad {
    Nothing,
    /// A top-level navigation the policy could not tell from a frame got
    /// through (macOS, Linux): replace the document cleanly.
    Restart(Url),
    /// A soft restart finished while an exit request waited for it.
    Quit,
}

impl<R: Runtime> Host<R> {
    /// Creates `ow-main` from the current snapshot (A.6 liveness measures).
    ///
    /// Never call it from a Tauri event hook (`on_event`, `on_page_load`,
    /// `on_navigation`, `on_webview_ready`): Tauri holds its plugin-store
    /// lock while it runs plugin hooks, and building a webview takes that
    /// lock again.
    pub(crate) fn create_main_webview(self: &Arc<Self>, url: Option<Url>) -> tauri::Result<()> {
        let snapshot = self.snapshot();
        let main = &self.info.config.main;
        let target = match url {
            Some(u) => WebviewUrl::External(u),
            None => WebviewUrl::App(main.url.trim_start_matches('/').into()),
        };
        let keeps_timers = crate::platform::hidden_main_webview_keeps_timers();
        let origin = super::windows::origin_string(&self.info.app_origin);
        let weak = Arc::downgrade(self);
        let mut builder = WebviewWindowBuilder::new(&self.app, MAIN_LABEL, target)
            .title(self.info.manifest.product_name.clone())
            .initialization_script(main_init_script(&origin, &snapshot))
            .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled)
            .focused(false)
            .skip_taskbar(true)
            .devtools(self.info.debug && main.devtools)
            .on_new_window(move |url, _features| {
                if let Some(host) = weak.upgrade() {
                    host.log(
                        LogLevel::Warn,
                        &format!(
                            "window.open in the main webview denied ({} URL)",
                            url.scheme()
                        ),
                    );
                }
                NewWindowResponse::Deny
            });
        if keeps_timers {
            builder = builder.visible(false);
        } else {
            // Technically visible: 1 x 1, transparent, at the origin (A.6).
            builder = builder
                .visible(true)
                .inner_size(1.0, 1.0)
                .position(0.0, 0.0)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .resizable(false)
                .always_on_bottom(true);
        }
        #[cfg(windows)]
        {
            builder = builder.additional_browser_args(&self.info.browser_args);
        }
        let window = builder.build()?;
        if !keeps_timers {
            let _ = window.set_ignore_cursor_events(true);
        }
        self.log(
            LogLevel::Info,
            if keeps_timers {
                "main webview created hidden (timers keep running)"
            } else {
                "main webview created as a 1 x 1 transparent window (timers keep running)"
            },
        );
        #[cfg(any(debug_assertions, feature = "devtools"))]
        if self.info.debug && main.devtools {
            window.open_devtools();
        }
        Ok(())
    }

    /// The navigation policy of `ow-main` (A.6): the first load is allowed;
    /// afterwards a reload of the current document is a soft restart in
    /// debug builds and cancelled in release builds, frames are allowed
    /// where the engine reports them (see [`main_navigation`]), and anything
    /// else is cancelled.
    pub(crate) fn main_navigation(self: &Arc<Self>, url: &Url) -> bool {
        let (loaded, current) =
            self.with_core(|c| (c.main_loaded, c.urls.get(MAIN_LABEL).cloned()));
        if !loaded {
            return true;
        }
        match main_navigation(
            url,
            &self.info.app_origin,
            current.as_deref(),
            NAVIGATION_HOOK_IS_TOP_LEVEL_ONLY,
        ) {
            MainNavigation::Allow => true,
            MainNavigation::Reload if self.info.debug => {
                let host = Arc::clone(self);
                let url = url.clone();
                tauri::async_runtime::spawn(async move { host.soft_restart(url) });
                false
            }
            MainNavigation::Reload => {
                self.log(
                    LogLevel::Warn,
                    "reload of the main webview cancelled (release build)",
                );
                false
            }
            MainNavigation::Cancel => {
                self.log(
                    LogLevel::Warn,
                    &format!(
                        "navigation of the main webview cancelled ({} URL)",
                        url.scheme()
                    ),
                );
                false
            }
        }
    }

    /// Page load hook for `ow-main`.
    pub(crate) fn main_page_load(self: &Arc<Self>, event: PageLoadEvent, url: &Url) {
        let href = url.to_string();
        let after = self.with_core(|c| {
            let previous = c.urls.insert(MAIN_LABEL.to_owned(), href.clone());
            match event {
                PageLoadEvent::Started => {
                    if c.main_loaded
                        && c.soft_restart.is_none()
                        && previous.as_deref() != Some(&href)
                    {
                        AfterMainLoad::Restart(url.clone())
                    } else {
                        AfterMainLoad::Nothing
                    }
                }
                PageLoadEvent::Finished => {
                    c.main_loaded = true;
                    if c.soft_restart.is_some() && c.restart_recreating {
                        c.soft_restart = None;
                        c.restart_recreating = false;
                        if std::mem::take(&mut c.quit_after_restart) {
                            return AfterMainLoad::Quit;
                        }
                    }
                    AfterMainLoad::Nothing
                }
            }
        });
        if event == PageLoadEvent::Finished {
            // E.1: `<UA>` from the platform webview's own user agent.
            self.request_user_agent();
        }
        match after {
            AfterMainLoad::Nothing => {}
            AfterMainLoad::Restart(url) => {
                self.log(
                    LogLevel::Warn,
                    "the main webview loaded another document; restarting it",
                );
                let host = Arc::clone(self);
                tauri::async_runtime::spawn(async move { host.soft_restart(url) });
            }
            AfterMainLoad::Quit => self.begin_quit(0),
        }
    }

    /// A.6 soft restart: close every app window, reject pending work, and
    /// recreate `ow-main` with a fresh snapshot once the old one is gone.
    /// Runs on an async-runtime thread, never inside a Tauri event hook.
    pub(crate) fn soft_restart(self: &Arc<Self>, url: Url) {
        let started = self.with_core(|c| {
            if c.soft_restart.is_some() || c.exiting {
                return None;
            }
            c.soft_restart = Some(url);
            c.restart_recreating = false;
            c.router.restart_main();
            c.router.document_unloaded(MAIN_LABEL);
            c.sinks.remove(MAIN_LABEL);
            c.close_requests.clear();
            c.quit = crate::lifecycle::QuitSequence::default();
            c.main_ready = false;
            c.main_loaded = false;
            c.flags = crate::snapshot::Flags::default();
            c.patch(vec![(
                "flags".into(),
                serde_json::to_value(c.flags).unwrap_or_default(),
            )]);
            let windows = c.windows.ids();
            c.restart_stale_windows.extend(windows.iter().copied());
            let evals: Vec<_> = c.evals.drain().map(|(_, e)| e).collect();
            Some((windows, evals))
        });
        let Some((windows, evals)) = started else {
            return;
        };
        self.log(LogLevel::Info, "main webview reload: soft restart");
        for e in evals {
            let _ =
                e.tx.send(Err(crate::Error::not_ready("The main webview restarted.")));
        }
        self.unregister_all_shortcuts();
        for id in windows {
            self.destroy_window(id);
        }
        match self.app.get_webview_window(MAIN_LABEL) {
            Some(main) => {
                if let Err(err) = main.destroy() {
                    self.log(
                        LogLevel::Error,
                        &format!("soft restart: destroying the main webview failed: {err}"),
                    );
                }
            }
            None => self.main_destroyed(),
        }
    }

    /// `ow-main`'s window was destroyed. Called from the plugin's
    /// `on_event` hook, so the new `ow-main` of a soft restart is created on
    /// a blocking-task thread (see [`Host::create_main_webview`]).
    pub(crate) fn main_destroyed(self: &Arc<Self>) {
        let after = self.with_core(|c| {
            c.urls.remove(MAIN_LABEL);
            match &c.soft_restart {
                Some(_) if c.restart_recreating => AfterDestroyed::Nothing,
                Some(url) => {
                    let url = url.clone();
                    c.restart_recreating = true;
                    AfterDestroyed::Recreate(url)
                }
                None if c.exiting => AfterDestroyed::Nothing,
                None => AfterDestroyed::Crashed,
            }
        });
        match after {
            AfterDestroyed::Nothing => {}
            AfterDestroyed::Recreate(url) => {
                let host = Arc::clone(self);
                tauri::async_runtime::spawn_blocking(move || {
                    if let Err(err) = host.create_main_webview(Some(url)) {
                        host.log(
                            LogLevel::Error,
                            &format!("soft restart: recreating the main webview failed: {err}"),
                        );
                        host.with_core(|c| {
                            c.soft_restart = None;
                            c.restart_recreating = false;
                        });
                        host.main_crashed();
                    }
                });
            }
            AfterDestroyed::Crashed => {
                self.log(LogLevel::Error, "the main webview was destroyed");
                self.main_crashed();
            }
        }
    }

    /// The `ow-main` render process died (A.6): log, drain analytics, then
    /// relaunch, or exit with code 1 at the crash limit. A second report of
    /// the same crash (the app's hook and the window's `Destroyed`) is
    /// ignored.
    pub(crate) fn main_crashed(self: &Arc<Self>) {
        if self.with_core(|c| std::mem::replace(&mut c.exiting, true)) {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let limit = self.info.config.main.crash_restart_limit;
        let mut decision = CrashDecision::Relaunch;
        let saved = self.ow_tauri.update(|s| {
            let mut history: Vec<u64> = s
                .extra
                .get("mainCrashes")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            decision = record_crash(&mut history, now, limit);
            s.extra
                .insert("mainCrashes".into(), serde_json::json!(history));
        });
        if let Err(err) = saved {
            self.log(
                LogLevel::Warn,
                &format!("could not record the crash: {}", err.kind()),
            );
        }
        let (code, relaunch) = match decision {
            CrashDecision::Relaunch => {
                self.log(LogLevel::Error, "the main webview crashed; relaunching");
                (0, true)
            }
            CrashDecision::Exit => {
                self.log(
                    LogLevel::Error,
                    &format!("the main webview crashed {limit} times within 60 s (main.crashRestartLimit); exiting"),
                );
                (1, false)
            }
        };
        let args: Vec<String> = self.info.argv.iter().skip(1).cloned().collect();
        self.with_core(|c| c.relaunch_args = relaunch.then_some(args));
        self.run_exit(code);
    }

    /// Re-reads the displays and pushes a `state` patch when they changed.
    pub(crate) fn poll_displays(self: &Arc<Self>) {
        let (displays, primary) = read_displays(&self.app, self.options.os_queries);
        let Ok(value) = serde_json::to_value(&displays) else {
            return;
        };
        self.with_core(|c| {
            c.patch(vec![
                ("displays".into(), value),
                ("primaryDisplayId".into(), primary.into()),
            ]);
        });
    }

    /// Whether `ow-main` exists.
    pub(crate) fn has_main_webview(&self) -> bool {
        self.app.get_webview_window(MAIN_LABEL).is_some()
    }
}
