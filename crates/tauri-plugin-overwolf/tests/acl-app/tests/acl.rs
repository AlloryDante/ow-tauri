//! Mock-runtime tests of the plugin's access control and IPC routing
//! (ARCHITECTURE 5.2, CONTRACT A.2, C).
//!
//! The ACL comes from the plugin's permission sets, the runtime capability
//! the plugin adds for `ow-main`, and `capabilities/renderer.json` (what an
//! app grants its `bw-*` webviews). Every command is invoked from every
//! webview class; a command either passes the ACL or is rejected by it, and
//! the plugin's own class check rejects callers that pass the ACL from the
//! wrong window.

#![expect(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions fail the test on any unexpected error"
)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{
    App, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_overwolf::{Builder, COMMANDS};

/// Commands only `bw-*` webviews may call.
const RENDERER_ONLY: [&str; 4] = ["ipc_invoke", "ipc_send", "ipc_skip", "eval_result"];
/// The `overwolf:renderer` set.
const RENDERER: [&str; 5] = [
    "ipc_subscribe",
    "ipc_invoke",
    "ipc_send",
    "ipc_skip",
    "eval_result",
];

type Captured = Arc<Mutex<Vec<(String, u32, Value)>>>;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ow-tauri-acl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A mock app with the plugin; channel sends are captured instead of
/// evaluated.
fn app(name: &str) -> (App<MockRuntime>, Captured) {
    let captured: Captured = Arc::default();
    let sink = Arc::clone(&captured);
    let mut context = ow_tauri_acl_tests::context();
    context.config_mut().plugins.0.insert(
        "overwolf".into(),
        json!({ "state": { "appDataDir": temp_dir(name) } }),
    );
    let app = mock_builder()
        .channel_interceptor(move |webview, callback: CallbackFn, _index, body| {
            let value = match body {
                InvokeResponseBody::Json(s) => serde_json::from_str(s).unwrap(),
                InvokeResponseBody::Raw(_) => Value::Null,
            };
            sink.lock()
                .unwrap()
                .push((webview.label().to_owned(), callback.0, value));
            true
        })
        .plugin(
            Builder::new()
                .manifest_json(ow_tauri_acl_tests::manifest())
                .companion_plugins(false)
                .main_webview(false)
                .skip_os_queries()
                .argv(vec!["acl-fixture".into()])
                .build(),
        )
        .build(context)
        .unwrap();
    (app, captured)
}

fn origin() -> &'static str {
    if cfg!(any(windows, target_os = "android")) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    }
}

/// `get_ipc_response` takes `AsRef<Webview>`, which only `WebviewWindow`
/// implements; child webviews need this wrapper.
struct AnyWebview(tauri::Webview<MockRuntime>);

impl AsRef<tauri::Webview<MockRuntime>> for AnyWebview {
    fn as_ref(&self) -> &tauri::Webview<MockRuntime> {
        &self.0
    }
}

