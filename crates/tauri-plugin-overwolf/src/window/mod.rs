//! Window classes, labels and the window registry (CONTRACT 0, A.2.3, A.3;
//! ARCHITECTURE 3.1).
//!
//! Every webview belongs to one class, decided by its label. App code never
//! sees labels; it sees Electron-style integer ids.
//!
//! ```
//! use tauri_plugin_overwolf::window::{classify, WebviewClass};
//! assert_eq!(classify("ow-main"), WebviewClass::Main);
//! assert_eq!(classify("bw-3"), WebviewClass::Ui(3));
//! assert_eq!(classify("bwr-3"), WebviewClass::Remote(3));
//! assert_eq!(classify("owad-bw-3-1"), WebviewClass::AdviewGuest);
//! assert_eq!(classify("ow-cmp"), WebviewClass::Cmp);
//! assert_eq!(classify("bw-03"), WebviewClass::Other);
//! ```

pub mod geometry;
pub mod options;

use std::collections::BTreeMap;

use crate::ipc::messages::WindowEventName;

/// The main webview's label.
pub const MAIN_LABEL: &str = crate::ipc::router::MAIN_LABEL;
/// The consent settings window's label (D.6.4).
pub const CMP_LABEL: &str = "ow-cmp";
/// The first startup consent window's label (D.6.1); later ones of the same
/// launch are `ow-cmp-startup-<n>` (D.6.2, `{}` body).
pub const CMP_STARTUP_LABEL: &str = "ow-cmp-startup";
/// The default-consent window's label (D.6.4).
pub const CMP_DEFAULT_LABEL: &str = "ow-cmp-default";

/// The class of a webview, from its label alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WebviewClass {
    /// `ow-main`.
    Main,
    /// `bw-<id>`: a UI or overlay webview (the registry knows which).
    Ui(u32),
    /// `bwr-<id>`: a remote page in window `bw-<id>`.
    Remote(u32),
    /// `owad-<embedder>-<n>`.
    AdviewGuest,
    /// `ow-cmp`, `ow-cmp-default`, `ow-cmp-startup` or
    /// `ow-cmp-startup-<n>`: the consent windows (D.6).
    Cmp,
    /// Anything else (app-created webviews).
    Other,
}

fn parse_id(s: &str) -> Option<u32> {
    if s.is_empty()
        || s.len() > 10
        || !s.bytes().all(|b| b.is_ascii_digit())
        || (s.len() > 1 && s.starts_with('0'))
    {
        return None;
    }
    s.parse().ok().filter(|id| *id > 0)
}

/// Classifies a webview label.
#[must_use]
pub fn classify(label: &str) -> WebviewClass {
    if label == MAIN_LABEL {
        WebviewClass::Main
    } else if label == CMP_LABEL
        || label == CMP_DEFAULT_LABEL
        || label == CMP_STARTUP_LABEL
        || label
            .strip_prefix("ow-cmp-startup-")
            .and_then(parse_id)
            .is_some()
    {
        WebviewClass::Cmp
    } else if let Some(id) = label.strip_prefix("bwr-").and_then(parse_id) {
        WebviewClass::Remote(id)
    } else if let Some(id) = label.strip_prefix("bw-").and_then(parse_id) {
        WebviewClass::Ui(id)
    } else if label.starts_with("owad-") && label.len() > "owad-".len() {
        WebviewClass::AdviewGuest
    } else {
        WebviewClass::Other
    }
}

/// `bw-<id>`: the window label and the app webview label.
#[must_use]
pub fn ui_label(id: u32) -> String {
    format!("bw-{id}")
}

/// `bwr-<id>`: the remote webview label.
#[must_use]
pub fn remote_label(id: u32) -> String {
    format!("bwr-{id}")
}

/// What a `bw-<id>` window currently is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    /// Local UI window.
    Ui,
    /// Overlay window (overlay package backend).
    Overlay,
    /// Shows a remote page in `bwr-<id>`; never moves back.
    Remote,
}

