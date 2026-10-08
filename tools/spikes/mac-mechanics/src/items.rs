//! The six W0c-A items (DESIGN-v2 §10 W0c A1–A6).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use objc2::msg_send;
use objc2::runtime::{AnyObject, Bool};
use objc2_foundation::{NSPoint, NSString};
use serde_json::{Value, json};
use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{LogicalPosition, LogicalSize, Manager, RunEvent, Webview, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent, Wry};

use crate::lab::{self, class_name, obj};
use crate::{Ctx, Native, sleep, t};

const TEMPLATE: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
const CUSTOM_UA: &str = "CustomApp/9.9 (app-set user agent)";

static MONITOR_LOG: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static NEW_WINDOW_LOG: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static NAV_LOG: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static ARMED: Mutex<Vec<(String, f64)>> = Mutex::new(Vec::new());
/// (label, WKWebView, NSWindow) of gesture guests.
static GESTURE_GUESTS: Mutex<Vec<(String, usize, usize)>> = Mutex::new(Vec::new());
/// (label, WKWebView) of app webviews (to name hit views).
static APP_VIEWS: Mutex<Vec<(String, usize)>> = Mutex::new(Vec::new());
static CLOSE_LOG: Mutex<Vec<Value>> = Mutex::new(Vec::new());
/// (window label, guest label, retained WKWebView).
static CLOSE_GUESTS: Mutex<Vec<(String, String, usize)>> = Mutex::new(Vec::new());
static EVENT_LOG: Mutex<Vec<Value>> = Mutex::new(Vec::new());

/// Activation window after an arming event (Chromium's transient activation).
const ACTIVATION_WINDOW_S: f64 = 5.0;

fn push(log: &Mutex<Vec<Value>>, v: Value) {
    if let Ok(mut l) = log.lock() {
        l.push(v);
    }
}
fn snapshot(log: &Mutex<Vec<Value>>) -> Vec<Value> {
    log.lock().map(|l| l.clone()).unwrap_or_default()
}

pub fn progress(s: &str) {
    eprintln!("spike-progress {} {s}", t());
}

fn hook_enabled() -> bool {
    std::env::var("SPIKE_HOOK").is_ok_and(|v| v == "1")
}

/// Builder additions per item (plugin probe, app close handler, terminate hook).
pub fn configure(item: &str, builder: tauri::Builder<Wry>) -> tauri::Builder<Wry> {
    match item {
        "close" => builder.plugin(close_probe()).on_window_event(|window, event| {
            let label = window.label().to_owned();
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    let action = match label.as_str() {
                        "cl-prevent" => {
                            api.prevent_close();
                            "prevent_close"
                        }
                        "cl-tray" => {
                            api.prevent_close();
                            let _ = window.hide();
                            "prevent_close + hide (close-to-tray)"
                        }
                        _ => "none",
                    };
                    push(
                        &CLOSE_LOG,
                        json!({ "t": t(), "who": "app on_window_event", "event": "CloseRequested", "label": label, "action": action }),
                    );
                }
                WindowEvent::Destroyed => push(
                    &CLOSE_LOG,
                    json!({ "t": t(), "who": "app on_window_event", "event": "Destroyed", "label": label }),
                ),
                _ => {}
            }
        }),
        "crash" if hook_enabled() => builder.on_web_content_process_terminate(|wv| {
            push(
                &EVENT_LOG,
                json!({ "t": t(), "event": "web-content-process-terminate hook", "label": wv.label() }),
            );
        }),
        _ => builder,
    }
}

pub fn run(ctx: &Ctx, item: &str) -> Value {
    match item {
        "ua" => ua(ctx),
        "gesture" => gesture(ctx),
        "zoom" => zoom(ctx),
        "close" => close(ctx),
        "crash" => crash(ctx),
        "storage" => storage(ctx),
        other => json!({ "error": format!("unknown SPIKE_ITEM {other}") }),
    }
}

fn guest_builder(ctx: &Ctx, label: &str, path: &str) -> WebviewBuilder<Wry> {
    let url = ctx.fx.url(path);
    let l = label.to_owned();
    let l2 = label.to_owned();
    let l3 = label.to_owned();
    WebviewBuilder::new(label, WebviewUrl::External(url.parse().expect("fixture url")))
        .focused(false)
        .zoom_hotkeys_enabled(false)
        .on_new_window(move |url, _features| {
            let armed = consume_arm(&l);
            push(&NEW_WINDOW_LOG, json!({ "t": t(), "label": l, "url": url.as_str(), "armed": armed, "decision": if armed.is_some() { "open in system browser (armed; spike opens nothing)" } else { "deny (not armed)" } }));
            NewWindowResponse::Deny
        })
        .on_navigation(move |url| {
            let allowed = url.scheme() == "about" || matches!(url.path(), "/guest.html" | "/storage.html");
            if !allowed {
                let armed = consume_arm(&l2);
                push(&NAV_LOG, json!({ "t": t(), "label": l2, "url": url.as_str(), "armed": armed }));
            }
            allowed
        })
        .on_page_load(move |_wv, payload| {
            let ev = match payload.event() {
                PageLoadEvent::Started => "Started",
                PageLoadEvent::Finished => "Finished",
            };
            push(&EVENT_LOG, json!({ "t": t(), "event": format!("page-load {ev}"), "label": l3, "url": payload.url().as_str() }));
        })
}

fn add_guest(win: &WebviewWindow, b: WebviewBuilder<Wry>, x: f64, y: f64, w: f64, h: f64) -> tauri::Result<Webview> {
    win.as_ref()
        .window()
        .add_child(b, LogicalPosition::new(x, y), LogicalSize::new(w, h))
}

// ------------------------------------------------------------------- A1 UA

fn discover(kvc: &Value, custom: Option<&str>, eval: Option<&str>) -> Value {
    let (source, native) = if kvc["respondsToUnderscoreUserAgent"] == true {
        ("guarded KVC userAgent", kvc["value"].as_str().map(str::to_owned))
    } else if let Some(c) = custom.filter(|c| !c.is_empty()) {
        ("customUserAgent", Some(c.to_owned()))
    } else {
        ("evaluateJavaScript navigator.userAgent", eval.map(str::to_owned))
    };
    let accepted = native.as_deref() == Some(TEMPLATE);
    json!({ "source": source, "read": native, "shapeAccepted": accepted, "result": if accepted { native.unwrap_or_default() } else { TEMPLATE.to_owned() }, "usedTemplate": !accepted })
}

