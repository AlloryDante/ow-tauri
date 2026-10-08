//! Native user activation of ad guests (DESIGN §4.9, D12).
//!
//! A guest's own `__host:gesture` message is never an authority: a page can
//! send it whenever it likes. A guest popup or an off-Overwolf top-level
//! navigation opens the system browser only when the OS says the user acted
//! on that guest:
//!
//! - **macOS**: wry owns the navigation delegates, so `WKNavigationAction`
//!   is not readable. One `NSEvent` local monitor (left mouse down and key
//!   down) sees every event of the process before `AppKit` delivers it. A
//!   mouse-down arms guest G only when the window's content view hit-tests
//!   the event to G's `WKWebView` or one of its subviews, G is shown (not
//!   `isHiddenOrHasHiddenAncestor`), G does not let input pass through, and
//!   the window is key. A Return, Space or keypad Enter key-down arms G only
//!   when G's `WKWebView` (or a subview) is the key window's first
//!   responder. A script cannot synthesise an `NSEvent`.
//! - **Windows**: WebView2 reports it (`IsUserInitiated` of popups and
//!   top-level navigations, plus the native input age over the guest for a
//!   script-initiated navigation right after a click), see
//!   `platform::webview`'s hooks. Nothing is registered here.
//! - **Linux**: no ads.
//!
//! An arming event opens the guest's activation window
//! (`guestLimits.activationWindowMs`), which the first open consumes
//! ([`crate::ads::OpenBudget`]).

use std::sync::Arc;

use tauri::{Runtime, Webview};

/// What arming a guest does: opens its activation window. Called on the
/// main thread.
pub(crate) type Arm = Arc<dyn Fn() + Send + Sync>;

/// Return, Space and keypad Enter (`kVK_Return`, `kVK_Space`,
/// `kVK_ANSI_KeypadEnter`): the keys that activate a focused control.
pub(crate) const ACTIVATION_KEYS: [u16; 3] = [36, 49, 76];

/// Whether a left mouse-down arms a guest (macOS rule above).
#[cfg_attr(
    not(any(target_os = "macos", test)),
    expect(dead_code, reason = "the macOS monitor applies it")
)]
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "the four independent conditions of DESIGN §4.9"
)]
pub(crate) fn mouse_arms(
    hit_in_guest: bool,
    guest_shown: bool,
    passthrough: bool,
    window_key: bool,
) -> bool {
    hit_in_guest && guest_shown && !passthrough && window_key
}

/// Whether a key-down arms a guest (macOS rule above).
#[cfg_attr(
    not(any(target_os = "macos", test)),
    expect(dead_code, reason = "the macOS monitor applies it")
)]
pub(crate) fn key_arms(key_code: u16, responder_in_guest: bool, window_key: bool) -> bool {
    ACTIVATION_KEYS.contains(&key_code) && responder_in_guest && window_key
}