/// Window state the plugin tracks to derive Electron events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent window state bits"
)]
pub struct WindowState {
    /// Minimized.
    pub minimized: bool,
    /// Maximized.
    pub maximized: bool,
    /// Full screen.
    pub fullscreen: bool,
    /// Visible.
    pub visible: bool,
}

/// One registered window.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent per-window flags"
)]
pub struct WindowEntry {
    /// Electron-style id.
    pub id: u32,
    /// Current kind.
    pub kind: WindowKind,
    /// Normalised `name` option (analytics).
    pub name: Option<String>,
    /// Last known state.
    pub state: WindowState,
    /// `ready-to-show` was sent.
    pub shown_ready: bool,
    /// The `title` constructor option, else `<PN>` (E.2 #7).
    pub title: String,
    /// A page load has finished in the window.
    pub loaded: bool,
    /// The analytics name, fixed the first time the window is visible with
    /// a loaded page (E.2 #7).
    pub analytics_name: Option<String>,
    /// Start of the current visible period, in host milliseconds.
    pub visible_since: Option<u64>,
    /// A minimize ended the last visible period. Restoring the window does
    /// not start a new one; showing it after a hide does (ow-electron
    /// (observed), E.2 #7).
    pub minimize_ended: bool,
    /// Whether the window was shown the last time its ad guests followed
    /// it (`None` before the first poll). A minimize does not change it:
    /// the guests follow a minimize on their own path.
    pub guests_shown: Option<bool>,
    /// The window is being minimized: the OS has started the minimize but
    /// does not report the window minimized yet. macOS reports it not
    /// visible during its minimize animation (about half a second); that is
    /// part of the minimize, not a hide.
    pub minimizing: bool,
}

/// What a visibility observation changed (E.2 #5, #7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisibilityChange {
    /// Nothing.
    None,
    /// A visible period started.
    Shown,
    /// A visible period ended.
    Ended(VisiblePeriod),
}

/// One finished visible period of a window (E.2 #7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisiblePeriod {
    /// The analytics name.
    pub name: String,
    /// The constructor title.
    pub title: String,
    /// Milliseconds visible.
    pub visible_ms: u64,
}

impl WindowEntry {
    fn fix_name(&mut self, url: Option<&str>) {
        if self.analytics_name.is_none()
            && self.loaded
            && let Some(url) = url
        {
            self.analytics_name = Some(crate::analytics::window_analytics_name(url));
        }
    }

    /// Records whether the window is visible now; `url` is the document
    /// it shows.
    ///
    /// ```
    /// use tauri_plugin_overwolf::window::{VisibilityChange, WindowKind, WindowRegistry, WindowState};
    /// let mut r = WindowRegistry::new();
    /// r.insert(1, WindowKind::Ui, WindowState::default());
    /// let e = r.get_mut(1).unwrap();
    /// e.title = "Example".into();
    /// e.page_finished(false, Some("tauri://localhost/index.html"));
    /// assert_eq!(e.observe_visibility(true, 1_000, Some("tauri://localhost/index.html")), VisibilityChange::Shown);
    /// let VisibilityChange::Ended(p) = e.observe_visibility(false, 2_600, None) else { panic!() };
    /// assert_eq!((p.name.as_str(), p.title.as_str(), p.visible_ms), ("index", "Example", 1_600));
    /// ```
    pub fn observe_visibility(
        &mut self,
        visible: bool,
        now_ms: u64,
        url: Option<&str>,
    ) -> VisibilityChange {
        self.observe_window(visible, false, now_ms, url)
    }

