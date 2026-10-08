//! App windows tracked from native Tauri events (DESIGN §4.3) and the
//! visibility ticker (§4.11).
//!
//! Every window the app creates is tracked by its Tauri label, except the
//! plugin's own consent windows (`ow-cmp*`). Tauri reports no show, hide or
//! minimize events (and `show()` of an unfocused window reports nothing at
//! all), so one ticker polls `is_visible` and `is_minimized` of every
//! tracked window every [`TICK`], in **one** main-thread hop per tick. A
//! poll drives:
//!
//! - E.2 #5, the first heartbeat with `hasVisibleWindow: true`: the first
//!   poll that sees a counted window visible and not minimized;
//! - E.2 #7, `<label>_window_closed`: a counted window's visible period ends
//!   when a poll sees it hidden or minimized, when it is destroyed, or at
//!   exit; its `name` is fixed the first time the window is seen visible
//!   with a loaded page, its `title` is the configured or native title;
//! - E.2 #9, the hourly heartbeat check;
//! - the ad guests, which follow their window ([`AppWindows::add_poll_listener`]).
//!
//! The ticker never parks while a tracked window exists (a hidden window can
//! be shown without any event); with no tracked window (a tray-only app) it
//! parks and wakes only for a window registration, a hold
//! ([`Ticker::hold`]) or the hourly heartbeat check.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::webview::PageLoadEvent;
use tauri::{Runtime, Webview, Window, WindowEvent};
use url::Url;

use super::{Core, LOG_TARGET, lock};

/// The longest window name `set_window_name` accepts (it becomes the
/// `x-ow-window` header, SEC-m1).
pub(crate) const MAX_WINDOW_NAME: usize = 128;

/// The ticker's period while any window is tracked (DESIGN §4.11).
pub(crate) const TICK: Duration = Duration::from_millis(250);

/// The title Tauri gives a window that sets none (`tauri-utils`
/// `WindowConfig` default). A window with this title reports `<PN>`
/// instead (DESIGN §4.3.4), as an Electron window shows the app name.
pub(crate) const PLACEHOLDER_TITLE: &str = "Tauri App";

/// What one poll saw of one window, for the ad guests that follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WindowObservation {
    /// The window's label.
    pub(crate) label: String,
    /// Shown and not minimized (nor being minimized).
    pub(crate) visible: bool,
    /// Minimized, or in its macOS minimize animation.
    pub(crate) minimized: bool,
    /// `visible` or `minimized` differs from the window's previous poll (or
    /// this is its first poll).
    pub(crate) changed: bool,
}

/// A function the ticker calls on the main thread after every poll with
/// what it saw and the session clock (milliseconds since setup).
pub(crate) type PollListener = Arc<dyn Fn(&[WindowObservation], u64) + Send + Sync>;

/// What a visibility observation changed (E.2 #5, #7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VisibilityChange {
    /// Nothing.
    None,
    /// A visible period started.
    Shown,
    /// A visible period ended.
    Ended(VisiblePeriod),
}

/// One finished visible period of a window (E.2 #7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VisiblePeriod {
    /// The analytics name.
    pub(crate) name: String,
    /// The window title.
    pub(crate) title: String,
    /// Milliseconds visible.
    pub(crate) visible_ms: u64,
}

/// One tracked window (DESIGN §4.3).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent window state bits"
)]
pub(crate) struct WindowEntry {
    /// Counts for analytics (not matched by `analytics.excludeWindows`, D6).
    pub(crate) counted: bool,
    /// The configured title, else the native title, `<PN>` for Tauri's
    /// placeholder (D5); `None` until read.
    pub(crate) title: Option<String>,
    /// The window's app webviews in creation order (D7).
    pub(crate) webviews: Vec<String>,
    /// The URL of the naming webview's last finished page load, already
    /// reduced to its analytics name.
    pub(crate) url_name: Option<String>,
    /// A page load has finished in the naming webview.
    pub(crate) loaded: bool,
    /// The analytics name, fixed the first time the window is visible with
    /// a loaded page (E.2 #7).
    pub(crate) name: Option<String>,
    /// Start of the current visible period, in session milliseconds.
    pub(crate) visible_since: Option<u64>,
    /// A minimize ended the last visible period. Restoring the window does
    /// not start a new one; showing it after a hide does (ow-electron,
    /// observed).
    pub(crate) minimize_ended: bool,
    /// macOS: the window is in its minimize animation, where the OS reports
    /// it neither visible nor minimized; that is part of the minimize, not
    /// a hide.
    pub(crate) minimizing: bool,
    /// The last poll's `(visible, minimized)`.
    pub(crate) last: Option<(bool, bool)>,
    /// The window has the keyboard focus.
    pub(crate) focused: bool,
    /// macOS: the `NSWindow*`, to match minimize notifications.
    pub(crate) native: Option<usize>,
}