fn ua(ctx: &Ctx) -> Value {
    let main = ctx.window("main", 900.0, 600.0, 40.0, 40.0);
    let early_main = {
        let n = ctx.native(main.as_ref());
        let started = Instant::now();
        let kvc = ctx.on_main(move || lab::kvc_user_agent(n.wk));
        json!({ "kvc": kvc, "kvcCallUs": started.elapsed().as_micros() as u64 })
    };
    let custom = WebviewWindowBuilder::new(&ctx.app, "custom", WebviewUrl::App("index.html".into()))
        .inner_size(600.0, 400.0)
        .position(60.0, 60.0)
        .visible(false)
        .user_agent(CUSTOM_UA)
        .build()
        .expect("custom window");
    {
        let addr = custom.ns_window().expect("ns_window") as usize;
        ctx.on_main(move || lab::make_invisible(addr, true));
    }
    let g_default = add_guest(
        &main,
        guest_builder(ctx, "owad-ua-default", "/guest.html?id=owad-ua-default"),
        500.0,
        40.0,
        300.0,
        250.0,
    )
    .expect("guest");
    let g_custom = add_guest(
        &main,
        guest_builder(ctx, "owad-ua-set", "/guest.html?id=owad-ua-set")
            .user_agent(&format!("{TEMPLATE} SpikeProduct/1.0.0 Tauri/{}", tauri::VERSION)),
        500.0,
        300.0,
        300.0,
        250.0,
    )
    .expect("guest");
    let early_guest = {
        let n = ctx.native(&g_default);
        ctx.on_main(move || lab::kvc_user_agent(n.wk))
    };
    let views: Vec<(&str, Webview, Option<String>)> = vec![
        ("main (app webview, no app UA)", main.as_ref().clone(), None),
        ("custom (app webview, app-set UA)", custom.as_ref().clone(), Some(CUSTOM_UA.into())),
        ("owad-ua-default (guest, no UA set)", g_default.clone(), None),
        (
            "owad-ua-set (guest, product UA set)",
            g_custom.clone(),
            Some("template + product tokens".into()),
        ),
    ];
    let mut rows = Vec::new();
    for (name, wv, app_ua) in views {
        ctx.wait_ready(&wv);
        let n = ctx.native(&wv);
        let started = Instant::now();
        let (kvc, custom_ua, app_name) = ctx.on_main(move || {
            (
                lab::kvc_user_agent(n.wk),
                lab::custom_user_agent(n.wk),
                lab::application_name_for_user_agent(n.wk),
            )
        });
        let kvc_us = started.elapsed().as_micros() as u64;
        let nav = ctx.native_eval(n.wk, "navigator.userAgent", 3000);
        let tauri_nav = ctx.eval(&wv, "navigator.userAgent");
        let id = format!("ua-{}", wv.label());
        ctx.eval(
            &wv,
            &format!(
                "(fetch({:?}, {{cache:'no-store'}}).catch(()=>{{}}), 1)",
                ctx.fx.url(&format!("/beacon?id={id}&k=ua"))
            ),
        );
        sleep(400);
        let server_ua: Vec<String> = ctx
            .fx
            .requests()
            .into_iter()
            .filter(|r| {
                r["beacon"]["id"] == id.as_str()
                    || (r["path"] == "/guest.html"
                        && r["query"]
                            .as_str()
                            .is_some_and(|q| q.split('&').any(|kv| kv == format!("id={}", wv.label()))))
            })
            .filter_map(|r| r["userAgent"].as_str().map(str::to_owned))
            .collect();
        let nav_s = nav["value"].as_str().map(str::to_owned);
        let disc = discover(&kvc, custom_ua.as_deref(), nav_s.as_deref());
        rows.push(json!({
            "webview": name,
            "appSetUserAgent": app_ua,
            "kvc": kvc,
            "kvcCallUsIncludingHop": kvc_us,
            "customUserAgent": custom_ua,
            "applicationNameForUserAgent": app_name,
            "navigatorUserAgentNative": nav,
            "navigatorUserAgentTauriEval": tauri_nav,
            "serverSawUserAgent": server_ua,
            "kvcEqualsNavigator": kvc["value"].as_str() == nav_s.as_deref(),
            "kvcEqualsTemplate": kvc["value"].as_str() == Some(TEMPLATE),
            "navigatorEqualsTemplate": nav_s.as_deref() == Some(TEMPLATE),
            "designDiscovery": disc,
        }));
    }
    let default_rows: Vec<&Value> = rows.iter().filter(|r| r["appSetUserAgent"].is_null()).collect();
    let all_default_equal = default_rows
        .iter()
        .all(|r| r["kvcEqualsNavigator"] == true && r["kvcEqualsTemplate"] == true && r["navigatorEqualsTemplate"] == true);
    let custom_row = &rows[1];
    let custom_rejected = custom_row["designDiscovery"]["usedTemplate"] == true && custom_row["designDiscovery"]["result"] == TEMPLATE;
    json!({
        "template": TEMPLATE,
        "earlyReadBeforeFirstLoad": { "main": early_main, "guest": early_guest },
        "rows": rows,
        "verdict": {
            "defaultUa_nativeRead_eq_navigator_eq_template": all_default_equal,
            "appSetCustomUa_rejectedByShapeCheck_templateUsed": custom_rejected,
            "verified": all_default_equal && custom_rejected,
        },
    })
}

// -------------------------------------------------------------- A2 gesture

fn arm(label: &str) {
    if let Ok(mut a) = ARMED.lock() {
        a.retain(|(l, _)| l != label);
        a.push((label.to_owned(), lab::uptime()));
    }
}

/// Consumes the label's activation if it is within the window; returns its age (s).
fn consume_arm(label: &str) -> Option<f64> {
    let mut a = ARMED.lock().ok()?;
    let now = lab::uptime();
    let pos = a.iter().position(|(l, at)| l == label && now - at <= ACTIVATION_WINDOW_S)?;
    let (_, at) = a.remove(pos);
    Some(now - at)
}

fn view_label(view: *mut AnyObject) -> Value {
    if view.is_null() {
        return Value::Null;
    }
    for (l, wk, _) in GESTURE_GUESTS.lock().map(|g| g.clone()).unwrap_or_default() {
        if lab::is_view_or_descendant(view, wk) {
            return json!(l);
        }
    }
    for (l, wk) in APP_VIEWS.lock().map(|g| g.clone()).unwrap_or_default() {
        if lab::is_view_or_descendant(view, wk) {
            return json!(l);
        }
    }
    json!(format!("other: {}", class_name(view)))
}

/// The plugin's arming rule (DESIGN-v2 §4.9 macOS row), run from the local
/// monitor for every left-mouse-down and key-down.
fn monitor_handler(ev: &AnyObject) {
    unsafe {
        let ty: usize = msg_send![ev, type];
        let mut win: *mut AnyObject = msg_send![ev, window];
        let event_window_nil = win.is_null();
        if win.is_null() {
            let app: *mut AnyObject = msg_send![objc2::class!(NSApplication), sharedApplication];
            win = msg_send![app, keyWindow];
        }
        if win.is_null() {
            push(&MONITOR_LOG, json!({ "t": t(), "type": ty, "window": null }));
            return;
        }
        let key: Bool = msg_send![win, isKeyWindow];
        let key = key.as_bool();
        let mut rec = json!({ "t": t(), "type": ty, "eventWindowNil": event_window_nil, "windowIsKey": key });
        let guests: Vec<(String, usize, usize)> = GESTURE_GUESTS.lock().map(|g| g.clone()).unwrap_or_default();
        let mut armed = Vec::new();
        if ty == 1 {
            let loc: NSPoint = msg_send![ev, locationInWindow];
            let cv: *mut AnyObject = msg_send![win, contentView];
            let hit: *mut AnyObject = msg_send![cv, hitTest: loc];
            rec["locationInWindow"] = json!([loc.x, loc.y]);
            rec["hitClass"] = class_name(hit);
            rec["hitView"] = view_label(hit);
            for (label, wk, nsw) in &guests {
                if *nsw != win as usize {
                    continue;
                }
                let hidden: Bool = msg_send![obj(*wk), isHiddenOrHasHiddenAncestor];
                if lab::is_view_or_descendant(hit, *wk) && !hidden.as_bool() && !lab::passthrough_on(*wk) && key {
                    arm(label);
                    armed.push(label.clone());
                }
            }
        } else if ty == 10 {
            let code: u16 = msg_send![ev, keyCode];
            let fr: *mut AnyObject = msg_send![win, firstResponder];
            rec["keyCode"] = json!(code);
            rec["firstResponderClass"] = class_name(fr);
            rec["firstResponderView"] = view_label(fr);
            if matches!(code, 36 | 49 | 76) {
                for (label, wk, nsw) in &guests {
                    if *nsw == win as usize && lab::is_view_or_descendant(fr, *wk) && key {
                        arm(label);
                        armed.push(label.clone());
                    }
                }
            }
        }
        rec["armed"] = json!(armed);
        push(&MONITOR_LOG, rec);
    }
}