    /// [`WindowEntry::observe_visibility`] for a window that may be
    /// minimized: a minimized window is not visible, its minimize ends the
    /// visible period, and its restore starts none (only a show after a
    /// hide does), as in ow-electron (observed).
    ///
    /// ```
    /// use tauri_plugin_overwolf::window::{VisibilityChange, WindowKind, WindowRegistry, WindowState};
    /// let mut r = WindowRegistry::new();
    /// r.insert(1, WindowKind::Ui, WindowState::default());
    /// let e = r.get_mut(1).unwrap();
    /// assert_eq!(e.observe_window(true, false, 0, None), VisibilityChange::Shown);
    /// assert!(matches!(e.observe_window(false, true, 20_000, None), VisibilityChange::Ended(_)));
    /// // Restored: no new visible period.
    /// assert_eq!(e.observe_window(true, false, 26_000, None), VisibilityChange::None);
    /// ```
    pub fn observe_window(
        &mut self,
        visible: bool,
        minimized: bool,
        now_ms: u64,
        url: Option<&str>,
    ) -> VisibilityChange {
        let minimized = self.counts_as_minimized(minimized);
        let visible = visible && !minimized;
        match (visible, self.visible_since) {
            (true, None) if self.minimize_ended => VisibilityChange::None,
            (true, None) => {
                self.visible_since = Some(now_ms);
                self.fix_name(url);
                VisibilityChange::Shown
            }
            (false, Some(_)) => {
                self.minimize_ended = minimized;
                self.end_visible_period(now_ms, url)
                    .map_or(VisibilityChange::None, VisibilityChange::Ended)
            }
            (false, None) if !minimized => {
                self.minimize_ended = false;
                VisibilityChange::None
            }
            _ => VisibilityChange::None,
        }
    }

    /// Whether a poll that read `os_minimized` sees a minimized window: the
    /// OS says so, the window is being minimized, or the host already
    /// applied its minimize (a poll that read the OS just before the
    /// minimize ended must not take it for a hide).
    fn counts_as_minimized(&self, os_minimized: bool) -> bool {
        os_minimized || self.minimizing || self.state.minimized
    }

    /// The ad guests' part of a visibility poll: `Some(shown)` when the
    /// guests must follow a show or a hide of the window, `None` when
    /// nothing changed or the window is minimized or being minimized (a
    /// minimize reaches the guests on its own path, with its own messages).
    ///
    /// ```
    /// use tauri_plugin_overwolf::window::{WindowKind, WindowRegistry, WindowState};
    /// let mut r = WindowRegistry::new();
    /// r.insert(1, WindowKind::Ui, WindowState::default());
    /// let e = r.get_mut(1).unwrap();
    /// assert_eq!(e.guests_follow(true, false), Some(true));
    /// e.minimizing = true;
    /// assert_eq!(e.guests_follow(false, false), None);
    /// e.minimizing = false;
    /// assert_eq!(e.guests_follow(false, false), Some(false));
    /// ```
    pub fn guests_follow(&mut self, visible: bool, minimized: bool) -> Option<bool> {
        if self.counts_as_minimized(minimized)
            || self.guests_shown.replace(visible) == Some(visible)
        {
            return None;
        }
        Some(visible)
    }

    /// A page load finished; fixes the name when the window is visible.
    pub fn page_finished(&mut self, visible: bool, url: Option<&str>) {
        self.loaded = true;
        if visible || self.visible_since.is_some() {
            self.fix_name(url);
        }
    }

    /// Ends the current visible period (hide, close or quit), if any.
    pub fn end_visible_period(&mut self, now_ms: u64, url: Option<&str>) -> Option<VisiblePeriod> {
        let since = self.visible_since.take()?;
        let name = self.analytics_name.clone().unwrap_or_else(|| {
            url.map_or_else(
                || "blank".to_owned(),
                crate::analytics::window_analytics_name,
            )
        });
        Some(VisiblePeriod {
            name,
            title: self.title.clone(),
            visible_ms: now_ms.saturating_sub(since),
        })
    }
}

/// The windows the plugin created for `BrowserWindow`s.
#[derive(Debug, Clone, Default)]
pub struct WindowRegistry {
    next_id: u32,
    entries: BTreeMap<u32, WindowEntry>,
    focus_order: Vec<u32>,
}

impl WindowRegistry {
    /// An empty registry; ids start at 1.
    #[must_use]
    pub fn new() -> Self {
        WindowRegistry {
            next_id: 1,
            entries: BTreeMap::new(),
            focus_order: Vec::new(),
        }
    }

