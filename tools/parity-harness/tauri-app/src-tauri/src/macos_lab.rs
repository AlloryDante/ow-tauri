//! macOS lab helpers of the harness app (never part of an app that ships):
//!
//! - [`hold_app_back`]: in the invisible lab the app never activates and no
//!   window becomes key. wry asks `NSApplication` to activate for every
//!   webview it creates, and tao's `show` makes the window key; both would
//!   take the keyboard from the app the user is typing in, even at alpha 0.
//! - [`prepare_invisible`]: alpha 0 and click-through before a window is
//!   shown, so it is on screen for the plugin (`is_visible`) and invisible
//!   for the user.
//! - [`web_process_pid`]: the web content process of a `WKWebView` (the
//!   `crash-guests` action ends it, as ow-electron's harness crashes a
//!   guest renderer).
//! - [`all_cookies`]: every cookie of the default website data store with
//!   the attributes ow-electron's `cookies.on('changed')` reports.
//!
//! Every function that touches `AppKit` or `WebKit` runs on the main thread.

use std::ffi::{CStr, c_char};
use std::sync::{Mutex, Once, OnceLock};

use block2::RcBlock;
use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{class, msg_send, sel};
use serde_json::{Value, json};

static KEY_FRONT_ORIGINAL: OnceLock<usize> = OnceLock::new();
static ACTIVATE_ORIGINAL: OnceLock<usize> = OnceLock::new();
static ACTIVATE_IGNORING_ORIGINAL: OnceLock<usize> = OnceLock::new();

type KeyFront = unsafe extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject);
type Activate = unsafe extern "C-unwind" fn(&AnyObject, Sel);
type ActivateIgnoring = unsafe extern "C-unwind" fn(&AnyObject, Sel, Bool);

extern "C-unwind" fn no_activate(_this: &AnyObject, _cmd: Sel) {}

extern "C-unwind" fn no_activate_ignoring(_this: &AnyObject, _cmd: Sel, _flag: Bool) {}

extern "C-unwind" fn order_front_only(this: &AnyObject, _cmd: Sel, _sender: *mut AnyObject) {
    // SAFETY: `this` is the NSWindow the message was sent to, on the main
    // thread; a public NSWindow method without arguments.
    let () = unsafe { msg_send![this, orderFrontRegardless] };
}

/// Replaces the instance method `selector` of `class` with `imp`, keeping
/// the original in `slot`.
fn replace_method(
    class: &AnyClass,
    selector: Sel,
    slot: &OnceLock<usize>,
    imp: objc2::runtime::Imp,
) {
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

/// Invisible lab only: `activate`, `activateIgnoringOtherApps:` do nothing
/// and `makeKeyAndOrderFront:` orders the window front without making it
/// key, for the rest of the process. Call before the app is built.
pub fn hold_app_back() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let activate: Activate = no_activate;
        let ignoring: ActivateIgnoring = no_activate_ignoring;
        let key_front: KeyFront = order_front_only;
        // SAFETY: function pointers cast to the runtime's `Imp` type.
        let (activate, ignoring, key_front) = unsafe {
            (
                std::mem::transmute::<Activate, objc2::runtime::Imp>(activate),
                std::mem::transmute::<ActivateIgnoring, objc2::runtime::Imp>(ignoring),
                std::mem::transmute::<KeyFront, objc2::runtime::Imp>(key_front),
            )
        };
        let app = class!(NSApplication);
        replace_method(app, sel!(activate), &ACTIVATE_ORIGINAL, activate);
        replace_method(
            app,
            sel!(activateIgnoringOtherApps:),
            &ACTIVATE_IGNORING_ORIGINAL,
            ignoring,
        );
        replace_method(
            class!(NSWindow),
            sel!(makeKeyAndOrderFront:),
            &KEY_FRONT_ORIGINAL,
            key_front,
        );
    });
}

/// The object at `address`, or `None` for null.
///
/// # Safety
///
/// `address` must be null or a live Objective-C object for the whole call.
unsafe fn object<'a>(address: usize) -> Option<&'a AnyObject> {
    // SAFETY: the caller guarantees a live object (or null).
    unsafe { (address as *const AnyObject).as_ref() }
}

/// Alpha 0 and click-through for the `NSWindow` at `ns_window`, before it
/// is shown (main thread). Returns the alpha read back.
pub fn prepare_invisible(ns_window: usize) -> Option<f64> {
    // SAFETY: Tauri's `ns_window()` of a live window, on the main thread.
    let window = unsafe { object(ns_window) }?;
    // SAFETY: public NSWindow setters and getter.
    unsafe {
        let () = msg_send![window, setAlphaValue: 0.0_f64];
        let () = msg_send![window, setIgnoresMouseEvents: Bool::YES];
        let alpha: f64 = msg_send![window, alphaValue];
        Some(alpha)
    }
}

