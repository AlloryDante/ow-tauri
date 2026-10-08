//! macOS driver. The app stays invisible and in the background for the
//! whole run:
//!
//! - `-[NSApplication activate]` / `activateIgnoringOtherApps:` do nothing
//!   (wry calls them for every webview it creates), and
//!   `-[NSWindow makeKeyAndOrderFront:]` becomes `orderFrontRegardless`;
//! - the window is alpha 0 and ignores mouse events at the window server;
//! - activation policy Accessory (no Dock icon, no app switcher entry).
//!
//! Input never leaves this process: the driver builds `NSEvent`s and hands
//! them to this app's own `NSApplication` (`SPIKE_DELIVERY=app`, default:
//! the real dispatch path, `-[NSApplication sendEvent:]` → key window →
//! first responder) or to the window (`SPIKE_DELIVERY=window`). Because the
//! app is never active, AppKit would not treat the window as key, so with
//! `SPIKE_FAKE_KEY=1` (default) this process alone is told it is:
//! `-[NSWindow isKeyWindow]` answers YES for the spike window,
//! `-[NSApplication keyWindow]` answers the spike window, and the window gets
//! `becomeKeyWindow` once. The window server never sees a key window or an
//! active app (the run script proves it: never frontmost, never visible).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, mpsc};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::{NSPoint, NSRect, NSString};
use serde_json::{Value, json};
use tauri::{AppHandle, WebviewWindow};

use crate::Key;

/// The spike window's `NSWindow*`.
static MAIN: AtomicUsize = AtomicUsize::new(0);
static ACTIVATIONS_SUPPRESSED: AtomicUsize = AtomicUsize::new(0);

static ACTIVATE: OnceLock<usize> = OnceLock::new();
static ACTIVATE_IGNORING: OnceLock<usize> = OnceLock::new();
static KEY_FRONT: OnceLock<usize> = OnceLock::new();
static IS_KEY: OnceLock<usize> = OnceLock::new();
static KEY_WINDOW: OnceLock<usize> = OnceLock::new();
static IS_ACTIVE: OnceLock<usize> = OnceLock::new();

/// Diagnostics: keys that reached the webview's superview (`TaoView` in
/// child mode, `WryWebViewParent` in content mode) because WebKit did not
/// handle them, and text the superview's `insertText:` received.
static SUPER_KEYDOWNS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static TAO_KEY_DOWN: OnceLock<usize> = OnceLock::new();
static TAO_INSERT: OnceLock<usize> = OnceLock::new();
static PARENT_KEY_DOWN: OnceLock<usize> = OnceLock::new();
static WIN_SEND: OnceLock<usize> = OnceLock::new();
static APP_SEND: OnceLock<usize> = OnceLock::new();
static APP_ROUTED: AtomicUsize = AtomicUsize::new(0);

/// `SPIKE_DELIVERY=app` (default): `-[NSApplication sendEvent:]` routes key
/// events as AppKit does for the active app's key window, which the
/// never-active spike app does not have at the window-server level:
/// keyDown → `-[NSWindow performKeyEquivalent:]` of the key window, then the
/// main menu's, then `-[NSWindow sendEvent:]` of the key window; keyUp and
/// flagsChanged → `-[NSWindow sendEvent:]`. Every caller goes through it,
/// including WebKit when it re-sends a key event the page did not handle
/// (`WebViewImpl::doneWithKeyEvent` → `[NSApp sendEvent:]`), which is the
/// path the unstable-only bugs are reported on.
extern "C-unwind" fn app_send(this: &AnyObject, cmd: Sel, event: &AnyObject) {
    let ty: usize = unsafe { msg_send![event, type] };
    let main = MAIN.load(Ordering::SeqCst);
    if main != 0 && matches!(ty, 10 | 11 | 12) {
        APP_ROUTED.fetch_add(1, Ordering::SeqCst);
        let win = obj(main);
        unsafe {
            if ty == 10 {
                let handled: Bool = msg_send![win, performKeyEquivalent: event];
                if handled.as_bool() {
                    note("NSApp: key window performKeyEquivalent: consumed", event);
                    return;
                }
                let menu: *mut AnyObject = msg_send![this, mainMenu];
                if !menu.is_null() {
                    let handled: Bool = msg_send![menu, performKeyEquivalent: event];
                    if handled.as_bool() {
                        note("NSApp: main menu consumed", event);
                        return;
                    }
                }
            }
            let () = msg_send![win, sendEvent: event];
        }
        return;
    }
    let imp = *APP_SEND.get().expect("app sendEvent");
    unsafe { std::mem::transmute::<usize, extern "C-unwind" fn(&AnyObject, Sel, &AnyObject)>(imp)(this, cmd, event) }
}
static WV_KEY_DOWN: OnceLock<usize> = OnceLock::new();

