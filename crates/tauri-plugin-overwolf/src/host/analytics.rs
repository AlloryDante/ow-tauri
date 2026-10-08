//! Host side of the analytics (CONTRACT E, DESIGN §4.8, §4.10): the session,
//! the request builder, `<UA>`, and the calls the lifecycle, windows and
//! ads code make.

use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde_json::{Map, Value};
use tauri::Runtime;

use super::{Core, lock};
use crate::analytics::session::{Event, Session};
use crate::analytics::transport::{BoxFuture, Dispatcher, RequestHooks};
use crate::analytics::user_agent::accepts_native;
use crate::analytics::{HostLabel, HostRequest, HostResponse, Reporter, compose_user_agent};
use crate::app_identity::AppIdentity;

/// How long host requests wait after `RunEvent::Ready` for the app
/// webview's real user agent before they use the template (DESIGN §4.10).
pub(crate) const UA_WAIT: Duration = Duration::from_millis(2500);

/// A label for `HostLabel` from the configuration: `analytics.hostLabel`
/// and `analytics.hostVersion` (the Tauri version by default).
pub(crate) fn host_label(config: &crate::config::AnalyticsConfig) -> HostLabel {
    HostLabel::new(
        config.host_label.clone(),
        config
            .host_version
            .clone()
            .unwrap_or_else(|| tauri::VERSION.to_owned()),
    )
}

/// The platform webview's default user agent, the template used until the
/// app webview's real one is known (DESIGN §4.10): the stock `WKWebView`
/// string on macOS; on Windows `WebView2`'s reduced string, which carries
/// only the runtime's major version; the `WebKitGTK` form on Linux.
pub(crate) fn fallback_platform_ua(webview_version: &str) -> String {
    crate::analytics::user_agent::template(
        crate::paths::TargetOs::current(),
        webview_version,
        std::env::consts::ARCH,
    )
}

/// `<UA>` composed from the template (E.1).
pub(crate) fn template_user_agent(app: &AppIdentity, label: &HostLabel) -> String {
    composed_user_agent(
        &fallback_platform_ua(&tauri::webview_version().unwrap_or_default()),
        app,
        label,
    )
}

/// `<UA>` composed from the platform user agent `platform_ua` (E.1).
fn composed_user_agent(platform_ua: &str, app: &AppIdentity, label: &HostLabel) -> String {
    compose_user_agent(
        platform_ua,
        &app.name,
        &app.version,
        label,
        crate::platform::safari_version(),
    )
}

/// The analytics service of one app.
#[derive(Debug)]
pub(crate) struct AnalyticsHost {
    /// The host request lane.
    pub(crate) dispatcher: Dispatcher,
    state: Mutex<AnalyticsState>,
}

#[derive(Debug)]
struct AnalyticsState {
    session: Session,
    reporter: Reporter,
}