impl WindowEntry {
    /// A newly registered window.
    fn new(counted: bool) -> Self {
        WindowEntry {
            counted,
            ..WindowEntry::default()
        }
    }

    /// The naming webview (D7): the webview labelled like the window, else
    /// the first one created in it.
    pub(crate) fn naming_webview<'a>(&'a self, window_label: &'a str) -> Option<&'a str> {
        self.webviews
            .iter()
            .find(|w| *w == window_label)
            .or_else(|| self.webviews.first())
            .map(String::as_str)
    }

    fn fix_name(&mut self) {
        if self.name.is_none() && self.loaded {
            self.name.clone_from(&self.url_name);
        }
    }

    /// Records one poll: the OS says `visible` and `minimized`. A minimized
    /// window is not visible, its minimize ends the visible period, and its
    /// restore starts none (only a show after a hide does), as in
    /// ow-electron (observed).
    pub(crate) fn observe(
        &mut self,
        visible: bool,
        minimized: bool,
        now_ms: u64,
    ) -> VisibilityChange {
        if minimized || visible {
            // The minimize finished, or was cancelled.
            self.minimizing = false;
        }
        let minimized = minimized || self.minimizing;
        let visible = visible && !minimized;
        match (visible, self.visible_since) {
            (true, None) if self.minimize_ended => VisibilityChange::None,
            (true, None) => {
                self.visible_since = Some(now_ms);
                self.fix_name();
                VisibilityChange::Shown
            }
            (false, Some(_)) => {
                self.minimize_ended = minimized;
                self.end_period(now_ms)
                    .map_or(VisibilityChange::None, VisibilityChange::Ended)
            }
            (false, None) if !minimized => {
                self.minimize_ended = false;
                VisibilityChange::None
            }
            _ => VisibilityChange::None,
        }
    }

    /// A page load finished in the naming webview; `url_name` is its
    /// analytics name. Fixes the name when the window is visible.
    pub(crate) fn page_finished(&mut self, url_name: String) {
        self.loaded = true;
        self.url_name = Some(url_name);
        if self.visible_since.is_some() {
            self.fix_name();
        }
    }

    /// Ends the current visible period (hide, minimize, destroy or exit), if
    /// any. Without a loaded page the name is `blank`, as ow-electron names a
    /// window that never loaded (observed).
    pub(crate) fn end_period(&mut self, now_ms: u64) -> Option<VisiblePeriod> {
        let since = self.visible_since.take()?;
        let name = self
            .name
            .clone()
            .or_else(|| self.url_name.clone())
            .unwrap_or_else(|| "blank".to_owned());
        Some(VisiblePeriod {
            name,
            title: self.title.clone().unwrap_or_default(),
            visible_ms: now_ms.saturating_sub(since),
        })
    }
}

/// The tracked windows and the state they share.
#[derive(Debug, Default)]
struct Windows {
    entries: BTreeMap<String, WindowEntry>,
    /// The first app webview ever registered (`<UA>` discovery, §4.10).
    first_app_webview: Option<String>,
    /// Reserved labels already reported as misused.
    reported: BTreeSet<String>,
}

/// Every app window, keyed by its Tauri label.
pub(crate) struct AppWindows {
    /// `analytics.excludeWindows` plus `Builder::exclude_windows`.
    exclude: Vec<String>,
    /// `set_window_name` overrides by window label.
    names: Mutex<BTreeMap<String, String>>,
    state: Mutex<Windows>,
    listeners: Mutex<Vec<PollListener>>,
}