extern "C-unwind" fn win_send(this: &AnyObject, cmd: Sel, event: &AnyObject) {
    let ty: usize = unsafe { msg_send![event, type] };
    if ty == 10 {
        note("window sendEvent: keyDown", event);
    }
    let imp = *WIN_SEND.get().expect("window sendEvent");
    unsafe { std::mem::transmute::<usize, extern "C-unwind" fn(&AnyObject, Sel, &AnyObject)>(imp)(this, cmd, event) }
}
extern "C-unwind" fn wv_key_down(this: &AnyObject, cmd: Sel, event: &AnyObject) {
    note("WryWebView keyDown:", event);
    let imp = *WV_KEY_DOWN.get().expect("webview keyDown");
    unsafe { std::mem::transmute::<usize, extern "C-unwind" fn(&AnyObject, Sel, &AnyObject)>(imp)(this, cmd, event) }
}

fn note(kind: &str, event: &AnyObject) {
    let code: u16 = unsafe { msg_send![event, keyCode] };
    if let Ok(mut v) = SUPER_KEYDOWNS.lock() {
        v.push(format!("{kind} keyCode={code}"));
    }
}
/// `SPIKE_MITIGATE=native`: a key that bubbled up to tao's content view
/// from a focused webview (WebKit re-sends keys the page did not handle) is
/// handled as wry's `WryWebViewParent` handles it in a stable build: offered
/// to the main menu as a key equivalent, never to `interpretKeyEvents:`.
fn mitigate_native() -> bool {
    std::env::var("SPIKE_MITIGATE").is_ok_and(|v| v == "native")
}

extern "C-unwind" fn tao_key_down(this: &AnyObject, cmd: Sel, event: &AnyObject) {
    note("TaoView keyDown:", event);
    if mitigate_native() {
        unsafe {
            let win: *mut AnyObject = msg_send![this, window];
            let fr: *mut AnyObject = if win.is_null() { std::ptr::null_mut() } else { msg_send![win, firstResponder] };
            let from_webview: bool = !fr.is_null() && {
                let is: Bool = msg_send![fr, isKindOfClass: class!(WKWebView)];
                is.as_bool()
            };
            if from_webview {
                note("TaoView keyDown: swallowed (mitigation)", event);
                let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
                let menu: *mut AnyObject = msg_send![app, mainMenu];
                if !menu.is_null() {
                    let _: Bool = msg_send![menu, performKeyEquivalent: event];
                }
                return;
            }
        }
    }
    let imp = *TAO_KEY_DOWN.get().expect("tao keyDown");
    unsafe { std::mem::transmute::<usize, extern "C-unwind" fn(&AnyObject, Sel, &AnyObject)>(imp)(this, cmd, event) }
}
extern "C-unwind" fn parent_key_down(this: &AnyObject, cmd: Sel, event: &AnyObject) {
    note("WryWebViewParent keyDown:", event);
    let imp = *PARENT_KEY_DOWN.get().expect("parent keyDown");
    unsafe { std::mem::transmute::<usize, extern "C-unwind" fn(&AnyObject, Sel, &AnyObject)>(imp)(this, cmd, event) }
}
extern "C-unwind" fn tao_insert(this: &AnyObject, cmd: Sel, text: &AnyObject, range: objc2_foundation::NSRange) {
    let s: *mut AnyObject = unsafe { msg_send![text, description] };
    let s = if s.is_null() { String::new() } else { unsafe { (*(s as *const NSString)).to_string() } };
    if let Ok(mut v) = SUPER_KEYDOWNS.lock() {
        v.push(format!("TaoView insertText: {:?}", s.chars().map(|c| format!("U+{:04X}", c as u32)).collect::<Vec<_>>()));
    }
    let imp = *TAO_INSERT.get().expect("tao insertText");
    unsafe {
        std::mem::transmute::<usize, extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, objc2_foundation::NSRange)>(imp)(this, cmd, text, range)
    }
}