    /// Allocates a new id (never reused in a process).
    pub fn allocate(&mut self) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    /// Registers window `id`.
    pub fn insert(&mut self, id: u32, kind: WindowKind, state: WindowState) {
        self.entries.insert(
            id,
            WindowEntry {
                id,
                kind,
                name: None,
                state,
                shown_ready: false,
                title: String::new(),
                loaded: false,
                analytics_name: None,
                visible_since: None,
                minimize_ended: false,
                guests_shown: None,
                minimizing: false,
            },
        );
    }

    /// Looks a window up.
    #[must_use]
    pub fn get(&self, id: u32) -> Option<&WindowEntry> {
        self.entries.get(&id)
    }

    /// Looks a window up mutably.
    pub fn get_mut(&mut self, id: u32) -> Option<&mut WindowEntry> {
        self.entries.get_mut(&id)
    }

    /// Removes a window.
    pub fn remove(&mut self, id: u32) -> Option<WindowEntry> {
        self.focus_order.retain(|w| *w != id);
        self.entries.remove(&id)
    }

    /// Every id, ascending.
    #[must_use]
    pub fn ids(&self) -> Vec<u32> {
        self.entries.keys().copied().collect()
    }

    /// Ids of windows that are not remote.
    #[must_use]
    pub fn local_ids(&self) -> Vec<u32> {
        self.entries
            .values()
            .filter(|e| e.kind != WindowKind::Remote)
            .map(|e| e.id)
            .collect()
    }

    /// Number of windows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no window is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The label IPC for window `id` goes to: `bw-<id>` for local windows,
    /// `None` for remote or unknown ones (C.5).
    #[must_use]
    pub fn ipc_target(&self, id: u32) -> Option<String> {
        self.entries
            .get(&id)
            .filter(|e| e.kind != WindowKind::Remote)
            .map(|e| ui_label(e.id))
    }

    /// Records that `id` gained focus.
    pub fn focused(&mut self, id: u32) {
        self.focus_order.retain(|w| *w != id);
        self.focus_order.push(id);
    }

    /// Window ids in the order `app.focus()` tries them: most recently
    /// focused first, then the rest from newest to oldest.
    #[must_use]
    pub fn focus_candidates(&self) -> Vec<u32> {
        let mut out: Vec<u32> = self.focus_order.iter().rev().copied().collect();
        for id in self.entries.keys().rev() {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        out
    }

    /// The most recently focused window that is visible, else the most
    /// recently created visible one.
    #[must_use]
    pub fn most_recent_visible(&self) -> Option<u32> {
        self.focus_order
            .iter()
            .rev()
            .copied()
            .find(|id| self.entries.get(id).is_some_and(|e| e.state.visible))
            .or_else(|| {
                self.entries
                    .values()
                    .rev()
                    .find(|e| e.state.visible)
                    .map(|e| e.id)
            })
    }
}

/// The Electron events implied by a state change, in Electron's order.
///
/// Tauri has no minimize event; the plugin derives `minimize`, `restore`,
/// `maximize`, `unmaximize` and the full-screen events from successive state
/// reads (A.3).
///
/// ```
/// use tauri_plugin_overwolf::ipc::messages::WindowEventName as E;
/// use tauri_plugin_overwolf::window::{derive_state_events, WindowState};
/// let normal = WindowState { visible: true, ..WindowState::default() };
/// let min = WindowState { minimized: true, ..normal };
/// assert_eq!(derive_state_events(normal, min), vec![E::Minimize]);
/// assert_eq!(derive_state_events(min, normal), vec![E::Restore]);
/// ```
#[must_use]
pub fn derive_state_events(prev: WindowState, now: WindowState) -> Vec<WindowEventName> {
    let mut out = Vec::new();
    if !prev.minimized && now.minimized {
        out.push(WindowEventName::Minimize);
        return out;
    }
    if prev.minimized && !now.minimized {
        out.push(WindowEventName::Restore);
    }
    if !now.minimized {
        if !prev.maximized && now.maximized {
            out.push(WindowEventName::Maximize);
        } else if prev.maximized && !now.maximized && !prev.minimized {
            out.push(WindowEventName::Unmaximize);
        }
    }
    if !prev.fullscreen && now.fullscreen {
        out.push(WindowEventName::EnterFullScreen);
    } else if prev.fullscreen && !now.fullscreen {
        out.push(WindowEventName::LeaveFullScreen);
    }
    out
}

/// Normalises the ow-electron `name` option: whitespace and special
/// characters removed, keeping letters, digits, `_` and `-`.
///
/// ```
/// use tauri_plugin_overwolf::window::normalize_window_name;
/// assert_eq!(normalize_window_name(" Main Window #1 "), "MainWindow1");
/// assert_eq!(normalize_window_name("in_game-hud"), "in_game-hud");
/// ```
#[must_use]
pub fn normalize_window_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use WindowEventName as E;

