//! Which analytics events a session sends, and when (CONTRACT E.2, E.3).
//!
//! [`Session`] is a pure state machine driven with a millisecond clock the
//! caller supplies, so the order, the opt-outs and the timers are tested
//! without a network or a real clock.
//!
//! ```
//! use tauri_plugin_overwolf::analytics::session::{Event, Session};
//! let mut s = Session::new(true);
//! let launch = s.start(0, true);
//! let names: Vec<String> = launch.iter().map(Event::describe).collect();
//! assert_eq!(names, ["app_first_launch", "app_start", "app_heartbeat", "400022", "400023"]);
//! assert_eq!(s.window_shown(1_500).len(), 2);
//! assert!(s.window_shown(2_000).is_empty(), "only the first window");
//! ```

use serde_json::{Map, Value};

use super::kind;

/// The hourly check of the periodic heartbeat (E.2 #9).
pub const HEARTBEAT_CHECK_MS: u64 = 60 * 60 * 1000;
/// The heartbeat is sent when this much time has passed since the last one.
pub const HEARTBEAT_INTERVAL_MS: u64 = 12 * 60 * 60 * 1000;
/// Crashes this soon (seconds) after the guest's last load or recovery are
/// recovered but not reported (E.2 #8, interim for R3-4).
pub const CRASH_REPORT_MIN_SESSION_SECS: u64 = 10;

/// One request the session wants sent.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A Counter request for `<label>_<event>`.
    Counter {
        /// The event, without the label (`app_start`).
        event: &'static str,
        /// Event fields, in order.
        fields: Vec<(String, Value)>,
    },
    /// An `InsertStats` request.
    Stats {
        /// The kind.
        kind: u32,
        /// `reason` for Kind 400024.
        reason: Option<String>,
    },
}

impl Event {
    /// A short label for tests and logs: the Counter event or the Kind.
    ///
    /// ```
    /// use tauri_plugin_overwolf::analytics::session::Event;
    /// assert_eq!(Event::Stats { kind: 400023, reason: None }.describe(), "400023");
    /// ```
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Event::Counter { event, .. } => (*event).to_owned(),
            Event::Stats { kind, .. } => kind.to_string(),
        }
    }

    fn counter(event: &'static str, fields: Vec<(&str, Value)>) -> Self {
        Event::Counter {
            event,
            fields: fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        }
    }

    fn stats(kind: u32) -> Self {
        Event::Stats { kind, reason: None }
    }
}

/// An event plus whether it belongs to the mandatory set (E.3).
#[derive(Debug, Clone, PartialEq)]
struct Pending {
    event: Event,
    mandatory: bool,
}

/// The analytics state of one session.
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent session switches (E.2, E.3)"
)]
pub struct Session {
    user_enabled: bool,
    anonymous_disabled: bool,
    started: bool,
    first_window_seen: bool,
    first_window_sent: bool,
    last_heartbeat_ms: u64,
    next_check_ms: u64,
    queued: Vec<Pending>,
}

impl Session {
    /// A session. `user_enabled` is the ow-tauri `analytics_set_user_enabled`
    /// switch (`false` sends nothing at all).
    #[must_use]
    pub fn new(user_enabled: bool) -> Self {
        Session {
            user_enabled,
            anonymous_disabled: false,
            started: false,
            first_window_seen: false,
            first_window_sent: false,
            last_heartbeat_ms: 0,
            next_check_ms: HEARTBEAT_CHECK_MS,
            queued: Vec::new(),
        }
    }

    /// `disableAnonymousAnalytics()`: only the mandatory set from now on.
    pub fn disable_anonymous(&mut self) {
        self.anonymous_disabled = true;
    }

    /// Whether `disableAnonymousAnalytics()` was called.
    #[must_use]
    pub fn anonymous_disabled(&self) -> bool {
        self.anonymous_disabled
    }

    /// Whether the ow-tauri user switch is on.
    #[must_use]
    pub fn user_enabled(&self) -> bool {
        self.user_enabled
    }

    /// The ow-tauri user switch.
    pub fn set_user_enabled(&mut self, enabled: bool) {
        self.user_enabled = enabled;
    }

    /// Whether the launch sequence has run.
    #[must_use]
    pub fn is_started(&self) -> bool {
        self.started
    }

    fn admit(&self, mandatory: bool) -> bool {
        self.user_enabled && (mandatory || !self.anonymous_disabled)
    }

    fn emit(&mut self, out: &mut Vec<Event>, event: Event, mandatory: bool) {
        if self.started {
            if self.admit(mandatory) {
                out.push(event);
            }
        } else {
            self.queued.push(Pending { event, mandatory });
        }
    }

    fn heartbeat(&mut self, out: &mut Vec<Event>, now_ms: u64, visible: bool) {
        self.last_heartbeat_ms = now_ms;
        self.emit(
            out,
            Event::counter(
                "app_heartbeat",
                vec![("hasVisibleWindow", Value::Bool(visible))],
            ),
            true,
        );
        self.emit(out, Event::stats(kind::HEARTBEAT), true);
    }