/// Wraps the superviews' `keyDown:` (and TaoView's `insertText:`) to log
/// what reaches them. Once, on the main thread, after the views exist.
fn install_diagnostics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        for (name, cell) in [("TaoView", &TAO_KEY_DOWN)] {
            if let Some(c) = AnyClass::get(&std::ffi::CString::new(name).unwrap()) {
                replace(c, sel!(keyDown:), cell, std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, &AnyObject), Imp>(tao_key_down));
                replace(c, sel!(insertText:replacementRange:), &TAO_INSERT, std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, objc2_foundation::NSRange), Imp>(tao_insert));
            }
        }
        if deliver_to_app() {
            if let Some(c) = AnyClass::get(c"TaoApp") {
                replace(c, sel!(sendEvent:), &APP_SEND, std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, &AnyObject), Imp>(app_send));
            }
        }
        if std::env::var("SPIKE_TRACE_ALL").is_ok_and(|v| v == "1") {
            if let Some(c) = AnyClass::get(c"TaoWindow") {
                replace(c, sel!(sendEvent:), &WIN_SEND, std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, &AnyObject), Imp>(win_send));
            }
            if let Some(c) = AnyClass::get(c"WKWebView") {
                replace(c, sel!(keyDown:), &WV_KEY_DOWN, std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, &AnyObject), Imp>(wv_key_down));
            }
        }
        if let Some(c) = AnyClass::get(c"wry::wkwebview::class::wry_web_view_parent::WryWebViewParent0.57.0") {
            replace(c, sel!(keyDown:), &PARENT_KEY_DOWN, std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, &AnyObject), Imp>(parent_key_down));
        }
    });
}

/// `SPIKE_FAKE_ACTIVE` (default on; `0` turns it off): `-[NSApplication isActive]` answers YES in this
/// process (the text input system keys its current input context off it).
fn fake_active() -> bool {
    std::env::var("SPIKE_FAKE_ACTIVE").map(|v| v != "0").unwrap_or(true)
}

extern "C-unwind" fn is_active(_this: &AnyObject, _cmd: Sel) -> Bool {
    Bool::YES
}

fn fake_key() -> bool {
    std::env::var("SPIKE_FAKE_KEY").map(|v| v != "0").unwrap_or(true)
}

fn activate_input_context() -> bool {
    std::env::var("SPIKE_ACTIVATE_INPUT_CONTEXT").map(|v| v != "0").unwrap_or(true)
}

/// Non-character keys (arrows, Delete, Return, Escape, F5) keep the
/// `CGEvent` string the keyboard layout computes for the key code, as a
/// real key press does (Left arrow: U+001C in the CGEvent, U+F702 in the
/// NSEvent's `characters`). `SPIKE_ARROW_STRINGS=function-keys` overrides
/// the CGEvent string with the NSEvent function-key character instead.
fn layout_strings() -> bool {
    std::env::var("SPIKE_ARROW_STRINGS").map(|v| v != "function-keys").unwrap_or(true)
}

fn deliver_to_app() -> bool {
    std::env::var("SPIKE_DELIVERY").map(|v| v != "window").unwrap_or(true)
}