impl std::fmt::Debug for AppWindows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppWindows")
            .field("exclude", &self.exclude)
            .field(
                "windows",
                &lock(&self.state).entries.keys().collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl Default for AppWindows {
    fn default() -> Self {
        AppWindows::new(&[])
    }
}

/// What applying one poll produced, sent after the lock is released.
#[derive(Debug, Default)]
struct PollOutcome {
    shown: bool,
    ended: Vec<VisiblePeriod>,
    observations: Vec<WindowObservation>,
    any_visible: bool,
}

impl AppWindows {
    /// No windows yet; `exclude` are the label globs never counted.
    pub(crate) fn new(exclude: &[String]) -> Self {
        AppWindows {
            exclude: exclude.to_vec(),
            names: Mutex::new(BTreeMap::new()),
            state: Mutex::new(Windows::default()),
            listeners: Mutex::new(Vec::new()),
        }
    }

    /// Whether the window `label` counts for analytics (D6): not
    /// plugin-owned and not matched by an exclusion glob.
    pub(crate) fn is_counted(&self, label: &str) -> bool {
        !crate::config::is_reserved_label(label)
            && !self.exclude.iter().any(|g| glob_matches(g, label))
    }

    /// `set_window_name` (D4): the analytics name of window `label`.
    pub(crate) fn set_name(&self, label: &str, name: &str) {
        lock(&self.names).insert(label.to_owned(), name.to_owned());
    }

    /// The `set_window_name` override of window `label`.
    pub(crate) fn name_override(&self, label: &str) -> Option<String> {
        lock(&self.names).get(label).cloned()
    }

    /// Whether no app window is tracked (the last one was destroyed, or
    /// none was created).
    pub(crate) fn is_empty(&self) -> bool {
        lock(&self.state).entries.is_empty()
    }

    /// Whether window `label` is tracked.
    #[cfg(test)]
    pub(crate) fn contains(&self, label: &str) -> bool {
        lock(&self.state).entries.contains_key(label)
    }

    /// The first app webview ever registered, if any (never a plugin
    /// webview): the webview `<UA>` discovery reads (§4.10).
    pub(crate) fn first_app_webview(&self) -> Option<String> {
        lock(&self.state).first_app_webview.clone()
    }

    /// The title window `label` reports in `window_closed` and as the guest
    /// `windowTitle` fallback (D5).
    #[allow(
        dead_code,
        reason = "the ads host (W2-A) uses it as the `windowTitle` fallback"
    )]
    pub(crate) fn title(&self, label: &str) -> Option<String> {
        lock(&self.state)
            .entries
            .get(label)
            .and_then(|e| e.title.clone())
    }

    /// Whether the last poll saw window `label` visible and not minimized.
    #[allow(dead_code, reason = "the ads host (W2-A) reads it at mount")]
    pub(crate) fn is_visible(&self, label: &str) -> Option<bool> {
        lock(&self.state)
            .entries
            .get(label)
            .and_then(|e| e.last)
            .map(|(visible, _)| visible)
    }

    /// The analytics name of window `label` (E.2 #7; `x-ow-window`,
    /// `__overwolf__.windowName`): the `set_window_name` override, else the
    /// name fixed when the window was first seen visible with a loaded page,
    /// else the same derivation on `current_url` (the embedder's document
    /// before that moment), else the naming webview's last loaded page,
    /// else `blank`.
    #[allow(dead_code, reason = "the ads host (W2-A) sends it as `x-ow-window`")]
    pub(crate) fn window_name<R: Runtime>(
        &self,
        core: &Core<R>,
        label: &str,
        current_url: Option<&Url>,
    ) -> String {
        if let Some(name) = self.name_override(label) {
            return name;
        }
        let (fixed, last) = lock(&self.state)
            .entries
            .get(label)
            .map(|e| (e.name.clone(), e.url_name.clone()))
            .unwrap_or_default();
        fixed
            .or_else(|| current_url.map(|u| url_name(core, u)))
            .or(last)
            .unwrap_or_else(|| "blank".to_owned())
    }

    /// Adds a function called on the main thread after every poll (the ad
    /// guests follow their window with it).
    #[allow(dead_code, reason = "the ads host (W2-A) follows the windows")]
    pub(crate) fn add_poll_listener(&self, listener: PollListener) {
        lock(&self.listeners).push(listener);
    }

    /// `on_window_ready`: registers the window and polls it at once (a
    /// window created off the main thread may already be shown, V8).
    pub(crate) fn window_ready<R: Runtime>(&self, core: &Arc<Core<R>>, window: &Window<R>) {
        let label = window.label();
        if crate::config::is_reserved_label(label) {
            if !core.consent.owns_window(label) {
                self.report_reserved(label);
            }
            return;
        }
        let native = native_address(window);
        self.ensure(label).native = native;
        core.ticker.wake();
        request_poll_soon(core);
    }

    /// `on_webview_ready`: adds an app webview to its window (D7).
    pub(crate) fn webview_ready<R: Runtime>(&self, core: &Arc<Core<R>>, webview: &Webview<R>) {
        let label = webview.label();
        if crate::config::is_reserved_label(label) {
            return;
        }
        let window = webview.window().label().to_owned();
        if crate::config::is_reserved_label(&window) {
            return;
        }
        {
            let mut state = lock(&self.state);
            if state.first_app_webview.is_none() {
                state.first_app_webview = Some(label.to_owned());
            }
        }
        let mut entries = self.ensure(&window);
        if !entries.webviews.iter().any(|w| w == label) {
            entries.webviews.push(label.to_owned());
        }
        drop(entries);
        core.ticker.wake();
    }

    /// A top-level page load of an app webview: a finished load of a
    /// window's naming webview gives the window its analytics name.
    pub(crate) fn page_load<R: Runtime>(
        &self,
        core: &Arc<Core<R>>,
        webview: &Webview<R>,
        event: PageLoadEvent,
        url: &Url,
    ) {
        if event != PageLoadEvent::Finished {
            return;
        }
        let window = webview.window().label().to_owned();
        let name = url_name(core, url);
        let mut state = lock(&self.state);
        if let Some(entry) = state.entries.get_mut(&window)
            && entry.naming_webview(&window) == Some(webview.label())
        {
            entry.page_finished(name);
        }
    }

    /// A window event of window `label`: focus and resize poll it at once;
    /// destruction ends its visible period (`window_closed`) and forgets it.
    pub(crate) fn window_event<R: Runtime>(
        &self,
        core: &Arc<Core<R>>,
        label: &str,
        event: &WindowEvent,
    ) {
        match event {
            WindowEvent::Destroyed => {
                lock(&self.names).remove(label);
                let ended = lock(&self.state)
                    .entries
                    .remove(label)
                    .and_then(|mut e| e.end_period(core.now()).filter(|_| e.counted));
                if let Some(p) = ended {
                    core.analytics
                        .window_closed(&p.name, &p.title, p.visible_ms);
                }
            }
            WindowEvent::Focused(focused) => {
                if let Some(e) = lock(&self.state).entries.get_mut(label) {
                    e.focused = *focused;
                }
                request_poll_soon(core);
            }
            WindowEvent::Resized(_) => request_poll_soon(core),
            _ => {}
        }
    }

    /// The entry of window `label`, created when missing.
    fn ensure(&self, label: &str) -> EntryGuard<'_> {
        let mut state = lock(&self.state);
        let counted = self.is_counted(label);
        state
            .entries
            .entry(label.to_owned())
            .or_insert_with(|| WindowEntry::new(counted));
        EntryGuard {
            state,
            label: label.to_owned(),
        }
    }

    fn report_reserved(&self, label: &str) {
        if lock(&self.state).reported.insert(label.to_owned()) {
            log::error!(
                target: LOG_TARGET,
                "label \"{label}\" is reserved for tauri-plugin-overwolf; the window is not tracked and cannot host ads"
            );
        }
    }

    /// The labels of every tracked window.
    fn labels(&self) -> Vec<String> {
        lock(&self.state).entries.keys().cloned().collect()
    }

    /// Applies one poll: `(label, visible, minimized)` as the OS reports
    /// them, `titles` the native titles read for windows that have none
    /// yet. Pure state changes under the lock.
    fn apply(
        &self,
        polled: &[(String, bool, bool)],
        titles: &BTreeMap<String, String>,
        product_name: &str,
        configured: &dyn Fn(&str) -> Option<String>,
        now_ms: u64,
    ) -> PollOutcome {
        let mut out = PollOutcome::default();
        let mut state = lock(&self.state);
        for (label, visible, minimized) in polled {
            let Some(entry) = state.entries.get_mut(label) else {
                continue;
            };
            if entry.title.is_none() {
                entry.title = Some(window_title(
                    configured(label).as_deref(),
                    titles.get(label).map(String::as_str),
                    product_name,
                ));
            }
            let change = entry.observe(*visible, *minimized, now_ms);
            if entry.counted {
                match change {
                    VisibilityChange::Shown => out.shown = true,
                    VisibilityChange::Ended(p) => out.ended.push(p),
                    VisibilityChange::None => {}
                }
            }
            let minimized = *minimized || entry.minimizing;
            let now = (*visible && !minimized, minimized);
            let changed = entry.last.replace(now) != Some(now);
            out.observations.push(WindowObservation {
                label: label.clone(),
                visible: now.0,
                minimized: now.1,
                changed,
            });
        }
        out.any_visible = state
            .entries
            .values()
            .any(|e| e.counted && e.visible_since.is_some());
        out
    }

    /// Polls every tracked window now and applies the result (analytics
    /// and the poll listeners). Reads the OS state through Tauri's getters,
    /// so it belongs on the main thread; the ticker runs it there.
    pub(crate) fn poll_now<R: Runtime>(&self, core: &Arc<Core<R>>) {
        let labels = self.labels();
        let untitled: BTreeSet<String> = {
            let state = lock(&self.state);
            labels
                .iter()
                .filter(|l| state.entries.get(*l).is_some_and(|e| e.title.is_none()))
                .cloned()
                .collect()
        };
        let mut polled = Vec::with_capacity(labels.len());
        let mut titles = BTreeMap::new();
        for label in labels {
            let Some(window) = crate::compat::window(&core.app, &label) else {
                continue;
            };
            let visible = window.is_visible().unwrap_or(false);
            let minimized = window.is_minimized().unwrap_or(false);
            if untitled.contains(&label)
                && let Ok(title) = window.title()
            {
                titles.insert(label.clone(), title);
            }
            self.drop_dead_webviews(core, &label);
            polled.push((label, visible, minimized));
        }
        let config = core.app.config();
        let configured = |label: &str| {
            config
                .app
                .windows
                .iter()
                .find(|w| w.label == label)
                .map(|w| w.title.clone())
        };
        let now = core.now();
        let out = self.apply(&polled, &titles, &core.identity.app.name, &configured, now);
        core.ticker.polls.fetch_add(1, Ordering::Relaxed);
        if out.shown {
            core.analytics.window_shown(now);
        }
        for p in &out.ended {
            core.analytics
                .window_closed(&p.name, &p.title, p.visible_ms);
        }
        core.analytics.tick(now, out.any_visible);
        let listeners = lock(&self.listeners).clone();
        for listener in listeners {
            listener(&out.observations, now);
        }
    }

    /// Forgets webviews of window `label` that no longer exist (Tauri
    /// reports no webview destruction, V7), so the naming webview is
    /// re-selected (PAR-minor-3).
    fn drop_dead_webviews<R: Runtime>(&self, core: &Core<R>, label: &str) {
        let webviews = lock(&self.state)
            .entries
            .get(label)
            .map(|e| e.webviews.clone())
            .unwrap_or_default();
        if webviews.len() < 2 {
            return;
        }
        let dead: Vec<String> = webviews
            .into_iter()
            .filter(|w| crate::compat::webview(&core.app, w).is_none())
            .collect();
        if dead.is_empty() {
            return;
        }
        if let Some(e) = lock(&self.state).entries.get_mut(label) {
            e.webviews.retain(|w| !dead.contains(w));
        }
    }

    /// macOS: a stage of the minimize of the window whose `NSWindow*` is
    /// `native`. From the start of the animation the window counts as
    /// minimized; at its end the window is polled at once.
    #[cfg_attr(
        not(target_os = "macos"),
        expect(dead_code, reason = "macOS reports minimize stages")
    )]
    fn minimize_stage<R: Runtime>(
        &self,
        core: &Arc<Core<R>>,
        native: usize,
        stage: crate::platform::webview::MinimizeStage,
    ) {
        use crate::platform::webview::MinimizeStage;
        let found = {
            let mut windows = lock(&self.state);
            windows
                .entries
                .values_mut()
                .find(|e| e.native == Some(native))
                .map(|e| e.minimizing = stage == MinimizeStage::Will)
                .is_some()
        };
        if found && stage == MinimizeStage::Did {
            request_poll_soon(core);
        }
    }

    /// Ends every open visible period (`window_closed`, E.2 #7) at exit.
    fn end_periods<R: Runtime>(&self, core: &Core<R>) {
        let now = core.now();
        let ended: Vec<VisiblePeriod> = lock(&self.state)
            .entries
            .values_mut()
            .filter(|e| e.counted)
            .filter_map(|e| e.end_period(now))
            .collect();
        for p in ended {
            core.analytics
                .window_closed(&p.name, &p.title, p.visible_ms);
        }
    }
}