    /// The launch sequence (E.2 #1, #3, #4, 400022, 400023), at
    /// `main_ready` or the 10 s fallback. `first_launch` is whether
    /// `firstLaunch` was absent from `ow-electron.json`. Events recorded
    /// before the start follow, in order. A second call returns nothing.
    pub fn start(&mut self, now_ms: u64, first_launch: bool) -> Vec<Event> {
        if self.started {
            return Vec::new();
        }
        self.started = true;
        self.next_check_ms = now_ms + HEARTBEAT_CHECK_MS;
        let mut out = Vec::new();
        if first_launch {
            self.emit(&mut out, Event::counter("app_first_launch", vec![]), true);
        }
        self.emit(&mut out, Event::counter("app_start", vec![]), false);
        self.last_heartbeat_ms = now_ms;
        self.emit(
            &mut out,
            Event::counter(
                "app_heartbeat",
                vec![("hasVisibleWindow", Value::Bool(false))],
            ),
            true,
        );
        if first_launch {
            self.emit(&mut out, Event::stats(kind::FIRST_LAUNCH), false);
        }
        self.emit(&mut out, Event::stats(kind::HEARTBEAT), true);
        if self.first_window_seen && !self.first_window_sent {
            self.first_window_sent = true;
            self.heartbeat(&mut out, now_ms, true);
        }
        for p in std::mem::take(&mut self.queued) {
            if self.admit(p.mandatory) {
                out.push(p.event);
            }
        }
        out
    }

    /// An app window became visible: the first one sends #5 (heartbeat with
    /// `hasVisibleWindow: true`, then 400023), once per run.
    pub fn window_shown(&mut self, now_ms: u64) -> Vec<Event> {
        let mut out = Vec::new();
        if self.first_window_seen {
            return out;
        }
        self.first_window_seen = true;
        if self.started {
            self.first_window_sent = true;
            self.heartbeat(&mut out, now_ms, true);
        }
        out
    }

    /// The periodic heartbeat (E.2 #9): an hourly check that sends when
    /// 12 h have passed since the last heartbeat. Call it often; it does
    /// nothing between checks.
    pub fn tick(&mut self, now_ms: u64, has_visible_window: bool) -> Vec<Event> {
        let mut out = Vec::new();
        if !self.started || now_ms < self.next_check_ms {
            return out;
        }
        // The check runs at its scheduled time, as an hourly timer would;
        // the timer step only notices it later.
        let mut due = self.next_check_ms;
        while self.next_check_ms <= now_ms {
            due = self.next_check_ms;
            self.next_check_ms += HEARTBEAT_CHECK_MS;
        }
        if due.saturating_sub(self.last_heartbeat_ms) >= HEARTBEAT_INTERVAL_MS {
            self.heartbeat(&mut out, now_ms, has_visible_window);
            self.last_heartbeat_ms = due;
        }
        out
    }

    /// A visible period of a window ended (E.2 #7). Periods shorter than
    /// 1 s send nothing; `length` is whole seconds, rounded down.
    pub fn window_closed(&mut self, name: &str, title: &str, visible_ms: u64) -> Vec<Event> {
        let mut out = Vec::new();
        let secs = visible_ms / 1000;
        if secs == 0 {
            return out;
        }
        self.emit(
            &mut out,
            Event::counter(
                "window_closed",
                vec![
                    ("name", Value::String(name.to_owned())),
                    ("title", Value::String(title.to_owned())),
                    ("length", Value::from(secs)),
                ],
            ),
            false,
        );
        out
    }

    /// An ad guest attached (E.2 #6).
    pub fn guest_attached(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        self.emit(&mut out, Event::stats(kind::GUEST_ATTACH), false);
        out
    }

    /// Whether a guest crash `session_secs` after the guest's last load or
    /// recovery is reported (E.2 #8).
    #[must_use]
    pub fn crash_reported(session_secs: u64) -> bool {
        session_secs >= CRASH_REPORT_MIN_SESSION_SECS
    }

    /// The Counter of a reported guest crash, sent before the reload.
    pub fn guest_crash_counter(&mut self, session_secs: u64, reason: &str) -> Vec<Event> {
        let mut out = Vec::new();
        if Self::crash_reported(session_secs) {
            self.emit(
                &mut out,
                Event::counter(
                    "owadview_crashed",
                    vec![
                        ("sessionTS", Value::from(session_secs)),
                        ("reason", Value::String(reason.to_owned())),
                    ],
                ),
                false,
            );
        }
        out
    }

    /// The Kind 400024 of a reported guest crash, sent after the reload.
    pub fn guest_crash_stats(&mut self, session_secs: u64, reason: &str) -> Vec<Event> {
        let mut out = Vec::new();
        if Self::crash_reported(session_secs) {
            self.emit(
                &mut out,
                Event::Stats {
                    kind: kind::GUEST_CRASH,
                    reason: Some(reason.to_owned()),
                },
                false,
            );
        }
        out
    }

