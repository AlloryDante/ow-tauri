//! Invisible lab (macOS) and the native helpers the items use.
//!
//! The app never shows and is never frontmost:
//! - `-[NSApplication activate]` / `activateIgnoringOtherApps:` do nothing
//!   (wry calls them when it creates webviews), and
//!   `-[NSWindow makeKeyAndOrderFront:]` becomes `orderFrontRegardless`;
//! - every window is alpha 0, has no shadow and ignores mouse events at the
//!   window server; activation policy Accessory (no Dock icon).
//!
//! Input never leaves this process: events are built here and handed to this
//! app's own `-[NSApplication sendEvent:]`. Because the app is never active,
//! this process alone is told which window is key (`isKeyWindow`,
//! `keyWindow`) and that the app is active (`isActive`). `run-mac.sh` proves
//! `everVisible:false` (window server) and `everFront:false` (Launch Services).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::{NSPoint, NSRect, NSString};
use serde_json::{Value, json};

/// The window this process reports as key (`NSWindow*`), 0 = none.
pub static KEY: AtomicUsize = AtomicUsize::new(0);
/// Whether the in-process key-window answer is on (gesture item turns it off
/// for the "window not key" case).
pub static FAKE_KEY_ON: AtomicBool = AtomicBool::new(true);
static ACTIVATIONS_SUPPRESSED: AtomicUsize = AtomicUsize::new(0);

static ACTIVATE: OnceLock<usize> = OnceLock::new();
static ACTIVATE_IGNORING: OnceLock<usize> = OnceLock::new();
static KEY_FRONT: OnceLock<usize> = OnceLock::new();
static IS_KEY: OnceLock<usize> = OnceLock::new();
static KEY_WINDOW: OnceLock<usize> = OnceLock::new();
static IS_ACTIVE: OnceLock<usize> = OnceLock::new();

extern "C-unwind" fn no_activate(_this: &AnyObject, _cmd: Sel) {
    ACTIVATIONS_SUPPRESSED.fetch_add(1, Ordering::SeqCst);
}
extern "C-unwind" fn no_activate_ignoring(_this: &AnyObject, _cmd: Sel, _flag: Bool) {
    ACTIVATIONS_SUPPRESSED.fetch_add(1, Ordering::SeqCst);
}
extern "C-unwind" fn key_front_regardless(this: &AnyObject, _cmd: Sel, _sender: *mut AnyObject) {
    // Never order a window in unless it is already invisible.
    unsafe {
        let () = msg_send![this, setAlphaValue: 0.0_f64];
        let () = msg_send![this, setIgnoresMouseEvents: true];
        let () = msg_send![this, orderFrontRegardless];
    }
}
extern "C-unwind" fn is_key_window(this: &AnyObject, cmd: Sel) -> Bool {
    let key = KEY.load(Ordering::SeqCst);
    if key != 0 && std::ptr::from_ref(this) as usize == key {
        return Bool::new(FAKE_KEY_ON.load(Ordering::SeqCst));
    }
    let imp = *IS_KEY.get().expect("original isKeyWindow");
    unsafe { std::mem::transmute::<usize, extern "C-unwind" fn(&AnyObject, Sel) -> Bool>(imp)(this, cmd) }
}
extern "C-unwind" fn key_window(this: &AnyObject, cmd: Sel) -> *mut AnyObject {
    let key = KEY.load(Ordering::SeqCst);
    if key != 0 && FAKE_KEY_ON.load(Ordering::SeqCst) {
        return key as *mut AnyObject;
    }
    let imp = *KEY_WINDOW.get().expect("original keyWindow");
    unsafe { std::mem::transmute::<usize, extern "C-unwind" fn(&AnyObject, Sel) -> *mut AnyObject>(imp)(this, cmd) }
}
extern "C-unwind" fn is_active(_this: &AnyObject, _cmd: Sel) -> Bool {
    Bool::YES
}

fn replace(class: &AnyClass, selector: Sel, slot: &OnceLock<usize>, imp: Imp) {
    let Some(method) = class.instance_method(selector) else {
        panic!("no method {selector:?}");
    };
    let _ = slot.set(method.implementation() as usize);
    unsafe {
        let types = objc2::ffi::method_getTypeEncoding(method);
        objc2::ffi::class_replaceMethod(std::ptr::from_ref(class).cast_mut(), selector, imp, types);
    }
}

