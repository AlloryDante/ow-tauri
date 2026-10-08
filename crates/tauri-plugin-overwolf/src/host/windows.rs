//! App windows tracked from native Tauri events (DESIGN §4.3) and the
//! visibility ticker (§4.11).
//!
//! W1 holds the frozen shape: the hooks [`dispatch`](super::dispatch) calls
//! and the name overrides of `set_window_name`. The visible-period state
//! machine, the naming rules, exclusions and the ticker arrive in W2; until
//! then no window period is reported.

#![allow(
    dead_code,
    clippy::unused_self,
    reason = "the window tracker (W2) reads the remaining state"
)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use tauri::webview::PageLoadEvent;
use tauri::{Runtime, Webview, Window, WindowEvent};
use url::Url;

use super::{Core, lock};

/// The longest window name `set_window_name` accepts (it becomes the
/// `x-ow-window` header, SEC-m1).
pub(crate) const MAX_WINDOW_NAME: usize = 128;

/// Every app window, keyed by its Tauri label.
#[derive(Debug, Default)]
pub(crate) struct AppWindows {
    /// `analytics.excludeWindows` plus `Builder::exclude_windows`.
    exclude: Vec<String>,
    /// `set_window_name` overrides by window label.
    names: Mutex<BTreeMap<String, String>>,
}

impl AppWindows {
    /// No windows yet; `exclude` are the label globs never counted.
    pub(crate) fn new(exclude: &[String]) -> Self {
        AppWindows {
            exclude: exclude.to_vec(),
            names: Mutex::new(BTreeMap::new()),
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

    /// `on_window_ready`: registers the window (W2).
    pub(crate) fn window_ready<R: Runtime>(&self, _core: &Arc<Core<R>>, _window: &Window<R>) {}

    /// `on_webview_ready`: may make the webview its window's naming
    /// webview (W2).
    pub(crate) fn webview_ready<R: Runtime>(&self, _core: &Arc<Core<R>>, _webview: &Webview<R>) {}

    /// A page load of an app webview (W2).
    pub(crate) fn page_load<R: Runtime>(
        &self,
        _core: &Arc<Core<R>>,
        _webview: &Webview<R>,
        _event: PageLoadEvent,
        _url: &Url,
    ) {
    }

    /// A window event of window `label`: forgets a destroyed window's name;
    /// periods and focus arrive in W2.
    pub(crate) fn window_event<R: Runtime>(
        &self,
        _core: &Arc<Core<R>>,
        label: &str,
        event: &WindowEvent,
    ) {
        if matches!(event, WindowEvent::Destroyed) {
            lock(&self.names).remove(label);
        }
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

/// The visibility ticker (DESIGN §4.11, W2).
#[derive(Debug, Default)]
pub(crate) struct Ticker {}

/// Starts the ticker at Ready (W2).
pub(crate) fn start_ticker<R: Runtime>(_core: &Arc<Core<R>>) {}

/// Ends every open visible period at exit (`window_closed`, E.2 #7; W2).
pub(crate) fn end_all_periods<R: Runtime>(_core: &Core<R>) {}

/// Whether `name` is a valid `set_window_name` value: 1 to
/// [`MAX_WINDOW_NAME`] printable ASCII characters (no CR, LF or other
/// control characters).
pub(crate) fn valid_window_name(name: &str) -> bool {
    (1..=MAX_WINDOW_NAME).contains(&name.len()) && name.bytes().all(|b| (0x20..0x7f).contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_matches("tray*", "tray"));
        assert!(glob_matches("tray*", "tray-menu"));
        assert!(glob_matches("a?c", "abc"));
        assert!(!glob_matches("a?c", "ac"));
        assert!(!glob_matches("tray", "tray2"));
        assert!(glob_matches("*", ""));
    }

    #[test]
    fn counted_windows() {
        let w = AppWindows::new(&["tray*".to_owned()]);
        assert!(w.is_counted("main"));
        assert!(!w.is_counted("tray-menu"));
        assert!(!w.is_counted("ow-cmp-default"));
        assert!(!w.is_counted("owad-1"));
    }

    #[test]
    fn window_names() {
        assert!(valid_window_name("settings"));
        assert!(!valid_window_name(""));
        assert!(!valid_window_name("a\r\nx-evil: 1"));
        assert!(!valid_window_name(&"x".repeat(129)));
        assert!(!valid_window_name("caf\u{e9}"));
        let w = AppWindows::new(&[]);
        w.set_name("main", "home");
        assert_eq!(w.name_override("main").as_deref(), Some("home"));
    }
}