/// The pid of the web content process of the `WKWebView` at `address`
/// (`_webProcessIdentifier`, `WebKit`'s own reading), on the main thread.
pub fn web_process_pid(address: usize) -> Option<i32> {
    // SAFETY: the live WKWebView of a webview (`with_webview`), on the main
    // thread.
    let view = unsafe { object(address) }?;
    let selector = sel!(_webProcessIdentifier);
    // SAFETY: NSObject protocol method on a live object.
    let responds: Bool = unsafe { msg_send![view, respondsToSelector: selector] };
    if !responds.as_bool() {
        return None;
    }
    // SAFETY: WebKit's getter of the process id (`pid_t`), checked above.
    let pid: i32 = unsafe { msg_send![view, _webProcessIdentifier] };
    (pid > 0).then_some(pid)
}

/// UTF-8 text of an `NSString` (or `None` for nil).
fn text(s: *mut AnyObject) -> Option<String> {
    // SAFETY: `s` is nil or an NSString returned by a getter just now.
    let s = unsafe { s.as_ref() }?;
    // SAFETY: NSString's UTF8String, valid while `s` lives.
    let p: *const c_char = unsafe { msg_send![s, UTF8String] };
    if p.is_null() {
        return None;
    }
    // SAFETY: a NUL-terminated C string owned by `s`.
    Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

/// The attributes of one `NSHTTPCookie` in the shape of Electron's
/// `Cookie` (`expirationDate` in Unix seconds, `session`, `sameSite` as
/// `WebKit` names it or `null`).
fn cookie_json(cookie: &AnyObject) -> Value {
    // SAFETY: public NSHTTPCookie getters on a live cookie.
    unsafe {
        let name: *mut AnyObject = msg_send![cookie, name];
        let value: *mut AnyObject = msg_send![cookie, value];
        let domain: *mut AnyObject = msg_send![cookie, domain];
        let path: *mut AnyObject = msg_send![cookie, path];
        let secure: Bool = msg_send![cookie, isSecure];
        let http_only: Bool = msg_send![cookie, isHTTPOnly];
        let session: Bool = msg_send![cookie, isSessionOnly];
        let expires: *mut AnyObject = msg_send![cookie, expiresDate];
        let same_site: *mut AnyObject = msg_send![cookie, sameSitePolicy];
        let expiration = expires.as_ref().map(|d| {
            let seconds: f64 = msg_send![d, timeIntervalSince1970];
            seconds
        });
        json!({
            "name": text(name),
            "value": text(value).unwrap_or_default(),
            "domain": text(domain),
            "path": text(path),
            "secure": secure.as_bool(),
            "httpOnly": http_only.as_bool(),
            "session": session.as_bool(),
            "sameSite": text(same_site).map(|s| s.to_lowercase()),
            "expirationDate": expiration,
        })
    }
}

/// Reads every cookie of the default website data store (the store the ad
/// guests, the consent windows and the app's webviews share) and calls
/// `done` with them, on the main thread.
pub fn all_cookies(done: impl FnOnce(Vec<Value>) + Send + 'static) {
    let done = Mutex::new(Some(done));
    let block = RcBlock::new(move |cookies: *mut AnyObject| {
        let mut out = Vec::new();
        // SAFETY: the NSArray WebKit passes to the completion handler.
        if let Some(array) = unsafe { cookies.as_ref() } {
            // SAFETY: NSArray count / objectAtIndex: in range.
            let count: usize = unsafe { msg_send![array, count] };
            for i in 0..count {
                let cookie: *mut AnyObject = unsafe { msg_send![array, objectAtIndex: i] };
                // SAFETY: an element of the array, alive with it.
                if let Some(cookie) = unsafe { cookie.as_ref() } {
                    out.push(cookie_json(cookie));
                }
            }
        }
        if let Some(f) = done
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            f(out);
        }
    });
    // SAFETY: WebKit's default data store and its cookie store, on the main
    // thread; the block is copied by WebKit.
    unsafe {
        let store: *mut AnyObject = msg_send![class!(WKWebsiteDataStore), defaultDataStore];
        let Some(store) = store.as_ref() else { return };
        let cookies: *mut AnyObject = msg_send![store, httpCookieStore];
        let Some(cookies) = cookies.as_ref() else {
            return;
        };
        let () = msg_send![cookies, getAllCookies: &*block];
    }
}
