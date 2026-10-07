//! Host side of the analytics (CONTRACT E): the session state, the user
//! agent, and the hooks the window, ads and lifecycle code call.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::{Map, Value};
use tauri::{Manager, Runtime};

use super::Host;
use crate::analytics::session::{Event, Session};
use crate::analytics::transport::Dispatcher;
use crate::analytics::{HostLabel, HostRequest, HostResponse, Reporter, compose_user_agent};
use crate::state::log::LogLevel;
use crate::window::VisibilityChange;

/// How long host requests, consent windows and ad guests wait at startup
/// for `ow-main`'s real user agent before they use the fallback (E.1).
const UA_WAIT: Duration = Duration::from_millis(2500);

/// Where `<UA>` comes from (E.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UaState {
    /// The fallback; nobody asked `ow-main` yet.
    Fallback,
    /// `ow-main` was asked for `navigator.userAgent`.
    Requested,
    /// Composed from the platform user agent.
    Reported,
}

/// The analytics service of one host.
#[derive(Debug)]
pub(crate) struct AnalyticsHost {
    pub(crate) dispatcher: Dispatcher,
    state: Mutex<AnalyticsCore>,
    /// Woken when the platform user agent is reported.
    ua_ready: tokio::sync::Notify,
}

#[derive(Debug)]
struct AnalyticsCore {
    session: Session,
    reporter: Reporter,
    /// Where `<UA>` comes from (E.1).
    ua: UaState,
    /// `firstLaunch` was absent from `ow-electron.json` at setup.
    first_launch: bool,
    /// `firstLaunch: true` was written this launch.
    first_launch_recorded: bool,
}

/// The platform webview's default user agent, used until `ow-main`'s real
/// one is known (E.1): the stock WKWebView string on macOS; on Windows
/// WebView2's reduced string, which carries only the runtime's major
/// version (`Chrome/141.0.0.0`); a WebKitGTK string on Linux.
pub(crate) fn fallback_platform_ua(webview_version: &str) -> String {
    if cfg!(windows) {
        let major = webview_version.split('.').next().unwrap_or_default();
        format!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36 Edg/{major}.0.0.0"
        )
    } else if cfg!(target_os = "macos") {
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)"
            .to_owned()
    } else {
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)".to_owned()
    }
}

