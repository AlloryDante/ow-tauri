//! Helpers shared by the mock-runtime suites: the fixture app, IPC calls
//! through the compiled ACL, a capturing analytics transport, a Channel
//! recorder, and a runner that drives the mock event loop past
//! `RunEvent::Ready`.
//!
//! Tauri's mock runtime handles one event-loop message per second, never
//! loads a page and never reports window events. Everything here reaches
//! the plugin through what an app has: commands invoked from a webview
//! (`get_ipc_response`), the test-util `Builder` hooks, `OverwolfExt`,
//! `RunEvent`s and the files under the state directory.

#![allow(
    dead_code,
    unreachable_pub,
    reason = "every test binary uses a subset of the helpers"
)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers fail the test on any unexpected error"
)]

use std::collections::BTreeMap;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, AppHandle, Manager, RunEvent, Url, Webview, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_overwolf::Builder;
use tauri_plugin_overwolf::analytics::{
    BoxFuture, CMP_EU_ONLY_URL, COUNTER_URL, HostRequest, HostResponse, INSERT_STATS_URL, Transport,
};

/// The ad document, inside the ad guests' runtime capability.
pub const ADVIEW_PAGE: &str = tauri_plugin_overwolf::ads::ADVIEW_URL;
/// A consent page, inside the consent windows' runtime capability.
pub const CMP_PAGE: &str = tauri_plugin_overwolf::consent::STARTUP_CMP_URL;
/// The longest a run may take before the runner fails the test.
pub const RUN_LIMIT: Duration = Duration::from_secs(120);
/// The prefix of a JavaScript `Channel` argument (Tauri's IPC format).
pub const CHANNEL_PREFIX: &str = "__CHANNEL__:";

/// Locks `m`, ignoring poisoning (a failed assertion elsewhere).
pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An empty directory for one app's state.
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ow-tauri-acl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The fixture's context with `extra` merged into `plugins.overwolf` (one
/// level deep), the state in a fresh directory, and config windows
/// `titles` (`[label, title]`, not created). Returns the state directory.
pub fn context(
    name: &str,
    extra: &Value,
    titles: &[(&str, &str)],
) -> (tauri::Context<MockRuntime>, PathBuf) {
    let dir = temp_dir(name);
    let mut context = ow_tauri_acl_tests::context();
    let plugins = &mut context.config_mut().plugins.0;
    let mut block = plugins
        .get("overwolf")
        .cloned()
        .unwrap_or_else(|| json!({}));
    block["state"] = json!({ "appDataDir": dir });
    if let (Some(base), Some(more)) = (block.as_object_mut(), extra.as_object()) {
        for (k, v) in more {
            match (base.get_mut(k), v) {
                (Some(Value::Object(b)), Value::Object(m)) => b.extend(m.clone()),
                _ => {
                    base.insert(k.clone(), v.clone());
                }
            }
        }
    }
    plugins.insert("overwolf".into(), block);
    for (label, title) in titles {
        let window: tauri::utils::config::WindowConfig = serde_json::from_value(json!({
            "label": label,
            "title": title,
            "create": false
        }))
        .unwrap();
        context.config_mut().app.windows.push(window);
    }
    (context, dir)
}

/// A mock app with `plugin` built from `context`; `channels` records every
/// Channel message instead of evaluating it in a page.
pub fn build(
    context: tauri::Context<MockRuntime>,
    plugin: Builder,
    channels: Option<&Arc<Channels>>,
) -> App<MockRuntime> {
    let mut builder = mock_builder().plugin(plugin.build());
    if let Some(channels) = channels {
        let sink = Arc::clone(channels);
        builder = builder.channel_interceptor(move |webview, callback, _index, body| {
            sink.record(webview.label(), callback, body);
            true
        });
    }
    builder.build(context).unwrap()
}

/// A mock app with the plugin's defaults and the app windows `labels`.
pub fn app(name: &str, labels: &[&str]) -> App<MockRuntime> {
    let (context, _dir) = context(name, &json!({}), &[]);
    let app = build(context, Builder::new(), None);
    for label in labels {
        window(&app, label, WebviewUrl::default());
    }
    app
}

/// An app window `label` (one webview of the same label) on `url`.
pub fn window<M: Manager<MockRuntime>>(manager: &M, label: &str, url: WebviewUrl) {
    WebviewWindowBuilder::new(manager, label, url)
        .build()
        .unwrap();
}