    #[test]
    fn labels() {
        assert_eq!(classify("bw-1"), WebviewClass::Ui(1));
        assert_eq!(classify("bw-0"), WebviewClass::Other);
        assert_eq!(classify("bw-"), WebviewClass::Other);
        assert_eq!(classify("bw-1a"), WebviewClass::Other);
        assert_eq!(classify("bw-99999999999"), WebviewClass::Other);
        assert_eq!(classify("bwr-12"), WebviewClass::Remote(12));
        assert_eq!(classify("owad-"), WebviewClass::Other);
        assert_eq!(classify("main"), WebviewClass::Other);
        assert_eq!(classify("ow-cmp-startup-0"), WebviewClass::Other);
        assert_eq!(classify("ow-cmp-startup-x"), WebviewClass::Other);
        assert_eq!(ui_label(7), "bw-7");
        assert_eq!(remote_label(7), "bwr-7");
    }

    #[test]
    fn a_minimize_ends_the_visible_period_and_a_restore_starts_none() {
        let mut r = WindowRegistry::new();
        r.insert(1, WindowKind::Ui, WindowState::default());
        let e = r.get_mut(1).unwrap();
        assert_eq!(
            e.observe_window(true, false, 0, None),
            VisibilityChange::Shown
        );
        // macOS: a minimized window is also not visible.
        let VisibilityChange::Ended(p) = e.observe_window(false, true, 20_000, None) else {
            panic!("the minimize ends the period")
        };
        assert_eq!(p.visible_ms, 20_000);
        assert_eq!(
            e.observe_window(false, true, 21_000, None),
            VisibilityChange::None
        );
        // Restored: no new period, so quitting sends nothing [OBS].
        assert_eq!(
            e.observe_window(true, false, 26_000, None),
            VisibilityChange::None
        );
        assert_eq!(e.end_visible_period(50_000, None), None);
        // hide() and show() again: a new period.
        assert_eq!(
            e.observe_window(false, false, 51_000, None),
            VisibilityChange::None
        );
        assert_eq!(
            e.observe_window(true, false, 52_000, None),
            VisibilityChange::Shown
        );
        // A window reported visible and minimized at once is not visible.
        assert!(matches!(
            e.observe_window(true, true, 60_000, None),
            VisibilityChange::Ended(_)
        ));
    }

    /// Regression (lab diff, `perf-minimize`): during the macOS minimize
    /// animation the window is reported neither visible nor minimized; the
    /// poll took that for a hide, so the guests got no `window-minimized`
    /// and the restore started a new visible period.
    #[test]
    fn a_window_being_minimized_is_not_hidden() {
        let mut r = WindowRegistry::new();
        r.insert(1, WindowKind::Ui, WindowState::default());
        let e = r.get_mut(1).unwrap();
        assert_eq!(
            e.observe_window(true, false, 0, None),
            VisibilityChange::Shown
        );
        assert_eq!(e.guests_follow(true, false), Some(true));
        e.minimizing = true;
        assert!(matches!(
            e.observe_window(false, false, 20_000, None),
            VisibilityChange::Ended(_)
        ));
        assert_eq!(e.guests_follow(false, false), None);
        e.minimizing = false;
        assert_eq!(e.guests_follow(false, true), None);
        // A poll that read the OS before the minimize ended, applied after
        // the host recorded it, is no hide either (lab: the guest got
        // `window-hidden` before `window-minimized`).
        e.state.minimized = true;
        assert_eq!(e.guests_follow(false, false), None);
        e.state.minimized = false;
        // Restored: the guests were never told the window was hidden, and
        // no new period starts.
        assert_eq!(
            e.observe_window(true, false, 26_000, None),
            VisibilityChange::None
        );
        assert_eq!(e.guests_follow(true, false), None);
    }