/// Starts arming guest `webview` from native input. `view` is the address
/// of its `WKWebView`, which the caller keeps alive
/// ([`NativeView`](super::webview::NativeView)) until [`unwatch`]. The
/// monitor is installed with the first guest. Does nothing off macOS.
pub(crate) fn watch<R: Runtime>(webview: &Webview<R>, view: usize, arm: Arm) {
    #[cfg(target_os = "macos")]
    {
        let label = webview.label().to_owned();
        let _ = webview.with_webview(move |pw| {
            macos::watch(label, view, pw.ns_window() as usize, arm);
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (webview, view, arm);
    }
}

/// Stops arming guest `label` (before its view is given back).
pub(crate) fn unwatch(label: &str) {
    #[cfg(target_os = "macos")]
    macos::unwatch(label);
    #[cfg(not(target_os = "macos"))]
    let _ = label;
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::{Arc, Mutex, Once, PoisonError};

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSPoint;

    use super::{Arm, key_arms, mouse_arms};

    /// `NSEventTypeLeftMouseDown`.
    const LEFT_MOUSE_DOWN: usize = 1;
    /// `NSEventTypeKeyDown`.
    const KEY_DOWN: usize = 10;
    /// `NSEventMaskLeftMouseDown | NSEventMaskKeyDown`.
    const MASK: u64 = (1 << LEFT_MOUSE_DOWN) | (1 << KEY_DOWN);

    /// One watched guest.
    struct Entry {
        label: String,
        /// Its `WKWebView*` (kept alive by the ads host).
        view: usize,
        /// Its `NSWindow*` (compared, never messaged).
        window: usize,
        arm: Arm,
    }

    static GUESTS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
    static MONITOR: Once = Once::new();

    pub(super) fn watch(label: String, view: usize, window: usize, arm: Arm) {
        if view == 0 || window == 0 {
            return;
        }
        {
            let mut guests = GUESTS.lock().unwrap_or_else(PoisonError::into_inner);
            guests.retain(|e| e.label != label);
            guests.push(Entry {
                label,
                view,
                window,
                arm,
            });
        }
        MONITOR.call_once(install);
    }

    pub(super) fn unwatch(label: &str) {
        GUESTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|e| e.label != label);
    }

    /// Whether `view` is `ancestor` or one of its subviews.
    ///
    /// # Safety
    ///
    /// `view` is null or a live object and `ancestor` a live `NSView`, on
    /// the main thread.
    unsafe fn within(view: *mut AnyObject, ancestor: usize) -> bool {
        if view.is_null() {
            return false;
        }
        if view as usize == ancestor {
            return true;
        }
        // SAFETY: the caller's contract; public NSObject / NSView API.
        unsafe {
            let is_view: bool = msg_send![view, isKindOfClass: class!(NSView)];
            is_view && {
                let inside: bool = msg_send![view, isDescendantOf: ancestor as *mut AnyObject];
                inside
            }
        }
    }

    /// The monitor's handler: arms the guests the event acts on, and lets
    /// the event through unchanged.
    fn on_event(event: &AnyObject) {
        // SAFETY: a live NSEvent of the local monitor, on the main thread;
        // the watched views are kept alive by the ads host while listed.
        unsafe {
            let kind: usize = msg_send![event, type];
            let window: *mut AnyObject = msg_send![event, window];
            if window.is_null() {
                return;
            }
            let key: bool = msg_send![window, isKeyWindow];
            let arms: Vec<Arm> = {
                let guests = GUESTS.lock().unwrap_or_else(PoisonError::into_inner);
                let here = guests.iter().filter(|e| e.window == window as usize);
                if kind == LEFT_MOUSE_DOWN {
                    let location: NSPoint = msg_send![event, locationInWindow];
                    let content: *mut AnyObject = msg_send![window, contentView];
                    if content.is_null() {
                        return;
                    }
                    let hit: *mut AnyObject = msg_send![content, hitTest: location];
                    here.filter(|e| {
                        let hidden: bool =
                            msg_send![e.view as *mut AnyObject, isHiddenOrHasHiddenAncestor];
                        mouse_arms(
                            within(hit, e.view),
                            !hidden,
                            crate::platform::webview::passthrough_on(e.view),
                            key,
                        )
                    })
                    .map(|e| Arc::clone(&e.arm))
                    .collect()
                } else if kind == KEY_DOWN {
                    let code: u16 = msg_send![event, keyCode];
                    let responder: *mut AnyObject = msg_send![window, firstResponder];
                    here.filter(|e| key_arms(code, within(responder, e.view), key))
                        .map(|e| Arc::clone(&e.arm))
                        .collect()
                } else {
                    Vec::new()
                }
            };
            for arm in arms {
                arm();
            }
        }
    }

    /// `+[NSEvent addLocalMonitorForEventsMatchingMask:handler:]`, once; the
    /// monitor lives for the process.
    fn install() {
        let block = block2::RcBlock::new(|event: *mut AnyObject| -> *mut AnyObject {
            if !event.is_null() {
                // SAFETY: a live NSEvent handed to the monitor.
                on_event(unsafe { &*event });
            }
            event
        });
        // SAFETY: public AppKit class method on the main thread (the ads
        // host registers guests from `with_webview` closures); `AppKit`
        // copies the block and keeps the returned monitor, which is kept
        // for the process.
        let monitor: Option<Retained<AnyObject>> = unsafe {
            msg_send![class!(NSEvent), addLocalMonitorForEventsMatchingMask: MASK, handler: &*block]
        };
        std::mem::forget(monitor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_arms_only_a_shown_key_window_guest_it_hits() {
        assert!(mouse_arms(true, true, false, true));
        assert!(!mouse_arms(false, true, false, true), "missed the guest");
        assert!(!mouse_arms(true, false, false, true), "hidden guest");
        assert!(!mouse_arms(true, true, true, true), "input passes through");
        assert!(!mouse_arms(true, true, false, false), "window not key");
    }

    #[test]
    fn only_activation_keys_on_a_focused_guest_arm() {
        for code in ACTIVATION_KEYS {
            assert!(key_arms(code, true, true), "{code}");
            assert!(!key_arms(code, false, true), "{code}: focus elsewhere");
            assert!(!key_arms(code, true, false), "{code}: window not key");
        }
        // Letters, Tab and Escape never arm.
        for code in [0_u16, 48, 53] {
            assert!(!key_arms(code, true, true), "{code}");
        }
    }
}