fn click(ctx: &Ctx, nsw: usize, target_wk: usize, x: f64, y: f64) -> Value {
    ctx.on_main(move || {
        let r = lab::frame_in_window(target_wk);
        let at = NSPoint::new(r.origin.x + x, r.origin.y + r.size.height - y);
        for ty in [1_usize, 2] {
            if let Some(ev) = lab::mouse_event(nsw, ty, at) {
                lab::app_send(&ev);
            }
        }
        json!({ "windowPoint": [at.x, at.y] })
    })
}

/// Return (36) or Space (49) through `-[NSApplication sendEvent:]`.
fn press(ctx: &Ctx, nsw: usize, code: u16) {
    ctx.on_main(move || unsafe {
        let number: isize = msg_send![obj(nsw), windowNumber];
        let chars = NSString::from_str(if code == 36 { "\r" } else { " " });
        for ty in [10_usize, 11] {
            let ev: Option<objc2::rc::Retained<AnyObject>> = msg_send![objc2::class!(NSEvent),
                keyEventWithType: ty,
                location: NSPoint::new(0.0, 0.0),
                modifierFlags: 0_usize,
                timestamp: lab::uptime(),
                windowNumber: number,
                context: std::ptr::null_mut::<AnyObject>(),
                characters: &*chars,
                charactersIgnoringModifiers: &*chars,
                isARepeat: Bool::NO,
                keyCode: code];
            if let Some(ev) = ev {
                // The stock dispatch runs the local monitors (the code under
                // test); in this never-active lab app it does not hand keys
                // on to the window (measured: no keydown reaches either
                // page), so the event is then given to the window directly,
                // as AppKit does for the active app's key window.
                lab::app_send(&ev);
                lab::window_send(nsw, &ev);
            }
        }
    });
}

fn make_first_responder(ctx: &Ctx, nsw: usize, view: usize) -> Value {
    ctx.on_main(move || unsafe {
        let ok: Bool = msg_send![obj(nsw), makeFirstResponder: obj(view)];
        let fr: *mut AnyObject = msg_send![obj(nsw), firstResponder];
        json!({ "made": ok.as_bool(), "firstResponder": class_name(fr) })
    })
}

struct Marks {
    mon: usize,
    nw: usize,
    nav: usize,
    req: usize,
}
fn marks(ctx: &Ctx) -> Marks {
    Marks {
        mon: snapshot(&MONITOR_LOG).len(),
        nw: snapshot(&NEW_WINDOW_LOG).len(),
        nav: snapshot(&NAV_LOG).len(),
        req: ctx.fx.requests().len(),
    }
}
fn since(ctx: &Ctx, m: &Marks) -> Value {
    let beacons: Vec<Value> = ctx
        .fx
        .requests()
        .into_iter()
        .skip(m.req)
        .filter(|r| r["path"] == "/beacon")
        .map(|r| json!([r["beacon"]["id"], r["beacon"]["k"], r["beacon"]["v"]]))
        .collect();
    json!({
        "monitor": snapshot(&MONITOR_LOG).into_iter().skip(m.mon).collect::<Vec<_>>(),
        "newWindow": snapshot(&NEW_WINDOW_LOG).into_iter().skip(m.nw).collect::<Vec<_>>(),
        "navigation": snapshot(&NAV_LOG).into_iter().skip(m.nav).collect::<Vec<_>>(),
        "guestBeacons": beacons,
    })
}

fn armed_in(v: &Value, label: &str) -> bool {
    v["monitor"].as_array().is_some_and(|m| {
        m.iter()
            .any(|e| e["armed"].as_array().is_some_and(|a| a.iter().any(|x| x == label)))
    })
}
fn opened_armed(v: &Value, label: &str) -> Option<bool> {
    v["newWindow"]
        .as_array()
        .and_then(|n| n.iter().find(|e| e["label"] == label))
        .map(|e| !e["armed"].is_null())
}
fn beacon(v: &Value, id: &str, k: &str) -> bool {
    v["guestBeacons"]
        .as_array()
        .is_some_and(|b| b.iter().any(|x| x[0] == id && x[1] == k))
}

