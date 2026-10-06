//! The hidden main webview `ow-main`: creation, liveness, navigation policy,
//! soft restart and crash handling (CONTRACT A.6, B.1.6).

use std::sync::Arc;

use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use url::Url;

use super::Host;
use crate::ipc::router::MAIN_LABEL;
use crate::lifecycle::{CrashDecision, record_crash};
use crate::screen::{ElectronDisplay, MonitorInfo};
use crate::state::log::LogLevel;
use crate::window::options::same_origin;

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

/// `window.__OW_TAURI_BOOTSTRAP__ = <json>;` followed by the runtime bundle.
pub(crate) fn main_init_script(snapshot: &serde_json::Value) -> String {
    format!("window.__OW_TAURI_BOOTSTRAP__ = {snapshot};\n{BOOTSTRAP_JS}")
}

impl<R: Runtime> Host<R> {
    /// Creates `ow-main` from the current snapshot (A.6 liveness measures).
    pub(crate) fn create_main_webview(self: &Arc<Self>, url: Option<Url>) -> tauri::Result<()> {
        let snapshot = self.snapshot();
        let main = &self.info.config.main;
        let target = match url {
            Some(u) => WebviewUrl::External(u),
            None => WebviewUrl::App(main.url.trim_start_matches('/').into()),
        };
        let keeps_timers = crate::platform::hidden_main_webview_keeps_timers();
        let mut builder = WebviewWindowBuilder::new(&self.app, MAIN_LABEL, target)
            .title(self.info.manifest.product_name.clone())
            .initialization_script(main_init_script(&snapshot))
            .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled)
            .focused(false)
            .skip_taskbar(true)
            .devtools(self.info.debug && main.devtools);
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
        #[cfg(any(debug_assertions, feature = "devtools"))]
        if self.info.debug && main.devtools {
            window.open_devtools();
        }
        Ok(())
    }

    /// The navigation policy of `ow-main` (A.6): the first load is allowed;
    /// afterwards release builds cancel every navigation and debug builds turn
    /// a same-origin navigation (reload, dev-server refresh) into a soft
    /// restart.
    pub(crate) fn main_navigation(self: &Arc<Self>, url: &Url) -> bool {
        let loaded = self.with_core(|c| c.main_loaded);
        if !loaded {
            return true;
        }
        if !self.info.debug {
            self.log(
                LogLevel::Warn,
                "navigation of the main webview cancelled (release build)",
            );
            return false;
        }
        if same_origin(url, &self.info.app_origin) {
            let host = Arc::clone(self);
            let url = url.clone();
            tauri::async_runtime::spawn(async move { host.soft_restart(url) });
        } else {
            self.log(
                LogLevel::Warn,
                "navigation of the main webview to another origin cancelled",
            );
        }
        false
    }

    /// Page load hook for `ow-main`.
    pub(crate) fn main_page_load(self: &Arc<Self>, event: PageLoadEvent) {
        if event == PageLoadEvent::Finished {
            self.with_core(|c| c.main_loaded = true);
        }
    }

    /// A.6 soft restart (debug builds): close every app window, reject
    /// pending work, and recreate `ow-main` with a fresh snapshot once the
    /// old one is gone.
    pub(crate) fn soft_restart(self: &Arc<Self>, url: Url) {
        self.log(LogLevel::Info, "main webview reload: soft restart");
        let (windows, evals) = self.with_core(|c| {
            c.soft_restart = Some(url);
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
            let evals: Vec<_> = c.evals.drain().map(|(_, e)| e).collect();
            (c.windows.ids(), evals)
        });
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

    /// `ow-main`'s window was destroyed.
    pub(crate) fn main_destroyed(self: &Arc<Self>) {
        let (restart, exiting) = self.with_core(|c| (c.soft_restart.take(), c.exiting));
        if let Some(url) = restart {
            if let Err(err) = self.create_main_webview(Some(url)) {
                self.log(
                    LogLevel::Error,
                    &format!("soft restart: recreating the main webview failed: {err}"),
                );
            }
            return;
        }
        if !exiting {
            self.log(LogLevel::Error, "the main webview was destroyed");
            self.main_crashed();
        }
    }

    /// The `ow-main` render process died (A.6): log, drain analytics, then
    /// relaunch, or exit with code 1 at the crash limit.
    pub(crate) fn main_crashed(self: &Arc<Self>) {
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
        self.finish_exit(code);
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