/// Installs the background guards. Call before the app builds any window.
pub fn hold_app_back() {
    unsafe {
        let app = class!(NSApplication);
        replace(
            app,
            sel!(activate),
            &ACTIVATE,
            std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel), Imp>(no_activate),
        );
        replace(
            app,
            sel!(activateIgnoringOtherApps:),
            &ACTIVATE_IGNORING,
            std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, Bool), Imp>(no_activate_ignoring),
        );
        let window = class!(NSWindow);
        replace(
            window,
            sel!(makeKeyAndOrderFront:),
            &KEY_FRONT,
            std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject), Imp>(key_front_regardless),
        );
        replace(
            window,
            sel!(isKeyWindow),
            &IS_KEY,
            std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel) -> Bool, Imp>(is_key_window),
        );
        replace(
            app,
            sel!(keyWindow),
            &KEY_WINDOW,
            std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel) -> *mut AnyObject, Imp>(key_window),
        );
        replace(
            app,
            sel!(isActive),
            &IS_ACTIVE,
            std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel) -> Bool, Imp>(is_active),
        );
    }
}

pub fn activations_suppressed() -> usize {
    ACTIVATIONS_SUPPRESSED.load(Ordering::SeqCst)
}

/// Alpha 0, no shadow, click-through at the window server; on screen without
/// key or activation (`order_front`), or kept off screen. Main thread.
pub fn make_invisible(ns_window: usize, order_front: bool) {
    let w = obj(ns_window);
    unsafe {
        let () = msg_send![w, setAlphaValue: 0.0_f64];
        let () = msg_send![w, setIgnoresMouseEvents: true];
        let () = msg_send![w, setHasShadow: false];
        if order_front {
            let () = msg_send![w, orderFrontRegardless];
        }
    }
}

pub fn obj<'a>(addr: usize) -> &'a AnyObject {
    unsafe { &*(addr as *const AnyObject) }
}

pub fn class_name(o: *mut AnyObject) -> Value {
    if o.is_null() {
        return Value::Null;
    }
    Value::String(unsafe { (*o).class().name().to_string_lossy().into_owned() })
}

pub fn ns_str(o: *mut AnyObject) -> Option<String> {
    if o.is_null() {
        return None;
    }
    Some(unsafe { (*(o as *const NSString)).to_string() })
}

pub fn responds(o: &AnyObject, s: Sel) -> bool {
    let r: Bool = unsafe { msg_send![o, respondsToSelector: s] };
    r.as_bool()
}

pub fn retain(addr: usize) -> usize {
    let r: *mut AnyObject = unsafe { msg_send![obj(addr), retain] };
    r as usize
}

pub fn uptime() -> f64 {
    unsafe {
        let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
        msg_send![info, systemUptime]
    }
}

// ---------------------------------------------------------------- UA (A1)

/// Guarded KVC: `valueForKey:@"userAgent"` only when the view answers
/// `_userAgent` (KVC's `_<key>` accessor), so a future WebKit without it
/// gives `None` instead of `NSUnknownKeyException`.
pub fn kvc_user_agent(wk: usize) -> Value {
    let o = obj(wk);
    let has = responds(o, sel!(_userAgent));
    let value = if has {
        let key = NSString::from_str("userAgent");
        let v: *mut AnyObject = unsafe { msg_send![o, valueForKey: &*key] };
        ns_str(v)
    } else {
        None
    };
    json!({ "respondsToUnderscoreUserAgent": has, "value": value })
}

pub fn custom_user_agent(wk: usize) -> Option<String> {
    let v: *mut AnyObject = unsafe { msg_send![obj(wk), customUserAgent] };
    ns_str(v)
}

pub fn application_name_for_user_agent(wk: usize) -> Option<String> {
    unsafe {
        let conf: *mut AnyObject = msg_send![obj(wk), configuration];
        if conf.is_null() {
            return None;
        }
        let v: *mut AnyObject = msg_send![conf, applicationNameForUserAgent];
        ns_str(v)
    }
}

// ------------------------------------------------- evaluateJavaScript (A4/A5/A6)

/// `-[WKWebView evaluateJavaScript:completionHandler:]` with the NSError
/// kept (wry's own callback drops it). Main thread; the answer is sent on
/// `tx` from the completion handler: `{ok, value | domain, code, message}`.
pub fn eval_native(wk: usize, js: &str, tx: Sender<Value>) {
    let started = uptime();
    let block = RcBlock::new(move |val: *mut AnyObject, err: *mut AnyObject| {
        let ms = (uptime() - started) * 1000.0;
        let v = if err.is_null() {
            let d: *mut AnyObject = if val.is_null() {
                std::ptr::null_mut()
            } else {
                unsafe { msg_send![val, description] }
            };
            json!({ "ok": true, "value": ns_str(d), "ms": ms })
        } else {
            unsafe {
                let domain: *mut AnyObject = msg_send![err, domain];
                let code: isize = msg_send![err, code];
                let message: *mut AnyObject = msg_send![err, localizedDescription];
                json!({ "ok": false, "domain": ns_str(domain), "code": code, "message": ns_str(message), "ms": ms })
            }
        };
        let _ = tx.send(v);
    });
    let js = NSString::from_str(js);
    unsafe {
        let () = msg_send![obj(wk), evaluateJavaScript: &*js, completionHandler: &*block];
    }
}

