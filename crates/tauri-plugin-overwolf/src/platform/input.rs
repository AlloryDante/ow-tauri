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
//! The native work is posted from a runtime task (`with_webview` from the
//! hook itself would run inline on the main thread while Tauri still
//! registers the webview), so it runs right after the webview is in its
//! window, in order with the webview's other native calls. `AppKit` resets
//! a view's next responder when the view moves to another superview, and
//! Tauri has no hook for `Webview::reparent`: a reparented app webview loses
//! the splice (documented limit).

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

/// `on_webview_ready` on macOS: applies [`key_fix_for`] to `webview`. The
/// focus at open goes to a window's own webview (the one whose label is the
/// window's), never to a further child webview of the window.
pub(crate) fn on_webview_ready<R: Runtime>(core: &Arc<Core<R>>, webview: &Webview<R>) {
    let fix = key_fix_for(core.options.macos_key_fix, webview.label());
    #[cfg(test)]
    tests::record(webview.label(), fix);
    let focus = match fix {
        KeyFix::Off => return,
        KeyFix::Splice => false,
        KeyFix::SpliceAndFocus => webview.label() == webview.window().label(),
    };
    let webview = webview.clone();
    tauri::async_runtime::spawn(async move {
        let _ = webview.with_webview(move |pw| {
            let view = pw.inner();
            native::splice(view);
            if focus {
                native::focus(view);
            }
        });
    });
}

/// The responder splice and the focus, on the main thread.
mod native {
    use std::ffi::c_void;
    use std::sync::OnceLock;

    use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
    use objc2::{class, msg_send, sel};

    /// The name of the spliced responder's class.
    pub(super) const SINK_CLASS: &std::ffi::CStr = c"OwTauriKeySink";

    /// The associated-object key that keeps a webview's sink alive with it.
    static SINK_KEY: u8 = 0;

    /// `keyDown:` of the sink: offers the event to the main menu as a key
    /// equivalent, as stable mode's `WryWebViewParent` does, and nothing
    /// else (the key never reaches the window's input context).
    extern "C-unwind" fn sink_key_down(_this: *mut AnyObject, _cmd: Sel, event: *mut AnyObject) {
        if event.is_null() {
            return;
        }
        // SAFETY: public AppKit calls on the main thread (`keyDown:` is
        // delivered there) with a live event.
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            if app.is_null() {
                return;
            }
            let menu: *mut AnyObject = msg_send![app, mainMenu];
            if !menu.is_null() {
                let _: Bool = msg_send![menu, performKeyEquivalent: event];
            }
        }
    }

    /// The sink class (an `NSResponder` subclass with [`sink_key_down`]),
    /// registered once; `None` if the runtime refused it.
    pub(super) fn sink_class() -> Option<&'static AnyClass> {
        static CLASS: OnceLock<Option<usize>> = OnceLock::new();
        let address = (*CLASS.get_or_init(|| {
            if let Some(existing) = AnyClass::get(SINK_CLASS) {
                return Some(std::ptr::from_ref(existing) as usize);
            }
            let mut builder = objc2::runtime::ClassBuilder::new(SINK_CLASS, class!(NSResponder))?;
            let key_down: extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) = sink_key_down;
            // SAFETY: `keyDown:` takes one object and returns nothing, as
            // the function's signature says.
            unsafe { builder.add_method(sel!(keyDown:), key_down) };
            Some(std::ptr::from_ref(builder.register()) as usize)
        }))?;
        // SAFETY: a registered class lives for the process.
        Some(unsafe { &*(address as *const AnyClass) })
    }

    /// Inserts one sink between the `WKWebView` at `view` and its next
    /// responder, unless its next responder is already a sink. Main thread.
    pub(super) fn splice(view: *mut c_void) {
        let Some(class) = sink_class() else { return };
        if view.is_null() {
            return;
        }
        // SAFETY: Tauri hands a live `WKWebView*` on the main thread; every
        // call is public AppKit / Objective-C runtime API. The sink is
        // retained by the view (associated object) and released with it.
        unsafe {
            let view: *mut AnyObject = view.cast();
            let next: *mut AnyObject = msg_send![view, nextResponder];
            if !next.is_null() {
                let is_sink: bool = msg_send![next, isKindOfClass: class];
                if is_sink {
                    return;
                }
            }
            let sink: *mut AnyObject = msg_send![class, new];
            if sink.is_null() {
                return;
            }
            let () = msg_send![sink, setNextResponder: next];
            let () = msg_send![view, setNextResponder: sink];
            objc2::ffi::objc_setAssociatedObject(
                view,
                (&raw const SINK_KEY).cast(),
                sink,
                objc2::ffi::OBJC_ASSOCIATION_RETAIN_NONATOMIC,
            );
            objc2::ffi::objc_release(sink);
        }
    }

    /// Makes the `WKWebView` at `view` its window's first responder (what
    /// `Webview::set_focus` does), without activating the app or ordering
    /// the window front. Main thread.
    pub(super) fn focus(view: *mut c_void) {
        if view.is_null() {
            return;
        }
        // SAFETY: Tauri hands a live `WKWebView*` on the main thread.
        unsafe {
            let window: *mut AnyObject = msg_send![view.cast::<AnyObject>(), window];
            if !window.is_null() {
                let _: Bool = msg_send![window, makeFirstResponder: view.cast::<AnyObject>()];
            }
        }
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
    fn the_sink_class_is_a_responder_registered_once() {
        let class = native::sink_class().unwrap();
        assert_eq!(class.name(), native::SINK_CLASS);
        assert!(class.instance_method(objc2::sel!(keyDown:)).is_some());
        let responder = objc2::class!(NSResponder);
        assert!(std::ptr::eq(class.superclass().unwrap(), responder));
        assert!(std::ptr::eq(native::sink_class().unwrap(), class));
    }

    #[test]
    fn the_splice_is_idempotent_and_keeps_the_chain() {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        // SAFETY: plain NSResponder objects stand in for a webview and its
        // parent view; the splice only uses NSResponder API.
        unsafe {
            let view: *mut AnyObject = msg_send![class!(NSResponder), new];
            let parent: *mut AnyObject = msg_send![class!(NSResponder), new];
            let () = msg_send![view, setNextResponder: parent];
            native::splice(view.cast());
            native::splice(view.cast());
            let sink: *mut AnyObject = msg_send![view, nextResponder];
            assert_eq!((*sink).class().name(), native::SINK_CLASS);
            let after: *mut AnyObject = msg_send![sink, nextResponder];
            assert_eq!(after, parent, "one sink, then the old next responder");
            objc2::ffi::objc_release(view);
            objc2::ffi::objc_release(parent);
        }
    }

    #[test]
    fn the_escape_hatch_turns_everything_off() {
        for label in ["main", "owad-1", "ow-cmp-startup"] {
            assert_eq!(key_fix_for(false, label), KeyFix::Off, "{label}");
        }
    }
}