    #[test]
    fn visible_periods() {
        let mut r = WindowRegistry::new();
        r.insert(1, WindowKind::Ui, WindowState::default());
        let e = r.get_mut(1).unwrap();
        e.title = "T".into();
        // Shown before any load: the name waits for the load.
        assert_eq!(
            e.observe_visibility(true, 0, Some("about:blank")),
            VisibilityChange::Shown
        );
        assert_eq!(e.analytics_name, None);
        e.page_finished(true, Some("tauri://localhost/pages/Main%20View.html?x=1"));
        assert_eq!(e.analytics_name.as_deref(), Some("MainView"));
        // Later navigations never rename it.
        e.page_finished(true, Some("tauri://localhost/other.html"));
        assert_eq!(
            e.observe_visibility(true, 500, None),
            VisibilityChange::None
        );
        // hide() ends the period; a later close sends nothing more.
        let VisibilityChange::Ended(p) = e.observe_visibility(false, 90_400, None) else {
            panic!("period ended")
        };
        assert_eq!(
            p,
            VisiblePeriod {
                name: "MainView".into(),
                title: "T".into(),
                visible_ms: 90_400
            }
        );
        assert_eq!(e.end_visible_period(91_000, None), None);
        // Shown again and closed while visible: another period.
        assert_eq!(
            e.observe_visibility(true, 100_000, None),
            VisibilityChange::Shown
        );
        assert_eq!(
            e.end_visible_period(101_500, None).map(|p| p.visible_ms),
            Some(1_500)
        );
        // A window shown but never loaded reports its current URL.
        r.insert(2, WindowKind::Ui, WindowState::default());
        let e = r.get_mut(2).unwrap();
        e.observe_visibility(true, 0, None);
        assert_eq!(
            e.end_visible_period(2_000, None).map(|p| p.name),
            Some("blank".into())
        );
    }

    #[test]
    fn registry() {
        let mut r = WindowRegistry::new();
        let a = r.allocate();
        let b = r.allocate();
        assert_eq!((a, b), (1, 2));
        let visible = WindowState {
            visible: true,
            ..WindowState::default()
        };
        r.insert(a, WindowKind::Ui, visible);
        r.insert(b, WindowKind::Remote, visible);
        assert_eq!(r.ipc_target(a).as_deref(), Some("bw-1"));
        assert_eq!(r.ipc_target(b), None);
        assert_eq!(r.ipc_target(9), None);
        assert_eq!(r.local_ids(), vec![1]);
        r.focused(a);
        r.focused(b);
        assert_eq!(r.most_recent_visible(), Some(b));
        assert_eq!(r.focus_candidates(), vec![b, a]);
        r.get_mut(b).unwrap().state.visible = false;
        assert_eq!(r.most_recent_visible(), Some(a));
        r.remove(a);
        assert_eq!(r.most_recent_visible(), None);
        assert_eq!(r.allocate(), 3, "ids are never reused");
    }

    #[test]
    fn derived_events() {
        let n = WindowState::default();
        let max = WindowState {
            maximized: true,
            ..n
        };
        let fs = WindowState {
            fullscreen: true,
            ..n
        };
        let min_max = WindowState {
            minimized: true,
            maximized: true,
            ..n
        };
        assert_eq!(derive_state_events(n, max), vec![E::Maximize]);
        assert_eq!(derive_state_events(max, n), vec![E::Unmaximize]);
        assert_eq!(derive_state_events(max, min_max), vec![E::Minimize]);
        assert_eq!(derive_state_events(min_max, max), vec![E::Restore]);
        assert_eq!(derive_state_events(n, fs), vec![E::EnterFullScreen]);
        assert_eq!(derive_state_events(fs, n), vec![E::LeaveFullScreen]);
        assert!(derive_state_events(n, n).is_empty());
    }
}
