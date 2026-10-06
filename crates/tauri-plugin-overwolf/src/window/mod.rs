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

pub mod options;

use std::collections::BTreeMap;

use crate::ipc::messages::WindowEventName;

/// The main webview's label.
pub const MAIN_LABEL: &str = crate::ipc::router::MAIN_LABEL;
/// The consent window's label.
pub const CMP_LABEL: &str = "ow-cmp";

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
    /// `ow-cmp`.
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
    } else if label == CMP_LABEL {
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
        assert_eq!(ui_label(7), "bw-7");
        assert_eq!(remote_label(7), "bwr-7");
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