extern "C-unwind" fn no_activate(_this: &AnyObject, _cmd: Sel) {
    ACTIVATIONS_SUPPRESSED.fetch_add(1, Ordering::SeqCst);
}
extern "C-unwind" fn no_activate_ignoring(_this: &AnyObject, _cmd: Sel, _flag: Bool) {
    ACTIVATIONS_SUPPRESSED.fetch_add(1, Ordering::SeqCst);
}
extern "C-unwind" fn key_front_regardless(this: &AnyObject, _cmd: Sel, _sender: *mut AnyObject) {
    let () = unsafe { msg_send![this, orderFrontRegardless] };
}
extern "C-unwind" fn is_key_window(this: &AnyObject, cmd: Sel) -> Bool {
    if std::ptr::from_ref(this) as usize == MAIN.load(Ordering::SeqCst) {
        return Bool::YES;
    }
    let imp = *IS_KEY.get().expect("original isKeyWindow");
    unsafe { std::mem::transmute::<usize, extern "C-unwind" fn(&AnyObject, Sel) -> Bool>(imp)(this, cmd) }
}
extern "C-unwind" fn key_window(this: &AnyObject, cmd: Sel) -> *mut AnyObject {
    let main = MAIN.load(Ordering::SeqCst);
    if main != 0 {
        return main as *mut AnyObject;
    }
    let imp = *KEY_WINDOW.get().expect("original keyWindow");
    unsafe {
        std::mem::transmute::<usize, extern "C-unwind" fn(&AnyObject, Sel) -> *mut AnyObject>(imp)(this, cmd)
    }
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

/// Installs the background guards (and the in-process key-window answer).
/// Call before the app builds any window.
pub fn hold_app_back() {
    unsafe {
        let app = class!(NSApplication);
        replace(app, sel!(activate), &ACTIVATE, std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel), Imp>(no_activate));
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
        if fake_key() {
            replace(window, sel!(isKeyWindow), &IS_KEY, std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel) -> Bool, Imp>(is_key_window));
            replace(app, sel!(keyWindow), &KEY_WINDOW, std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel) -> *mut AnyObject, Imp>(key_window));
        }
        if fake_active() {
            replace(app, sel!(isActive), &IS_ACTIVE, std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel) -> Bool, Imp>(is_active));
        }
    }
}

/// Alpha 0, click-through at the window server, on screen without key or
/// activation. Runs in `setup` (main thread).
pub fn make_invisible(main: &WebviewWindow) {
    let Ok(ptr) = main.ns_window() else { return };
    let w = ptr as usize;
    let obj = obj(w);
    unsafe {
        let () = msg_send![obj, setAlphaValue: 0.0_f64];
        let () = msg_send![obj, setIgnoresMouseEvents: true];
        let () = msg_send![obj, setHasShadow: false];
        let () = msg_send![obj, orderFrontRegardless];
    }
    MAIN.store(w, Ordering::SeqCst);
}

fn obj<'a>(addr: usize) -> &'a AnyObject {
    unsafe { &*(addr as *const AnyObject) }
}

fn class_name(o: *mut AnyObject) -> Value {
    if o.is_null() {
        return Value::Null;
    }
    Value::String(unsafe { (*o).class().name().to_string_lossy().into_owned() })
}

fn uptime() -> f64 {
    unsafe {
        let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
        msg_send![info, systemUptime]
    }
}

/// Every view under `view`, depth first, as (class name, address).
fn views(view: *mut AnyObject, out: &mut Vec<(String, usize)>) {
    if view.is_null() {
        return;
    }
    let name = unsafe { (*view).class().name().to_string_lossy().into_owned() };
    out.push((name, view as usize));
    unsafe {
        let subs: *mut AnyObject = msg_send![view, subviews];
        let n: usize = msg_send![subs, count];
        for i in 0..n {
            let v: *mut AnyObject = msg_send![subs, objectAtIndex: i];
            views(v, out);
        }
    }
}

pub struct Driver {
    app: AppHandle,
}

impl Driver {
    pub fn new(app: &AppHandle, _main: &WebviewWindow) -> Self {
        let d = Self { app: app.clone() };
        d.on_main(install_diagnostics);
        if fake_key() {
            d.on_main(|| unsafe {
                let w = obj(MAIN.load(Ordering::SeqCst));
                let () = msg_send![w, becomeKeyWindow];
            });
        }
        d
    }

