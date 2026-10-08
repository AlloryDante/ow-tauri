//! W0c spike B (Windows mechanics). One Tauri app that stands up an ad-guest
//! child webview over a LOOPBACK fixture page (never an ad) and runs the
//! DESIGN-v2 §10 W0c-B probes B1–B6 and B8, writing a JSON verdict per item
//! to `SPIKE_OUT`. The COM probes are Windows-only (src/win.rs); on other OSes
//! they answer `{"unsupported": ...}` so the app still cargo-checks locally.
//!
//! Lab rules: test/loopback only, never an ad, never a click on an ad. On the
//! Windows CI runner the windows may show on the runner's own desktop, which
//! is nobody's screen.

#[cfg(windows)]
mod win;
mod loopback;

use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// The privileged app command the SEC-B1 (B5) probe tries to reach from a
/// guest's local-origin frame. Its capability is scoped to window `main`.
#[tauri::command]
fn spike_marker() -> String {
    "SPIKE-SECRET-MARKER".to_owned()
}

/// Shared record of native (COM) events the Windows hooks observe, keyed by
/// guest label.
#[derive(Default)]
pub struct Events(pub Mutex<Value>);

#[cfg_attr(not(windows), allow(dead_code))]
impl Events {
    fn push(&self, label: &str, kind: &str, entry: Value) {
        let mut g = self.0.lock().unwrap();
        if !g.is_object() {
            *g = json!({});
        }
        let arr = g
            .as_object_mut()
            .unwrap()
            .entry(format!("{label}:{kind}"))
            .or_insert_with(|| json!([]));
        if let Some(a) = arr.as_array_mut() {
            a.push(entry);
        }
    }
    fn snapshot(&self) -> Value {
        self.0.lock().unwrap().clone()
    }
}

/// Runs `js` in the webview `label` and returns its JSON result (or null on
/// timeout). Works from any thread.
fn eval(app: &tauri::AppHandle, label: &str, js: &str) -> Value {
    let Some(webview) = app.webviews().get(label).cloned() else {
        return json!({ "error": format!("no webview {label}") });
    };
    let (tx, rx) = mpsc::channel();
    let wrapped = format!("(() => {{ try {{ return JSON.stringify({js}); }} catch (e) {{ return JSON.stringify({{ error: String(e) }}); }} }})()");
    if webview
        .eval_with_callback(wrapped, move |s| {
            let _ = tx.send(s);
        })
        .is_err()
    {
        return json!({ "error": "eval dispatch failed" });
    }
    match rx.recv_timeout(Duration::from_secs(6)) {
        Ok(s) => serde_json::from_str::<Value>(&s)
            .ok()
            .and_then(|v| v.as_str().map(|inner| serde_json::from_str::<Value>(inner).unwrap_or(Value::String(inner.to_owned()))))
            .unwrap_or(Value::Null),
        Err(_) => json!({ "error": "eval timeout" }),
    }
}

fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

const MAIN: &str = "main";
const G2: &str = "g2";
const AUX: &str = "aux";
const GUEST_RAW: &str = "owad-raw";
const GUEST_GUARD: &str = "owad-guarded";
const GUEST_AUX: &str = "owad-aux";