    /// `<label>_sub_info` (E.2 #10); kept under `disableAnonymousAnalytics()`.
    /// Sent at once even before the launch sequence (the command requires
    /// `main_ready`).
    #[must_use]
    pub fn sub_info(&self, options: &Map<String, Value>) -> Vec<Event> {
        if !self.admit(true) {
            return Vec::new();
        }
        vec![Event::Counter {
            event: "sub_info",
            fields: super::sub_info_fields(options),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(events: &[Event]) -> Vec<String> {
        events.iter().map(Event::describe).collect()
    }

    #[test]
    fn launch_order_first_and_second() {
        let mut first = Session::new(true);
        assert_eq!(
            names(&first.start(0, true)),
            [
                "app_first_launch",
                "app_start",
                "app_heartbeat",
                "400022",
                "400023"
            ]
        );
        assert!(first.start(5, true).is_empty(), "idempotent");
        let mut second = Session::new(true);
        let events = second.start(0, false);
        assert_eq!(names(&events), ["app_start", "app_heartbeat", "400023"]);
        assert_eq!(
            events[1],
            Event::Counter {
                event: "app_heartbeat",
                fields: vec![("hasVisibleWindow".to_owned(), Value::Bool(false))]
            }
        );
    }

    #[test]
    fn disabled_anonymous_keeps_the_mandatory_set() {
        let mut s = Session::new(true);
        s.disable_anonymous();
        assert_eq!(
            names(&s.start(0, true)),
            ["app_first_launch", "app_heartbeat", "400023"]
        );
        assert_eq!(names(&s.window_shown(10)), ["app_heartbeat", "400023"]);
        assert!(s.guest_attached().is_empty());
        assert!(s.window_closed("index", "App", 5_000).is_empty());
        assert!(s.guest_crash_counter(20, "killed").is_empty());
        assert!(s.guest_crash_stats(20, "killed").is_empty());
        let opts = serde_json::json!({"userId": "u"});
        assert_eq!(names(&s.sub_info(opts.as_object().unwrap())), ["sub_info"]);
        assert_eq!(
            names(&s.tick(HEARTBEAT_INTERVAL_MS + HEARTBEAT_CHECK_MS, false)),
            ["app_heartbeat", "400023"]
        );
    }

    #[test]
    fn user_switch_off_sends_nothing() {
        let mut s = Session::new(false);
        assert!(s.start(0, true).is_empty());
        assert!(s.window_shown(1).is_empty());
        let opts = serde_json::json!({"userId": "u"});
        assert!(s.sub_info(opts.as_object().unwrap()).is_empty());
        s.set_user_enabled(true);
        assert_eq!(names(&s.guest_attached()), ["400025"]);
    }

    #[test]
    fn events_before_start_follow_the_launch_sequence() {
        let mut s = Session::new(true);
        assert!(s.window_shown(100).is_empty());
        assert!(s.guest_attached().is_empty());
        assert_eq!(
            names(&s.start(200, false)),
            [
                "app_start",
                "app_heartbeat",
                "400023",
                "app_heartbeat",
                "400023",
                "400025"
            ]
        );
        // A disable before the start applies to the queued events too.
        let mut s = Session::new(true);
        s.guest_attached();
        s.disable_anonymous();
        assert_eq!(names(&s.start(0, false)), ["app_heartbeat", "400023"]);
    }

    #[test]
    fn window_closed_fields_and_threshold() {
        let mut s = Session::new(true);
        s.start(0, false);
        assert!(s.window_closed("a", "T", 999).is_empty());
        let e = s.window_closed("index", "", 1_500);
        assert_eq!(
            e,
            [Event::Counter {
                event: "window_closed",
                fields: vec![
                    ("name".into(), Value::from("index")),
                    ("title".into(), Value::from("")),
                    ("length".into(), Value::from(1)),
                ]
            }]
        );
    }

    #[test]
    fn crash_threshold() {
        let mut s = Session::new(true);
        s.start(0, false);
        assert!(s.guest_crash_counter(9, "killed").is_empty());
        assert!(s.guest_crash_stats(2, "killed").is_empty());
        assert_eq!(
            s.guest_crash_counter(10, "crashed"),
            [Event::Counter {
                event: "owadview_crashed",
                fields: vec![
                    ("sessionTS".into(), Value::from(10)),
                    ("reason".into(), Value::from("crashed")),
                ]
            }]
        );
        assert_eq!(
            s.guest_crash_stats(30, "killed"),
            [Event::Stats {
                kind: kind::GUEST_CRASH,
                reason: Some("killed".into())
            }]
        );
    }

    #[test]
    fn periodic_heartbeat_with_fake_clock() {
        let mut s = Session::new(true);
        s.start(0, false);
        // The first window heartbeat 2 s in moves the 12 h mark.
        s.window_shown(2_000);
        let hour = HEARTBEAT_CHECK_MS;
        let mut sent = Vec::new();
        let mut now = 0;
        while now <= 26 * hour {
            if !s.tick(now, true).is_empty() {
                sent.push(now / hour);
            }
            now += 250 * 1000;
        }
        // Checks at whole hours; 12 h after 2 s is first met at hour 13,
        // then 12 h after that at hour 25.
        assert_eq!(sent, [13, 25]);
    }
}