    fn on_main<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = mpsc::channel();
        self.app
            .run_on_main_thread(move || {
                let _ = tx.send(f());
            })
            .expect("run on main thread");
        rx.recv().expect("main thread answer")
    }

    fn snapshot() -> Value {
        let w = MAIN.load(Ordering::SeqCst);
        let win = obj(w);
        unsafe {
            let fr: *mut AnyObject = msg_send![win, firstResponder];
            let ctx: *mut AnyObject = msg_send![class!(NSTextInputContext), currentInputContext];
            let client: *mut AnyObject = if ctx.is_null() { std::ptr::null_mut() } else { msg_send![ctx, client] };
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let active: Bool = msg_send![app, isActive];
            let is_key: Bool = msg_send![win, isKeyWindow];
            let cv: *mut AnyObject = msg_send![win, contentView];
            let mut tree = Vec::new();
            views(cv, &mut tree);
            json!({
                "firstResponder": class_name(fr),
                "currentInputContextClient": class_name(client),
                "appIsActive(real)": active.as_bool(),
                "isKeyWindow(in-process)": is_key.as_bool(),
                "viewTree": tree.iter().take(4).map(|(n, _)| n.clone()).collect::<Vec<_>>(),
            })
        }
    }

    pub fn after_keys(&self) -> Value {
        self.on_main(|| unsafe {
            let win = obj(MAIN.load(Ordering::SeqCst));
            let fr: *mut AnyObject = msg_send![win, firstResponder];
            let cur: *mut AnyObject = msg_send![class!(NSTextInputContext), currentInputContext];
            let client: *mut AnyObject = if cur.is_null() { std::ptr::null_mut() } else { msg_send![cur, client] };
            let reached: Vec<String> = SUPER_KEYDOWNS.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
            json!({ "firstResponder": class_name(fr), "currentInputContextClient": class_name(client), "reachedSuperview": reached })
        })
    }

    pub fn environment(&self) -> Value {
        let snap = self.on_main(Self::snapshot);
        json!({
            "delivery": if deliver_to_app() { "NSApplication sendEvent:" } else { "NSWindow sendEvent:" },
            "fakeKey": fake_key(),
            "activateInputContext": activate_input_context(),
            "fakeActive": fake_active(),
            "diagnostics": { "taoKeyDown": TAO_KEY_DOWN.get().is_some(), "taoInsert": TAO_INSERT.get().is_some(), "parentKeyDown": PARENT_KEY_DOWN.get().is_some(), "windowSendEvent": WIN_SEND.get().is_some(), "webviewKeyDown": WV_KEY_DOWN.get().is_some(), "appRouting": APP_SEND.get().is_some(), "appRouted": APP_ROUTED.load(Ordering::SeqCst) },
            "mitigation": std::env::var("SPIKE_MITIGATE").unwrap_or_default(),
            "eventSource": if cg_events() { "CGEvent (private source) -> NSEvent" } else { "NSEvent keyEventWithType" },
            "activationsSuppressed": ACTIVATIONS_SUPPRESSED.load(Ordering::SeqCst),
            "snapshot": snap,
        })
    }

    /// A left click at the page point `prep.{x,y}` (CSS px of the main
    /// webview, which starts at the window's top-left), sent to the window.
    pub fn click(&self, prep: &Value) -> Value {
        let x = prep["x"].as_f64().unwrap_or(10.0);
        let y = prep["y"].as_f64().unwrap_or(10.0);
        let inner_h = prep["inner"][1].as_f64().unwrap_or(568.0);
        let activate_ctx = activate_input_context();
        self.on_main(move || unsafe {
            let w = MAIN.load(Ordering::SeqCst);
            let win = obj(w);
            let cv: *mut AnyObject = msg_send![win, contentView];
            // The main webview (the first one in the tree) in window
            // coordinates (bottom-left origin). Its page can start below
            // the view's top edge (the window's content inset), so page y
            // counts from `inner_height` above the view's bottom edge.
            let mut tree = Vec::new();
            views(cv, &mut tree);
            let Some(&(_, wv)) = tree.iter().find(|(n, _)| n.contains("WryWebView")) else {
                return json!({ "error": "no webview" });
            };
            let wv = wv as *mut AnyObject;
            let bounds: NSRect = msg_send![wv, bounds];
            let r: NSRect = msg_send![wv, convertRect: bounds, toView: std::ptr::null_mut::<AnyObject>()];
            let at = NSPoint::new(r.origin.x + x, r.origin.y + inner_h - y);
            let number: isize = msg_send![win, windowNumber];
            for ty in [1_usize, 2] {
                let ev: Option<Retained<AnyObject>> = msg_send![class!(NSEvent),
                    mouseEventWithType: ty,
                    location: at,
                    modifierFlags: 0_usize,
                    timestamp: uptime(),
                    windowNumber: number,
                    context: std::ptr::null_mut::<AnyObject>(),
                    eventNumber: 0_isize,
                    clickCount: 1_isize,
                    pressure: 1.0_f32];
                if let Some(ev) = ev {
                    let () = msg_send![win, sendEvent: &*ev];
                }
            }
            let fr: *mut AnyObject = msg_send![win, firstResponder];
            let after_click = class_name(fr);
            let mut fallback = false;
            let is_webview = after_click.as_str().is_some_and(|n| n.contains("WebView"));
            if !is_webview {
                // A click did not make the webview first responder: do it
                // directly (what a click into the webview does).
                let _: Bool = msg_send![win, makeFirstResponder: wv];
                fallback = true;
            }
            let fr: *mut AnyObject = msg_send![win, firstResponder];
            let mut fr_ctx = Value::Null;
            if activate_ctx && !fr.is_null() {
                // An active app's key window has its first responder's
                // input context active (AppKit does it on activation); the
                // never-active spike app needs it done here, or the text
                // input system (dead keys, IME) never sees the keys.
                let ctx: *mut AnyObject = msg_send![fr, inputContext];
                fr_ctx = class_name(ctx);
                if !ctx.is_null() {
                    let () = msg_send![ctx, activate];
                }
            }
            let cur: *mut AnyObject = msg_send![class!(NSTextInputContext), currentInputContext];
            let client: *mut AnyObject = if cur.is_null() { std::ptr::null_mut() } else { msg_send![cur, client] };
            json!({ "at": [x, y], "windowPoint": [at.x, at.y], "firstResponderAfterClick": after_click, "makeFirstResponderFallback": fallback, "firstResponder": class_name(fr), "currentInputContextClient": class_name(client), "firstResponderInputContext": fr_ctx })
        })
    }

    pub fn key(&self, key: Key) {
        const SHIFT: usize = 1 << 17;
        const OPTION: usize = 1 << 19;
        const ARROW: usize = (1 << 23) | (1 << 21); // function | numericPad
        // (keyCode, characters, charactersIgnoringModifiers, flags)
        let mut events: Vec<(u16, String, String, usize)> = Vec::new();
        match key {
            Key::Ch(c) => {
                let (code, shift) = keycode(c);
                events.push((code, c.to_string(), c.to_string(), if shift { SHIFT } else { 0 }));
            }
            // SPIKE_ARROW_STRINGS=layout keeps the CGEvent string the layout
            // computes for the key code (what the HID system fills in)
            // instead of the NSEvent function-key character.
            Key::Left | Key::Right | Key::Down | Key::Up => {
                let (code, f) = match key {
                    Key::Left => (123, "\u{F702}"),
                    Key::Right => (124, "\u{F703}"),
                    Key::Down => (125, "\u{F701}"),
                    _ => (126, "\u{F700}"),
                };
                let layout = layout_strings();
                events.push((code, if layout { "\u{0}".into() } else { f.into() }, f.into(), ARROW));
            }
            Key::Back => events.push((51, if layout_strings() { "\u{0}".into() } else { "\u{7f}".into() }, "\u{7f}".into(), 0)),
            Key::Enter => events.push((36, if layout_strings() { "\u{0}".into() } else { "\r".into() }, "\r".into(), 0)),
            Key::Escape => events.push((53, if layout_strings() { "\u{0}".into() } else { "\u{1b}".into() }, "\u{1b}".into(), 0)),
            Key::FKey => events.push((96, if layout_strings() { "\u{0}".into() } else { "\u{F708}".into() }, "\u{F708}".into(), 1 << 23)),
            Key::Dead(c) => {
                // ABC layout: Option+<key> is the dead accent, then the base.
                let (accent, base): (char, char) = match c {
                    'é' => ('e', 'e'),
                    'ü' => ('u', 'u'),
                    'ñ' => ('n', 'n'),
                    'à' => ('`', 'a'),
                    'ô' => ('i', 'o'),
                    _ => panic!("no dead-key recipe for {c}"),
                };
                let layout = std::env::var("SPIKE_DEAD_STRINGS").map(|v| v != "empty").unwrap_or(true);
                let (a, b) = if layout { ("\u{0}".to_string(), "\u{0}".to_string()) } else { (String::new(), base.to_string()) };
                events.push((keycode(accent).0, a, accent.to_string(), OPTION));
                events.push((keycode(base).0, b, base.to_string(), 0));
            }
            Key::Uni(_) => {}
        }
        let to_app = deliver_to_app();
        for (code, chars, raw, flags) in events {
            self.on_main(move || unsafe {
                let w = MAIN.load(Ordering::SeqCst);
                let win = obj(w);
                let number: isize = msg_send![win, windowNumber];
                let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
                // Modifier keys as the HID system reports them: a
                // flagsChanged before and after the key (CGEvent path only).
                let modifier = if flags & OPTION_FLAG != 0 { Some((58_u16, OPTION_FLAG)) } else if flags & SHIFT_FLAG != 0 { Some((56_u16, SHIFT_FLAG)) } else { None };
                if cg_events() {
                    if let Some((mcode, mflag)) = modifier {
                        if let Some(ev) = cg_flags_event(mcode, mflag) {
                            if to_app { let () = msg_send![app, sendEvent: &*ev]; } else { let () = msg_send![win, sendEvent: &*ev]; }
                        }
                    }
                }
                for ty in [10_usize, 11] {
                    if cg_events() {
                        let ev = cg_key_event(code, ty == 10, flags, &chars);
                        if let Some(ev) = ev {
                            if to_app {
                                let () = msg_send![app, sendEvent: &*ev];
                            } else {
                                let () = msg_send![win, sendEvent: &*ev];
                            }
                        }
                        continue;
                    }
                    let chars = NSString::from_str(&chars);
                    let raw = NSString::from_str(&raw);
                    let _ = number;
                    let ev: Option<Retained<AnyObject>> = msg_send![class!(NSEvent),
                        keyEventWithType: ty,
                        location: NSPoint::new(0.0, 0.0),
                        modifierFlags: flags,
                        timestamp: uptime(),
                        windowNumber: number,
                        context: std::ptr::null_mut::<AnyObject>(),
                        characters: &*chars,
                        charactersIgnoringModifiers: &*raw,
                        isARepeat: Bool::NO,
                        keyCode: code];
                    if let Some(ev) = ev {
                        if to_app {
                            let () = msg_send![app, sendEvent: &*ev];
                        } else {
                            let () = msg_send![win, sendEvent: &*ev];
                        }
                    }
                }
                if cg_events() && modifier.is_some() {
                    if let Some(ev) = cg_flags_event(modifier.map_or(0, |m| m.0), 0) {
                        if to_app { let () = msg_send![app, sendEvent: &*ev]; } else { let () = msg_send![win, sendEvent: &*ev]; }
                    }
                }
            });
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
    }
}