fn main() {
    let out = std::env::var("SPIKE_OUT").unwrap_or_else(|_| "win-mechanics-result.json".into());
    let stop = Arc::new(AtomicBool::new(false));
    let port = loopback::start(Arc::clone(&stop)).expect("loopback server");
    let guest_url = format!("http://127.0.0.1:{port}/guest-fixture.html");

    let events: Arc<Events> = Arc::new(Events::default());
    let events_setup = Arc::clone(&events);
    let close_facts: Arc<Mutex<Value>> = Arc::new(Mutex::new(json!({})));
    let close_setup = Arc::clone(&close_facts);

    let builder = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![spike_marker]);

    let guest_url_setup = guest_url.clone();
    let builder = builder.setup(move |app| {
        let handle = app.handle().clone();

        // main window (app origin) + a raw loopback guest (no frame guard).
        let main = WebviewWindowBuilder::new(&handle, MAIN, WebviewUrl::App("index.html".into()))
            .title("win-mechanics main")
            .inner_size(960.0, 640.0)
            .position(20.0, 20.0)
            .resizable(false)
            .build()?;
        add_guest(&handle, &main, GUEST_RAW, &guest_url_setup, &events_setup, false)?;

        // a second app-origin window + a guarded loopback guest.
        let g2 = WebviewWindowBuilder::new(&handle, G2, WebviewUrl::App("index.html".into()))
            .title("win-mechanics g2")
            .inner_size(520.0, 480.0)
            .position(1000.0, 20.0)
            .build()?;
        add_guest(&handle, &g2, GUEST_GUARD, &guest_url_setup, &events_setup, true)?;

        // aux window + guest for the close-timing probe (B4).
        let aux = WebviewWindowBuilder::new(&handle, AUX, WebviewUrl::App("index.html".into()))
            .title("win-mechanics aux")
            .inner_size(400.0, 320.0)
            .position(20.0, 700.0)
            .build()?;
        add_guest(&handle, &aux, GUEST_AUX, &guest_url_setup, &events_setup, false)?;

        // B4: on the aux window's close, record guest liveness inline and
        // again from a check posted through the event-loop proxy (R3).
        let aux_handle = handle.clone();
        let close_rec = Arc::clone(&close_setup);
        aux.on_window_event(move |ev| {
            if let tauri::WindowEvent::CloseRequested { .. } = ev {
                let inline = probe_guest_alive(&aux_handle, GUEST_AUX);
                close_rec.lock().unwrap()["inline"] = inline;
                // Posted through the proxy from a helper thread, so it runs on
                // the event loop AFTER this callback returns (not inline).
                let posted_handle = aux_handle.clone();
                let posted_rec = Arc::clone(&close_rec);
                std::thread::spawn(move || {
                    let (tx, rx) = mpsc::channel();
                    let h2 = posted_handle.clone();
                    let _ = posted_handle.run_on_main_thread(move || {
                        let _ = tx.send(probe_guest_alive(&h2, GUEST_AUX));
                    });
                    if let Ok(v) = rx.recv_timeout(Duration::from_secs(3)) {
                        posted_rec.lock().unwrap()["posted"] = v;
                    } else {
                        posted_rec.lock().unwrap()["posted"] = json!({ "error": "no proxy result" });
                    }
                });
            }
        });

        // Driver thread.
        let drive_handle = handle.clone();
        let drive_events = Arc::clone(&events_setup);
        let drive_close = Arc::clone(&close_setup);
        let guest_origin = format!("http://127.0.0.1:{port}");
        std::thread::spawn(move || {
            let result = drive(&drive_handle, &drive_events, &drive_close, &guest_origin);
            let text = serde_json::to_string_pretty(&result).unwrap_or_default();
            let _ = std::fs::write(&out, &text);
            eprintln!("spike: wrote {out}");
            drive_handle.exit(0);
        });
        Ok(())
    });

    let app = builder
        .build(tauri::generate_context!())
        .expect("build the spike app");
    app.run(|_, _| {});
}

/// Adds an ad-guest child webview over `url` on `window`, with the Windows ads
/// browser args (web security off, as the plugin's guests run) and
/// `focused(false)`. On Windows, installs the COM hooks (and, when `guarded`,
/// the local-origin frame guard). On macOS the child is created too (the shim
/// gives `unstable`), without the COM hooks.
fn add_guest(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    label: &str,
    url: &str,
    events: &Arc<Events>,
    guarded: bool,
) -> tauri::Result<()> {
    let parsed = url.parse().expect("guest url");
    #[allow(unused_mut)]
    let mut builder = tauri::webview::WebviewBuilder::new(label, WebviewUrl::External(parsed))
        .focused(false);
    #[cfg(windows)]
    {
        builder = builder.additional_browser_args(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
             --disable-background-timer-throttling --disable-renderer-backgrounding \
             --disable-backgrounding-occluded-windows --disable-web-security \
             --allow-running-insecure-content",
        );
    }
    let win_handle = window.as_ref().window();
    let guest = spike_miniplugin::add_guest(
        &win_handle,
        builder,
        tauri::LogicalPosition::new(480.0, 0.0),
        tauri::LogicalSize::new(matches_size(window), 480.0),
    )?;
    let _ = app;
    #[cfg(windows)]
    {
        let label = label.to_owned();
        let events = Arc::clone(events);
        guest.with_webview(move |pw| {
            win::install_guest_hooks(&label, &pw.controller(), events, guarded);
        })?;
    }
    #[cfg(not(windows))]
    {
        let _ = (events, guarded, &guest);
    }
    Ok(())
}

