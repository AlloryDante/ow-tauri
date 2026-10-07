//! The plugin's runtime: shared state, delivery of host messages, timers and
//! the log. Commands and Tauri event hooks call into [`Host`].
//!
//! Locking rule: [`Host::with_core`] holds the core lock only for pure state
//! changes. Every Tauri call (creating, destroying or evaluating in a
//! webview, sending on a channel) happens after the lock is released, because
//! Tauri may run window event handlers, which take the lock, on the calling
//! thread.

pub(crate) mod ads;
pub(crate) mod analytics;
pub(crate) mod consent;
pub(crate) mod cookies;
mod main_webview;
mod setup;
mod windows;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::oneshot;
use url::Url;

use crate::config::{Config, Switches};
use crate::error::Error;
use crate::fs_scope::FsScope;
use crate::identity::AppIdentity;
use crate::ipc::messages::HostMessage;
use crate::ipc::router::{MAIN_LABEL, Router};
use crate::lifecycle::{QuitAction, QuitSequence};
use crate::manifest::EmbeddedManifest;
use crate::paths::TargetOs;
use crate::snapshot::{Flags, StateHub};
use crate::state::StateDir;
use crate::state::log::{LogLevel, Logger};
use crate::state::ow_electron::OwElectronFile;
use crate::state::ow_tauri::OwTauriFile;
use crate::window::WindowRegistry;

pub(crate) use main_webview::monitors;
pub(crate) use setup::{SetupOptions, displays_of, setup as build_host};

/// Interval of the timer task.
const TICK: Duration = Duration::from_millis(250);
/// The slow poll (window state, displays) runs every this many ticks (2 s).
const SLOW_EVERY: u64 = 8;
/// `main_ready` fallback (A.2.1).
const MAIN_READY_FALLBACK_MS: u64 = 10_000;
/// Ticks between two lab `core-stats.jsonl` records (10 s).
const LAB_STATS_EVERY: u64 = 40;
/// `window_eval` result timeout.
pub(crate) const EVAL_TIMEOUT_MS: u64 = 30_000;

/// A channel a webview subscribed with `ipc_subscribe`.
pub(crate) type Sink = Channel<Vec<HostMessage>>;

/// Facts fixed at setup.
#[derive(Debug)]
pub(crate) struct Info {
    pub(crate) config: Config,
    pub(crate) manifest: EmbeddedManifest,
    pub(crate) identity: AppIdentity,
    pub(crate) muid: String,
    pub(crate) muid_v2: String,
    pub(crate) phase_percent: u8,
    pub(crate) utm_params: Option<Value>,
    /// `cmp.unifiedConsentString` of `ow-electron.json` at launch: the
    /// `consent` and `consentFull` of every ad guest (D.2).
    pub(crate) launch_consent: String,
    pub(crate) state_dir: StateDir,
    pub(crate) fs_scope: FsScope,
    pub(crate) app_origin: Url,
    /// Browser arguments for every webview (A.1.1); WebView2 only.
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "browser arguments apply to WebView2 only")
    )]
    pub(crate) browser_args: String,
    pub(crate) argv: Vec<String>,
    /// Session switches; read by the ads service.
    pub(crate) switches: Switches,
    /// The ads data store's user data folder, `<appData>/<PN>/EBWebView-ow`
    /// (A.1.1); WebView2 only.
    #[cfg_attr(
        not(windows),
        expect(
            dead_code,
            reason = "the ads data store folder applies to WebView2 only"
        )
    )]
    pub(crate) ads_data_dir: std::path::PathBuf,
    /// `app.getPath('userData')`, `<appData>/<PN>`: electron-updater keeps
    /// its staging id in `.updaterId` there (I.2 #5).
    pub(crate) user_data_dir: std::path::PathBuf,
    pub(crate) debug: bool,
    pub(crate) os: TargetOs,
}

/// A `close` event waiting for `window_close_reply`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CloseRequest {
    pub(crate) window: u32,
    pub(crate) deadline: u64,
}

/// A `window_eval` waiting for `eval_result`.
#[derive(Debug)]
pub(crate) struct PendingEval {
    pub(crate) window: u32,
    pub(crate) tx: oneshot::Sender<Result<Option<Value>, Error>>,
    pub(crate) deadline: u64,
}

