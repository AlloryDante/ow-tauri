//! W0c-A spike: the macOS mechanisms DESIGN-v2 relies on, measured on real
//! WKWebViews with Tauri 2.12.1 (`unstable`, guests as child webviews).
//!
//! `SPIKE_ITEM` picks one item per process (`ua`, `gesture`, `zoom`, `close`,
//! `crash`, `storage`); the result is written to `SPIKE_OUT` as JSON and the
//! app exits. Everything runs in the invisible lab (`lab.rs`); guests load
//! only loopback fixture pages (`fixture.rs`), never ads.

#[cfg(not(target_os = "macos"))]
compile_error!("this spike is macOS-only");

mod fixture;
mod items;
mod lab;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::{AppHandle, Manager, Webview, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use fixture::Fixture;

/// Shared helpers for the items (driver thread only).
pub struct Ctx {
    pub app: AppHandle,
    pub fx: Fixture,
}

/// Native pointers of one webview (addresses; dereferenced on the main
/// thread only).
#[derive(Clone, Copy, Debug)]
pub struct Native {
    pub wk: usize,
    pub controller: usize,
    pub ns_window: usize,
}

impl Ctx {
    pub fn on_main<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = mpsc::channel();
        self.app
            .run_on_main_thread(move || {
                let _ = tx.send(f());
            })
            .expect("run on main thread");
        rx.recv().expect("main thread answer")
    }

    pub fn native(&self, wv: &Webview) -> Native {
        let (tx, rx) = mpsc::channel();
        wv.with_webview(move |pw| {
            let _ = tx.send(Native {
                wk: pw.inner() as usize,
                controller: pw.controller() as usize,
                ns_window: pw.ns_window() as usize,
            });
        })
        .expect("with_webview");
        rx.recv_timeout(Duration::from_secs(5)).expect("platform webview")
    }

    /// Tauri `eval_with_callback` (wry: `evaluateJavaScript`, error dropped).
    pub fn eval(&self, wv: &Webview, js: &str) -> Value {
        let (tx, rx) = mpsc::channel();
        let js = format!("(() => {{ try {{ return {js}; }} catch (e) {{ return {{ error: String(e) }}; }} }})()");
        if wv
            .eval_with_callback(js, move |s| {
                let _ = tx.send(serde_json::from_str::<Value>(&s).unwrap_or(Value::String(s)));
            })
            .is_err()
        {
            return json!({ "evalError": true });
        }
        rx.recv_timeout(Duration::from_secs(5)).unwrap_or(json!({ "timeout": true }))
    }

    /// Native `evaluateJavaScript:completionHandler:` with the error kept,
    /// waiting at most `timeout_ms` for the completion handler.
    pub fn native_eval(&self, wk: usize, js: &str, timeout_ms: u64) -> Value {
        let (tx, rx) = mpsc::channel();
        let js = js.to_owned();
        let started = Instant::now();
        self.on_main(move || lab::eval_native(wk, &js, tx));
        match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
            Ok(v) => v,
            Err(_) => json!({ "timeout": true, "waitedMs": started.elapsed().as_millis() as u64 }),
        }
    }

    /// Starts a native evaluation and returns the receiver (no wait).
    pub fn native_eval_start(&self, wk: usize, js: &str) -> mpsc::Receiver<Value> {
        let (tx, rx) = mpsc::channel();
        let js = js.to_owned();
        self.on_main(move || lab::eval_native(wk, &js, tx));
        rx
    }

    pub fn wait_ready(&self, wv: &Webview) -> bool {
        let start = Instant::now();
        while self.eval(wv, "!!window.__ready") != Value::Bool(true) {
            if start.elapsed() > Duration::from_secs(20) {
                return false;
            }
            sleep(100);
        }
        true
    }

    /// An invisible app window (alpha 0 before it is ever ordered in).
    pub fn window(&self, label: &str, w: f64, h: f64, x: f64, y: f64) -> WebviewWindow {
        let win = WebviewWindowBuilder::new(&self.app, label, WebviewUrl::App("index.html".into()))
            .title(label)
            .inner_size(w, h)
            .position(x, y)
            .resizable(false)
            .visible(false)
            .build()
            .expect("build window");
        let addr = win.ns_window().expect("ns_window") as usize;
        self.on_main(move || lab::make_invisible(addr, true));
        win
    }

    pub fn make_key(&self, win: &WebviewWindow) {
        let addr = win.ns_window().expect("ns_window") as usize;
        lab::KEY.store(addr, std::sync::atomic::Ordering::SeqCst);
        self.on_main(move || {
            let () = unsafe { objc2::msg_send![lab::obj(addr), becomeKeyWindow] };
        });
    }
}

static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Milliseconds since the process started (the spike's one clock; the
/// fixture server's request times use it too).
pub fn t() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

pub fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn main() {
    let item = std::env::var("SPIKE_ITEM").unwrap_or_else(|_| "ua".into());
    let out = std::env::var("SPIKE_OUT").unwrap_or_else(|_| format!("spike-{item}.json"));
    let _ = t();
    std::panic::set_hook(Box::new(|info| {
        eprintln!("spike-panic {} {info}", t());
    }));
    let fx = Fixture::start();

    lab::hold_app_back();

    let builder = tauri::Builder::default().activate_ignoring_other_apps(false);
    let builder = items::configure(&item, builder);
    let item_setup = item.clone();
    let fx_setup = fx.clone();
    let builder = builder.setup(move |app| {
        let handle = app.handle().clone();
        let item = item_setup.clone();
        let out = out.clone();
        let fx = fx_setup.clone();
        std::thread::spawn(move || {
            if let Some(ms) = std::env::var("SPIKE_DELAY_MS").ok().and_then(|v| v.parse().ok()) {
                sleep(ms);
            }
            let ctx = Ctx { app: handle.clone(), fx };
            let started = Instant::now();
            let mut result = items::run(&ctx, &item);
            result["item"] = json!(item);
            result["tauri"] = json!(tauri::VERSION);
            result["webkitVersion"] = json!(tauri::webview_version().ok());
            result["os"] = json!(os_version());
            result["activationsSuppressed"] = json!(lab::activations_suppressed());
            result["elapsedMs"] = json!(started.elapsed().as_millis() as u64);
            let text = serde_json::to_string_pretty(&result).unwrap_or_default();
            let _ = std::fs::write(&out, text);
            eprintln!("spike: wrote {out}: {}", result["verdict"]);
            handle.exit(0);
        });
        Ok(())
    });
    let mut app = builder.build(tauri::generate_context!()).expect("build the spike app");
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    // Closing the last window must not end the run (the close item closes
    // every window it makes); only the driver's `exit(0)` does.
    app.run(|_, event| {
        if let tauri::RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}

fn os_version() -> Value {
    let v = std::process::Command::new("sw_vers").arg("-productVersion").output().ok();
    json!(v.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()))
}

/// Re-export for items.
pub fn get_webview(app: &AppHandle, label: &str) -> Option<Webview> {
    app.get_webview(label)
}