/// A locked entry of [`AppWindows`].
struct EntryGuard<'a> {
    state: std::sync::MutexGuard<'a, Windows>,
    label: String,
}

impl std::ops::Deref for EntryGuard<'_> {
    type Target = WindowEntry;
    fn deref(&self) -> &WindowEntry {
        self.state
            .entries
            .get(&self.label)
            .unwrap_or_else(|| unreachable!("the guard holds an inserted entry"))
    }
}

impl std::ops::DerefMut for EntryGuard<'_> {
    fn deref_mut(&mut self) -> &mut WindowEntry {
        self.state
            .entries
            .get_mut(&self.label)
            .unwrap_or_else(|| unreachable!("the guard holds an inserted entry"))
    }
}

/// The analytics name of a page at `url` (DESIGN §4.3.3): the app origin's
/// `index.html` rule for local app pages.
pub(crate) fn url_name<R: Runtime>(core: &Core<R>, url: &Url) -> String {
    let id = &core.identity;
    let app_page = crate::commands::is_app_page(
        url,
        id.dev_origin.as_deref(),
        &id.config.ads.allowed_embedder_origins,
    );
    crate::analytics::app_window_name(url.as_str(), app_page)
}

/// The title of a window (DESIGN §4.3.4, D5, PAR-minor-2): the title its
/// `tauri.conf.json` entry declares (it wins over a `set_title` in setup),
/// else its native title, where Tauri's placeholder or an empty title
/// reports `<PN>`.
pub(crate) fn window_title(
    configured: Option<&str>,
    native: Option<&str>,
    product_name: &str,
) -> String {
    let title = configured.or(native).unwrap_or_default();
    if title.is_empty() || title == PLACEHOLDER_TITLE {
        product_name.to_owned()
    } else {
        title.to_owned()
    }
}