fn gesture(ctx: &Ctx) -> Value {
    let main = ctx.window("main", 900.0, 600.0, 40.0, 40.0);
    ctx.make_key(&main);
    let g1 = add_guest(
        &main,
        guest_builder(ctx, "owad-g1", "/guest.html?id=owad-g1"),
        500.0,
        40.0,
        300.0,
        250.0,
    )
    .expect("g1");
    let g2 = add_guest(
        &main,
        guest_builder(ctx, "owad-g2", "/guest.html?id=owad-g2"),
        20.0,
        300.0,
        300.0,
        200.0,
    )
    .expect("g2");
    for wv in [main.as_ref(), &g1, &g2] {
        ctx.wait_ready(wv);
    }
    let app_n = ctx.native(main.as_ref());
    let g1n = ctx.native(&g1);
    let g2n = ctx.native(&g2);
    let nsw = app_n.ns_window;
    if let Ok(mut g) = GESTURE_GUESTS.lock() {
        g.push(("owad-g1".into(), g1n.wk, g1n.ns_window));
        g.push(("owad-g2".into(), g2n.wk, g2n.ns_window));
    }
    if let Ok(mut a) = APP_VIEWS.lock() {
        a.push(("main (app webview)".into(), app_n.wk));
    }
    let g2wk = g2n.wk;
    ctx.on_main(move || lab::set_input_passthrough(g2wk, true));
    ctx.on_main(|| lab::add_local_monitor(monitor_handler));
    sleep(300);
    let mut cases = Vec::new();
    let app_log = |ctx: &Ctx| ctx.eval(main.as_ref(), "window.__appLog.splice(0)");
    app_log(ctx);

    // 1. Mouse on the guest's button.
    let m = marks(ctx);
    let c = click(ctx, nsw, g1n.wk, 60.0, 30.0);
    sleep(800);
    let mut r = since(ctx, &m);
    let ok = armed_in(&r, "owad-g1") && opened_armed(&r, "owad-g1") == Some(true) && beacon(&r, "owad-g1", "click");
    r["click"] = c;
    r["appPage"] = app_log(ctx);
    cases.push(json!({ "case": "1 mouse-down on the guest's button", "expect": "armed owad-g1; page click; window.open → on_new_window while armed", "ok": ok, "data": r }));

    // 2. Mouse on the app page where no guest is.
    let m = marks(ctx);
    let c = click(ctx, nsw, app_n.wk, 70.0, 70.0);
    sleep(600);
    let mut r = since(ctx, &m);
    let app = app_log(ctx);
    let ok =
        !armed_in(&r, "owad-g1") && !armed_in(&r, "owad-g2") && app.as_array().is_some_and(|a| a.iter().any(|e| e["t"] == "mousedown"));
    r["click"] = c;
    r["appPage"] = app;
    cases.push(json!({ "case": "2 mouse-down on the app page (no guest there)", "expect": "nothing armed; app page gets the mousedown", "ok": ok, "data": r }));

    // 3. Mouse on the app under a pass-through guest.
    let m = marks(ctx);
    let c = click(ctx, nsw, g2n.wk, 150.0, 100.0);
    sleep(600);
    let mut r = since(ctx, &m);
    let app = app_log(ctx);
    let hit_app = r["monitor"]
        .as_array()
        .is_some_and(|a| a.iter().any(|e| e["hitView"] == "main (app webview)"));
    let ok = !armed_in(&r, "owad-g2")
        && hit_app
        && !beacon(&r, "owad-g2", "mousedown")
        && app.as_array().is_some_and(|a| a.iter().any(|e| e["t"] == "mousedown"));
    r["click"] = c;
    r["appPage"] = app;
    cases.push(json!({ "case": "3 mouse-down on the app under the pass-through guest owad-g2", "expect": "hitTest = app webview; owad-g2 not armed; app page gets the mousedown; guest gets none", "ok": ok, "data": r }));

    // 4. Script-only click + open in the guest (no NSEvent).
    let m = marks(ctx);
    let e = ctx.eval(&g1, "(document.getElementById('btn').click(), 'clicked')");
    sleep(600);
    let mut r = since(ctx, &m);
    let ok = !armed_in(&r, "owad-g1") && opened_armed(&r, "owad-g1") != Some(true);
    r["eval"] = e;
    cases.push(json!({ "case": "4 script click() + window.open in the guest (evaluateJavaScript, no NSEvent)", "expect": "not armed; any on_new_window is denied", "ok": ok, "data": r }));

    // 4b. Timer-driven open (an ad script's own popup attempt).
    let m = marks(ctx);
    let e = ctx.eval(&g1, "(setTimeout(() => { const w = window.open('/popup.html?from=timer', '_blank'); window.__beacon('timer-open', w === null ? 'null' : 'object'); }, 50), 'scheduled')");
    sleep(700);
    let mut r = since(ctx, &m);
    let ok = !armed_in(&r, "owad-g1") && opened_armed(&r, "owad-g1") != Some(true);
    r["eval"] = e;
    cases.push(json!({ "case": "4b setTimeout window.open in the guest (no gesture)", "expect": "not armed; WebKit blocks it or on_new_window denies", "ok": ok, "data": r }));

    // 5/6. Return / Space with the guest first responder and its button focused.
    for (n, code, name) in [(5, 36_u16, "Return"), (6, 49_u16, "Space")] {
        let fr = make_first_responder(ctx, nsw, g1n.wk);
        ctx.eval(
            &g1,
            "(document.getElementById('btn').focus(), document.activeElement && document.activeElement.id)",
        );
        if let Ok(mut a) = ARMED.lock() {
            a.clear();
        }
        let m = marks(ctx);
        press(ctx, nsw, code);
        sleep(800);
        let mut r = since(ctx, &m);
        let ok = armed_in(&r, "owad-g1") && opened_armed(&r, "owad-g1") == Some(true);
        r["makeFirstResponder"] = fr;
        cases.push(json!({ "case": format!("{n} {name} with the guest first responder (button focused)"), "expect": "armed owad-g1 by key-down; button activates; on_new_window while armed", "ok": ok, "data": r }));
    }

    // 7. Return with the app webview first responder.
    let fr = make_first_responder(ctx, nsw, app_n.wk);
    ctx.eval(main.as_ref(), "(document.getElementById('field').focus(), 1)");
    if let Ok(mut a) = ARMED.lock() {
        a.clear();
    }
    let m = marks(ctx);
    press(ctx, nsw, 36);
    sleep(600);
    let mut r = since(ctx, &m);
    let app = app_log(ctx);
    let ok = !armed_in(&r, "owad-g1") && !armed_in(&r, "owad-g2");
    r["makeFirstResponder"] = fr;
    r["appPage"] = app;
    cases.push(json!({ "case": "7 Return with the app webview first responder", "expect": "nothing armed", "ok": ok, "data": r }));

    // 8. Mouse on the guest while the window is not key.
    lab::FAKE_KEY_ON.store(false, std::sync::atomic::Ordering::SeqCst);
    let m = marks(ctx);
    let c = click(ctx, nsw, g1n.wk, 60.0, 30.0);
    sleep(700);
    lab::FAKE_KEY_ON.store(true, std::sync::atomic::Ordering::SeqCst);
    let mut r = since(ctx, &m);
    let ok = !armed_in(&r, "owad-g1") && opened_armed(&r, "owad-g1") != Some(true);
    r["click"] = c;
    cases.push(
        json!({ "case": "8 mouse-down on the guest while its window is not key", "expect": "not armed (SEC-M2)", "ok": ok, "data": r }),
    );

    // 9. Hidden guest: a click where it was.
    let g1wk = g1n.wk;
    ctx.on_main(move || {
        let () = unsafe { msg_send![obj(g1wk), setHidden: true] };
    });
    let m = marks(ctx);
    let c = click(ctx, nsw, g1n.wk, 60.0, 30.0);
    sleep(600);
    ctx.on_main(move || {
        let () = unsafe { msg_send![obj(g1wk), setHidden: false] };
    });
    let mut r = since(ctx, &m);
    let ok = !armed_in(&r, "owad-g1");
    r["click"] = c;
    cases.push(json!({ "case": "9 mouse-down where the hidden guest sits", "expect": "not armed", "ok": ok, "data": r }));

    // 10. Activation expiry: arm by a click on the guest's empty area, wait > window, script open.
    let m = marks(ctx);
    click(ctx, nsw, g1n.wk, 250.0, 230.0);
    sleep(5300);
    ctx.eval(&g1, "(document.getElementById('btn').click(), 1)");
    sleep(600);
    let r = since(ctx, &m);
    let ok = armed_in(&r, "owad-g1") && opened_armed(&r, "owad-g1") != Some(true);
    cases.push(json!({ "case": "10 click on the guest, then a script open 5.3 s later", "expect": "armed by the click, expired at the open → deny", "ok": ok, "data": r }));

    let failed: Vec<&Value> = cases.iter().filter(|c| c["ok"] != true).map(|c| &c["case"]).collect();
    json!({
        "cases": cases,
        "verdict": { "cases": cases.len(), "failed": failed, "verified": failed.is_empty() },
    })
}

// ----------------------------------------------------------------- A3 zoom

fn zoom(ctx: &Ctx) -> Value {
    let main = ctx.window("main", 900.0, 600.0, 40.0, 40.0);
    let g = add_guest(
        &main,
        guest_builder(ctx, "owad-z1", "/guest.html?id=owad-z1"),
        500.0,
        40.0,
        300.0,
        250.0,
    )
    .expect("guest");
    ctx.wait_ready(main.as_ref());
    ctx.wait_ready(&g);
    let scale = main.scale_factor().unwrap_or(0.0);
    let mut rows = Vec::new();
    let mut ok_all = true;
    for z in [1.0_f64, 0.8, 1.25, 1.0] {
        for wv in [main.as_ref(), &g] {
            let _ = wv.set_zoom(z);
        }
        sleep(600);
        for (name, wv) in [("app webview", main.as_ref()), ("guest owad-z1", &g)] {
            let n: Native = ctx.native(wv);
            let page = ctx.eval(wv, "({ dpr: window.devicePixelRatio, innerWidth: window.innerWidth, innerHeight: window.innerHeight, vvScale: window.visualViewport && window.visualViewport.scale, box: (() => { const r = document.getElementById('box').getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })() })");
            let (pz, frame, backing) = ctx.on_main(move || {
                let f = lab::frame_in_window(n.wk);
                (lab::page_zoom(n.wk), [f.size.width, f.size.height], lab::backing_scale(n.ns_window))
            });
            let dpr = page["dpr"].as_f64().unwrap_or(0.0);
            let est = if scale > 0.0 { dpr / scale } else { 0.0 };
            let iw = page["innerWidth"].as_f64().unwrap_or(0.0);
            let points_per_css = if iw > 0.0 { frame[0] / iw } else { 0.0 };
            let ok = (est - z).abs() < 0.01 && (points_per_css - z).abs() < 0.02;
            ok_all &= ok;
            rows.push(json!({
                "zoomSet": z, "webview": name, "pageZoomReadback": pz, "windowScaleFactor": scale, "backingScaleFactor": backing,
                "page": page, "nativeFramePoints": frame,
                "dprOverScaleFactor": est, "nativePointsPerCssPx": points_per_css, "formulaHolds": ok,
            }));
        }
    }
    json!({ "rows": rows, "verdict": { "zoomEqualsDprOverScaleFactor": ok_all, "verified": ok_all } })
}