fn matches_size(_window: &tauri::WebviewWindow) -> f64 {
    440.0
}

/// Whether the guest webview still exists and can evaluate script right now.
fn probe_guest_alive(app: &tauri::AppHandle, label: &str) -> Value {
    let present = app.webviews().contains_key(label);
    let alive = if present {
        eval(app, label, "(typeof window.__alive === 'function' ? window.__alive() : 'no-fn')")
    } else {
        Value::Null
    };
    json!({ "present": present, "evalResult": alive })
}

/// Shape regex for the Windows default WebView2 UA (reduced UA), DESIGN §4.10.
#[cfg_attr(not(windows), allow(dead_code))]
fn ua_shape_ok(ua: &str) -> bool {
    // Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/<n>.0.0.0 Safari/537.36 Edg/<n>.0.0.0
    let ok_prefix = ua.starts_with("Mozilla/5.0 (Windows NT ")
        && ua.contains("AppleWebKit/537.36 (KHTML, like Gecko) Chrome/")
        && ua.contains(" Safari/537.36 Edg/");
    let reduced = ua.contains(".0.0.0 Safari/537.36 Edg/") && ua.contains(".0.0.0");
    ok_prefix && reduced
}

fn drive(
    app: &tauri::AppHandle,
    events: &Arc<Events>,
    close_facts: &Arc<Mutex<Value>>,
    guest_origin: &str,
) -> Value {
    // Let the guests load.
    sleep(2500);
    let mut r = serde_json::Map::new();

    // ---- B1: WebView2 runtime version + webview_version() + fixedRuntime ----
    r.insert("B1".into(), b1(app));

    // ---- B2: UA read + shape ----
    r.insert("B2".into(), b2(app));

    // ---- B6: zoom with ZoomFactor ----
    r.insert("B6".into(), b6(app));

    // ---- B3: IsUserInitiated for popups + script top navigations ----
    r.insert("B3".into(), b3(app, events));

    // ---- B5: SEC-B1 reproduction, raw vs guarded ----
    r.insert("B5".into(), b5(app, events, guest_origin));

    // ---- B4: close timing ----
    r.insert("B4".into(), b4(app, close_facts));

    // ---- B8: raw COM pointer re-wrapped with a different webview2-com ----
    r.insert("B8".into(), b8(app));

    r.insert(
        "meta".into(),
        json!({
            "note": "loopback fixture guests only; never an ad; no ad click",
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        }),
    );
    Value::Object(r)
}

fn b1(app: &tauri::AppHandle) -> Value {
    let version = tauri::webview_version().unwrap_or_else(|e| format!("error: {e}"));
    let _ = app;
    json!({
        "webview_version": version,
        "note": "tauri::webview_version() == wry GetAvailableCoreWebView2BrowserVersionString(null); honours WEBVIEW2_BROWSER_EXECUTABLE_FOLDER which tauri sets for fixedRuntime before runtime creation (tauri app.rs:2483-2496)",
    })
}

#[cfg(windows)]
fn b2(app: &tauri::AppHandle) -> Value {
    let native = win::read_ua(app, MAIN).unwrap_or_else(|e| json!({ "error": e }));
    let nav = eval(app, MAIN, "navigator.userAgent");
    let nav_s = nav.as_str().unwrap_or("");
    json!({
        "native_read": native,
        "navigator_userAgent": nav,
        "shape_ok": ua_shape_ok(nav_s),
        "match_native_vs_navigator": native.get("userAgent").and_then(|v| v.as_str()) == Some(nav_s),
    })
}