fn invoke(app: &App<MockRuntime>, label: &str, cmd: &str, body: Value) -> Result<Value, Value> {
    let webview = AnyWebview(app.get_webview(label).unwrap());
    get_ipc_response(
        &webview,
        InvokeRequest {
            cmd: format!("plugin:overwolf|{cmd}"),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: origin().parse().unwrap(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// Rejected by Tauri's ACL.
    Acl,
    /// Passed the ACL; rejected by the plugin's class check.
    Forbidden,
    /// Passed both (the command ran or rejected its arguments).
    Reached,
}

fn outcome(result: &Result<Value, Value>) -> Outcome {
    match result {
        Err(Value::String(s)) if s.contains("not allowed") => Outcome::Acl,
        Err(e) if e.get("code") == Some(&json!("forbidden")) => Outcome::Forbidden,
        _ => Outcome::Reached,
    }
}

/// A body that passes the ACL but cannot run anything with lasting effect:
/// invalid arguments for commands that would exit or open UI.
fn probe_body(cmd: &str) -> Value {
    match cmd {
        "app_exit" => json!({ "code": "not a number" }),
        "app_relaunch" => json!({ "args": 1 }),
        "dialog_open" | "dialog_save" => json!({ "properties": 1 }),
        "dialog_message" => json!({ "buttons": 1 }),
        _ => json!({}),
    }
}

/// Webviews of every class.
fn webviews(app: &App<MockRuntime>) {
    let url = || WebviewUrl::App("index.html".into());
    WebviewWindowBuilder::new(app, "ow-main", url())
        .build()
        .unwrap();
    let bw = WebviewWindowBuilder::new(app, "bw-1", url())
        .build()
        .unwrap();
    let window = bw.as_ref().window();
    let size = LogicalSize::new(10.0, 10.0);
    let at = LogicalPosition::new(0.0, 0.0);
    window
        .add_child(WebviewBuilder::new("owad-1", url()), at, size)
        .unwrap();
    window
        .add_child(WebviewBuilder::new("bw-9", url()), at, size)
        .unwrap();
    let remote = WebviewWindowBuilder::new(app, "bw-2", url())
        .build()
        .unwrap();
    remote
        .as_ref()
        .window()
        .add_child(WebviewBuilder::new("bwr-2", url()), at, size)
        .unwrap();
    WebviewWindowBuilder::new(app, "ow-cmp", url())
        .build()
        .unwrap();
    WebviewWindowBuilder::new(app, "settings", url())
        .build()
        .unwrap();
}

#[test]
fn every_command_is_scoped_to_its_webview_class() {
    let (app, _) = app("matrix");
    webviews(&app);
    let mut failures = Vec::new();
    for cmd in COMMANDS {
        for label in ["ow-main", "bw-1", "owad-1", "bwr-2", "ow-cmp", "settings"] {
            let want = match label {
                "ow-main" if !RENDERER_ONLY.contains(cmd) => Outcome::Reached,
                "bw-1" if RENDERER.contains(cmd) => Outcome::Reached,
                _ => Outcome::Acl,
            };
            if label == "ow-main" && *cmd == "app_quit" {
                continue; // covered by `quit_request_round_trip`
            }
            let got = outcome(&invoke(&app, label, cmd, probe_body(cmd)));
            if got != want {
                failures.push(format!("{cmd} from {label}: want {want:?}, got {got:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn child_webview_named_like_a_window_is_refused_by_the_class_check() {
    let (app, _) = app("child");
    webviews(&app);
    // `bw-9` matches the renderer capability by label, but it is a child of
    // window `bw-1`; the plugin refuses it.
    let r = invoke(&app, "bw-9", "ipc_skip", json!({ "epoch": "x", "seq": 1 }));
    assert_eq!(outcome(&r), Outcome::Forbidden, "{r:?}");
    let r = invoke(&app, "bw-1", "ipc_skip", json!({ "epoch": "x", "seq": 1 }));
    assert_eq!(outcome(&r), Outcome::Reached, "{r:?}");
}

#[test]
fn quit_request_round_trip() {
    let (app, _) = app("quit");
    webviews(&app);
    invoke(&app, "ow-main", "app_quit", json!({})).unwrap();
    // The first request id of a fresh app is 1; preventing cancels the quit.
    invoke(
        &app,
        "ow-main",
        "app_quit_reply",
        json!({ "requestId": 1, "prevent": true }),
    )
    .unwrap();
    let r = invoke(
        &app,
        "ow-main",
        "app_quit_reply",
        json!({ "requestId": 1, "prevent": false }),
    );
    assert_eq!(r.unwrap_err()["code"], "not-found");
}

fn wait_for(
    captured: &Captured,
    pred: impl Fn(&[(String, u32, Value)]) -> bool,
) -> Vec<(String, u32, Value)> {
    let start = Instant::now();
    loop {
        let snapshot = captured.lock().unwrap().clone();
        if pred(&snapshot) {
            return snapshot;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timed out; got {snapshot:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The messages delivered to `label`, flattened from the batches (each
/// channel send carries one `HostMessage[]`).
fn messages(all: &[(String, u32, Value)], label: &str) -> Vec<Value> {
    all.iter()
        .filter(|(l, _, _)| l == label)
        .flat_map(|(_, _, v)| v.as_array().cloned().unwrap_or_default())
        .collect()
}

/// `ow-main` plus one `BrowserWindow` made through `window_create`.
fn main_and_window(app: &App<MockRuntime>) -> String {
    WebviewWindowBuilder::new(app, "ow-main", WebviewUrl::App("index.html".into()))
        .build()
        .unwrap();
    let created = invoke(
        app,
        "ow-main",
        "window_create",
        json!({ "options": { "show": false, "width": 400, "height": 300 }, "preload": null, "windowClass": "ui" }),
    )
    .unwrap();
    assert_eq!(created["id"], 1);
    created["label"].as_str().unwrap().to_owned()
}

#[test]
fn invoke_reaches_main_and_the_reply_reaches_the_window() {
    let (app, captured) = app("route");
    assert_eq!(main_and_window(&app), "bw-1");
    let main_epoch = invoke(
        &app,
        "ow-main",
        "ipc_subscribe",
        json!({ "onMessage": "__CHANNEL__:11" }),
    )
    .unwrap();
    assert!(main_epoch["epoch"].is_string());
    invoke(&app, "ow-main", "ipc_main_ready", json!({})).unwrap();
    let epoch = invoke(
        &app,
        "bw-1",
        "ipc_subscribe",
        json!({ "onMessage": "__CHANNEL__:21" }),
    )
    .unwrap()["epoch"]
        .clone();

    // A stale epoch is refused.
    let stale = invoke(
        &app,
        "bw-1",
        "ipc_invoke",
        json!({ "channel": "ping", "args": [1], "epoch": "stale", "seq": 1 }),
    );
    assert_eq!(stale.unwrap_err()["code"], "not-ready");

    let accepted = invoke(
        &app,
        "bw-1",
        "ipc_invoke",
        json!({ "channel": "ping", "args": [1], "epoch": epoch, "seq": 1 }),
    )
    .unwrap();
    let id = accepted["id"].as_u64().unwrap();

    let all = wait_for(&captured, |m| !messages(m, "ow-main").is_empty());
    let to_main = messages(&all, "ow-main");
    let request = to_main.iter().find(|m| m["type"] == "ipc").unwrap();
    assert_eq!(request["kind"], "invoke");
    assert_eq!(request["id"], id);
    assert_eq!(request["channel"], "ping");
    assert_eq!(request["sender"]["windowId"], 1);
    assert_eq!(request["sender"]["label"], "bw-1");

    // A message to the window sent before the reply arrives before it (C.5).
    invoke(
        &app,
        "ow-main",
        "ipc_emit",
        json!({ "target": 1, "channel": "progress", "args": [50], "seq": 1 }),
    )
    .unwrap();
    invoke(
        &app,
        "ow-main",
        "ipc_reply",
        json!({ "id": id, "ok": true, "value": 2, "seq": 2 }),
    )
    .unwrap();
    let all = wait_for(&captured, |m| messages(m, "bw-1").len() >= 2);
    let to_window = messages(&all, "bw-1");
    assert_eq!(to_window[0]["type"], "ipc");
    assert_eq!(to_window[0]["kind"], "message");
    assert_eq!(to_window[0]["channel"], "progress");
    assert_eq!(
        to_window[1],
        json!({ "type": "ipc-result", "id": id, "ok": true, "value": 2 })
    );
}

#[test]
fn reply_without_value_stays_undefined() {
    let (app, captured) = app("undefined");
    main_and_window(&app);
    invoke(
        &app,
        "ow-main",
        "ipc_subscribe",
        json!({ "onMessage": "__CHANNEL__:11" }),
    )
    .unwrap();
    invoke(&app, "ow-main", "ipc_main_ready", json!({})).unwrap();
    let epoch = invoke(
        &app,
        "bw-1",
        "ipc_subscribe",
        json!({ "onMessage": "__CHANNEL__:21" }),
    )
    .unwrap()["epoch"]
        .clone();
    let id = invoke(
        &app,
        "bw-1",
        "ipc_invoke",
        json!({ "channel": "noop", "args": [], "epoch": epoch, "seq": 1 }),
    )
    .unwrap()["id"]
        .as_u64()
        .unwrap();
    invoke(
        &app,
        "ow-main",
        "ipc_reply",
        json!({ "id": id, "ok": true, "seq": 1 }),
    )
    .unwrap();
    let all = wait_for(&captured, |m| !messages(m, "bw-1").is_empty());
    let result = &messages(&all, "bw-1")[0];
    assert_eq!(result["type"], "ipc-result");
    assert!(result.get("value").is_none(), "{result}");
}

#[test]
fn bootstrap_matches_the_snapshot_shape() {
    let (app, _) = app("bootstrap");
    webviews(&app);
    let snapshot = invoke(&app, "ow-main", "bootstrap", json!({})).unwrap();
    for key in [
        "seq",
        "versions",
        "manifest",
        "identity",
        "utmParams",
        "switches",
        "paths",
        "isPackaged",
        "locale",
        "displays",
        "primaryDisplayId",
        "packages",
        "flags",
    ] {
        assert!(snapshot.get(key).is_some(), "missing {key}");
    }
    assert_eq!(snapshot["manifest"]["productName"], "ACL Fixture");
    assert_eq!(snapshot["packages"]["backend"], "none");
    invoke(&app, "ow-main", "disable_ads_fpd", json!({})).unwrap();
    let after = invoke(&app, "ow-main", "bootstrap", json!({})).unwrap();
    assert_eq!(after["flags"]["adsFpdDisabled"], true);
    assert!(after["seq"].as_u64() > snapshot["seq"].as_u64());
}

#[test]
fn fs_commands_stay_in_scope() {
    let (app, _) = app("fs");
    webviews(&app);
    let snapshot = invoke(&app, "ow-main", "bootstrap", json!({})).unwrap();
    let user_data = PathBuf::from(snapshot["paths"]["userData"].as_str().unwrap());
    let file = user_data.join("nested/prefs.json");
    let path = file.to_string_lossy();
    assert_eq!(
        invoke(&app, "ow-main", "fs_read_text", json!({ "path": path })).unwrap(),
        Value::Null
    );
    invoke(
        &app,
        "ow-main",
        "fs_write_text",
        json!({ "path": path, "data": "{}" }),
    )
    .unwrap();
    assert_eq!(
        invoke(&app, "ow-main", "fs_read_text", json!({ "path": path })).unwrap(),
        "{}"
    );
    assert_eq!(
        invoke(&app, "ow-main", "fs_exists", json!({ "path": path })).unwrap(),
        true
    );
    let outside = std::env::temp_dir().join("ow-tauri-acl-outside.txt");
    let r = invoke(
        &app,
        "ow-main",
        "fs_write_text",
        json!({ "path": outside, "data": "x" }),
    );
    assert_eq!(r.unwrap_err()["code"], "forbidden");
    let escape = user_data.join("../escape.txt");
    let r = invoke(&app, "ow-main", "fs_read_text", json!({ "path": escape }));
    assert_eq!(r.unwrap_err()["code"], "forbidden");
    let manifest =
        PathBuf::from(snapshot["paths"]["appPath"].as_str().unwrap()).join("package.json");
    let text = invoke(&app, "ow-main", "fs_read_text", json!({ "path": manifest })).unwrap();
    assert!(text.as_str().unwrap().contains("acl-fixture"));
    let r = invoke(
        &app,
        "ow-main",
        "shell_open_path",
        json!({ "path": outside }),
    );
    assert_eq!(r.unwrap(), "path does not exist");
}