// ---------------------------------------------------------------- A4 close

fn close_probe() -> tauri::plugin::TauriPlugin<Wry> {
    tauri::plugin::Builder::<Wry>::new("closeprobe")
        .on_event(|app, event| {
            let RunEvent::WindowEvent { label, event, .. } = event else {
                return;
            };
            match event {
                WindowEvent::CloseRequested { .. } => {
                    push(
                        &CLOSE_LOG,
                        json!({ "t": t(), "who": "plugin on_event", "event": "CloseRequested", "label": label }),
                    );
                    let guests: Vec<(String, usize)> = CLOSE_GUESTS
                        .lock()
                        .map(|g| g.iter().filter(|(w, _, _)| w == label).map(|(_, g, wk)| (g.clone(), *wk)).collect())
                        .unwrap_or_default();
                    // Fallback signal: a message sent right at CloseRequested.
                    if let Some((g, wk)) = guests.first() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        lab::eval_native(
                            *wk,
                            "(window.__beacon('at-close-requested-native', document.visibilityState), 1)",
                            tx,
                        );
                        let g = g.clone();
                        std::thread::spawn(move || {
                            let r = rx.recv_timeout(Duration::from_secs(3)).unwrap_or(json!({ "timeout": true }));
                            push(
                                &CLOSE_LOG,
                                json!({ "t": t(), "who": "completion", "what": "native eval at CloseRequested", "guest": g, "result": r }),
                            );
                        });
                    }
                    if let Some((g, _)) = guests.get(1) {
                        let r = app.get_webview(g).map(|w| {
                            w.eval("window.__beacon('at-close-requested-tauri', document.visibilityState)")
                                .is_ok()
                        });
                        push(
                            &CLOSE_LOG,
                            json!({ "t": t(), "who": "plugin on_event", "what": "tauri eval at CloseRequested", "guest": g, "queued": r }),
                        );
                    }
                    // Primary mechanism: a check posted through the event-loop proxy from a helper thread.
                    let app = app.clone();
                    let label = label.clone();
                    std::thread::spawn(move || {
                        // `late` variant: post the check only after the window is destroyed.
                        if label.contains("late") {
                            sleep(100);
                        }
                        let posted = t();
                        let app2 = app.clone();
                        let _ = app.run_on_main_thread(move || close_check(&app2, &label, posted, guests));
                    });
                }
                WindowEvent::Destroyed => push(
                    &CLOSE_LOG,
                    json!({ "t": t(), "who": "plugin on_event", "event": "Destroyed", "label": label }),
                ),
                _ => {}
            }
        })
        .build()
}

fn close_check(app: &tauri::AppHandle, label: &str, posted: u64, guests: Vec<(String, usize)>) {
    let ran = t();
    let window = app.get_window(label);
    let window_state = match &window {
        Some(w) => json!({ "present": true, "isVisible": w.is_visible().map_err(|e| e.to_string()) }),
        None => json!({ "present": false }),
    };
    let destroyed_before = snapshot(&CLOSE_LOG)
        .iter()
        .any(|e| e["label"] == label && e["event"] == "Destroyed");
    let mut gs = Vec::new();
    for (g, wk) in guests {
        let tauri_wv = app.get_webview(&g);
        let tauri_eval = tauri_wv
            .as_ref()
            .map(|w| w.eval("window.__beacon('check-tauri', document.visibilityState)").is_ok());
        // Can the plugin still reach the native view through Tauri here?
        let inline = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let inline2 = inline.clone();
        let with_webview = tauri_wv.as_ref().map(|w| {
            w.with_webview(move |pw| inline2.store(pw.inner() as usize, std::sync::atomic::Ordering::SeqCst))
                .map_err(|e| e.to_string())
        });
        let with_webview_inline_ptr = inline.load(std::sync::atomic::Ordering::SeqCst);
        let inline3 = inline.clone();
        let g3 = g.clone();
        std::thread::spawn(move || {
            sleep(1000);
            push(
                &CLOSE_LOG,
                json!({ "t": t(), "who": "later", "what": "with_webview from posted check ran by +1 s", "guest": g3, "ran": inline3.load(std::sync::atomic::Ordering::SeqCst) != 0 }),
            );
        });
        let (superview, window_ptr, pid) = unsafe {
            let s: *mut AnyObject = msg_send![obj(wk), superview];
            let w: *mut AnyObject = msg_send![obj(wk), window];
            (!s.is_null(), !w.is_null(), lab::web_process_id(wk))
        };
        let (tx, rx) = std::sync::mpsc::channel();
        lab::eval_native(
            wk,
            "(window.__beacon('check-native', document.visibilityState), document.visibilityState)",
            tx,
        );
        let g2 = g.clone();
        std::thread::spawn(move || {
            let r = rx.recv_timeout(Duration::from_secs(3)).unwrap_or(json!({ "timeout": true }));
            push(
                &CLOSE_LOG,
                json!({ "t": t(), "who": "completion", "what": "native eval in posted check", "guest": g2, "result": r }),
            );
        });
        gs.push(json!({ "guest": g, "tauriWebviewPresent": tauri_wv.is_some(), "tauriEvalQueued": tauri_eval, "withWebview": with_webview.map(|r| r.is_ok()), "withWebviewRanInline": with_webview_inline_ptr != 0, "withWebviewInlinePtrIsSame": with_webview_inline_ptr == wk, "wkHasSuperview": superview, "wkHasWindow": window_ptr, "webProcessId": pid }));
    }
    push(
        &CLOSE_LOG,
        json!({ "t": ran, "who": "posted check", "label": label, "postedAt": posted, "window": window_state, "destroyedEventBeforeCheck": destroyed_before, "guests": gs }),
    );
}