#[cfg(windows)]
fn b6(app: &tauri::AppHandle) -> Value {
    let mut out = serde_json::Map::new();
    for z in [0.8_f64, 1.25] {
        let set = win::set_zoom(app, GUEST_RAW, z);
        sleep(400);
        let dpr = eval(app, GUEST_RAW, "window.devicePixelRatio");
        let scale = app
            .webviews()
            .get(GUEST_RAW)
            .and_then(|w| w.window().scale_factor().ok())
            .unwrap_or(1.0);
        out.insert(
            format!("zoom_{z}"),
            json!({ "set": set, "devicePixelRatio": dpr, "scale_factor": scale, "zoom_formula_dpr_over_scale": dpr.as_f64().map(|d| d / scale) }),
        );
    }
    let _ = win::set_zoom(app, GUEST_RAW, 1.0);
    Value::Object(out)
}

#[cfg(windows)]
fn b3(app: &tauri::AppHandle, events: &Arc<Events>) -> Value {
    // 1) a script-initiated window.open WITHOUT any user gesture.
    let _ = eval(app, GUEST_RAW, "(window.open('https://example.com/script-open','_blank'), 'opened')");
    sleep(300);
    // 2) a real OS click on the guest's button, which opens a popup and then
    // (after 50 ms) does a script-initiated top navigation.
    let click = win::click_guest(app, MAIN, GUEST_RAW);
    sleep(700);
    json!({
        "click": click,
        "events": events.snapshot(),
        "reads": "see events owad-raw:new-window (IsUserInitiated) and owad-raw:nav (IsUserInitiated) entries",
    })
}

#[cfg(windows)]
fn b5(app: &tauri::AppHandle, events: &Arc<Events>, guest_origin: &str) -> Value {
    let raw = eval(app, GUEST_RAW, "await window.__secB1()");
    let guarded = eval(app, GUEST_GUARD, "await window.__secB1()");
    json!({
        "guest_origin": guest_origin,
        "raw_no_guard": raw,
        "guarded": guarded,
        "events": events.snapshot(),
        "interpretation": "raw shows the B1 chain; guarded must block the tauri.localhost frame (NavigationStarting/FrameNavigationStarting cancel) and 403 *.localhost; ordering vs wry protocol handler in owad-guarded:wrr / owad-guarded:nav entries",
    })
}

fn b4(app: &tauri::AppHandle, close_facts: &Arc<Mutex<Value>>) -> Value {
    // Close the aux window; the on_window_event handler records liveness.
    // (`get_window`, not `get_webview_window`, which is None once a child
    // webview is attached — SPA F3.)
    if let Some(aux) = app.get_window(AUX) {
        let _ = aux.close();
    }
    sleep(1500);
    close_facts.lock().unwrap().clone()
}

fn b8(_app: &tauri::AppHandle) -> Value {
    json!({
        "note": "built as the separate b8-rewrap crate (two webview2-com-sys versions); see CI out/b8-build.txt / out/b8-run.txt for compile+link+run",
    })
}

// ---- non-Windows stubs so the app cargo-checks on this Mac ----
#[cfg(not(windows))]
fn b2(app: &tauri::AppHandle) -> Value {
    json!({ "unsupported": "windows-only", "navigator_userAgent": eval(app, MAIN, "navigator.userAgent") })
}
#[cfg(not(windows))]
fn b6(_app: &tauri::AppHandle) -> Value {
    json!({ "unsupported": "windows-only (ZoomFactor is a WebView2 API)" })
}
#[cfg(not(windows))]
fn b3(_app: &tauri::AppHandle, _events: &Arc<Events>) -> Value {
    json!({ "unsupported": "windows-only (IsUserInitiated is a WebView2 API)" })
}
#[cfg(not(windows))]
fn b5(app: &tauri::AppHandle, _events: &Arc<Events>, guest_origin: &str) -> Value {
    json!({ "unsupported": "windows-only frame guard", "guest_origin": guest_origin, "raw_no_guard": eval(app, GUEST_RAW, "await window.__secB1()") })
}