/// A child webview `label` on `url` in window `window` (Tauri's `unstable`
/// child webviews, which the plugin's `ads` feature enables on Windows and
/// macOS).
#[cfg(any(windows, target_os = "macos"))]
pub fn child_webview<M: Manager<MockRuntime>>(
    manager: &M,
    window: &str,
    label: &str,
    url: WebviewUrl,
) {
    manager
        .get_window(window)
        .unwrap()
        .add_child(
            tauri::webview::WebviewBuilder::new(label, url),
            tauri::LogicalPosition::new(0.0, 0.0),
            tauri::LogicalSize::new(300.0, 250.0),
        )
        .unwrap();
}

/// The webview `label` (also a child webview on Windows and macOS).
pub fn webview<M: Manager<MockRuntime>>(manager: &M, label: &str) -> Option<Webview<MockRuntime>> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        manager.get_webview(label)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        manager
            .get_webview_window(label)
            .map(|w| AsRef::<Webview<MockRuntime>>::as_ref(&w).clone())
    }
}

/// The origin Tauri serves app pages from on this OS.
pub fn origin() -> &'static str {
    if cfg!(any(windows, target_os = "android")) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    }
}

/// Invokes `plugin:overwolf|<cmd>` from `webview` as a page at `url`, through
/// Tauri's ACL and the plugin's handler.
pub fn invoke_from(
    webview: &Webview<MockRuntime>,
    url: &str,
    cmd: &str,
    body: Value,
) -> Result<Value, Value> {
    /// `get_ipc_response` takes anything that is `AsRef<Webview>`, which a
    /// `Webview` itself is not.
    struct Caller<'a>(&'a Webview<MockRuntime>);
    impl AsRef<Webview<MockRuntime>> for Caller<'_> {
        fn as_ref(&self) -> &Webview<MockRuntime> {
            self.0
        }
    }
    get_ipc_response(
        &Caller(webview),
        InvokeRequest {
            cmd: format!("{}{cmd}", tauri_plugin_overwolf::COMMAND_PREFIX),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: url.parse().unwrap(),
            body: InvokeBody::Json(body),
            headers: tauri::http::HeaderMap::default(),
            invoke_key: INVOKE_KEY.to_owned(),
        },
    )
    .map(|b| match b {
        InvokeResponseBody::Json(s) => serde_json::from_str(&s).unwrap(),
        InvokeResponseBody::Raw(_) => Value::Null,
    })
}

/// [`invoke_from`] the webview labelled `label`.
pub fn invoke<M: Manager<MockRuntime>>(
    manager: &M,
    label: &str,
    url: &str,
    cmd: &str,
    body: Value,
) -> Result<Value, Value> {
    let Some(webview) = webview(manager, label) else {
        return Err(json!(format!("test: no webview {label}")));
    };
    invoke_from(&webview, url, cmd, body)
}

/// Where a call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Refused by Tauri's ACL.
    Acl,
    /// Allowed by the ACL but no handler (not registered).
    NotRegistered,
    /// Refused by the plugin's caller gate (or another `forbidden`).
    Forbidden,
    /// Reached the handler (which may reject its arguments).
    Reached,
}

/// The [`Outcome`] of an invoke result.
pub fn outcome(result: &Result<Value, Value>) -> Outcome {
    match result {
        Err(Value::String(s)) if s.contains("not allowed") => Outcome::Acl,
        Err(Value::String(s)) if s.contains("not found") => Outcome::NotRegistered,
        Err(e) if e.get("code") == Some(&json!("forbidden")) => Outcome::Forbidden,
        _ => Outcome::Reached,
    }
}

/// The plugin error code of a failed call (`None` for a success or a
/// Tauri error string).
pub fn code(result: &Result<Value, Value>) -> Option<&str> {
    result.as_ref().err()?.get("code")?.as_str()
}

/// Arguments that reach a handler without lasting effect: commands that
/// would wait for `RunEvent::Ready` or send a request get invalid ones.
pub fn probe_body(cmd: &str) -> Value {
    match cmd {
        "set_window_name" | "adview_event" | "cmp_event" => json!({ "name": "probe" }),
        "set_analytics_user_enabled" | "set_anonymous_analytics_preference" => {
            json!({ "enabled": true })
        }
        "generate_user_email_hashes" => json!({ "email": "user@example.com" }),
        _ => json!({}),
    }
}

/// A valid `adview_mount` body for element `element_id`, its events on
/// the page's Channel `channel`.
pub fn mount_body(element_id: &str, channel: u32) -> Value {
    json!({
        "request": {
            "elementId": element_id,
            "attributes": {
                "cid": "example-cid",
                "slotsize": "300x250",
                "adstyle": "default",
                "performance": false
            },
            "rect": { "x": 10.0, "y": 20.0, "width": 300.0, "height": 250.0 },
            "visible": true,
            "devicePixelRatio": 1.0,
            "innerWidth": 800.0
        },
        "onEvent": format!("{CHANNEL_PREFIX}{channel}")
    })
}