fn close(ctx: &Ctx) -> Value {
    let mut variants = Vec::new();
    let list = std::env::var("SPIKE_CLOSE_VARIANTS")
        .unwrap_or_else(|_| "close prevent tray destroy close close close close close close close close close".into());
    for (i, v) in list.split_whitespace().enumerate() {
        let v = v.to_owned();
        let v = v.as_str();
        let label = if matches!(v, "prevent" | "tray") {
            format!("cl-{v}")
        } else {
            format!("cl-{v}{i}")
        };
        progress(&format!("close: {label}"));
        let win = ctx.window(&label, 700.0, 500.0, 40.0, 40.0);
        let ga = format!("owad-{v}{i}-a");
        let gb = format!("owad-{v}{i}-b");
        let a = add_guest(
            &win,
            guest_builder(ctx, &ga, &format!("/guest.html?id={ga}")),
            350.0,
            20.0,
            300.0,
            200.0,
        )
        .expect("guest a");
        let b = add_guest(
            &win,
            guest_builder(ctx, &gb, &format!("/guest.html?id={gb}")),
            350.0,
            250.0,
            300.0,
            200.0,
        )
        .expect("guest b");
        for wv in [win.as_ref(), &a, &b] {
            ctx.wait_ready(wv);
        }
        let an = ctx.native(&a);
        let bn = ctx.native(&b);
        let (ra, rb) = ctx.on_main(move || (lab::retain(an.wk), lab::retain(bn.wk)));
        if let Ok(mut g) = CLOSE_GUESTS.lock() {
            g.push((label.clone(), ga.clone(), ra));
            g.push((label.clone(), gb.clone(), rb));
        }
        let pids = ctx.on_main(move || (lab::web_process_id(ra), lab::web_process_id(rb)));
        let log_mark = snapshot(&CLOSE_LOG).len();
        let req_mark = ctx.fx.requests().len();
        let triggered = t();
        progress("close: trigger");
        let trigger = if v == "destroy" { win.destroy() } else { win.close() };
        sleep(2500);
        let log: Vec<Value> = snapshot(&CLOSE_LOG).into_iter().skip(log_mark).collect();
        let beacons: Vec<Value> = ctx
            .fx
            .requests()
            .into_iter()
            .skip(req_mark)
            .filter(|r| r["path"] == "/beacon")
            .map(|r| json!({ "t": r["t"], "id": r["beacon"]["id"], "k": r["beacon"]["k"], "v": r["beacon"]["v"] }))
            .collect();
        let after = ctx
            .app
            .get_window(&label)
            .map(|w| json!({ "present": true, "isVisible": w.is_visible().ok() }))
            .unwrap_or(json!({ "present": false }));
        let pids_alive = [pids.0, pids.1].map(|p| p.map(|p| p > 0 && unsafe { libc::kill(p, 0) } == 0));
        let check = log.iter().find(|e| e["who"] == "posted check").cloned().unwrap_or(Value::Null);
        let k = |key: &str| {
            beacons
                .iter()
                .filter(|b| b["k"] == key)
                .map(|b| b["id"].clone())
                .collect::<Vec<_>>()
        };
        variants.push(json!({
            "variant": v, "trigger": format!("{}() → {:?}", if v == "destroy" { "destroy" } else { "close" }, trigger.map_err(|e| e.to_string())),
            "triggeredAt": triggered, "guestPidsBefore": [pids.0, pids.1], "guestPidsAliveAfter2500ms": pids_alive,
            "windowAfter": after, "postedCheck": check, "log": log, "beacons": beacons,
            "summary": {
                "checkSawWindowGone": check["window"]["present"] == false,
                "checkGuestsTauriPresent": check["guests"].as_array().map(|g| g.iter().map(|x| x["tauriWebviewPresent"].clone()).collect::<Vec<_>>()),
                "beaconAtCloseRequestedNative": k("at-close-requested-native"),
                "beaconAtCloseRequestedTauri": k("at-close-requested-tauri"),
                "beaconCheckNative": k("check-native"),
                "beaconCheckTauri": k("check-tauri"),
                "pagehide": k("pagehide"),
                "visibilitychange": beacons.iter().filter(|b| b["k"] == "visibilitychange").map(|b| json!([b["id"], b["v"]])).collect::<Vec<_>>(),
            },
        }));
        if ctx.app.get_window(&label).is_some() {
            let _ = ctx.app.get_window(&label).map(|w| w.destroy());
            sleep(800);
        }
        let (ra2, rb2) = (ra, rb);
        ctx.on_main(move || unsafe {
            let () = msg_send![obj(ra2), release];
            let () = msg_send![obj(rb2), release];
        });
    }
    let lates: Vec<&Value> = variants.iter().filter(|v| v["variant"] == "late").collect();
    let late_summary: Vec<Value> = lates.iter().map(|v| json!({ "checkAfterDestroyed": v["postedCheck"]["destroyedEventBeforeCheck"], "window": v["postedCheck"]["window"], "nativeEvalFromRetainedHandleReached": v["summary"]["beaconCheckNative"] })).collect();
    let closes: Vec<&Value> = variants.iter().filter(|v| v["variant"] == "close").collect();
    let check_before_destroyed = closes
        .iter()
        .filter(|v| v["postedCheck"]["destroyedEventBeforeCheck"] == false)
        .count();
    let native_landed = closes
        .iter()
        .filter(|v| v["summary"]["beaconCheckNative"].as_array().is_some_and(|a| a.len() == 2))
        .count();
    let window_gone_signal = closes
        .iter()
        .filter(|v| v["postedCheck"]["window"]["isVisible"].get("Err").is_some() || v["postedCheck"]["window"]["present"] == false)
        .count();
    json!({ "variants": variants, "verdict": {
        "closeRuns": closes.len(),
        "lateCheck(posted 100 ms after CloseRequested)": late_summary,
        "postedCheckRanBeforeDestroyed": check_before_destroyed,
        "postedCheckSawCloseCertain(isVisible Err or window absent)": window_gone_signal,
        "nativeEvalFromCheckReachedBothGuestPages": native_landed,
        "preventedCheckSawWindowVisible": variants.iter().any(|v| v["variant"] == "prevent" && v["postedCheck"]["window"]["isVisible"]["Ok"] == true),
        "trayCheckSawWindowHidden": variants.iter().any(|v| v["variant"] == "tray" && v["postedCheck"]["window"]["isVisible"]["Ok"] == false),
        "verified": closes.len() > 0 && check_before_destroyed == closes.len() && native_landed == closes.len() && window_gone_signal == closes.len(),
    } })
}

// ---------------------------------------------------------------- A5 crash

fn pid_alive(p: Option<i32>) -> Option<bool> {
    p.map(|p| p > 0 && unsafe { libc::kill(p, 0) } == 0)
}