impl AnalyticsHost {
    /// A session; `disable_anonymous` is the configuration and builder
    /// switch, which applies before the burst.
    pub(crate) fn new(
        dispatcher: Dispatcher,
        reporter: Reporter,
        user_enabled: bool,
        disable_anonymous: bool,
    ) -> Self {
        let mut session = Session::new(user_enabled);
        if disable_anonymous {
            session.disable_anonymous();
        }
        AnalyticsHost {
            dispatcher,
            state: Mutex::new(AnalyticsState { session, reporter }),
        }
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

    /// Runs `f` on the session and queues the requests of the events it
    /// returns, in order, under the session lock (so two callers never
    /// interleave their requests). Returns how many were queued.
    pub(crate) fn run(&self, f: impl FnOnce(&mut Session) -> Vec<Event>) -> usize {
        let state = &mut *lock(&self.state);
        let events = f(&mut state.session);
        let requests = Self::requests(&state.reporter, events);
        let n = requests.len();
        for r in requests {
            drop(self.dispatcher.send(r, true));
        }
        n
    }

    /// Whether the launch burst was queued.
    #[cfg(test)]
    pub(crate) fn is_started(&self) -> bool {
        lock(&self.state).session.is_started()
    }

    /// The launch burst (E.2 #1, #3, #4, 400022, 400023). Idempotent.
    pub(crate) fn start(&self, now_ms: u64, first_launch: bool) -> usize {
        self.run(|s| {
            if s.is_started() {
                Vec::new()
            } else {
                s.start(now_ms, first_launch)
            }
        })
    }

    /// `disableAnonymousAnalytics()`; returns whether the burst had already
    /// been queued (a late call, R10).
    pub(crate) fn disable_anonymous(&self) -> bool {
        let state = &mut *lock(&self.state);
        let late = state.session.is_started();
        state.session.disable_anonymous();
        late
    }

    /// The user switch (`analytics.userSwitch` only).
    pub(crate) fn set_user_enabled(&self, enabled: bool) {
        lock(&self.state).session.set_user_enabled(enabled);
    }

    /// Replaces `<UA>` in the request builder.
    pub(crate) fn set_user_agent(&self, ua: String) {
        lock(&self.state).reporter.user_agent = ua;
    }

    /// `<UA>` as currently known.
    pub(crate) fn user_agent(&self) -> String {
        lock(&self.state).reporter.user_agent.clone()
    }

    /// The first visible counted window (E.2 #5, once per run).
    pub(crate) fn window_shown(&self, now_ms: u64) -> usize {
        self.run(|s| s.window_shown(now_ms))
    }

    /// A counted window's visible period ended (E.2 #7).
    pub(crate) fn window_closed(&self, name: &str, title: &str, visible_ms: u64) -> usize {
        self.run(|s| s.window_closed(name, title, visible_ms))
    }

    /// The hourly heartbeat check (E.2 #9); `has_visible_window` is whether
    /// a counted window is visible now.
    pub(crate) fn tick(&self, now_ms: u64, has_visible_window: bool) -> usize {
        self.run(|s| s.tick(now_ms, has_visible_window))
    }

    /// How long until the next hourly heartbeat check, from `now_ms` (the
    /// parked ticker's wake-up, DESIGN §4.11); a full check period before
    /// the burst.
    pub(crate) fn until_next_check(&self, now_ms: u64) -> Duration {
        let state = lock(&self.state);
        let next = if state.session.is_started() {
            state.session.next_check_ms()
        } else {
            now_ms + crate::analytics::session::HEARTBEAT_CHECK_MS
        };
        Duration::from_millis(next.saturating_sub(now_ms).max(1))
    }

    /// An ad guest attached (E.2 #6, 400025).
    #[cfg_attr(
        not(ow_tauri_ads),
        allow(dead_code, reason = "only the ads host calls it")
    )]
    pub(crate) fn guest_attached(&self) -> usize {
        self.run(Session::guest_attached)
    }

    /// The Counter of a reported guest crash (E.2 #8), before the reload.
    #[cfg_attr(
        not(ow_tauri_ads),
        allow(dead_code, reason = "only the ads host calls it")
    )]
    pub(crate) fn guest_crash_counter(&self, session_secs: u64, reason: &str) -> usize {
        self.run(|s| s.guest_crash_counter(session_secs, reason))
    }

    /// The 400024 of a reported guest crash (E.2 #8), after the reload.
    #[cfg_attr(
        not(ow_tauri_ads),
        allow(dead_code, reason = "only the ads host calls it")
    )]
    pub(crate) fn guest_crash_stats(&self, session_secs: u64, reason: &str) -> usize {
        self.run(|s| s.guest_crash_stats(session_secs, reason))
    }

    /// The `cmp-eu-only` request (E.2 #2), `None` when the user switch is
    /// off.
    pub(crate) fn cmp_eu_only_request(&self) -> Option<HostRequest> {
        let state = lock(&self.state);
        state
            .session
            .user_enabled()
            .then(|| state.reporter.cmp_eu_only())
    }

    /// `<label>_sub_info` (E.2 #10). Resolves after the response or the
    /// failure, never with an error.
    pub(crate) async fn sub_info(&self, options: &Map<String, Value>) {
        let requests = {
            let state = lock(&self.state);
            Self::requests(&state.reporter, state.session.sub_info(options))
        };
        for r in requests {
            match self.dispatcher.send(r, true).await {
                Ok(HostResponse { status, .. }) if status < 400 => {}
                Ok(HostResponse { status, .. }) => {
                    log::debug!(target: super::LOG_TARGET, "sub_info: HTTP {status}");
                }
                Err(err) => log::debug!(target: super::LOG_TARGET, "sub_info failed: {err}"),
            }
        }
    }
}