/// Waits until `done` holds, at most `limit`; returns whether it held.
pub fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    done()
}

/// Records every request; answers `cmp-eu-only` with its body (or never)
/// and everything else with an empty 200.
pub struct Capture {
    requests: Mutex<Vec<HostRequest>>,
    eu_only: Option<Vec<u8>>,
}

impl Capture {
    /// `cmp-eu-only` answers `body`.
    pub fn answering(body: &str) -> Arc<Self> {
        Arc::new(Capture {
            requests: Mutex::new(Vec::new()),
            eu_only: Some(body.as_bytes().to_vec()),
        })
    }

    /// `cmp-eu-only` never answers: no startup consent window opens while
    /// a test runs (consent stays at its `euOnlyTimeoutMs` bound, 60 s).
    pub fn hanging_eu_only() -> Arc<Self> {
        Arc::new(Capture {
            requests: Mutex::new(Vec::new()),
            eu_only: None,
        })
    }

    /// Every request so far.
    pub fn requests(&self) -> Vec<HostRequest> {
        lock(&self.requests).clone()
    }

    /// The analytics requests (Counter and `InsertStats`) so far, in order.
    pub fn analytics(&self) -> Vec<HostRequest> {
        self.requests()
            .into_iter()
            .filter(|r| r.url.starts_with(COUNTER_URL) || r.url.starts_with(INSERT_STATS_URL))
            .collect()
    }

    /// How many `cmp-eu-only` requests were sent.
    pub fn eu_only_count(&self) -> usize {
        self.requests()
            .iter()
            .filter(|r| r.url.starts_with(CMP_EU_ONLY_URL))
            .count()
    }

    /// The Counter `Extra` objects named `name` (`tauri_window_closed`, ...).
    pub fn extras(&self, name: &str) -> Vec<Value> {
        self.requests()
            .iter()
            .filter_map(|r| {
                let pairs = query(&r.url);
                (pairs.get("Name").map(String::as_str) == Some(name))
                    .then(|| serde_json::from_str(pairs.get("Extra")?).ok())?
            })
            .collect()
    }
}

impl Transport for Capture {
    fn send(&self, request: HostRequest) -> BoxFuture<Result<HostResponse, String>> {
        let eu_only = request.url.starts_with(CMP_EU_ONLY_URL);
        lock(&self.requests).push(request);
        match (eu_only, self.eu_only.clone()) {
            (true, None) => Box::pin(std::future::pending()),
            (true, Some(body)) => Box::pin(async move {
                Ok(HostResponse {
                    status: 200,
                    body,
                    ..HostResponse::default()
                })
            }),
            (false, _) => Box::pin(async {
                Ok(HostResponse {
                    status: 200,
                    ..HostResponse::default()
                })
            }),
        }
    }
}

/// The decoded query of `url`.
pub fn query(url: &str) -> BTreeMap<String, String> {
    Url::parse(url)
        .map(|u| u.query_pairs().into_owned().collect())
        .unwrap_or_default()
}

/// What one analytics request reports: the Counter name, or
/// `InsertStats <Kind>`.
pub fn event_name(request: &HostRequest) -> String {
    if let Some(name) = query(&request.url).get("Name") {
        return name.clone();
    }
    let kind = request
        .body
        .as_deref()
        .and_then(|b| serde_json::from_slice::<Value>(b).ok())
        .and_then(|v| v.get("Kind").and_then(Value::as_u64))
        .unwrap_or_default();
    format!("InsertStats {kind}")
}

/// Every Channel message, by the webview it was sent to and the page's
/// channel id (Tauri's `Builder::channel_interceptor`).
#[derive(Default)]
pub struct Channels(Mutex<Vec<(String, u32, Value)>>);

impl Channels {
    fn record(&self, webview: &str, callback: CallbackFn, body: &InvokeResponseBody) {
        let value = match body {
            InvokeResponseBody::Json(s) => serde_json::from_str(s).unwrap_or(Value::Null),
            InvokeResponseBody::Raw(_) => Value::Null,
        };
        lock(&self.0).push((webview.to_owned(), callback.0, value));
    }

    /// The messages channel `id` of webview `webview` received, in order.
    pub fn messages(&self, webview: &str, id: u32) -> Vec<Value> {
        lock(&self.0)
            .iter()
            .filter(|(w, c, _)| w == webview && *c == id)
            .map(|(_, _, v)| v.clone())
            .collect()
    }