fn crash(ctx: &Ctx) -> Value {
    let main = ctx.window("main", 900.0, 600.0, 40.0, 40.0);
    let c1 = add_guest(
        &main,
        guest_builder(ctx, "owad-c1", "/guest.html?id=owad-c1"),
        500.0,
        40.0,
        300.0,
        250.0,
    )
    .expect("c1");
    let c2 = add_guest(
        &main,
        guest_builder(ctx, "owad-c2", "/guest.html?id=owad-c2"),
        500.0,
        320.0,
        300.0,
        250.0,
    )
    .expect("c2");
    let hid = ctx.window("hid", 400.0, 300.0, 80.0, 80.0);
    let c3 = add_guest(
        &hid,
        guest_builder(ctx, "owad-c3", "/guest.html?id=owad-c3"),
        50.0,
        20.0,
        300.0,
        250.0,
    )
    .expect("c3");
    for wv in [main.as_ref(), &c1, &c2, hid.as_ref(), &c3] {
        ctx.wait_ready(wv);
    }
    let n_main = ctx.native(main.as_ref());
    let n1 = ctx.native(&c1);
    let n2 = ctx.native(&c2);
    let n3 = ctx.native(&c3);
    let n_hid = ctx.native(hid.as_ref());
    let pids = ctx.on_main(move || {
        json!({ "main": lab::web_process_id(n_main.wk), "owad-c1": lab::web_process_id(n1.wk), "owad-c2": lab::web_process_id(n2.wk), "owad-c3": lab::web_process_id(n3.wk), "hid": lab::web_process_id(n_hid.wk) })
    });
    let occl_main = ctx.on_main(move || lab::occlusion_visible(n_main.ns_window));

    // Alive, visible-window page.
    let alive = ctx.native_eval(n1.wk, "1", 3000);
    let alive_vis = ctx.eval(&c1, "document.visibilityState");

    // Throttled but alive: guest in a hidden (ordered-out) window.
    let _ = hid.hide();
    sleep(3000);
    let t0 = ctx.eval(&c3, "window.__ticks");
    sleep(2000);
    let t1 = ctx.eval(&c3, "window.__ticks");
    let occl_hid = ctx.on_main(move || lab::occlusion_visible(n_hid.ns_window));
    let hidden_vis = ctx.eval(&c3, "document.visibilityState");
    let hidden_probe = ctx.native_eval(n3.wk, "1", 3000);
    let main_t0 = ctx.eval(&c1, "window.__ticks");
    sleep(2000);
    let main_t1 = ctx.eval(&c1, "window.__ticks");

    // Hung page: a 3 s busy loop, then a probe with a 1.5 s bound.
    let busy = ctx.native_eval_start(
        n2.wk,
        "(() => { const s = Date.now(); while (Date.now() - s < 3000) {} return 'busy-done'; })()",
    );
    sleep(100);
    let hung_probe = ctx.native_eval(n2.wk, "1", 1500);
    let busy_done = busy.recv_timeout(Duration::from_secs(5)).unwrap_or(json!({ "timeout": true }));
    let hung_probe_late = ctx.native_eval(n2.wk, "1", 3000);

    // Kill c1's WebContent process.
    let ev_mark = snapshot(&EVENT_LOG).len();
    let victim = pids["owad-c1"].as_i64().unwrap_or(0) as i32;
    let shared_with: Vec<String> = pids
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, v)| *k != "owad-c1" && v.as_i64() == Some(victim as i64))
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default();
    let killed_at = t();
    let kill_rc = if victim > 0 {
        unsafe { libc::kill(victim, libc::SIGKILL) }
    } else {
        -1
    };
    let mut hook_ms = Value::Null;
    let mut pid_trace = Vec::new();
    // Probe after SPIKE_PROBE_DELAY_MS (default 300 ms) without waiting for anything else.
    let delay: u64 = std::env::var("SPIKE_PROBE_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let mut waited = 0;
    while waited < delay {
        sleep(25);
        waited += 25;
        let p = ctx.on_main(move || lab::web_process_id(n1.wk));
        pid_trace.push(json!([t().saturating_sub(killed_at), p]));
        if hook_ms.is_null() {
            if let Some(e) = snapshot(&EVENT_LOG)
                .iter()
                .skip(ev_mark)
                .find(|e| e["label"] == "owad-c1" && e["event"] == "web-content-process-terminate hook")
            {
                hook_ms = json!(e["t"].as_u64().unwrap_or(0).saturating_sub(killed_at));
            }
        }
    }
    let probe_at = t().saturating_sub(killed_at);
    let probe_dead = ctx.native_eval(n1.wk, "1", 3000);
    let pid_after = ctx.on_main(move || lab::web_process_id(n1.wk));
    let tauri_eval_dead = ctx.eval(&c1, "1");
    sleep(3000);
    let probe_dead_late = ctx.native_eval(n1.wk, "1", 3000);
    let pid_after_late = ctx.on_main(move || lab::web_process_id(n1.wk));
    let events_after: Vec<Value> = snapshot(&EVENT_LOG).into_iter().skip(ev_mark).collect();
    let others_alive = json!({
        "main": pid_alive(pids["main"].as_i64().map(|p| p as i32)),
        "owad-c2": pid_alive(pids["owad-c2"].as_i64().map(|p| p as i32)),
        "owad-c3": pid_alive(pids["owad-c3"].as_i64().map(|p| p as i32)),
        "hid": pid_alive(pids["hid"].as_i64().map(|p| p as i32)),
        "owad-c2 eval": ctx.native_eval(n2.wk, "1", 2000),
    });
    let is_terminated = |v: &Value| v["ok"] == false && v["domain"] == "WKErrorDomain" && v["code"] == 2;
    let any_wk_error = |v: &Value| v["ok"] == false && v["domain"] == "WKErrorDomain";
    let verified = alive["ok"] == true
        && hidden_probe["ok"] == true
        && hung_probe["timeout"] == true
        && is_terminated(&probe_dead)
        && kill_rc == 0
        && shared_with.is_empty();
    json!({
        "terminateHookInstalled": hook_enabled(),
        "pids": pids,
        "victimSharedWith": shared_with,
        "mainWindowOcclusionVisible": occl_main,
        "alive": { "probe": alive, "visibilityState": alive_vis, "ticksPer2s": main_t1.as_i64().unwrap_or(0) - main_t0.as_i64().unwrap_or(0) },
        "throttledHidden": { "windowOcclusionVisible": occl_hid, "visibilityState": hidden_vis, "ticksPer2s": t1.as_i64().unwrap_or(0) - t0.as_i64().unwrap_or(0), "probe": hidden_probe },
        "hung": { "probeWithin1500ms": hung_probe, "busyResult": busy_done, "probeAfterBusy": hung_probe_late },
        "killed": { "pid": victim, "killRc": kill_rc, "killedAt": killed_at, "probeSentMsAfterKill": probe_at, "webProcessIdTrace(msAfterKill,pid)": pid_trace, "hookFiredAfterMs": hook_ms, "probe": probe_dead, "webProcessIdAfter": pid_after, "tauriEvalCallback": tauri_eval_dead, "probeAfter3s": probe_dead_late, "webProcessIdAfter3s": pid_after_late, "eventsAfterKill": events_after, "othersAlive": others_alive },
        "verdict": {
            "aliveProbeOk": alive["ok"] == true,
            "throttledProbeOk": hidden_probe["ok"] == true,
            "hungProbeTimesOut": hung_probe["timeout"] == true,
            "deadProbeIsWKErrorWebContentProcessTerminated": is_terminated(&probe_dead),
            "deadProbeIsAnyWKError": any_wk_error(&probe_dead),
            "deadProbeError": { "domain": probe_dead["domain"], "code": probe_dead["code"] },
            "pageAutoReloadedAfterKill": events_after.iter().any(|e| e["label"] == "owad-c1" && e["event"] == "page-load Started"),
            "verified": verified,
        },
    })
}

// -------------------------------------------------------------- A6 storage

const SHIM: &str = "/*ow-shim*/ window.__shimRuns = (window.__shimRuns || 0) + 1;";
const RESTORE_MARKER: &str = "/*ow-ss-restore*/";

fn snapshot_js(origin: &str) -> String {
    format!(
        "(() => {{ if (window.top !== window || location.origin !== {origin:?}) return null; const o = {{}}; for (let i = 0; i < sessionStorage.length; i++) {{ const k = sessionStorage.key(i); o[k] = sessionStorage.getItem(k); }} const s = JSON.stringify(o); return s.length > 1048576 ? 'TOO_BIG:' + s.length : s; }})()"
    )
}

fn prelude(origin: &str, json_data: &str) -> String {
    // The data is embedded as a JSON string literal and parsed (no eval).
    let literal = serde_json::to_string(json_data).unwrap_or_else(|_| "\"{}\"".into());
    format!(
        "{RESTORE_MARKER}(function () {{ try {{ if (window.top !== window || location.origin !== {origin:?}) return; var d = JSON.parse({literal}); for (var k in d) sessionStorage.setItem(k, d[k]); window.__restoredBy = 'prelude'; }} catch (e) {{ window.__restoreError = String(e); }} }})();"
    )
}