/// Where `<UA>` stands (DESIGN §4.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UaState {
    /// The template; discovery has not finished.
    Pending,
    /// Final: discovered, or the template after discovery gave up.
    Final,
}

/// `<UA>` discovery state: host requests wait until it is final, at most
/// [`UA_WAIT`] after Ready.
#[derive(Debug)]
pub(crate) struct UserAgent {
    state: Mutex<UaState>,
    ready: tokio::sync::Notify,
}

impl UserAgent {
    /// Discovery pending.
    pub(crate) fn new() -> Self {
        UserAgent {
            state: Mutex::new(UaState::Pending),
            ready: tokio::sync::Notify::new(),
        }
    }

    /// Whether `<UA>` is final.
    pub(crate) fn is_final(&self) -> bool {
        *lock(&self.state) == UaState::Final
    }

    /// Marks `<UA>` final and wakes the waiting requests.
    pub(crate) fn finish(&self) {
        *lock(&self.state) = UaState::Final;
        self.ready.notify_waiters();
    }

    /// Waits until `<UA>` is final or `deadline` has passed.
    pub(crate) async fn wait(&self, deadline: std::time::Instant) {
        loop {
            let notified = self.ready.notified();
            if self.is_final() {
                return;
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() || tokio::time::timeout(left, notified).await.is_err() {
                return;
            }
        }
    }
}

/// Starts `<UA>` discovery at Ready (DESIGN §4.10): reads the user agent of
/// the first app webview natively, without blocking the main thread, and
/// makes `<UA>` final. A read that fails or does not have the platform
/// default's shape (an app-set user agent) leaves the template; so does an
/// app without any webview at Ready (a tray app). Host requests wait for it
/// at most [`UA_WAIT`].
pub(crate) fn start_user_agent_discovery<R: Runtime>(core: &Arc<Core<R>>) {
    // Tauri's mock runtime never runs `with_webview` closures.
    let native = super::windows::native_runtime::<R>();
    let webview = core
        .windows
        .first_app_webview()
        .filter(|_| native)
        .and_then(|label| crate::compat::webview(&core.app, &label));
    let Some(webview) = webview else {
        core.ua.finish();
        return;
    };
    let weak = Arc::downgrade(core);
    let read = crate::platform::ua::read_native(&webview, move |ua| {
        if let Some(core) = weak.upgrade() {
            accept_native_user_agent(&core, ua.as_deref());
        }
    });
    if read.is_err() {
        core.ua.finish();
        return;
    }
    // The webview may close before it answers.
    let weak = Arc::downgrade(core);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(UA_WAIT).await;
        if let Some(core) = weak.upgrade() {
            core.ua.finish();
        }
    });
}

/// Uses a natively read user agent for `<UA>` when it has the platform
/// default's shape, then makes `<UA>` final.
fn accept_native_user_agent<R: Runtime>(core: &Core<R>, native: Option<&str>) {
    if core.ua.is_final() {
        return;
    }
    match native {
        Some(ua) if accepts_native(crate::paths::TargetOs::current(), ua) => {
            let label = host_label(&core.identity.config.analytics);
            core.analytics
                .set_user_agent(composed_user_agent(ua, &core.identity.app, &label));
        }
        Some(_) => log::debug!(
            target: super::LOG_TARGET,
            "the app webview's user agent is not the platform default; Overwolf requests use the default"
        ),
        None => {}
    }
    core.ua.finish();
}

/// The dispatcher's hooks into the core (E.1): requests wait for the final
/// `<UA>`.
pub(crate) struct CoreRequestHooks<R: Runtime>(pub(crate) Weak<Core<R>>);

impl<R: Runtime> RequestHooks for CoreRequestHooks<R> {
    fn wait_user_agent(&self) -> BoxFuture<()> {
        let core = self.0.upgrade();
        Box::pin(async move {
            if let Some(core) = core {
                let deadline = core
                    .lifecycle
                    .ready_at()
                    .unwrap_or_else(std::time::Instant::now)
                    + UA_WAIT;
                core.ua.wait(deadline).await;
            }
        })
    }

