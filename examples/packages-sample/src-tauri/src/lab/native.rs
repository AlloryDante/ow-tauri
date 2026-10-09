//! macOS calls of the invisible lab (feature `lab` only; adapted from the
//! ad showcase's lab): [`hold_app_back`], [`prepare_invisible`] and
//! [`order_front`] keep the app in the background, never activated, its
//! window on screen at alpha 0 without becoming key.
//!
//! Every function that touches a window runs on the main thread.

use std::sync::{Once, OnceLock};

use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{class, msg_send, sel};

/// The object at `address`, or `None` for null.
///
/// # Safety
///
/// `address` must be null or a live Objective-C object for the whole call
/// that uses the returned reference.
unsafe fn object<'a>(address: usize) -> Option<&'a AnyObject> {
    // SAFETY: the caller guarantees a live object (or null).
    unsafe { (address as *const AnyObject).as_ref() }
}

/// The replaced implementations (kept so nothing else is lost).
static ACTIVATE: OnceLock<usize> = OnceLock::new();
/// See [`ACTIVATE`].
static ACTIVATE_IGNORING: OnceLock<usize> = OnceLock::new();
/// See [`ACTIVATE`].
static KEY_FRONT: OnceLock<usize> = OnceLock::new();

/// `-[NSApplication activate]` without effect.
extern "C-unwind" fn no_activate(_this: &AnyObject, _cmd: Sel) {}

/// `-[NSApplication activateIgnoringOtherApps:]` without effect.
extern "C-unwind" fn no_activate_ignoring(_this: &AnyObject, _cmd: Sel, _flag: Bool) {}

/// `-[NSWindow makeKeyAndOrderFront:]` that only orders the window front.
extern "C-unwind" fn order_front_only(this: &AnyObject, _cmd: Sel, _sender: *mut AnyObject) {
    // SAFETY: `this` is the NSWindow the message was sent to, on the main
    // thread; a public NSWindow method without arguments.
    let () = unsafe { msg_send![this, orderFrontRegardless] };
}

/// Replaces the instance method `selector` of `class` with `imp`, keeping
/// the original in `slot`.
fn replace_method(class: &AnyClass, selector: Sel, slot: &OnceLock<usize>, imp: Imp) {
    let Some(method) = class.instance_method(selector) else {
        return;
    };
    let _ = slot.set(method.implementation() as usize);
    // SAFETY: a method of a registered class; its encoding outlives the call.
    let types = unsafe { objc2::ffi::method_getTypeEncoding(method) };
    // SAFETY: `imp` has the selector's signature, as `types` says.
    unsafe {
        objc2::ffi::class_replaceMethod(std::ptr::from_ref(class).cast_mut(), selector, imp, types);
    }
}

/// For the rest of the process: the app never activates and no window
/// becomes key (`WebKit` asks `NSApplication` to activate for every webview
/// it creates; showing a window makes it key). Call before the app is built.
pub fn hold_app_back() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        type Activate = extern "C-unwind" fn(&AnyObject, Sel);
        type Ignoring = extern "C-unwind" fn(&AnyObject, Sel, Bool);
        type KeyFront = extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject);
        let (activate, ignoring, key_front): (Activate, Ignoring, KeyFront) =
            (no_activate, no_activate_ignoring, order_front_only);
        // SAFETY: function pointers cast to the runtime's untyped `Imp`; each
        // is installed only for a selector with its exact signature.
        let (activate, ignoring, key_front) = unsafe {
            (
                std::mem::transmute::<Activate, Imp>(activate),
                std::mem::transmute::<Ignoring, Imp>(ignoring),
                std::mem::transmute::<KeyFront, Imp>(key_front),
            )
        };
        let app = class!(NSApplication);
        replace_method(app, sel!(activate), &ACTIVATE, activate);
        replace_method(
            app,
            sel!(activateIgnoringOtherApps:),
            &ACTIVATE_IGNORING,
            ignoring,
        );
        replace_method(
            class!(NSWindow),
            sel!(makeKeyAndOrderFront:),
            &KEY_FRONT,
            key_front,
        );
    });
}

/// Alpha 0 and click-through for the window at `ns_window` (before it is
/// shown, main thread).
pub fn prepare_invisible(ns_window: usize) {
    // SAFETY: Tauri's `ns_window()` of a live window, on the main thread
    // (the setup hook); public NSWindow setters.
    unsafe {
        if let Some(window) = object(ns_window) {
            let () = msg_send![window, setAlphaValue: 0.0_f64];
            let () = msg_send![window, setIgnoresMouseEvents: Bool::YES];
        }
    }
}

/// Orders the window at `ns_window` front at alpha 0, without making it key
/// or activating the app (main thread).
pub fn order_front(ns_window: usize) {
    // SAFETY: the live window of `order_front`'s caller, on the main thread;
    // public NSWindow methods.
    unsafe {
        if let Some(window) = object(ns_window) {
            let () = msg_send![window, setAlphaValue: 0.0_f64];
            let () = msg_send![window, orderFrontRegardless];
        }
    }
}