/// Whether `R` is Tauri's native runtime: Tauri's mock runtime has no
/// native windows or webviews (its handles point at nothing, and it never
/// runs `with_webview` closures).
pub(crate) fn native_runtime<R: Runtime>() -> bool {
    std::any::TypeId::of::<R>() == std::any::TypeId::of::<tauri::Wry>()
}

/// macOS: the window's `NSWindow*`.
fn native_address<R: Runtime>(window: &Window<R>) -> Option<usize> {
    #[cfg(target_os = "macos")]
    {
        if !native_runtime::<R>() {
            return None;
        }
        window.ns_window().ok().map(|p| p as usize)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        None
    }
}

/// `*` (any run) and `?` (one character) label globs (D6).
pub(crate) fn glob_matches(glob: &str, label: &str) -> bool {
    fn go(g: &[char], l: &[char]) -> bool {
        match (g.first(), l.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&g[1..], l) || (!l.is_empty() && go(g, &l[1..])),
            (Some('?'), Some(_)) => go(&g[1..], &l[1..]),
            (Some(a), Some(b)) if a == b => go(&g[1..], &l[1..]),
            _ => false,
        }
    }
    let g: Vec<char> = glob.chars().collect();
    let l: Vec<char> = label.chars().collect();
    go(&g, &l)
}