/// ANSI (ABC layout) virtual key code and whether Shift is needed.
fn keycode(c: char) -> (u16, bool) {
    let lower = c.to_ascii_lowercase();
    let shift = c.is_ascii_uppercase() || "!@#$%^&*()_+{}|:\"<>?~".contains(c);
    let base = match c {
        '!' => '1',
        '@' => '2',
        _ => lower,
    };
    let code = match base {
        'a' => 0, 's' => 1, 'd' => 2, 'f' => 3, 'h' => 4, 'g' => 5, 'z' => 6, 'x' => 7,
        'c' => 8, 'v' => 9, 'b' => 11, 'q' => 12, 'w' => 13, 'e' => 14, 'r' => 15, 'y' => 16,
        't' => 17, '1' => 18, '2' => 19, '3' => 20, '4' => 21, '6' => 22, '5' => 23, '=' => 24,
        '9' => 25, '7' => 26, '-' => 27, '8' => 28, '0' => 29, ']' => 30, 'o' => 31, 'u' => 32,
        '[' => 33, 'i' => 34, 'p' => 35, 'l' => 37, 'j' => 38, '\'' => 39, 'k' => 40, ';' => 41,
        '\\' => 42, ',' => 43, '/' => 44, 'n' => 45, 'm' => 46, '.' => 47, ' ' => 49, '`' => 50,
        _ => panic!("no key code for {c:?}"),
    };
    (code, shift)
}