    fn user_agent(&self) -> Option<String> {
        self.0.upgrade().map(|c| c.analytics.user_agent())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_name_the_platform() {
        let ua = fallback_platform_ua("141.0.3537.57");
        if cfg!(windows) {
            assert!(ua.contains("Chrome/141.0.0.0") && ua.ends_with("Edg/141.0.0.0"));
        } else {
            assert!(ua.starts_with("Mozilla/5.0 ("));
        }
    }

    use crate::host::windows::tests::{Capture, mock_app, wait_until, window};
    use serde_json::json;

    /// DESIGN §4.10: an app-set user agent never reaches Overwolf's
    /// requests; one with the platform default's shape does.
    #[test]
    fn only_a_platform_default_native_user_agent_is_used() {
        let (_app, dir, core) = mock_app("ua-native", &json!({}), &[], Capture::answering("{}"));
        let template = core.analytics.user_agent();
        assert!(!core.ua.is_final());
        accept_native_user_agent(&core, Some("CustomApp/9.9 (Macintosh)"));
        assert!(core.ua.is_final());
        assert_eq!(core.analytics.user_agent(), template, "custom UA rejected");

        let (_app2, dir2, core2) =
            mock_app("ua-native2", &json!({}), &[], Capture::answering("{}"));
        let platform = fallback_platform_ua(&tauri::webview_version().unwrap_or_default());
        let native = platform.replace("10.0", "11.0");
        accept_native_user_agent(&core2, Some(&native));
        let ua = core2.analytics.user_agent();
        if cfg!(windows) {
            // The native user agent with the app's token (E.1).
            let label = host_label(&core2.identity.config.analytics);
            assert_eq!(
                ua,
                composed_user_agent(&native, &core2.identity.app, &label),
                "{ua}"
            );
            assert!(ua.contains("Windows NT 11.0"), "{ua}");
        } else {
            // macOS accepts the template only; Linux never reads natively.
            assert_eq!(ua, template);
        }
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    /// The mock runtime has no webview to read: `<UA>` is final at Ready
    /// (the template), so the burst does not wait.
    #[test]
    fn discovery_without_a_readable_webview_is_final_at_once() {
        let (app, dir, core) = mock_app("ua-final", &json!({}), &[], Capture::answering("{}"));
        window(&app, "main");
        crate::host::lifecycle::on_ready(&core);
        assert!(core.ua.is_final());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// R10: `setAnonymousAnalyticsPreference(false)` of an earlier launch
    /// applies at the next Ready, before the burst.
    #[test]
    fn the_persisted_preference_applies_at_the_next_ready() {
        let capture = Capture::answering("{}");
        let (_app, dir, core) = mock_app("analytics-pref", &json!({}), &[], capture.clone());
        crate::ext::Overwolf(core.clone())
            .set_anonymous_analytics_preference(false)
            .unwrap();
        let stored = std::fs::read_to_string(core.identity.state_dir.ow_tauri_json()).unwrap();
        drop(core);

        // The next launch, same state directory.
        let capture2 = Capture::answering("{}");
        let (_app2, dir2, core2) = mock_app(
            "analytics-pref-next",
            &json!({ "state": { "appDataDir": dir.clone() } }),
            &[],
            capture2.clone(),
        );
        assert!(stored.contains("\"anonymousAnalytics\": false"), "{stored}");
        crate::host::lifecycle::on_ready(&core2);
        assert!(wait_until(Duration::from_secs(5), || capture2
            .counters()
            .iter()
            .any(|c| c.ends_with("_app_heartbeat"))));
        std::thread::sleep(Duration::from_millis(100));
        let counters = capture2.counters();
        assert!(
            !counters.iter().any(|c| c.ends_with("_app_start")),
            "{counters:?}"
        );
        assert!(counters.iter().any(|c| c.ends_with("_app_first_launch")));
        assert!(
            capture.requests().is_empty(),
            "no Ready in the first launch"
        );
        drop(core2);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn user_agent_waits_end_at_the_deadline_or_when_final() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let ua = UserAgent::new();
        let start = std::time::Instant::now();
        rt.block_on(ua.wait(start + Duration::from_millis(30)));
        assert!(start.elapsed() >= Duration::from_millis(30));
        assert!(!ua.is_final());
        ua.finish();
        let start = std::time::Instant::now();
        rt.block_on(ua.wait(start + Duration::from_secs(30)));
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