/// Mutable state, behind one lock.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent lifecycle facts, each read on its own"
)]
pub(crate) struct Core {
    pub(crate) router: Router,
    pub(crate) sinks: HashMap<String, Sink>,
    pub(crate) state: StateHub,
    pub(crate) windows: WindowRegistry,
    pub(crate) quit: QuitSequence,
    pub(crate) request_ids: u64,
    pub(crate) close_requests: HashMap<u64, CloseRequest>,
    pub(crate) evals: HashMap<u64, PendingEval>,
    pub(crate) next_eval: u64,
    /// Global shortcuts this plugin registered: the shortcut's id (its
    /// meaning, so `Ctrl+K` and `CommandOrControl+K` match where they are the
    /// same keys) to the normalised accelerator.
    pub(crate) shortcuts: BTreeMap<u32, String>,
    pub(crate) flags: Flags,
    pub(crate) main_ready: bool,
    pub(crate) main_ready_warned: bool,
    pub(crate) main_loaded: bool,
    pub(crate) relaunch_args: Option<Vec<String>>,
    pub(crate) exiting: bool,
    /// Set from the start of a soft restart until the new `ow-main` finishes
    /// its first load (A.6).
    pub(crate) soft_restart: Option<Url>,
    /// The new `ow-main` of the current soft restart is being created.
    pub(crate) restart_recreating: bool,
    /// An exit request arrived during a soft restart; the quit sequence runs
    /// once the new `ow-main` has loaded.
    pub(crate) quit_after_restart: bool,
    /// Windows closed by the current soft restart: their late events never
    /// reach the new `ow-main`, which did not create them.
    pub(crate) restart_stale_windows: BTreeSet<u32>,
    /// The current top-level document URL of each webview, from its page
    /// loads (`IpcSender.url` and the `ow-main` reload check).
    pub(crate) urls: HashMap<String, String>,
    /// The last in-page URL of each webview since its current load started
    /// (`navigation_in_page`): a `did-finish-load` reports it instead of the
    /// URL the load started with, as Electron's does.
    pub(crate) in_page_urls: HashMap<String, String>,
    pub(crate) ticks: u64,
    /// The ads service (D).
    pub(crate) ads: ads::AdsCore,
    /// The consent service (D.6).
    pub(crate) consent: consent::ConsentCore,
    /// URLs a host without OS queries (Tauri's mock runtime) would have
    /// opened in the system browser; tests read them.
    pub(crate) browser_opens: Vec<String>,
    /// The update client (I).
    pub(crate) updater: crate::updater::UpdaterCore,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core")
            .field("router", &self.router)
            .field("sinks", &self.sinks.keys().collect::<Vec<_>>())
            .field("windows", &self.windows)
            .field("quit", &self.quit)
            .finish_non_exhaustive()
    }
}

impl Core {
    pub(crate) fn next_request_id(&mut self) -> u64 {
        self.request_ids += 1;
        self.request_ids
    }

    /// Queues a message for `ow-main`. Events of windows a soft restart
    /// closed are dropped: the new `ow-main` never knew them.
    pub(crate) fn queue_main(&mut self, message: HostMessage) {
        if let HostMessage::Window { id, .. } = &message
            && self.restart_stale_windows.contains(id)
        {
            return;
        }
        self.router.push(MAIN_LABEL, message);
    }

    /// The sizes of the host's tables and queues, for the lab's
    /// `core-stats.jsonl`: over an idle run none of them may keep growing.
    fn lab_stats(&self, windows: &[u32]) -> Value {
        serde_json::json!({
            "router": self.router.lab_stats(),
            "windows": windows.len(),
            "sinks": self.sinks.len(),
            "urls": self.urls.len(),
            "inPageUrls": self.in_page_urls.len(),
            "closeRequests": self.close_requests.len(),
            "evals": self.evals.len(),
            "adGuests": self.ads.guests.len(),
            "consentWaiters": self.consent.waiters.len(),
            "consentHidden": self.consent.hidden.len(),
            "consentResolved": self.consent.resolved.len(),
            "browserOpens": self.browser_opens.len(),
            "restartStaleWindows": self.restart_stale_windows.len(),
        })
    }

    /// Applies state patches and queues the `state` message for `ow-main`
    /// when anything changed.
    pub(crate) fn patch(&mut self, patches: Vec<(String, Value)>) {
        if let Some(msg) = self.state.patch_if_changed(patches) {
            self.queue_main(msg);
        }
    }
}