fn storage(ctx: &Ctx) -> Value {
    let origin = format!("http://127.0.0.1:{}", ctx.fx.port);
    let main = ctx.window("main", 900.0, 600.0, 40.0, 40.0);
    let label = "owad-s";
    progress("storage: window");
    let first = add_guest(
        &main,
        guest_builder(ctx, label, "/storage.html").initialization_script(SHIM),
        100.0,
        100.0,
        300.0,
        250.0,
    )
    .expect("guest");
    progress("storage: guest added");
    ctx.wait_ready(&first);
    progress("storage: ready");
    let set = ctx.eval(&first, "(sessionStorage.setItem('k1', 'v1'), sessionStorage.setItem('uni \\u2603', '\\u2603 snow \\u2713 \"q\" <x>'), sessionStorage.setItem('big', 'x'.repeat(200000)), sessionStorage.length)");
    let old_n = ctx.native(&first);
    let old_pid = ctx.on_main(move || lab::web_process_id(old_n.wk));

    progress("storage: set");
    // 1. Snapshot (bounded 200 ms).
    let snap = ctx.native_eval(old_n.wk, &snapshot_js(&origin), 200);
    let data = snap["value"].as_str().unwrap_or("{}").to_owned();

    // Size bound: ~0.9 MiB and ~2 MiB of storage.
    ctx.eval(&first, "(sessionStorage.setItem('mb', 'y'.repeat(600000)), 1)");
    let snap_09 = ctx.native_eval(old_n.wk, &snapshot_js(&origin), 200);
    ctx.eval(&first, "(sessionStorage.setItem('mb', 'y'.repeat(2000000)), 1)");
    let snap_2 = ctx.native_eval(old_n.wk, &snapshot_js(&origin), 200);
    ctx.eval(&first, "(sessionStorage.removeItem('mb'), 1)");
    let size_rows = json!({
        "snapshot~0.25MiB": { "ok": snap["ok"], "ms": snap["ms"], "timeout": snap["timeout"], "chars": snap["value"].as_str().map(str::len) },
        "snapshot~0.85MiB": { "ok": snap_09["ok"], "ms": snap_09["ms"], "timeout": snap_09["timeout"], "chars": snap_09["value"].as_str().map(str::len) },
        "snapshot~2MiB": { "ok": snap_2["ok"], "ms": snap_2["ms"], "timeout": snap_2["timeout"], "value": snap_2["value"].as_str().map(|s| if s.len() > 40 { format!("{}…", &s[..40]) } else { s.to_owned() }) },
    });

    // 2. Recreate: close the old view, add a new one with the same label and the restore prelude.
    let recreate = |ctx: &Ctx, old: Webview, data: &str| -> (Value, Option<Webview>) {
        let started = Instant::now();
        let closed = old.close().map_err(|e| e.to_string());
        progress("storage: old closed");
        let builder = guest_builder(ctx, label, "/storage.html")
            .initialization_script(SHIM)
            .initialization_script(prelude(&origin, data));
        let first_try = add_guest(&main, builder, 100.0, 100.0, 300.0, 250.0);
        progress(&format!("storage: add_child first try ok={}", first_try.is_ok()));
        let (wv, retried, err) = match first_try {
            Ok(wv) => (Some(wv), false, None),
            Err(e) => {
                let e = e.to_string();
                let mut got = None;
                for _ in 0..50 {
                    sleep(20);
                    if ctx.app.get_webview(label).is_none() {
                        let b = guest_builder(ctx, label, "/storage.html")
                            .initialization_script(SHIM)
                            .initialization_script(prelude(&origin, data));
                        got = add_guest(&main, b, 100.0, 100.0, 300.0, 250.0).ok();
                        break;
                    }
                }
                (got, true, Some(e))
            }
        };
        (
            json!({ "close": closed, "addChildFirstTry": err.is_none(), "firstTryError": err, "retried": retried, "ms": started.elapsed().as_millis() as u64 }),
            wv,
        )
    };
    progress("storage: snapshots done");
    let (rec1, wv1) = recreate(ctx, first.clone(), &data);
    progress("storage: recreated");
    let Some(wv1) = wv1 else {
        return json!({ "error": "recreate failed", "recreate": rec1, "verdict": { "verified": false } });
    };
    progress("storage: wait ready 1");
    ctx.wait_ready(&wv1);
    progress("storage: ready 1");
    let n1 = ctx.native(&wv1);
    progress("storage: native 1");
    let new_pid = ctx.on_main(move || lab::web_process_id(n1.wk));
    let after_recreate = ctx.eval(&wv1, "({ first: window.__first, restoredBy: window.__restoredBy || null, restoreError: window.__restoreError || null, shimRuns: window.__shimRuns, bigLen: (sessionStorage.getItem('big') || '').length })");
    let restored_before_scripts = after_recreate["first"]["k1"] == "v1"
        && after_recreate["first"]["uni"] == "\u{2603} snow \u{2713} \"q\" <x>"
        && after_recreate["first"]["lengths"]["big"] == 200000;

    // 3. One-shot: page clears its storage, then reloads in place.
    ctx.eval(&wv1, "(sessionStorage.clear(), sessionStorage.setItem('after', '1'), 1)");
    if std::env::var("SPIKE_SKIP_RELOAD1").is_err() {
        let _ = wv1.reload();
        sleep(300);
        ctx.wait_ready(&wv1);
    }
    let plain_reload = ctx.eval(&wv1, "({ first: window.__first, restoredBy: window.__restoredBy || null })");
    let plain_reapplied = plain_reload["first"]["k1"] == "v1";

    // 4. Same, with the prelude removed natively after the recreated document loaded.
    ctx.eval(
        &wv1,
        "(sessionStorage.clear(), sessionStorage.setItem('k1', 'v1'), sessionStorage.setItem('k2', 'v2'), 1)",
    );
    let snap2 = ctx.native_eval(n1.wk, &snapshot_js(&origin), 200);
    let data2 = snap2["value"].as_str().unwrap_or("{}").to_owned();
    let (rec2, wv2) = recreate(ctx, wv1.clone(), &data2);
    let Some(wv2) = wv2 else {
        return json!({ "error": "second recreate failed", "recreate": rec2, "verdict": { "verified": false } });
    };
    progress("storage: wait ready 2");
    ctx.wait_ready(&wv2);
    progress("storage: ready 2");
    let n2 = ctx.native(&wv2);
    progress("storage: native 2");
    let after_recreate2 = ctx.eval(&wv2, "({ first: window.__first, restoredBy: window.__restoredBy || null })");
    let controller = n2.controller;
    progress("storage: removing");
    let removal = ctx.on_main(move || lab::remove_user_scripts_containing(controller, RESTORE_MARKER));
    progress("storage: removed");
    ctx.eval(&wv2, "(sessionStorage.clear(), sessionStorage.setItem('after', '1'), 1)");
    let _ = wv2.reload();
    sleep(300);
    ctx.wait_ready(&wv2);
    let removed_reload = ctx.eval(&wv2, "({ first: window.__first, restoredBy: window.__restoredBy || null, shimRuns: window.__shimRuns, hasTauriInternals: typeof window.__TAURI_INTERNALS__ })");
    let one_shot_ok = removed_reload["first"]["keys"] == json!(["after"]) && removed_reload["shimRuns"] == 1;

    let verified = snap["ok"] == true && restored_before_scripts && one_shot_ok;
    json!({
        "origin": origin,
        "set": set,
        "pids": { "old": old_pid, "recreated": new_pid },
        "sizes": size_rows,
        "recreate1": rec1,
        "afterRecreate": after_recreate,
        "plainPrelude_inPlaceReloadAfterClear": plain_reload,
        "recreate2": rec2,
        "afterRecreate2": after_recreate2,
        "preludeRemoval": { "userScriptsBefore": removal.0, "removed": removal.1, "after": removal.2 },
        "removedPrelude_inPlaceReloadAfterClear": removed_reload,
        "events": snapshot(&EVENT_LOG),
        "verdict": {
            "snapshotWithin200ms": snap["ok"] == true,
            "restoredBeforeFirstPageScript": restored_before_scripts,
            "plainPreludeReappliesOnLaterReload(problem)": plain_reapplied,
            "oneShotByNativeRemoval": one_shot_ok,
            "verified": verified,
        },
    })
}