/// The visibility ticker (DESIGN §4.11).
#[derive(Debug, Default)]
pub(crate) struct Ticker {
    /// The ticker task was started.
    started: AtomicBool,
    /// Wakes a parked ticker (a permit is kept when nobody waits).
    wake: Arc<tokio::sync::Notify>,
    /// A main-thread poll is queued and has not run yet.
    hop_pending: Arc<AtomicBool>,
    /// Holds that keep the ticker running without windows.
    holds: AtomicUsize,
    /// Polls applied (tests and lab traces read it).
    polls: AtomicU64,
    /// Main-thread hops queued (tests read it).
    hops: AtomicU64,
}

impl Ticker {
    /// Wakes a parked ticker.
    pub(crate) fn wake(&self) {
        self.wake.notify_one();
    }

    /// Keeps the ticker polling every [`TICK`] while the returned hold
    /// lives, even without a tracked window (a pending deadline, DESIGN
    /// §4.11).
    #[allow(dead_code, reason = "the ads host (W2-A) holds it for its deadlines")]
    pub(crate) fn hold(self: &Arc<Self>) -> TickerHold {
        self.holds.fetch_add(1, Ordering::SeqCst);
        self.wake();
        TickerHold(Arc::clone(self))
    }

    /// How many polls were applied.
    #[allow(dead_code, reason = "read by tests and lab traces")]
    pub(crate) fn polls(&self) -> u64 {
        self.polls.load(Ordering::Relaxed)
    }