    /// The `name` of every message channel `id` of `webview` received.
    pub fn names(&self, webview: &str, id: u32) -> Vec<String> {
        self.messages(webview, id)
            .iter()
            .map(|m| m["name"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    /// Every `(webview, channel id)` that received a message.
    pub fn receivers(&self) -> Vec<(String, u32)> {
        let mut all: Vec<(String, u32)> = lock(&self.0)
            .iter()
            .map(|(w, c, _)| (w.clone(), *c))
            .collect();
        all.sort();
        all.dedup();
        all
    }
}

/// Held while a mock event loop runs. In a debug build on macOS, Tauri's
/// `RunEvent::Ready` handler sets the Dock icon (`setApplicationIconImage`)
/// on whatever thread runs the loop, which is a test thread here. `AppKit`
/// drawing from two test threads at once crashes the binary (SIGSEGV /
/// SIGTRAP in `-[NSDockTile display]`, seen on the macOS CI runner and in
/// about 2 of 3 local runs of `lifecycle`), so the loops take turns.
static EVENT_LOOP: Mutex<()> = Mutex::new(());

/// Runs `app` on the mock event loop. Once `RunEvent::Ready` arrived (the
/// plugin's start, DESIGN §4.2), `worker` runs on a thread of its own;
/// then every window is destroyed so the loop ends (`ExitRequested`,
/// `Exit`). `on_event` sees every event on the loop thread after the
/// plugin. Returns the worker's value; a worker panic fails the test after
/// the app exited. The run fails when the app has not exited after
/// [`RUN_LIMIT`].
///
/// One event loop runs at a time in a test binary ([`EVENT_LOOP`]).
pub fn run_with<T, W, E>(app: App<MockRuntime>, worker: W, mut on_event: E) -> T
where
    T: Send + 'static,
    W: FnOnce(&AppHandle<MockRuntime>) -> T + Send + 'static,
    E: FnMut(&AppHandle<MockRuntime>, &RunEvent) + 'static,
{
    let _one_loop = EVENT_LOOP.lock().unwrap_or_else(PoisonError::into_inner);
    let slot: Arc<Mutex<Option<std::thread::Result<T>>>> = Arc::new(Mutex::new(None));
    let out = Arc::clone(&slot);
    let mut worker = Some(worker);
    let started = Instant::now();
    app.run(move |handle, event| {
        if let (RunEvent::Ready, Some(worker)) = (&event, worker.take()) {
            let handle = handle.clone();
            let out = Arc::clone(&out);
            std::thread::spawn(move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker(&handle)));
                *lock(&out) = Some(result);
                close_all(&handle);
            });
        }
        if let RunEvent::MainEventsCleared = event {
            assert!(
                started.elapsed() < RUN_LIMIT,
                "the app did not exit within {RUN_LIMIT:?}"
            );
        }
        on_event(handle, &event);
    });
    let result = lock(&slot).take().expect("the worker ran");
    match result {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// [`run_with`] without an event observer.
pub fn run<T, W>(app: App<MockRuntime>, worker: W) -> T
where
    T: Send + 'static,
    W: FnOnce(&AppHandle<MockRuntime>) -> T + Send + 'static,
{
    run_with(app, worker, |_, _| {})
}

/// Destroys every window (app and plugin windows), so the mock loop asks
/// to exit once its window map is empty.
pub fn close_all(handle: &AppHandle<MockRuntime>) {
    #[cfg(any(windows, target_os = "macos"))]
    for window in handle.windows().into_values() {
        let _ = window.destroy();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    for window in handle.webview_windows().into_values() {
        let _ = window.destroy();
    }
}

/// A loopback HTTP/1.1 server: every request line is recorded and answered
/// by `route` (`None`: 404), one request per connection.
pub struct Server {
    /// `http://127.0.0.1:<port>`.
    pub base: String,
    seen: Arc<Mutex<Vec<String>>>,
}

/// What a [`Server`] answers for a request path (query removed).
pub type Route = Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

impl Server {
    /// Starts a server on a free loopback port.
    pub fn start(route: Route) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                loop {
                    let mut header = String::new();
                    match reader.read_line(&mut header) {
                        Ok(0) | Err(_) => break,
                        Ok(_) if header == "\r\n" => break,
                        Ok(_) => {}
                    }
                }
                let line = line.trim_end().to_owned();
                let path = line
                    .split(' ')
                    .nth(1)
                    .unwrap_or_default()
                    .split('?')
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                lock(&log).push(line);
                let (status, body) = match route(&path) {
                    Some(body) => ("200 OK", body),
                    None => ("404 Not Found", Vec::new()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        Server { base, seen }
    }

    /// Every request line so far (`GET /feed/latest.yml?... HTTP/1.1`).
    pub fn seen(&self) -> Vec<String> {
        lock(&self.seen).clone()
    }
}