/// `SPIKE_EVENT_SOURCE=ns` builds key events with `+[NSEvent
/// keyEventWithType:...]`; the default (`cg`) builds a `CGEvent` from a
/// private event source (as the HID system does for a real key press; never
/// posted to the window server) and wraps it with `+[NSEvent eventWithCGEvent:]`.
fn cg_events() -> bool {
    std::env::var("SPIKE_EVENT_SOURCE").map(|v| v != "ns").unwrap_or(true)
}

#[repr(C)]
pub struct CGEvent {
    _private: [u8; 0],
}
unsafe impl objc2::encode::RefEncode for CGEvent {
    const ENCODING_REF: objc2::encode::Encoding =
        objc2::encode::Encoding::Pointer(&objc2::encode::Encoding::Struct("__CGEvent", &[]));
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceCreate(state: i32) -> *mut std::ffi::c_void;
    fn CGEventCreateKeyboardEvent(source: *mut std::ffi::c_void, key: u16, down: bool) -> *mut CGEvent;
    fn CGEventSetFlags(event: *mut CGEvent, flags: u64);
    fn CGEventSetType(event: *mut CGEvent, ty: u32);
    fn CGEventKeyboardSetUnicodeString(event: *mut CGEvent, len: usize, chars: *const u16);
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const std::ffi::c_void);
}

