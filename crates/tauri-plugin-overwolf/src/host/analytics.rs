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
/// only the runtime's major version.
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

/// `<UA>` composed from the template (E.1).
pub(crate) fn template_user_agent(app: &AppIdentity, label: &HostLabel) -> String {
    compose_user_agent(
        &fallback_platform_ua(&tauri::webview_version().unwrap_or_default()),
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
    #[allow(dead_code, reason = "the window tracker (W2) checks it")]
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
    #[allow(dead_code, reason = "UA discovery (W2, DESIGN §4.10) sets it")]
    pub(crate) fn set_user_agent(&self, ua: String) {
        lock(&self.state).reporter.user_agent = ua;
    }

    /// `<UA>` as currently known.
    pub(crate) fn user_agent(&self) -> String {
        lock(&self.state).reporter.user_agent.clone()
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

/// Starts `<UA>` discovery at Ready (DESIGN §4.10). Discovery reads an app
/// webview's user agent asynchronously (W2); until then the template is
/// final at once.
pub(crate) fn start_user_agent_discovery<R: Runtime>(core: &Arc<Core<R>>) {
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