impl AnalyticsHost {
    pub(crate) fn new(
        dispatcher: Dispatcher,
        reporter: Reporter,
        user_enabled: bool,
        first_launch: bool,
    ) -> Self {
        AnalyticsHost {
            dispatcher,
            state: Mutex::new(AnalyticsCore {
                session: Session::new(user_enabled),
                reporter,
                ua: UaState::Fallback,
                first_launch,
                first_launch_recorded: false,
            }),
            ua_ready: tokio::sync::Notify::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, AnalyticsCore> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A request for `navigator.userAgent` failed: the next page load asks
    /// again.
    fn retry_user_agent(&self) {
        let mut core = self.lock();
        if core.ua == UaState::Requested {
            core.ua = UaState::Fallback;
        }
    }

    /// `<UA>` as currently known.
    pub(crate) fn user_agent(&self) -> String {
        self.lock().reporter.user_agent.clone()
    }

    /// Whether the ow-tauri user switch is on (`false` sends nothing).
    pub(crate) fn user_enabled(&self) -> bool {
        self.lock().session.user_enabled()
    }

    /// `firstLaunch` was absent from `ow-electron.json` at setup: this is
    /// the app's first launch.
    pub(crate) fn first_launch(&self) -> bool {
        self.lock().first_launch
    }

    /// A copy of the request builder.
    pub(crate) fn reporter(&self) -> Reporter {
        self.lock().reporter.clone()
    }

    fn requests(reporter: &Reporter, events: Vec<Event>) -> Vec<HostRequest> {
        events
            .into_iter()
            .map(|e| match e {
                Event::Counter { event, fields } => reporter.counter(event, &fields),
                Event::Stats { kind, reason } => reporter.insert_stats(kind, reason.as_deref()),
            })
            .collect()
    }

    /// Runs `f` on the session and sends the events it returns, in order.
    fn run(&self, f: impl FnOnce(&mut Session) -> Vec<Event>) -> usize {
        let requests = {
            let mut core = self.lock();
            let events = f(&mut core.session);
            Self::requests(&core.reporter, events)
        };
        let n = requests.len();
        for r in requests {
            drop(self.dispatcher.send(r, true));
        }
        n
    }
}

/// A label for `HostLabel` from the configuration.
pub(crate) fn host_label(config: &crate::config::AnalyticsConfig) -> HostLabel {
    HostLabel::new(
        config.host_label.clone(),
        config
            .host_version
            .clone()
            .unwrap_or_else(|| tauri::VERSION.to_owned()),
    )
}

impl<R: Runtime> Host<R> {
    /// The platform user agent of `ow-main`, from its first `ipc_subscribe`
    /// or read by the host when its page loads ([`Self::request_user_agent`]);
    /// `<UA>` is composed from the first report (E.1).
    pub(crate) fn report_user_agent(&self, default_ua: &str) {
        let default_ua = default_ua.trim();
        if default_ua.is_empty() || default_ua.len() > 1024 {
            return;
        }
        {
            let mut core = self.analytics.lock();
            if core.ua == UaState::Reported {
                return;
            }
            core.ua = UaState::Reported;
            let label = core.reporter.label.clone();
            core.reporter.user_agent = compose_user_agent(
                default_ua,
                &self.info.manifest.product_name,
                &self.info.manifest.version,
                &label,
                crate::platform::safari_version(),
            );
        }
        self.analytics.ua_ready.notify_waiters();
    }

    /// Asks `ow-main` for `navigator.userAgent` once, so `<UA>` does not
    /// depend on the bootstrap reporting it (E.1). Called on its page loads.
    pub(crate) fn request_user_agent(self: &Arc<Self>) {
        {
            let mut core = self.analytics.lock();
            if core.ua != UaState::Fallback {
                return;
            }
            core.ua = UaState::Requested;
        }
        let Some(main) = self.app.get_webview_window(crate::ipc::router::MAIN_LABEL) else {
            self.analytics.retry_user_agent();
            return;
        };
        let weak = Arc::downgrade(self);
        let asked = main.eval_with_callback("navigator.userAgent", move |json| {
            let Some(host) = weak.upgrade() else { return };
            match serde_json::from_str::<String>(&json) {
                Ok(ua) => host.report_user_agent(&ua),
                Err(_) => host.analytics.retry_user_agent(),
            }
        });
        if asked.is_err() {
            self.analytics.retry_user_agent();
        }
    }

    /// Waits until `<UA>` is final: at once when it is known or there is no
    /// `ow-main` to report it, otherwise until it is reported or [`UA_WAIT`]
    /// after startup has passed.
    pub(crate) async fn wait_user_agent(self: &Arc<Self>) {
        if !self.options.main_webview {
            return;
        }
        let deadline = self.started + UA_WAIT;
        loop {
            let notified = self.analytics.ua_ready.notified();
            if self.analytics.lock().ua == UaState::Reported {
                return;
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() || tokio::time::timeout(left, notified).await.is_err() {
                return;
            }
        }
    }

    /// `<UA>` for guests, consent windows and host requests.
    pub(crate) fn user_agent(&self) -> String {
        self.analytics.user_agent()
    }

    /// The launch sequence (E.2): at `main_ready`, or after the 10 s
    /// fallback. Idempotent.
    pub(crate) fn start_analytics(&self) {
        let now = self.now();
        let (requests, first_launch) = {
            let mut core = self.analytics.lock();
            if core.session.is_started() {
                return;
            }
            let first_launch = core.first_launch;
            let events = core.session.start(now, first_launch);
            (
                AnalyticsHost::requests(&core.reporter, events),
                first_launch,
            )
        };
        let n = requests.len();
        for r in requests {
            drop(self.analytics.dispatcher.send(r, true));
        }
        if first_launch {
            self.record_first_launch();
        }
        self.log(
            LogLevel::Debug,
            &format!("analytics started ({n} requests)"),
        );
    }

    /// Writes `firstLaunch: true` to `ow-electron.json` once per launch,
    /// on a first launch only. The consent start calls it before the startup
    /// consent flow can save `cmp`, so the file's keys come in ow-electron's
    /// order (`firstLaunch`, then `cmp`, observed); the analytics start calls
    /// it again in case consent never started.
    pub(crate) fn record_first_launch(&self) {
        {
            let mut core = self.analytics.lock();
            if !core.first_launch || std::mem::replace(&mut core.first_launch_recorded, true) {
                return;
            }
        }
        if let Err(err) = self.ow_electron.set_first_launch() {
            self.log(
                LogLevel::Warn,
                &format!("could not record the first launch: {err}"),
            );
        }
    }

    /// `disable_anonymous_analytics`.
    pub(crate) fn disable_anonymous_analytics(&self) {
        self.analytics.run(|s| {
            s.disable_anonymous();
            Vec::new()
        });
    }

    /// `analytics_set_user_enabled`.
    pub(crate) fn set_analytics_user_enabled(&self, enabled: bool) {
        self.analytics.run(|s| {
            s.set_user_enabled(enabled);
            Vec::new()
        });
    }

    /// An app window became visible.
    pub(crate) fn analytics_window_shown(&self) {
        let now = self.now();
        self.analytics.run(|s| s.window_shown(now));
    }

    /// A visible period of an app window ended.
    pub(crate) fn analytics_window_closed(&self, name: &str, title: &str, visible_ms: u64) {
        self.analytics
            .run(|s| s.window_closed(name, title, visible_ms));
    }

    /// The periodic heartbeat check (E.2 #9).
    pub(crate) fn analytics_tick(&self, has_visible_window: bool) {
        let now = self.now();
        self.analytics.run(|s| s.tick(now, has_visible_window));
    }

    /// Kind 400025 (E.2 #6).
    pub(crate) fn analytics_guest_attached(&self) {
        self.analytics.run(Session::guest_attached);
    }

    /// The crash Counter (E.2 #8), before the reload.
    pub(crate) fn analytics_guest_crash_counter(&self, session_secs: u64, reason: &str) {
        self.analytics
            .run(|s| s.guest_crash_counter(session_secs, reason));
    }

    /// The crash Kind 400024 (E.2 #8), after the reload.
    pub(crate) fn analytics_guest_crash_stats(&self, session_secs: u64, reason: &str) {
        self.analytics
            .run(|s| s.guest_crash_stats(session_secs, reason));
    }

    /// The document URL window `id` shows (its app or remote webview).
    fn window_url(core: &super::Core, id: u32) -> Option<String> {
        let kind = core.windows.get(id)?.kind;
        let label = if kind == crate::window::WindowKind::Remote {
            crate::window::remote_label(id)
        } else {
            crate::window::ui_label(id)
        };
        core.urls.get(&label).cloned()
    }

    /// Polls the visibility of every window (each timer step): starts and
    /// ends visible periods (E.2 #5, #7) and runs the heartbeat check (#9).
    pub(crate) fn poll_visibility(self: &Arc<Self>) {
        let ids = self.with_core(|c| c.windows.ids());
        // (id, visible, minimized). macOS reports a minimized window as not
        // visible; its minimize is not a hide (the guests' minimize messages
        // come from `ads_window_minimized`).
        let visible: Vec<(u32, bool, bool)> = ids
            .into_iter()
            .map(|id| {
                let w = self.app.get_window(&crate::window::ui_label(id));
                let v = w.as_ref().is_some_and(|w| w.is_visible().unwrap_or(false));
                let m = w
                    .as_ref()
                    .is_some_and(|w| w.is_minimized().unwrap_or(false));
                (id, v, m)
            })
            .collect();
        let any_visible = self.apply_poll(&visible);
        self.analytics_tick(any_visible);
    }

    /// One visibility poll's observations, `(id, visible, minimized)` as
    /// the OS reports them: starts and ends visible periods and moves the
    /// guests with their window. Returns whether any window is in a visible
    /// period.
    pub(crate) fn apply_poll(self: &Arc<Self>, visible: &[(u32, bool, bool)]) -> bool {
        let now = self.now();
        let (ended, guests, any_visible) = self.with_core(|c| {
            let mut shown = false;
            let mut ended = Vec::new();
            let mut guests = Vec::new();
            for &(id, v, minimized) in visible {
                let url = Self::window_url(c, id);
                let Some(entry) = c.windows.get_mut(id) else {
                    continue;
                };
                match entry.observe_window(v, minimized, now, url.as_deref()) {
                    VisibilityChange::Shown => shown = true,
                    VisibilityChange::Ended(p) => ended.push(p),
                    VisibilityChange::None => {}
                }
                // The guests follow the window itself, not its analytics
                // periods; a minimize reaches them through
                // `ads_window_minimized`.
                if let Some(v) = entry.guests_follow(v, minimized) {
                    guests.push((id, v));
                }
            }
            let any_visible = c.windows.ids().iter().any(|id| {
                c.windows
                    .get(*id)
                    .is_some_and(|e| e.visible_since.is_some())
            });
            // The first-visible-window heartbeat is queued under the same
            // lock that starts the visible period: a guest attaching on
            // another thread right after sees the window visible, and its
            // 400025 must not overtake the heartbeat (E.2 #5, #6). The
            // analytics session never takes the core lock.
            if shown {
                self.analytics_window_shown();
            }
            (ended, guests, any_visible)
        });
        for (id, v) in guests {
            if v {
                self.ads_window_shown(id);
            } else {
                self.ads_window_hidden(id);
            }
        }
        for p in ended {
            self.analytics_window_closed(&p.name, &p.title, p.visible_ms);
        }
        any_visible
    }

    /// A page of window `id` finished loading: fixes its analytics name
    /// when it is visible.
    pub(crate) fn analytics_page_finished(self: &Arc<Self>, id: u32, url: &str) {
        self.with_core(|c| {
            if let Some(entry) = c.windows.get_mut(id) {
                let visible = entry.state.visible;
                entry.page_finished(visible, Some(url));
            }
        });
    }

    /// Ends the visible period of window `id` (closed or destroyed).
    pub(crate) fn analytics_window_gone(self: &Arc<Self>, id: u32) {
        let now = self.now();
        let period = self.with_core(|c| {
            let url = Self::window_url(c, id);
            c.windows
                .get_mut(id)
                .and_then(|e| e.end_visible_period(now, url.as_deref()))
        });
        if let Some(p) = period {
            self.analytics_window_closed(&p.name, &p.title, p.visible_ms);
        }
    }

    /// Quit while windows are visible: each visible period ends (E.2 #7).
    pub(crate) fn analytics_end_all_periods(self: &Arc<Self>) {
        let ids = self.with_core(|c| c.windows.ids());
        for id in ids {
            self.analytics_window_gone(id);
        }
    }

    /// `<label>_sub_info` (E.2 #10); resolves after the response or the
    /// failure, never with an error.
    pub(crate) async fn analytics_sub_info(self: &Arc<Self>, options: Map<String, Value>) {
        let requests = {
            let core = self.analytics.lock();
            AnalyticsHost::requests(&core.reporter, core.session.sub_info(&options))
        };
        for r in requests {
            match self.analytics.dispatcher.send(r, true).await {
                Ok(HostResponse { status, .. }) if status < 400 => {}
                Ok(HostResponse { status, .. }) => {
                    self.log(LogLevel::Debug, &format!("sub_info: HTTP {status}"));
                }
                Err(err) => self.log(LogLevel::Debug, &format!("sub_info failed: {err}")),
            }
        }
    }
}