const SHIFT_FLAG: usize = 1 << 17;
const OPTION_FLAG: usize = 1 << 19;

/// A flagsChanged event for modifier key `code` with `flags` held after it.
fn cg_flags_event(code: u16, flags: usize) -> Option<Retained<AnyObject>> {
    unsafe {
        let source = CGEventSourceCreate(-1);
        let ev = CGEventCreateKeyboardEvent(source, code, flags != 0);
        if ev.is_null() {
            return None;
        }
        CGEventSetType(ev, 12); // kCGEventFlagsChanged
        CGEventSetFlags(ev, flags as u64);
        let ns: Option<Retained<AnyObject>> = msg_send![class!(NSEvent), eventWithCGEvent: ev];
        CFRelease(ev.cast());
        if !source.is_null() {
            CFRelease(source);
        }
        ns
    }
}

fn cg_key_event(code: u16, down: bool, flags: usize, chars: &str) -> Option<Retained<AnyObject>> {
    unsafe {
        // kCGEventSourceStatePrivate: no shared modifier state with the HID system.
        let source = CGEventSourceCreate(-1);
        let ev = CGEventCreateKeyboardEvent(source, code, down);
        if ev.is_null() {
            return None;
        }
        CGEventSetFlags(ev, flags as u64);
        // "\u{0}" = keep the string the layout computed for the key code.
        if chars != "\u{0}" {
            let utf16: Vec<u16> = chars.encode_utf16().collect();
            CGEventKeyboardSetUnicodeString(ev, utf16.len(), utf16.as_ptr());
        }
        let ns: Option<Retained<AnyObject>> = msg_send![class!(NSEvent), eventWithCGEvent: ev];
        CFRelease(ev.cast());
        if !source.is_null() {
            CFRelease(source);
        }
        ns
    }
}