/// The view's WebContent pid through WebKit SPI (`_webProcessIdentifier`),
/// guarded. Lab measurement only; the plugin design does not use it.
pub fn web_process_id(wk: usize) -> Option<i32> {
    let o = obj(wk);
    if !responds(o, sel!(_webProcessIdentifier)) {
        return None;
    }
    let pid: i32 = unsafe { msg_send![o, _webProcessIdentifier] };
    Some(pid)
}

// --------------------------------------------------------------- zoom (A3)

pub fn page_zoom(wk: usize) -> f64 {
    unsafe { msg_send![obj(wk), pageZoom] }
}

/// The view's frame in window coordinates (points, bottom-left origin).
pub fn frame_in_window(view: usize) -> NSRect {
    unsafe {
        let bounds: NSRect = msg_send![obj(view), bounds];
        msg_send![obj(view), convertRect: bounds, toView: std::ptr::null_mut::<AnyObject>()]
    }
}

pub fn backing_scale(ns_window: usize) -> f64 {
    unsafe { msg_send![obj(ns_window), backingScaleFactor] }
}

pub fn occlusion_visible(ns_window: usize) -> bool {
    let s: usize = unsafe { msg_send![obj(ns_window), occlusionState] };
    s & (1 << 1) != 0
}

// ------------------------------------------------------------ gesture (A2)

pub fn is_view_or_descendant(view: *mut AnyObject, ancestor: usize) -> bool {
    if view.is_null() {
        return false;
    }
    if view as usize == ancestor {
        return true;
    }
    let is_view: Bool = unsafe { msg_send![view, isKindOfClass: class!(NSView)] };
    if !is_view.as_bool() {
        return false;
    }
    let d: Bool = unsafe { msg_send![view, isDescendantOf: obj(ancestor)] };
    d.as_bool()
}

/// `-[NSEvent addLocalMonitorForEventsMatchingMask:handler:]` for
/// left-mouse-down and key-down; `on` sees each event first and the event
/// continues unchanged.
pub fn add_local_monitor(on: impl Fn(&AnyObject) + 'static) -> usize {
    const LEFT_MOUSE_DOWN: u64 = 1 << 1;
    const KEY_DOWN: u64 = 1 << 10;
    let block = RcBlock::new(move |ev: *mut AnyObject| -> *mut AnyObject {
        if !ev.is_null() {
            on(unsafe { &*ev });
        }
        ev
    });
    let monitor: *mut AnyObject =
        unsafe { msg_send![class!(NSEvent), addLocalMonitorForEventsMatchingMask: LEFT_MOUSE_DOWN | KEY_DOWN, handler: &*block] };
    let _ = retain(monitor as usize);
    monitor as usize
}

/// A mouse event at `at` (window coordinates) for window `ns_window`.
pub fn mouse_event(ns_window: usize, ty: usize, at: NSPoint) -> Option<Retained<AnyObject>> {
    let number: isize = unsafe { msg_send![obj(ns_window), windowNumber] };
    unsafe {
        msg_send![class!(NSEvent),
            mouseEventWithType: ty,
            location: at,
            modifierFlags: 0_usize,
            timestamp: uptime(),
            windowNumber: number,
            context: std::ptr::null_mut::<AnyObject>(),
            eventNumber: 0_isize,
            clickCount: 1_isize,
            pressure: 1.0_f32]
    }
}

/// `-[NSApplication sendEvent:]` of this app (the stock dispatch path that
/// runs local monitors).
pub fn app_send(ev: &AnyObject) {
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let () = msg_send![app, sendEvent: ev];
    }
}

pub fn window_send(ns_window: usize, ev: &AnyObject) {
    let () = unsafe { msg_send![obj(ns_window), sendEvent: ev] };
}

// ------------------------------------------------------ pass-through (A2)

static PASSTHROUGH_KEY: u8 = 0;
static ORIGINAL_HIT_TEST: OnceLock<(usize, usize)> = OnceLock::new();
type HitTest = unsafe extern "C-unwind" fn(&AnyObject, Sel, NSPoint) -> *mut AnyObject;

