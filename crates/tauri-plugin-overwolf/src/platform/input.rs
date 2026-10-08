//! macOS input integrity [R1] (DESIGN §4.6a).
//!
//! With Tauri's `unstable` feature every webview is a wry child webview, and
//! wry's child mode leaves the key responder chain without the stable mode's
//! parent view: key equivalents and some typed characters reach the page
//! wrongly, and a new window's page gets no keys until the first click. The
//! fix, applied from `host::dispatch::on_webview_ready` to every webview and
//! again after any reparent:
//!
//! 1. **Responder splice**: one small `NSResponder` between the `WKWebView`
//!    and its parent view whose `keyDown:` only offers the event to the main
//!    menu, as stable mode's parent view does. Per webview, idempotent, no
//!    class swizzling.
//! 2. **Focus at open**: app webviews (never ad guests or consent webviews)
//!    get `set_focus()` when their window is created and is key.
//!
//! [`Builder::macos_key_fix(false)`](crate::Builder::macos_key_fix) turns
//! both off for apps that ship their own fix. Ad guests stay unfocusable
//! either way (§4.4.1).
//!
//! W1 freezes the call site and the decision ([`key_fix_for`]); W2-A fills
//! [`on_webview_ready`] with the native work.

use std::sync::Arc;

use tauri::{Runtime, Webview};

use crate::config::is_reserved_label;
use crate::host::Core;

/// What the key fix does for one webview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyFix {
    /// Nothing: the app turned the fix off.
    Off,
    /// The responder splice only (ad guests and consent webviews).
    Splice,
    /// The responder splice and the focus at window open (app webviews).
    SpliceAndFocus,
}

/// The key fix for the webview `label`, with the builder switch `enabled`.
pub(crate) fn key_fix_for(enabled: bool, label: &str) -> KeyFix {
    if !enabled {
        KeyFix::Off
    } else if is_reserved_label(label) {
        KeyFix::Splice
    } else {
        KeyFix::SpliceAndFocus
    }
}

/// `on_webview_ready` on macOS: applies [`key_fix_for`] to `webview`.
pub(crate) fn on_webview_ready<R: Runtime>(core: &Arc<Core<R>>, webview: &Webview<R>) {
    let fix = key_fix_for(core.options.macos_key_fix, webview.label());
    #[cfg(test)]
    tests::record(webview.label(), fix);
    match fix {
        // W2-A: the responder splice (and, for app webviews, the focus at
        // window open) per DESIGN §4.6a.
        KeyFix::Off | KeyFix::Splice | KeyFix::SpliceAndFocus => {}
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::{Mutex, PoisonError};

    use super::*;

    /// Every decision [`on_webview_ready`] made in this test process, by
    /// webview label.
    static SEEN: Mutex<Vec<(String, KeyFix)>> = Mutex::new(Vec::new());

    pub(super) fn record(label: &str, fix: KeyFix) {
        SEEN.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((label.to_owned(), fix));
    }

    /// The decisions made for the webview `label`.
    pub(crate) fn seen(label: &str) -> Vec<KeyFix> {
        SEEN.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(l, _)| l == label)
            .map(|(_, fix)| *fix)
            .collect()
    }

    #[test]
    fn app_webviews_get_the_splice_and_the_focus() {
        assert_eq!(key_fix_for(true, "main"), KeyFix::SpliceAndFocus);
        assert_eq!(key_fix_for(true, "settings-2"), KeyFix::SpliceAndFocus);
    }

    #[test]
    fn plugin_webviews_get_the_splice_only() {
        assert_eq!(key_fix_for(true, "owad-1"), KeyFix::Splice);
        assert_eq!(key_fix_for(true, "ow-cmp-startup"), KeyFix::Splice);
    }

    #[test]
    fn the_escape_hatch_turns_everything_off() {
        for label in ["main", "owad-1", "ow-cmp-startup"] {
            assert_eq!(key_fix_for(false, label), KeyFix::Off, "{label}");
        }
    }
}