    /// How many main-thread hops were queued.
    #[allow(dead_code, reason = "read by tests")]
    pub(crate) fn hops(&self) -> u64 {
        self.hops.load(Ordering::Relaxed)
    }
}

/// Keeps the ticker awake while it lives ([`Ticker::hold`]).
#[derive(Debug)]
#[allow(dead_code, reason = "the ads host (W2-A) holds it for its deadlines")]
pub(crate) struct TickerHold(Arc<Ticker>);

impl Drop for TickerHold {
    fn drop(&mut self) {
        self.0.holds.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Queues one poll of every tracked window on the main thread, unless one
/// is already queued. Never blocks. Called from the main thread, the poll
/// runs inline: hooks use [`request_poll_soon`].
pub(crate) fn request_poll<R: Runtime>(core: &Arc<Core<R>>) {
    if core.ticker.hop_pending.swap(true, Ordering::SeqCst) {
        return;
    }
    let weak = Arc::downgrade(core);
    let pending = Arc::clone(&core.ticker.hop_pending);
    let queued = core.app.run_on_main_thread(move || {
        pending.store(false, Ordering::SeqCst);
        if let Some(core) = weak.upgrade()
            && !core.lifecycle.has_exited()
        {
            core.windows.poll_now(&core);
        }
    });
    if queued.is_err() {
        core.ticker.hop_pending.store(false, Ordering::SeqCst);
    } else {
        core.ticker.hops.fetch_add(1, Ordering::Relaxed);
    }
}

/// [`request_poll`] from a runtime task, for hooks and event handlers: they
/// run on the main thread with Tauri's plugin store locked, where a hop
/// would run inline (and a poll listener that creates a webview would wait
/// for that lock).
pub(crate) fn request_poll_soon<R: Runtime>(core: &Arc<Core<R>>) {
    let weak = Arc::downgrade(core);
    tauri::async_runtime::spawn(async move {
        if let Some(core) = weak.upgrade() {
            request_poll(&core);
        }
    });
}

/// Starts the ticker at Ready (once).
pub(crate) fn start_ticker<R: Runtime>(core: &Arc<Core<R>>) {
    if core.ticker.started.swap(true, Ordering::SeqCst) {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let weak = Arc::downgrade(core);
        crate::platform::webview::observe_minimize(move |native, stage| {
            if let Some(core) = weak.upgrade() {
                core.windows.minimize_stage(&core, native, stage);
            }
        });
    }
    let weak = Arc::downgrade(core);
    let wake = Arc::clone(&core.ticker.wake);
    tauri::async_runtime::spawn(async move {
        loop {
            let Some(core) = weak.upgrade() else { return };
            if core.lifecycle.has_exited() {
                return;
            }
            let parked = core.windows.is_empty() && core.ticker.holds.load(Ordering::SeqCst) == 0;
            if parked {
                // Tray-only: no poll. Wake for a window, a hold, or the
                // next hourly heartbeat check (E.2 #9).
                let wait = core.analytics.until_next_check(core.now());
                drop(core);
                let woken = tokio::time::timeout(wait, wake.notified()).await.is_ok();
                if !woken && let Some(core) = weak.upgrade() {
                    if core.windows.is_empty() {
                        core.analytics.tick(core.now(), false);
                    } else {
                        request_poll(&core);
                    }
                }
                continue;
            }
            request_poll(&core);
            drop(core);
            tokio::time::sleep(TICK).await;
        }
    });
}

/// Ends every open visible period at exit (`window_closed`, E.2 #7) and
/// lets the ticker stop.
pub(crate) fn end_all_periods<R: Runtime>(core: &Core<R>) {
    core.windows.end_periods(core);
    core.ticker.wake();
}

/// Whether `name` is a valid `set_window_name` value: 1 to
/// [`MAX_WINDOW_NAME`] printable ASCII characters (no CR, LF or other
/// control characters).
pub(crate) fn valid_window_name(name: &str) -> bool {
    (1..=MAX_WINDOW_NAME).contains(&name.len()) && name.bytes().all(|b| (0x20..0x7f).contains(&b))
}

#[cfg(test)]
pub(crate) mod tests;