extern "C-unwind" fn passthrough_hit_test(this: &AnyObject, cmd: Sel, point: NSPoint) -> *mut AnyObject {
    let flag = unsafe { objc2::ffi::objc_getAssociatedObject(this, (&raw const PASSTHROUGH_KEY).cast()) };
    if !flag.is_null() {
        return std::ptr::null_mut();
    }
    match ORIGINAL_HIT_TEST.get() {
        Some(&(_, imp)) => unsafe { std::mem::transmute::<usize, HitTest>(imp)(this, cmd, point) },
        None => std::ptr::null_mut(),
    }
}

pub fn passthrough_on(wk: usize) -> bool {
    !unsafe { objc2::ffi::objc_getAssociatedObject(obj(wk), (&raw const PASSTHROUGH_KEY).cast()) }.is_null()
}

/// The plugin's pass-through mechanism (`platform/webview.rs`
/// `set_input_passthrough`): the web view class's `hitTest:` answers `nil`
/// while a per-view flag is set. Main thread.
pub fn set_input_passthrough(wk: usize, on: bool) {
    let o = obj(wk);
    let class = o.class();
    if ORIGINAL_HIT_TEST.get().is_none() {
        let method = class.instance_method(sel!(hitTest:)).expect("hitTest:");
        let _ = ORIGINAL_HIT_TEST.set((std::ptr::from_ref(class) as usize, method.implementation() as usize));
        unsafe {
            let types = objc2::ffi::method_getTypeEncoding(method);
            let replacement: HitTest = passthrough_hit_test;
            objc2::ffi::class_replaceMethod(
                std::ptr::from_ref(class).cast_mut(),
                sel!(hitTest:),
                std::mem::transmute::<HitTest, Imp>(replacement),
                types,
            );
        }
    }
    let value: *mut AnyObject = if on { wk as *mut AnyObject } else { std::ptr::null_mut() };
    unsafe {
        objc2::ffi::objc_setAssociatedObject(
            wk as *mut AnyObject,
            (&raw const PASSTHROUGH_KEY).cast(),
            value,
            objc2::ffi::OBJC_ASSOCIATION_ASSIGN,
        );
    }
}

// ----------------------------------------------------- user scripts (A6)

/// Number of `WKUserScript`s in a `WKUserContentController`.
pub fn user_script_count(controller: usize) -> usize {
    unsafe {
        let scripts: *mut AnyObject = msg_send![obj(controller), userScripts];
        msg_send![scripts, count]
    }
}

/// Removes every user script whose source contains `marker` and re-adds the
/// others in order (`removeAllUserScripts` is the only public removal API).
/// Returns (before, removed, after).
pub fn remove_user_scripts_containing(controller: usize, marker: &str) -> (usize, usize, usize) {
    unsafe {
        let c = obj(controller);
        let scripts: *mut AnyObject = msg_send![c, userScripts];
        // +0 snapshot that keeps the script objects alive past removeAllUserScripts.
        let copy: *mut AnyObject = msg_send![objc2::class!(NSArray), arrayWithArray: scripts];
        let n: usize = msg_send![copy, count];
        let mut keep: Vec<*mut AnyObject> = Vec::new();
        let mut removed = 0;
        for i in 0..n {
            let s: *mut AnyObject = msg_send![copy, objectAtIndex: i];
            let src: *mut AnyObject = msg_send![s, source];
            if ns_str(src).is_some_and(|t| t.contains(marker)) {
                removed += 1;
            } else {
                keep.push(s);
            }
        }
        let () = msg_send![c, removeAllUserScripts];
        for s in &keep {
            let () = msg_send![c, addUserScript: *s];
        }
        let after = user_script_count(controller);
        (n, removed, after)
    }
}

// ------------------------------------------------- main dispatch queue (A4)

unsafe extern "C" {
    static _dispatch_main_q: u8;
    fn dispatch_async_f(queue: *const u8, context: *mut std::ffi::c_void, work: extern "C" fn(*mut std::ffi::c_void));
}

extern "C" fn run_boxed(ctx: *mut std::ffi::c_void) {
    let f: Box<Box<dyn FnOnce()>> = unsafe { Box::from_raw(ctx.cast()) };
    f();
}

/// `dispatch_async_f(dispatch_get_main_queue(), …)`: runs `f` on the main
/// thread after the current run-loop work, without a thread hop.
pub fn dispatch_main(f: Box<dyn FnOnce()>) {
    let ctx = Box::into_raw(Box::new(f)).cast();
    unsafe { dispatch_async_f(&raw const _dispatch_main_q, ctx, run_boxed) };
}