/// The plugin runtime for one app.
pub(crate) struct Host<R: Runtime> {
    pub(crate) app: AppHandle<R>,
    pub(crate) info: Info,
    pub(crate) logger: Logger,
    pub(crate) ow_tauri: OwTauriFile,
    /// `ow-electron.json` (F.2); written by the consent service and the
    /// first launch.
    pub(crate) ow_electron: OwElectronFile,
    /// The analytics service (E).
    pub(crate) analytics: analytics::AnalyticsHost,
    pub(crate) options: SetupOptions,
    /// The `Overwolf::updater` handle (A.5).
    pub(crate) updater_api: crate::updater::Updater<R>,
    core: Mutex<Core>,
    started: Instant,
    flush_scheduled: AtomicBool,
}

impl<R: Runtime> std::fmt::Debug for Host<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl<R: Runtime> Host<R> {
    /// Milliseconds since setup (router and timer clock).
    pub(crate) fn now(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn lock(&self) -> MutexGuard<'_, Core> {
        self.core.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Writes one log line (and mirrors it to the `log` crate).
    pub(crate) fn log(&self, level: LogLevel, message: &str) {
        crate::lab::record(
            "plugin-log.jsonl",
            || serde_json::json!({ "level": level.as_str(), "message": message }),
        );
        match level {
            LogLevel::Debug => log::debug!(target: "ow-tauri", "{message}"),
            LogLevel::Info => log::info!(target: "ow-tauri", "{message}"),
            LogLevel::Warn => log::warn!(target: "ow-tauri", "{message}"),
            LogLevel::Error => log::error!(target: "ow-tauri", "{message}"),
        }
        if level != LogLevel::Debug || self.info.debug {
            self.logger.write(level, message);
        }
    }

    /// Runs `f` under the core lock, then forwards router logs and schedules
    /// delivery of anything queued.
    pub(crate) fn with_core<T>(self: &Arc<Self>, f: impl FnOnce(&mut Core) -> T) -> T {
        let (out, logs, has_output) = {
            let mut core = self.lock();
            let out = f(&mut core);
            let logs = core.router.take_logs();
            let has_output = core.router.has_output();
            (out, logs, has_output)
        };
        for line in logs {
            self.log(
                if line.warn {
                    LogLevel::Warn
                } else {
                    LogLevel::Debug
                },
                &line.message,
            );
        }
        if has_output {
            self.schedule_flush();
        }
        out
    }

    /// Delivers queued messages at the end of the current event-loop turn,
    /// one channel send per webview.
    pub(crate) fn schedule_flush(self: &Arc<Self>) {
        if self.flush_scheduled.swap(true, Ordering::AcqRel) {
            return;
        }
        let host = Arc::clone(self);
        if self.app.run_on_main_thread(move || host.flush()).is_err() {
            self.flush();
        }
    }

    /// Sends every queued batch now.
    pub(crate) fn flush(&self) {
        self.flush_scheduled.store(false, Ordering::Release);
        let batches: Vec<(String, Sink, Vec<HostMessage>)> = {
            let mut core = self.lock();
            let drained = core.router.drain();
            drained
                .into_iter()
                .filter_map(|(label, msgs)| {
                    core.sinks
                        .get(&label)
                        .cloned()
                        .map(|sink| (label, sink, msgs))
                })
                .collect()
        };
        for (label, sink, msgs) in batches {
            if let Err(err) = sink.send(msgs) {
                self.log(
                    LogLevel::Warn,
                    &format!("delivery to {label} failed: {err}"),
                );
            }
        }
    }

    /// Queues one message for `ow-main`.
    pub(crate) fn send_main(self: &Arc<Self>, message: HostMessage) {
        self.with_core(|c| c.queue_main(message));
    }

    /// Starts the timer task.
    pub(crate) fn spawn_ticker(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(TICK).await;
                let Some(host) = weak.upgrade() else { break };
                host.tick();
            }
        });
    }

    /// One timer step: router timers, close and eval timeouts, quit step
    /// timeouts, the `main_ready` fallback and the slow polls.
    pub(crate) fn tick(self: &Arc<Self>) {
        let now = self.now();
        let (closes, evals, quit, slow, warn_ready, stats) = self.with_core(|c| {
            c.ticks += 1;
            c.router.tick(now);
            let expired: Vec<u64> = c
                .close_requests
                .iter()
                .filter(|(_, r)| now >= r.deadline)
                .map(|(id, _)| *id)
                .collect();
            let closes: Vec<u32> = expired
                .into_iter()
                .filter_map(|id| c.close_requests.remove(&id))
                .map(|r| r.window)
                .collect();
            let expired: Vec<u64> = c
                .evals
                .iter()
                .filter(|(_, e)| now >= e.deadline)
                .map(|(id, _)| *id)
                .collect();
            let evals: Vec<PendingEval> = expired
                .into_iter()
                .filter_map(|id| c.evals.remove(&id))
                .collect();
            let windows = c.windows.ids();
            let mut ids = c.request_ids;
            let quit = c.quit.tick(now, &windows, &mut ids);
            c.request_ids = ids;
            let warn_ready = !c.main_ready && !c.main_ready_warned && now >= MAIN_READY_FALLBACK_MS;
            if warn_ready {
                c.main_ready_warned = true;
            }
            let stats = (c.ticks % LAB_STATS_EVERY == 0 && crate::lab::trace_on())
                .then(|| c.lab_stats(&windows));
            (
                closes,
                evals,
                quit,
                c.ticks % SLOW_EVERY == 0,
                warn_ready,
                stats,
            )
        });
        if let Some(mut stats) = stats {
            stats["nativeWebviews"] = self.app.webviews().len().into();
            crate::lab::record("core-stats.jsonl", || stats);
        }
        for e in evals {
            let _ = e.tx.send(Err(Error::ipc_timeout(
                "executeJavaScript did not report a result within 30 s.",
            )));
        }
        for window in closes {
            self.destroy_window(window);
        }
        self.run_quit_actions(quit);
        if warn_ready {
            self.log(
                LogLevel::Warn,
                "main_ready did not arrive within 10 s; starting analytics and packages anyway",
            );
            self.start_analytics();
        }
        self.poll_visibility();
        self.ads_tick(now);
        self.consent_tick(now);
        if slow {
            self.poll_windows();
            self.poll_displays();
        }
    }

    /// Executes quit-sequence actions (A.6).
    pub(crate) fn run_quit_actions(self: &Arc<Self>, actions: Vec<QuitAction>) {
        for action in actions {
            match action {
                QuitAction::Lifecycle { event, request_id } => {
                    self.send_main(HostMessage::lifecycle(event, Some(request_id), None));
                }
                QuitAction::CloseWindows(list) => self.with_core(|c| {
                    for (window, request_id) in list {
                        c.queue_main(HostMessage::Window {
                            id: window,
                            event: crate::ipc::messages::WindowEventName::Close,
                            request_id: Some(request_id),
                            data: None,
                        });
                    }
                }),
                QuitAction::DestroyWindow(window) => self.destroy_window(window),
                QuitAction::Cancelled => self.log(LogLevel::Info, "quit cancelled by the app"),
                QuitAction::Finish { exit_code } => self.finish_exit(exit_code),
            }
        }
    }

    /// Starts the quit sequence (A.6 step 1).
    pub(crate) fn begin_quit(self: &Arc<Self>, exit_code: i32) {
        let now = self.now();
        let action = self.with_core(|c| {
            let mut ids = c.request_ids;
            let a = c.quit.begin(exit_code, now, &mut ids);
            c.request_ids = ids;
            a
        });
        if let Some(a) = action {
            self.run_quit_actions(vec![a]);
        }
    }

    /// A.6 step 5 (and `app.exit()`): analytics drain, `quit`, pending update
    /// install, exit. A scheduled relaunch starts from [`Host::on_exit`].
    pub(crate) fn finish_exit(self: &Arc<Self>, exit_code: i32) {
        let already = self.with_core(|c| std::mem::replace(&mut c.exiting, true));
        if !already {
            self.run_exit(exit_code);
        }
    }

    /// The body of [`Host::finish_exit`]; the caller has set `exiting`.
    fn run_exit(self: &Arc<Self>, exit_code: i32) {
        // Quit while windows are visible ends their visible periods (E.2 #7).
        self.analytics_end_all_periods();
        let host = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            crate::analytics::drain(&host.analytics.dispatcher, crate::analytics::DRAIN_LIMIT)
                .await;
            host.send_main(HostMessage::lifecycle("quit", None, Some(exit_code)));
            host.flush_on_main_thread().await;
            host.updater_install_at_exit().await;
            host.log(LogLevel::Info, &format!("exiting with code {exit_code}"));
            host.app.exit(exit_code);
        });
    }

    /// Sends everything queued from the main thread, after any flush already
    /// scheduled there, and waits (at most 1 s) until it has run. Sending
    /// only from the main thread keeps the A.3 delivery order.
    async fn flush_on_main_thread(self: &Arc<Self>) {
        let (tx, rx) = oneshot::channel();
        let host = Arc::clone(self);
        let posted = self.app.run_on_main_thread(move || {
            host.flush();
            let _ = tx.send(());
        });
        if posted.is_err() {
            self.flush();
            return;
        }
        let _ = tokio::time::timeout(Duration::from_secs(1), rx).await;
    }

    /// `RunEvent::Exit`: starts the relaunch `app.relaunch()` or a crash
    /// scheduled. It runs after the plugins registered before this one
    /// (`tauri-plugin-single-instance` is registered first, A.5) have
    /// released their single-instance lock, so the new process is not
    /// turned away as a second instance.
    pub(crate) fn on_exit(&self) {
        let relaunch = self.lock().relaunch_args.take();
        if let Some(args) = relaunch {
            self.spawn_relaunch(&args);
        }
    }

    /// Starts a new instance of the app with `args`.
    pub(crate) fn spawn_relaunch(&self, args: &[String]) {
        let exe = tauri::process::current_binary(&self.app.env());
        match exe.and_then(|exe| std::process::Command::new(exe).args(args).spawn()) {
            Ok(_) => self.log(LogLevel::Info, "relaunching"),
            Err(err) => self.log(LogLevel::Error, &format!("relaunch failed: {}", err.kind())),
        }
    }

    /// An exit request without an exit code (the last window closed, or the
    /// OS: Cmd+Q, logoff, Ctrl+C). The caller always prevents the request;
    /// this decides what happens instead (A.6).
    pub(crate) fn exit_requested(self: &Arc<Self>) {
        enum Next {
            Nothing,
            Quit,
            Exit,
        }
        let has_main = self.has_main_webview();
        let next = self.with_core(|c| {
            if c.exiting || c.quit.is_running() {
                Next::Nothing
            } else if c.soft_restart.is_some() {
                // `ow-main` is being replaced; the quit runs once the new
                // one has loaded.
                c.quit_after_restart = true;
                Next::Nothing
            } else if has_main {
                Next::Quit
            } else {
                // No main webview to run the quit sequence (it is disabled
                // or gone): exit instead of swallowing the request.
                Next::Exit
            }
        });
        match next {
            Next::Nothing => {}
            Next::Quit => self.begin_quit(0),
            Next::Exit => self.finish_exit(0),
        }
    }

    /// The current snapshot (`bootstrap`, and the one injected into a new
    /// `ow-main`), with the cursor read now (A.2.1 `cursor`).
    pub(crate) fn snapshot(&self) -> Value {
        let mut value = self.lock().state.snapshot().clone();
        if let (Some(cursor), Value::Object(map)) = (
            main_webview::read_cursor(&self.app, self.options.os_queries),
            &mut value,
        ) && let Ok(cursor) = serde_json::to_value(cursor)
        {
            map.insert("cursor".into(), cursor);
        }
        value
    }

    /// Opens `url` in the system browser through `tauri-plugin-opener`. A
    /// host without OS queries (Tauri's mock runtime) only records it, so
    /// tests never launch a browser.
    pub(crate) fn open_in_browser(self: &Arc<Self>, url: &Url) -> Result<(), Error> {
        if !self.options.os_queries {
            self.with_core(|c| c.browser_opens.push(url.to_string()));
            return Ok(());
        }
        if crate::lab::block_os_surface(
            "open_in_browser",
            || serde_json::json!({ "url": url.as_str() }),
        ) {
            return Ok(());
        }
        tauri_plugin_opener::open_url(url.as_str(), None::<&str>).map_err(|err| {
            self.log(
                LogLevel::Warn,
                &format!("opening a URL in the system browser failed: {err}"),
            );
            Error::io("The system could not open the URL.")
        })
    }

    /// Reads a text file from the embedded app assets.
    pub(crate) fn read_asset_text(&self, path: &str) -> Result<String, Error> {
        crate::window::options::validate_asset_path(path)?;
        let asset = self
            .app
            .asset_resolver()
            .get(path.trim_start_matches('/').to_owned())
            .ok_or_else(|| {
                Error::not_found("The app asset does not exist.")
                    .with_data(serde_json::json!({ "path": path }))
            })?;
        String::from_utf8(asset.bytes)
            .map_err(|_| Error::invalid_argument("The app asset is not UTF-8 text."))
    }
}
