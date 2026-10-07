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

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
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
use tauri_plugin_overwolf::{Builder, COMMANDS, OverwolfExt};

/// Commands only `bw-*` webviews may call.
const RENDERER_ONLY: [&str; 10] = [
    "ipc_invoke",
    "ipc_send",
    "ipc_skip",
    "eval_result",
    "navigation_external",
    "navigation_in_page",
    "adview_mount",
    "adview_update",
    "adview_unmount",
    "adview_command",
];
/// The `overwolf:renderer` set.
const RENDERER: [&str; 11] = [
    "ipc_subscribe",
    "ipc_invoke",
    "ipc_send",
    "ipc_skip",
    "eval_result",
    "navigation_external",
    "navigation_in_page",
    "adview_mount",
    "adview_update",
    "adview_unmount",
    "adview_command",
];
/// Commands of remote Overwolf pages only: ad guests and consent windows.
const REMOTE_ONLY: [&str; 2] = ["adview_event", "cmp_event"];
/// The ad document, inside the ad guests' capability.
const ADVIEW_PAGE: &str = "https://www.overwolf.com/monsdk/electron/latest/adview.html";
/// The startup consent page, inside the consent windows' capability.
const CMP_PAGE: &str =
    "https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/ow-cmp-v2.html";

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
                .skip_updater_os_steps()
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
    invoke_from(app, label, origin(), cmd, body)
}

/// [`invoke`] from a document at `url`.
fn invoke_from(
    app: &App<MockRuntime>,
    label: &str,
    url: &str,
    cmd: &str,
    body: Value,
) -> Result<Value, Value> {
    let webview = AnyWebview(app.get_webview(label).unwrap());
    get_ipc_response(
        &webview,
        InvokeRequest {
            cmd: format!("plugin:overwolf|{cmd}"),
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
        "cmp_event" => json!({ "name": "ready" }),
        "adview_event" => json!({ "name": "probe" }),
        _ => json!({}),
    }
}

/// A `BrowserWindow` made through `window_create`; returns its id.
fn create_window(app: &App<MockRuntime>) -> u64 {
    let created = invoke(
        app,
        "ow-main",
        "window_create",
        json!({ "options": { "show": false, "width": 400, "height": 300 }, "preload": null, "windowClass": "ui" }),
    )
    .unwrap();
    created["id"].as_u64().unwrap()
}

/// Webviews of every class. `bw-1` and `bw-2` are `BrowserWindow`s made
/// through `window_create` (the plugin refuses `bw-*` webviews it did not
/// create); `bw-9` is a child webview named like a window.
fn webviews(app: &App<MockRuntime>) {
    let url = || WebviewUrl::App("index.html".into());
    WebviewWindowBuilder::new(app, "ow-main", url())
        .build()
        .unwrap();
    assert_eq!(create_window(app), 1);
    assert_eq!(create_window(app), 2);
    let window = app.get_window("bw-1").unwrap();
    let size = LogicalSize::new(10.0, 10.0);
    let at = LogicalPosition::new(0.0, 0.0);
    window
        .add_child(WebviewBuilder::new("owad-1", url()), at, size)
        .unwrap();
    window
        .add_child(WebviewBuilder::new("bw-9", url()), at, size)
        .unwrap();
    app.get_window("bw-2")
        .unwrap()
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
                _ if REMOTE_ONLY.contains(cmd) => Outcome::Acl,
                "ow-main" if !RENDERER_ONLY.contains(cmd) => Outcome::Reached,
                "bw-1" if RENDERER.contains(cmd) => Outcome::Reached,
                _ => Outcome::Acl,
            };
            if label == "ow-main" && (*cmd == "app_quit" || *cmd == "is_cmp_required") {
                // Covered by `quit_request_round_trip` and `consent_sequencing`:
                // both wait for events the matrix does not send.
                continue;
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
    assert_eq!(create_window(app), 1);
    "bw-1".to_owned()
}

/// Subscribes `ow-main` (ready) and the given windows; returns the window
/// epochs in order.
fn subscribe_all(app: &App<MockRuntime>, windows: &[&str]) -> Vec<Value> {
    invoke(
        app,
        "ow-main",
        "ipc_subscribe",
        json!({ "onMessage": "__CHANNEL__:11" }),
    )
    .unwrap();
    invoke(app, "ow-main", "ipc_main_ready", json!({})).unwrap();
    windows
        .iter()
        .enumerate()
        .map(|(i, label)| {
            invoke(
                app,
                label,
                "ipc_subscribe",
                json!({ "onMessage": format!("__CHANNEL__:{}", 21 + i) }),
            )
            .unwrap()["epoch"]
                .clone()
        })
        .collect()
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
        "switches",
        "paths",
        "isPackaged",
        "locale",
        "displays",
        "primaryDisplayId",
        "packages",
        "flags",
        "firstLaunch",
        "ipcLimits",
    ] {
        assert!(snapshot.get(key).is_some(), "missing {key}");
    }
    assert_eq!(snapshot["ipcLimits"]["maxMessageBytes"], 8_388_608);
    // The mock runtime has no cursor; the key is absent then.
    assert!(snapshot.get("cursor").is_none());
    // F.2: no stored `utmParams` means the key is absent (`undefined`).
    assert!(snapshot.get("utmParams").is_none());
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

#[test]
fn a_window_at_a_remote_origin_is_refused_by_the_acl() {
    let (app, _) = app("remote-origin");
    webviews(&app);
    for cmd in [
        "ipc_subscribe",
        "ipc_invoke",
        "ipc_send",
        "ipc_skip",
        "eval_result",
    ] {
        let r = invoke_from(&app, "bw-1", "https://evil.example/", cmd, json!({}));
        assert_eq!(outcome(&r), Outcome::Acl, "{cmd}: {r:?}");
    }
    let r = invoke_from(
        &app,
        "ow-main",
        "https://evil.example/",
        "bootstrap",
        json!({}),
    );
    assert_eq!(outcome(&r), Outcome::Acl, "{r:?}");
}

#[test]
fn a_bw_window_the_plugin_did_not_create_is_refused() {
    let (app, _) = app("unregistered");
    webviews(&app);
    // Passes the ACL by its label, but no `window_create` made it.
    WebviewWindowBuilder::new(&app, "bw-7", WebviewUrl::App("index.html".into()))
        .build()
        .unwrap();
    let r = invoke(&app, "bw-7", "ipc_skip", json!({ "epoch": "x", "seq": 1 }));
    assert_eq!(outcome(&r), Outcome::Forbidden, "{r:?}");
}

#[test]
fn windows_only_receive_their_own_messages_and_replies() {
    let (app, captured) = app("isolation");
    main_and_window(&app);
    assert_eq!(create_window(&app), 2);
    let epochs = subscribe_all(&app, &["bw-1", "bw-2"]);
    let id1 = invoke(
        &app,
        "bw-1",
        "ipc_invoke",
        json!({ "channel": "who", "args": [], "epoch": epochs[0], "seq": 1 }),
    )
    .unwrap()["id"]
        .as_u64()
        .unwrap();
    let id2 = invoke(
        &app,
        "bw-2",
        "ipc_invoke",
        json!({ "channel": "who", "args": [], "epoch": epochs[1], "seq": 1 }),
    )
    .unwrap()["id"]
        .as_u64()
        .unwrap();
    invoke(
        &app,
        "ow-main",
        "ipc_emit",
        json!({ "target": 2, "channel": "only-two", "args": [], "seq": 1 }),
    )
    .unwrap();
    invoke(
        &app,
        "ow-main",
        "ipc_reply",
        json!({ "id": id2, "ok": true, "value": "two", "seq": 2 }),
    )
    .unwrap();
    invoke(
        &app,
        "ow-main",
        "ipc_reply",
        json!({ "id": id1, "ok": true, "value": "one", "seq": 1 }),
    )
    .unwrap();
    let all = wait_for(&captured, |m| {
        messages(m, "bw-1").len() + messages(m, "bw-2").len() >= 3
    });
    let one = messages(&all, "bw-1");
    let two = messages(&all, "bw-2");
    assert_eq!(
        one,
        vec![json!({ "type": "ipc-result", "id": id1, "ok": true, "value": "one" })]
    );
    assert_eq!(two.len(), 2, "{two:?}");
    assert_eq!(two[0]["channel"], "only-two");
    assert_eq!(two[1]["id"], id2);
}

#[test]
fn a_rejected_emit_does_not_hold_back_the_next_reply() {
    let (app, captured) = app("emit-skip");
    main_and_window(&app);
    let epochs = subscribe_all(&app, &["bw-1"]);
    let id = invoke(
        &app,
        "bw-1",
        "ipc_invoke",
        json!({ "channel": "get", "args": [], "epoch": epochs[0], "seq": 1 }),
    )
    .unwrap()["id"]
        .as_u64()
        .unwrap();
    // seq 1: an invalid channel, rejected after the target is known.
    let r = invoke(
        &app,
        "ow-main",
        "ipc_emit",
        json!({ "target": 1, "channel": "", "args": [], "seq": 1 }),
    );
    assert_eq!(r.unwrap_err()["code"], "invalid-argument");
    // seq 2: the main runtime reports a reply that never reached the plugin.
    invoke(
        &app,
        "ow-main",
        "ipc_emit_skip",
        json!({ "target": 1, "seq": 2 }),
    )
    .unwrap();
    let start = Instant::now();
    invoke(
        &app,
        "ow-main",
        "ipc_reply",
        json!({ "id": id, "ok": true, "value": 3, "seq": 3 }),
    )
    .unwrap();
    let all = wait_for(&captured, |m| !messages(m, "bw-1").is_empty());
    assert!(
        start.elapsed() < Duration::from_millis(900),
        "delivered without waiting for the 1 s gap timeout"
    );
    assert_eq!(messages(&all, "bw-1")[0]["value"], 3);
}

#[test]
fn loading_a_remote_url_switches_the_window_to_a_remote_webview() {
    let (app, captured) = app("remote-switch");
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    invoke(
        &app,
        "ow-main",
        "window_load",
        json!({ "id": 1, "target": { "kind": "url", "url": "https://example.com/" } }),
    )
    .unwrap();
    assert!(
        app.get_webview("bwr-1").is_some(),
        "the remote webview exists"
    );
    assert!(
        app.get_webview("bw-1").is_none(),
        "the app webview is closed"
    );
    assert!(app.get_window("bw-1").is_some(), "the window keeps its id");
    // `webContents.send` to a remote window is dropped, never delivered.
    invoke(
        &app,
        "ow-main",
        "ipc_emit",
        json!({ "target": 1, "channel": "x", "args": [], "seq": 1 }),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(messages(&captured.lock().unwrap(), "bw-1").is_empty());
    // The remote webview has no capability.
    let r = invoke_from(
        &app,
        "bwr-1",
        "https://example.com/",
        "ipc_subscribe",
        json!({}),
    );
    assert_eq!(outcome(&r), Outcome::Acl, "{r:?}");
}

#[test]
fn navigation_policy_and_window_open() {
    let (app, captured) = app("navigation");
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    let ow = app.overwolf();
    let at = |s: &str| tauri::Url::parse(s).unwrap();
    let app_page = format!("{}/page.html", origin());
    assert!(ow.test_navigation("bw-1", &at(&app_page)));
    assert!(ow.test_navigation("bw-1", &at("about:srcdoc")));
    assert!(ow.test_navigation("bw-1", &at("blob:tauri://localhost/1")));
    assert!(!ow.test_navigation("bw-1", &at("file:///etc/hosts")));
    // ow-main: the first document, then a frame or a reload.
    let main_page = format!("{}/index.html", origin());
    ow.test_page_load("ow-main", &at(&main_page), true);
    assert!(ow.test_navigation("ow-main", &at("about:srcdoc")));
    assert!(!ow.test_navigation("ow-main", &at("https://example.com/")));
    // Rust reports the page loads of `bw-1` with the document URL.
    ow.test_page_load("bw-1", &at(&app_page), true);
    let all = wait_for(&captured, |m| {
        messages(m, "ow-main")
            .iter()
            .any(|x| x["event"] == "did-finish-load")
    });
    let loaded = messages(&all, "ow-main")
        .into_iter()
        .find(|x| x["event"] == "did-finish-load")
        .unwrap();
    assert_eq!(loaded["data"]["url"], app_page.as_str());
}

#[test]
fn remote_pages_get_one_command_each() {
    let (app, _) = app("remote");
    webviews(&app);
    let mut failures = Vec::new();
    for cmd in COMMANDS {
        let cases = [
            ("owad-1", ADVIEW_PAGE, *cmd == "adview_event"),
            ("owad-1", "https://evil.example/monsdk/electron/x", false),
            ("owad-1", CMP_PAGE, false),
            ("ow-cmp", CMP_PAGE, *cmd == "cmp_event"),
            ("ow-cmp", ADVIEW_PAGE, false),
            ("bw-1", ADVIEW_PAGE, false),
        ];
        for (label, url, allowed) in cases {
            // The ACL lets the one command through; the plugin then refuses
            // these webviews (an unregistered guest, a document URL outside
            // the consent scope).
            let want = if allowed {
                Outcome::Forbidden
            } else {
                Outcome::Acl
            };
            let got = outcome(&invoke_from(&app, label, url, cmd, probe_body(cmd)));
            if got != want {
                failures.push(format!(
                    "{cmd} from {label} at {url}: want {want:?}, got {got:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An app whose plugin config has `extra` merged in.
fn app_with(name: &str, extra: Value) -> (App<MockRuntime>, Captured) {
    let captured: Captured = Arc::default();
    let sink = Arc::clone(&captured);
    let mut context = ow_tauri_acl_tests::context();
    let mut config = json!({ "state": { "appDataDir": temp_dir(name) } });
    if let (Some(c), Value::Object(e)) = (config.as_object_mut(), extra) {
        c.extend(e);
    }
    context
        .config_mut()
        .plugins
        .0
        .insert("overwolf".into(), config);
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
                .skip_updater_os_steps()
                .argv(vec!["acl-fixture".into()])
                .build(),
        )
        .build(context)
        .unwrap();
    (app, captured)
}

fn mount_body(element_id: &str) -> Value {
    json!({
        "elementId": element_id,
        "attributes": {
            "cid": "", "slotsize": "300x250", "adstyle": "", "customTracking": null,
            "performance": false, "unit": null, "pageurl": ""
        },
        "rect": { "x": 10, "y": 20, "width": 300, "height": 250, "devicePixelRatio": 1 },
        "visible": true
    })
}

/// The `adview-event` host messages `bw-1` received, as `(source, name)`.
fn adview_events(all: &[(String, u32, Value)]) -> Vec<(String, String)> {
    messages(all, "bw-1")
        .into_iter()
        .filter(|m| m["type"] == "adview-event")
        .map(|m| {
            (
                m["source"].as_str().unwrap_or_default().to_owned(),
                m["name"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

#[test]
fn adview_guest_lifecycle_crash_and_recovery_cap() {
    let (app, captured) = app_with("guest", json!({ "ads": { "maxRecoveries": 1 } }));
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    let ow = app.overwolf();
    let mounted = invoke(&app, "bw-1", "adview_mount", mount_body("e1")).unwrap();
    let guest = mounted["guestLabel"].as_str().unwrap().to_owned();
    assert_eq!(guest, "owad-bw-1-1");
    assert!(app.get_webview(&guest).is_some());
    // D.6.5: no navigation before consent or 3 s after the mount.
    let t0 = ow.test_now();
    ow.test_ads_tick(t0);
    assert_eq!(ow.test_guest(&guest).unwrap()["navigated"], false);
    ow.test_ads_tick(t0 + 3_100);
    assert_eq!(ow.test_guest(&guest).unwrap()["navigated"], true);
    // The ad document loads (the mock runtime does not navigate by itself).
    let page = tauri::Url::parse(ADVIEW_PAGE).unwrap();
    app.get_webview(&guest)
        .unwrap()
        .navigate(page.clone())
        .unwrap();
    ow.test_page_load(&guest, &page, false);
    ow.test_page_load(&guest, &page, true);
    // did-finish-load waits for the shim's dom-ready (ow-electron's order).
    invoke_from(
        &app,
        &guest,
        ADVIEW_PAGE,
        "adview_event",
        json!({ "name": "__host:domReady" }),
    )
    .unwrap();
    assert_eq!(ow.test_guest(&guest).unwrap()["domReady"], true);
    // Guest messages reach the embedder; internal ones do not.
    invoke_from(
        &app,
        &guest,
        ADVIEW_PAGE,
        "adview_event",
        json!({ "name": "__host:ready", "data": {} }),
    )
    .unwrap();
    invoke_from(
        &app,
        &guest,
        ADVIEW_PAGE,
        "adview_event",
        json!({ "slotId": "someone-else", "name": "impression", "data": { "n": 1 } }),
    )
    .unwrap();
    let bad = invoke_from(
        &app,
        &guest,
        ADVIEW_PAGE,
        "adview_event",
        json!({ "name": "bad name!" }),
    );
    assert_eq!(bad.unwrap_err()["code"], "invalid-argument");
    let all = wait_for(&captured, |m| {
        adview_events(m).iter().any(|(_, n)| n == "impression")
    });
    let events = adview_events(&all);
    assert!(
        events.contains(&("host".into(), "did-attach".into())),
        "{events:?}"
    );
    let pos = |name: &str| events.iter().position(|(s, n)| s == "host" && n == name);
    assert!(
        pos("dom-ready").unwrap() < pos("did-finish-load").unwrap(),
        "{events:?}"
    );
    assert!(
        events.contains(&("guest".into(), "impression".into())),
        "{events:?}"
    );
    assert!(
        !events.iter().any(|(_, n)| n.starts_with("__host:")),
        "{events:?}"
    );
    // A crash: render-process-gone, then a reload (one recovery).
    ow.test_guest_crashed(&guest, tauri_plugin_overwolf::ads::GoneReason::Crashed);
    assert_eq!(ow.test_guest(&guest).unwrap()["recoveries"], 1);
    wait_for(&captured, |m| {
        adview_events(m)
            .iter()
            .any(|(_, n)| n == "render-process-gone")
    });
    // ads.maxRecoveries = 1: the next crash closes the guest.
    ow.test_guest_crashed(&guest, tauri_plugin_overwolf::ads::GoneReason::Oom);
    assert!(ow.test_guest(&guest).is_none());
    // Commands on a closed element: not-found; unmount stays idempotent.
    let r = invoke(
        &app,
        "bw-1",
        "adview_command",
        json!({ "elementId": "e1", "command": "reload", "args": [] }),
    );
    assert_eq!(r.unwrap_err()["code"], "not-found");
    invoke(&app, "bw-1", "adview_unmount", json!({ "elementId": "e1" })).unwrap();
}

/// The native steps the host took for the guest `label`, by lab `kind`.
fn native_steps(app: &App<MockRuntime>, label: &str) -> Vec<Value> {
    app.overwolf()
        .test_guest_trace()
        .into_iter()
        .filter(|e| e["label"] == label && e.get("kind").is_some())
        .collect()
}

/// AF-10: guests are transparent from creation unless
/// `ads.transparentGuests` is off.
#[test]
fn guests_are_transparent_unless_configured_off() {
    let (app, _) = app("guest-transparent");
    main_and_window(&app);
    let guest = invoke(&app, "bw-1", "adview_mount", mount_body("e1")).unwrap()["guestLabel"]
        .as_str()
        .unwrap()
        .to_owned();
    let kinds: Vec<Value> = native_steps(&app, &guest)
        .into_iter()
        .map(|e| e["kind"].clone())
        .collect();
    assert_eq!(kinds, [json!("transparent")]);

    let (app, _) = app_with(
        "guest-opaque",
        json!({ "ads": { "transparentGuests": false } }),
    );
    main_and_window(&app);
    let guest = invoke(&app, "bw-1", "adview_mount", mount_body("e1")).unwrap()["guestLabel"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(native_steps(&app, &guest).is_empty());
}

/// A `performance` element's mount: the whole viewport.
fn performance_mount_body(element_id: &str) -> Value {
    let mut body = mount_body(element_id);
    body["attributes"]["performance"] = json!(true);
    body["rect"] = json!({ "x": 0, "y": 0, "width": 400, "height": 300, "devicePixelRatio": 1 });
    body
}

/// The guest label of a mount result.
fn guest_of(mounted: &Value) -> String {
    mounted["guestLabel"].as_str().unwrap().to_owned()
}

/// AF-11: the performance guest is raised to the top of its window when it
/// mounts and whenever another guest of that window mounts after it.
#[test]
fn the_performance_guest_stays_on_top() {
    let (app, _) = app("guest-zorder");
    main_and_window(&app);
    create_window(&app);
    let zorders = || -> Vec<(String, String)> {
        app.overwolf()
            .test_guest_trace()
            .into_iter()
            .filter(|e| e["kind"] == "zorder")
            .map(|e| {
                (
                    e["label"].as_str().unwrap().to_owned(),
                    e["after"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    };
    // Standard guests alone are never restacked.
    let standard = guest_of(&invoke(&app, "bw-1", "adview_mount", mount_body("s1")).unwrap());
    assert!(zorders().is_empty());
    let perf =
        guest_of(&invoke(&app, "bw-1", "adview_mount", performance_mount_body("p1")).unwrap());
    assert_eq!(zorders(), [(perf.clone(), perf.clone())]);
    // A guest mounted after it, or remounted, goes under it.
    let later = guest_of(&invoke(&app, "bw-1", "adview_mount", mount_body("s2")).unwrap());
    let again = guest_of(&invoke(&app, "bw-1", "adview_mount", mount_body("s1")).unwrap());
    assert_ne!(again, standard);
    // Another window's guests leave this window's stacking alone.
    invoke(&app, "bw-2", "adview_mount", mount_body("s1")).unwrap();
    assert_eq!(
        zorders(),
        [
            (perf.clone(), perf.clone()),
            (perf.clone(), later),
            (perf.clone(), again),
        ]
    );
}

/// AF-12: clicks pass through a performance guest from its mount until its
/// first `display_ad_loaded`, switched in the host without a round trip;
/// standard guests never pass input through.
#[test]
fn the_performance_guest_passes_input_through_until_its_first_ad() {
    let (app, captured) = app("guest-passthrough");
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    let ow = app.overwolf();
    let perf =
        guest_of(&invoke(&app, "bw-1", "adview_mount", performance_mount_body("p1")).unwrap());
    let standard = guest_of(&invoke(&app, "bw-1", "adview_mount", mount_body("s1")).unwrap());
    let switches = |label: &str| -> Vec<Value> {
        native_steps(&app, label)
            .into_iter()
            .filter(|e| e["kind"] == "passthrough")
            .map(|e| e["on"].clone())
            .collect()
    };
    assert_eq!(switches(&perf), [json!(true)]);
    assert_eq!(ow.test_guest(&perf).unwrap()["passthrough"], true);
    assert!(switches(&standard).is_empty());
    assert_eq!(ow.test_guest(&standard).unwrap()["passthrough"], false);
    // Both guests load the ad document (D.6.5: 3 s after the mount).
    ow.test_ads_tick(ow.test_now() + 3_100);
    let page = tauri::Url::parse(ADVIEW_PAGE).unwrap();
    for label in [&perf, &standard] {
        app.get_webview(label)
            .unwrap()
            .navigate(page.clone())
            .unwrap();
        ow.test_page_load(label, &page, true);
    }
    let event = |label: &str, name: &str| {
        invoke_from(
            &app,
            label,
            ADVIEW_PAGE,
            "adview_event",
            json!({ "name": name, "data": {} }),
        )
        .unwrap();
    };
    // Other messages leave it alone; the first display_ad_loaded ends it,
    // a second one changes nothing.
    event(&perf, "impression");
    assert_eq!(ow.test_guest(&perf).unwrap()["passthrough"], true);
    event(&perf, "display_ad_loaded");
    assert_eq!(switches(&perf), [json!(true), json!(false)]);
    assert_eq!(ow.test_guest(&perf).unwrap()["passthrough"], false);
    event(&perf, "display_ad_loaded");
    event(&standard, "display_ad_loaded");
    assert_eq!(switches(&perf), [json!(true), json!(false)]);
    assert!(switches(&standard).is_empty());
    // The event still reaches the element, after the switch.
    let all = wait_for(&captured, |m| {
        adview_events(m)
            .iter()
            .filter(|(s, n)| s == "guest" && n == "display_ad_loaded")
            .count()
            == 3
    });
    assert!(
        adview_events(&all).contains(&("guest".into(), "impression".into())),
        "{:?}",
        adview_events(&all)
    );
}

#[test]
fn adview_update_command_and_window_close() {
    let (app, _) = app("guest-update");
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    let ow = app.overwolf();
    let guest = invoke(&app, "bw-1", "adview_mount", mount_body("e1")).unwrap()["guestLabel"]
        .as_str()
        .unwrap()
        .to_owned();
    invoke(
        &app,
        "bw-1",
        "adview_update",
        json!({ "elementId": "e1", "visible": false, "attributes": { "customTracking": { "a": 1 } } }),
    )
    .unwrap();
    assert_eq!(ow.test_guest(&guest).unwrap()["visible"], false);
    for (command, args) in [
        ("setAudioMuted", json!([false])),
        ("setPageUrl", json!(["https://example.com/page"])),
        ("sendCommand", json!(["x", 1])),
        ("reload", json!([])),
    ] {
        invoke(
            &app,
            "bw-1",
            "adview_command",
            json!({ "elementId": "e1", "command": command, "args": args }),
        )
        .unwrap();
    }
    // B.3.3: `setPageUrl` and `sendCommand` reach the guest as private
    // messages `{type, data: [...args]}`, as in ow-electron (observed).
    let messages: Vec<Value> = ow
        .test_guest_trace()
        .into_iter()
        .filter(|e| e["label"] == guest.as_str() && e["via"] == "private-message")
        .map(|e| e["message"].clone())
        .filter(|m| m["type"] == "setPageUrl" || m["type"] == "sendCommand")
        .collect();
    assert_eq!(
        messages,
        [
            json!({ "type": "setPageUrl", "data": ["https://example.com/page"] }),
            json!({ "type": "sendCommand", "data": ["x", 1] }),
        ]
    );
    let r = invoke(
        &app,
        "bw-1",
        "adview_update",
        json!({ "elementId": "nope", "visible": true }),
    );
    assert_eq!(r.unwrap_err()["code"], "not-found");
    // A second mount of the same element replaces the guest.
    let again = invoke(&app, "bw-1", "adview_mount", mount_body("e1")).unwrap();
    assert_eq!(again["guestLabel"], "owad-bw-1-2");
    assert!(ow.test_guest(&guest).is_none());
    // Guests close with their embedder window.
    ow.test_window_destroyed("bw-1");
    assert!(ow.test_guest("owad-bw-1-2").is_none());
}

#[test]
fn consent_sequencing() {
    let (app, _) = app("consent");
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    let ow = app.overwolf();
    // A guest mounted before consent waits for the startup window (D.6.5).
    let guest = invoke(&app, "bw-1", "adview_mount", mount_body("e1")).unwrap()["guestLabel"]
        .as_str()
        .unwrap()
        .to_owned();
    // RunEvent::Ready: the cmp-eu-only request (refused in test builds, so
    // as after a failed request) and then the startup window (D.6.1).
    ow.test_start_consent();
    let start = Instant::now();
    while !ow
        .test_hidden_consent_windows()
        .contains(&"ow-cmp-startup".to_owned())
    {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "no startup window"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let window = app.get_webview_window("ow-cmp-startup").unwrap();
    assert!(window.url().unwrap().as_str().starts_with(CMP_PAGE));
    // isCMPRequired() waits for the startup page's load.
    let handle = app.handle().clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let required = tauri::async_runtime::block_on(handle.overwolf().test_is_cmp_required());
        tx.send(required).unwrap();
    });
    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "resolved before the load"
    );
    assert!(!ow.test_consent_gate_open());
    ow.test_ads_tick(ow.test_now());
    assert_eq!(ow.test_guest(&guest).unwrap()["navigated"], false);
    let url = window.url().unwrap();
    ow.test_page_load("ow-cmp-startup", &url, true);
    assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap());
    // The page saves consent, then closes itself.
    invoke_from(
        &app,
        "ow-cmp-startup",
        CMP_PAGE,
        "cmp_event",
        json!({ "name": "saveConsent", "data": { "consent": "CQTESTSTRING" } }),
    )
    .unwrap();
    let bad = invoke_from(
        &app,
        "ow-cmp-startup",
        CMP_PAGE,
        "cmp_event",
        json!({ "name": "saveConsent", "data": { "consent": "has space" } }),
    );
    assert_eq!(bad.unwrap_err()["code"], "invalid-argument");
    invoke_from(
        &app,
        "ow-cmp-startup",
        CMP_PAGE,
        "cmp_event",
        json!({ "name": "saveUnifiedConsent", "data": { "consent": "cmp=CQTESTSTRING&ac=2~1" } }),
    )
    .unwrap();
    invoke_from(
        &app,
        "ow-cmp-startup",
        CMP_PAGE,
        "cmp_event",
        json!({ "name": "close" }),
    )
    .unwrap();
    let start = Instant::now();
    while !ow.test_consent_gate_open() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "gate never opened"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    ow.test_ads_tick(ow.test_now());
    assert_eq!(ow.test_guest(&guest).unwrap()["navigated"], true);
    let state: Value =
        serde_json::from_slice(&std::fs::read(ow.state_dir().join("ow-electron.json")).unwrap())
            .unwrap();
    // F.2: `firstLaunch` (written at Ready) comes before `cmp`.
    let keys: Vec<&String> = state.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["firstLaunch", "cmp"]);
    assert_eq!(state["cmp"]["cmpString"], "CQTESTSTRING");
    assert_eq!(
        state["cmp"]["unifiedConsentString"],
        "cmp%3DCQTESTSTRING%26ac%3D2~1"
    );
    assert!(state["cmp"]["timeStamp"].is_u64());
    // Later calls resolve at once (the result is cached for the launch).
    let handle = app.handle().clone();
    assert!(tauri::async_runtime::block_on(
        handle.overwolf().test_is_cmp_required()
    ));
}

#[test]
fn guest_visibility_follows_the_window_and_the_element() {
    let (app, _) = app("guest-visibility");
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    let ow = app.overwolf();
    let guest = invoke(&app, "bw-1", "adview_mount", mount_body("e1")).unwrap()["guestLabel"]
        .as_str()
        .unwrap()
        .to_owned();
    let state = |key: &str| ow.test_guest(&guest).unwrap()[key].clone();
    assert_eq!(state("visibilityState"), "visible");
    // Hidden window: hidden document.
    ow.test_ads_window_visible(1, false);
    assert_eq!(state("embedderHidden"), true);
    assert_eq!(state("visibilityState"), "hidden");
    // An element update while the window is hidden keeps it hidden.
    invoke(
        &app,
        "bw-1",
        "adview_update",
        json!({ "elementId": "e1", "visible": true }),
    )
    .unwrap();
    assert_eq!(state("visibilityState"), "hidden");
    // Shown again: visible again.
    ow.test_ads_window_visible(1, true);
    assert_eq!(state("visibilityState"), "visible");
    // A hidden element stays hidden across a window hide and show.
    invoke(
        &app,
        "bw-1",
        "adview_update",
        json!({ "elementId": "e1", "visible": false }),
    )
    .unwrap();
    assert_eq!(state("visibilityState"), "hidden");
    ow.test_ads_window_visible(1, false);
    ow.test_ads_window_visible(1, true);
    assert_eq!(state("visibilityState"), "hidden");
    invoke(
        &app,
        "bw-1",
        "adview_update",
        json!({ "elementId": "e1", "visible": true }),
    )
    .unwrap();
    assert_eq!(state("visibilityState"), "visible");
    // A closing window hides its guests' documents before they go
    // (regression: they were destroyed while still visible).
    ow.test_ads_window_closing(1);
    assert_eq!(state("visibilityState"), "hidden");
}

/// Host requests as the analytics transport received them.
#[derive(Default)]
struct Requests(Mutex<Vec<tauri_plugin_overwolf::analytics::HostRequest>>);

impl tauri_plugin_overwolf::analytics::Transport for Requests {
    fn send(
        &self,
        request: tauri_plugin_overwolf::analytics::HostRequest,
    ) -> tauri_plugin_overwolf::analytics::BoxFuture<
        Result<tauri_plugin_overwolf::analytics::HostResponse, String>,
    > {
        self.0.lock().unwrap().push(request);
        Box::pin(async {
            Ok(tauri_plugin_overwolf::analytics::HostResponse {
                status: 200,
                ..Default::default()
            })
        })
    }
}

#[test]
fn a_shown_window_counts_before_a_guest_that_attaches_after_it() {
    let requests = Arc::new(Requests::default());
    let mut context = ow_tauri_acl_tests::context();
    context.config_mut().plugins.0.insert(
        "overwolf".into(),
        json!({ "state": { "appDataDir": temp_dir("shown-before-attach") } }),
    );
    let app = mock_builder()
        .plugin(
            Builder::new()
                .manifest_json(ow_tauri_acl_tests::manifest())
                .companion_plugins(false)
                .main_webview(false)
                .skip_os_queries()
                .skip_updater_os_steps()
                .analytics_transport(Arc::clone(&requests) as _)
                .argv(vec!["acl-fixture".into()])
                .build(),
        )
        .build(context)
        .unwrap();
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    invoke(&app, "ow-main", "main_ready", json!({})).unwrap();
    // The window is shown (the mock reports every window visible) and a
    // guest attaches before any visibility poll ran.
    invoke(&app, "bw-1", "adview_mount", mount_body("e1")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let names = loop {
        let names: Vec<String> = requests
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|r| {
                if r.url.contains("hasVisibleWindow%22%3Atrue") {
                    "visible-heartbeat".to_owned()
                } else if r.url.contains("InsertStats") {
                    let body = String::from_utf8_lossy(r.body.as_deref().unwrap_or_default());
                    if body.contains("400025") {
                        "400025"
                    } else {
                        "stats"
                    }
                    .to_owned()
                } else {
                    "other".to_owned()
                }
            })
            .collect();
        if names.iter().any(|n| n == "400025") || Instant::now() > deadline {
            break names;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let heartbeat = names.iter().position(|n| n == "visible-heartbeat");
    let attach = names.iter().position(|n| n == "400025");
    assert!(
        heartbeat.is_some() && attach.is_some() && heartbeat < attach,
        "the first-visible-window heartbeat precedes 400025: {names:?}"
    );
}

#[test]
fn page_reload_runs_on_its_own_timer() {
    let (app, _) = app("guest-reload");
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    let ow = app.overwolf();
    let guest = invoke(&app, "bw-1", "adview_mount", mount_body("e1")).unwrap()["guestLabel"]
        .as_str()
        .unwrap()
        .to_owned();
    ow.test_ads_tick(ow.test_now() + 3_100);
    let page = tauri::Url::parse(ADVIEW_PAGE).unwrap();
    app.get_webview(&guest)
        .unwrap()
        .navigate(page.clone())
        .unwrap();
    ow.test_page_load(&guest, &page, true);
    invoke_from(
        &app,
        &guest,
        ADVIEW_PAGE,
        "adview_event",
        json!({ "name": "__host:ready", "data": {} }),
    )
    .unwrap();
    assert_eq!(ow.test_guest(&guest).unwrap()["ready"], true);
    invoke_from(
        &app,
        &guest,
        ADVIEW_PAGE,
        "adview_event",
        json!({ "name": "__host:reload" }),
    )
    .unwrap();
    assert_eq!(ow.test_guest(&guest).unwrap()["reloadScheduled"], true);
    // About 70 ms later, without a host tick: a new load has started.
    let start = Instant::now();
    while ow.test_guest(&guest).unwrap()["reloadScheduled"] == true {
        assert!(start.elapsed() < Duration::from_secs(2), "reload never ran");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(ow.test_guest(&guest).unwrap()["ready"], false);
    // A navigation off Overwolf bounces back as a new, watched load.
    invoke_from(
        &app,
        &guest,
        ADVIEW_PAGE,
        "adview_event",
        json!({ "name": "__host:domReady" }),
    )
    .unwrap();
    let off = tauri::Url::parse("https://advertiser.example/landing").unwrap();
    ow.test_page_load(&guest, &off, false);
    assert_eq!(ow.test_guest(&guest).unwrap()["domReady"], false);
}

#[test]
fn a_custom_cmp_url_may_load_in_the_settings_window_only() {
    let (app, _) = app("cmp-custom");
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    let ow = app.overwolf();
    // The window opens on a data: preloader (regression: Tauri refused
    // data: URLs, so every settings window failed to open).
    invoke(
        &app,
        "ow-main",
        "open_cmp_window",
        json!({ "options": { "cmpURL": "https://cmp.example.test/privacy/settings.html" } }),
    )
    .expect("the settings window opens");
    assert!(app.get_webview_window("ow-cmp").is_some());
    let custom = tauri::Url::parse("https://cmp.example.test/privacy/settings.html?tab=x").unwrap();
    let other = tauri::Url::parse("https://other.example.test/").unwrap();
    assert!(ow.test_navigation("ow-cmp", &custom));
    assert!(!ow.test_navigation("ow-cmp", &other));
    assert!(!ow.test_navigation("ow-cmp-startup", &custom));
    // Overwolf's own pages stay allowed.
    let overwolf = tauri::Url::parse(CMP_PAGE).unwrap();
    assert!(ow.test_navigation("ow-cmp", &overwolf));
}

#[test]
fn navigation_external_opens_the_browser_and_reports_will_navigate() {
    let (app, captured) = app("nav-external");
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    let ow = app.overwolf();
    invoke(
        &app,
        "bw-1",
        "navigation_external",
        json!({ "url": "https://example.com/docs?x=1" }),
    )
    .unwrap();
    assert_eq!(ow.test_browser_opens(), ["https://example.com/docs?x=1"]);
    let all = wait_for(&captured, |m| {
        messages(m, "ow-main")
            .iter()
            .any(|x| x["event"] == "will-navigate")
    });
    let event = messages(&all, "ow-main")
        .into_iter()
        .find(|x| x["event"] == "will-navigate")
        .unwrap();
    assert_eq!(event["type"], "window");
    assert_eq!(event["id"], 1);
    assert_eq!(event["data"]["url"], "https://example.com/docs?x=1");
    // Not a web URL, the app's own origin, and main-process callers: refused.
    for url in ["file:///etc/hosts", "javascript:alert(1)", "not a url"] {
        let r = invoke(&app, "bw-1", "navigation_external", json!({ "url": url }));
        assert_eq!(r.unwrap_err()["code"], "invalid-argument", "{url}");
    }
    let own = format!("{}/index.html", origin());
    let r = invoke(&app, "bw-1", "navigation_external", json!({ "url": own }));
    assert_eq!(r.unwrap_err()["code"], "invalid-argument");
    let r = invoke(
        &app,
        "ow-main",
        "navigation_external",
        json!({ "url": "https://example.com/" }),
    );
    assert_eq!(outcome(&r), Outcome::Acl, "{r:?}");
    assert_eq!(ow.test_browser_opens().len(), 1);
}

#[test]
fn in_page_navigations_move_the_window_url_and_reach_main() {
    let (app, captured) = app("nav-in-page");
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    let ow = app.overwolf();
    let at = |s: &str| tauri::Url::parse(s).unwrap();
    let page = format!("{}/index.html", origin());
    let in_page = |url: &str| invoke(&app, "bw-1", "navigation_in_page", json!({ "url": url }));
    // Nothing loaded yet: nothing to navigate in.
    assert_eq!(
        in_page(&format!("{page}#/")).unwrap_err()["code"],
        "invalid-argument"
    );
    // During the load (a hash router sets `#/` before the load finishes),
    // did-finish-load reports the in-page URL, as Electron's does.
    ow.test_page_load("bw-1", &at(&page), false);
    in_page(&format!("{page}#/")).unwrap();
    ow.test_page_load("bw-1", &at(&page), true);
    in_page(&format!("{origin}/settings?tab=2", origin = origin())).unwrap();
    // The current URL again: nothing new.
    in_page(&format!("{origin}/settings?tab=2", origin = origin())).unwrap();
    // Another origin, or not a URL: refused.
    for url in ["https://example.com/#/", "not a url"] {
        assert_eq!(
            in_page(url).unwrap_err()["code"],
            "invalid-argument",
            "{url}"
        );
    }
    let all = wait_for(&captured, |m| {
        messages(m, "ow-main")
            .iter()
            .filter(|x| x["event"] == "did-navigate-in-page")
            .count()
            == 2
            && messages(m, "ow-main")
                .iter()
                .any(|x| x["event"] == "did-finish-load")
    });
    let main = messages(&all, "ow-main");
    let events: Vec<(String, String)> = main
        .iter()
        .filter(|x| {
            x["id"] == 1
                && (x["event"] == "did-navigate-in-page" || x["event"] == "did-finish-load")
        })
        .map(|x| {
            (
                x["event"].as_str().unwrap().to_owned(),
                x["data"]["url"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        events,
        [
            ("did-navigate-in-page".to_owned(), format!("{page}#/")),
            ("did-finish-load".to_owned(), format!("{page}#/")),
            (
                "did-navigate-in-page".to_owned(),
                format!("{}/settings?tab=2", origin())
            ),
        ]
    );
    // A new load starts from its own URL again.
    ow.test_page_load("bw-1", &at(&page), false);
    ow.test_page_load("bw-1", &at(&page), true);
    let all = wait_for(&captured, |m| {
        messages(m, "ow-main")
            .iter()
            .filter(|x| x["event"] == "did-finish-load")
            .count()
            == 2
    });
    let last = messages(&all, "ow-main")
        .into_iter()
        .rfind(|x| x["event"] == "did-finish-load")
        .unwrap();
    assert_eq!(last["data"]["url"], page.as_str());
    // Main-process callers are refused by the ACL.
    let r = invoke(
        &app,
        "ow-main",
        "navigation_in_page",
        json!({ "url": page }),
    );
    assert_eq!(outcome(&r), Outcome::Acl, "{r:?}");
}

#[test]
fn a_minimized_window_hides_its_guests_until_restored() {
    let (app, _) = app("guest-minimized");
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    let ow = app.overwolf();
    let guest = invoke(&app, "bw-1", "adview_mount", mount_body("e1")).unwrap()["guestLabel"]
        .as_str()
        .unwrap()
        .to_owned();
    let state = |key: &str| ow.test_guest(&guest).unwrap()[key].clone();
    assert_eq!(state("visibilityState"), "visible");
    ow.test_window_minimized(1, true);
    assert_eq!(state("embedderMinimized"), true);
    assert_eq!(state("visibilityState"), "hidden");
    // Shown but still minimized: still hidden.
    ow.test_ads_window_visible(1, true);
    assert_eq!(state("visibilityState"), "hidden");
    ow.test_window_minimized(1, false);
    assert_eq!(state("visibilityState"), "visible");
}

#[test]
fn a_new_document_subscription_closes_the_old_documents_guests() {
    let (app, _) = app("guest-resubscribe");
    main_and_window(&app);
    subscribe_all(&app, &["bw-1"]);
    let ow = app.overwolf();
    let guest = invoke(&app, "bw-1", "adview_mount", mount_body("e1")).unwrap()["guestLabel"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(ow.test_guest(&guest).is_some());
    invoke(
        &app,
        "bw-1",
        "ipc_subscribe",
        json!({ "onMessage": "__CHANNEL__:31" }),
    )
    .unwrap();
    assert!(ow.test_guest(&guest).is_none());
}

/// The minisign public key and a prehashed signature of the four bytes
/// `test` (the minisign-verify crate's test vector).
const UPDATE_PUBKEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
const UPDATE_SIG: &str = "untrusted comment: signature from minisign secret key
RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=
trusted comment: timestamp:1556193335\tfile:test
y/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";
/// Base64 SHA-512 of `test`.
const TEST_SHA512: &str =
    "7iaw3Ur350mqGo7jwQrpkj9hiYB3Lkc/iBml1JQODbJ6wYX4oOHV+E+IvIh/1nsUNzLDBMxfqa2Ob1f1ACio/w==";

type Routes = Arc<Mutex<Vec<(String, u16, Vec<u8>)>>>;

/// A local feed server: path (without query) to status and body. Returns
/// the base URL and the request log (full request targets).
fn feed_server(routes: Routes) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = Arc::clone(&log);
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
                if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                    break;
                }
            }
            let target = line.split(' ').nth(1).unwrap_or("/").to_owned();
            seen.lock().unwrap().push(target.clone());
            let path = target.split('?').next().unwrap_or("/").to_owned();
            let found = routes
                .lock()
                .unwrap()
                .iter()
                .find(|(p, _, _)| *p == path)
                .map(|(_, s, b)| (*s, b.clone()));
            let (status, body) = found.unwrap_or((404, b"missing".to_vec()));
            let head = format!(
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    (base, log)
}

fn feed_name() -> &'static str {
    if cfg!(windows) {
        "/feed/latest.yml"
    } else if cfg!(target_os = "macos") {
        "/feed/latest-mac.yml"
    } else {
        "/feed/latest-linux.yml"
    }
}

fn feed(version: &str, sha512: &str, extra: &str) -> Vec<u8> {
    format!(
        "version: {version}\nfiles:\n  - url: App-Setup.exe\n    sha512: {sha512}\n    size: 4\n    IsAdminRightsRequired: false\n  - url: App-mac.zip\n    sha512: {sha512}\n    size: 4\n  - url: App.AppImage\n    sha512: {sha512}\n    size: 4\nreleaseDate: '2026-10-01T00:00:00.000Z'\n{extra}"
    )
    .into_bytes()
}

fn updater_events(all: &[(String, u32, Value)]) -> Vec<Value> {
    messages(all, "ow-main")
        .into_iter()
        .filter(|m| m["type"] == "updater")
        .collect()
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one end-to-end story: configure, check, download, install"
)]
fn updater_checks_downloads_verifies_and_installs_at_exit() {
    let routes: Routes = Arc::default();
    let (base, log) = feed_server(Arc::clone(&routes));
    {
        let mut r = routes.lock().unwrap();
        r.push((feed_name().into(), 200, feed("0.2.0", TEST_SHA512, "")));
        for f in ["App-Setup.exe", "App-mac.zip", "App.AppImage"] {
            r.push((format!("/feed/{f}"), 200, b"test".to_vec()));
            r.push((
                format!("/feed/{f}.sig"),
                200,
                UPDATE_SIG.as_bytes().to_vec(),
            ));
        }
    }
    let (app, captured) = app_with("updater", json!({ "updater": { "pubkey": UPDATE_PUBKEY } }));
    main_and_window(&app);
    subscribe_all(&app, &[]);
    let ow = app.overwolf();

    // Nothing configured yet.
    let r = invoke(&app, "ow-main", "updater_check", json!({}));
    assert_eq!(r.unwrap_err()["code"], "invalid-argument");
    let r = invoke(&app, "ow-main", "updater_quit_and_install", json!({}));
    assert_eq!(r.unwrap_err()["code"], "not-found");
    // Plain http only for loopback hosts in debug builds.
    for url in ["http://example.com/feed", "ftp://127.0.0.1/feed"] {
        let r = invoke(
            &app,
            "ow-main",
            "updater_configure",
            json!({ "provider": "generic", "url": url }),
        );
        assert_eq!(r.unwrap_err()["code"], "invalid-argument", "{url}");
    }
    let r = invoke(
        &app,
        "ow-main",
        "updater_configure",
        json!({ "provider": "github", "url": format!("{base}/feed") }),
    );
    assert_eq!(r.unwrap_err()["code"], "invalid-argument");
    invoke(
        &app,
        "ow-main",
        "updater_configure",
        json!({ "provider": "generic", "url": format!("{base}/feed"), "autoDownload": false }),
    )
    .unwrap();

    let result = invoke(&app, "ow-main", "updater_check", json!({})).unwrap();
    assert_eq!(result["isUpdateAvailable"], true);
    assert_eq!(result["updateInfo"]["version"], "0.2.0");
    assert_eq!(result["versionInfo"], result["updateInfo"]);
    assert_eq!(
        result["updateInfo"]["files"][0]["isAdminRightsRequired"],
        false
    );
    // The key as the feed spells it reaches the app too (electron-updater
    // hands over the parsed YAML).
    assert_eq!(
        result["updateInfo"]["files"][0]["IsAdminRightsRequired"],
        false
    );
    let feed_request = log
        .lock()
        .unwrap()
        .iter()
        .find(|t| t.starts_with(feed_name()))
        .cloned()
        .unwrap();
    assert!(feed_request.contains("?noCache="), "{feed_request}");

    let files = invoke(&app, "ow-main", "updater_download", json!({})).unwrap();
    let file = PathBuf::from(files[0].as_str().unwrap());
    assert_eq!(std::fs::read(&file).unwrap(), b"test");
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(|t| t.to_ascii_lowercase().ends_with(".sig"))
    );
    // A second download reuses the verified file.
    let again = invoke(&app, "ow-main", "updater_download", json!({})).unwrap();
    assert_eq!(again, files);

    let all = wait_for(&captured, |m| {
        updater_events(m)
            .iter()
            .any(|e| e["event"] == "update-downloaded")
    });
    let events: Vec<String> = updater_events(&all)
        .iter()
        .map(|e| e["event"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        events,
        [
            "error",
            "error",
            "checking-for-update",
            "update-available",
            "download-progress",
            "update-downloaded"
        ]
    );
    let downloaded = updater_events(&all).pop().unwrap();
    assert_eq!(downloaded["info"]["downloadedFile"], files[0]);
    let progress = updater_events(&all)
        .into_iter()
        .find(|e| e["event"] == "download-progress")
        .unwrap();
    assert_eq!(progress["progress"]["total"], 4);
    assert_eq!(progress["progress"]["delta"], 4);
    assert_eq!(progress["progress"]["transferred"], 4);
    assert_eq!(progress["progress"]["percent"], 100.0);
    // electron-updater's key order.
    let keys: Vec<&String> = progress["progress"].as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        ["total", "delta", "transferred", "percent", "bytesPerSecond"]
    );

    invoke(
        &app,
        "ow-main",
        "updater_quit_and_install",
        json!({ "isSilent": true, "isForceRunAfter": true }),
    )
    .unwrap();
    let installs = tauri::async_runtime::block_on(ow.test_updater_install_at_exit());
    assert_eq!(installs.len(), 1, "{installs:?}");
    let install = &installs[0];
    assert_eq!(install["silent"], true);
    assert_eq!(install["forceRunAfter"], true);
    if cfg!(windows) {
        assert_eq!(install["args"], json!(["/S", "/UPDATE", "/R"]));
        assert_eq!(install["relaunch"], false);
    } else {
        assert_eq!(install["relaunch"], true);
    }
    // The request is used once; the automatic install needs
    // autoInstallOnAppQuit, which is on by default.
    let installs = tauri::async_runtime::block_on(ow.test_updater_install_at_exit());
    assert_eq!(installs.len(), 2);
    assert_eq!(installs[1]["silent"], true);
    assert_eq!(installs[1]["forceRunAfter"], false);
    assert_eq!(installs[1]["relaunch"], false);
    // A non-silent quitAndInstall runs the app after the install
    // (electron-updater's autoRunAppAfterInstall, default true).
    invoke(
        &app,
        "ow-main",
        "updater_quit_and_install",
        json!({ "isSilent": false, "isForceRunAfter": false }),
    )
    .unwrap();
    let installs = tauri::async_runtime::block_on(ow.test_updater_install_at_exit());
    assert_eq!(installs[2]["forceRunAfter"], true);
    if cfg!(windows) {
        assert_eq!(installs[2]["args"], json!(["/UPDATE", "/R"]));
    }

    // No stagingPercentage: no staging id is created (electron-updater
    // creates it lazily).
    assert!(updater_id_file("updater").is_none());
}

/// `<userData>/.updaterId`, where electron-updater keeps the staging id.
fn updater_id_file(name: &str) -> Option<PathBuf> {
    walk(&temp_dir_path(name))
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n == ".updaterId"))
}

/// The directory [`temp_dir`] made for `name`, without clearing it.
fn temp_dir_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ow-tauri-acl-{name}-{}", std::process::id()))
}

#[test]
fn updater_fails_closed() {
    let routes: Routes = Arc::default();
    let (base, _) = feed_server(Arc::clone(&routes));
    let (app, captured) = app_with(
        "updater-closed",
        json!({ "updater": { "pubkey": UPDATE_PUBKEY } }),
    );
    main_and_window(&app);
    subscribe_all(&app, &[]);
    let configure = |extra: Value| {
        let mut body =
            json!({ "provider": "generic", "url": format!("{base}/feed"), "autoDownload": false });
        if let (Some(b), Value::Object(e)) = (body.as_object_mut(), extra) {
            b.extend(e);
        }
        invoke(&app, "ow-main", "updater_configure", body).unwrap();
    };
    let set_routes = |list: Vec<(String, u16, Vec<u8>)>| *routes.lock().unwrap() = list;
    configure(json!({}));

    // No feed: a network error.
    let r = invoke(&app, "ow-main", "updater_check", json!({}));
    assert_eq!(r.unwrap_err()["code"], "network");
    // Not YAML: invalid-argument.
    set_routes(vec![(feed_name().into(), 200, b"- [".to_vec())]);
    let r = invoke(&app, "ow-main", "updater_check", json!({}));
    assert_eq!(r.unwrap_err()["code"], "invalid-argument");
    // The running version (build metadata ignored), an older one and a 0 %
    // rollout are no update.
    for (version, extra) in [
        ("0.1.0", ""),
        ("0.0.9", ""),
        ("0.1.0+build.7", ""),
        ("0.2.0", "stagingPercentage: 0\n"),
    ] {
        set_routes(vec![(
            feed_name().into(),
            200,
            feed(version, TEST_SHA512, extra),
        )]);
        let r = invoke(&app, "ow-main", "updater_check", json!({})).unwrap();
        assert_eq!(r["isUpdateAvailable"], false, "{version} {extra}");
        let d = invoke(&app, "ow-main", "updater_download", json!({}));
        assert_eq!(d.unwrap_err()["code"], "not-found");
    }
    // Downgrades when allowed.
    configure(json!({ "allowDowngrade": true }));
    set_routes(vec![(
        feed_name().into(),
        200,
        feed("0.0.9", TEST_SHA512, ""),
    )]);
    let r = invoke(&app, "ow-main", "updater_check", json!({})).unwrap();
    assert_eq!(r["isUpdateAvailable"], true);
    configure(json!({ "allowDowngrade": false }));

    let files = |sha: &str, body: &[u8], sig: Option<&str>| {
        let mut list = vec![(feed_name().to_owned(), 200, feed("0.3.0", sha, ""))];
        for f in ["App-Setup.exe", "App-mac.zip", "App.AppImage"] {
            list.push((format!("/feed/{f}"), 200, body.to_vec()));
            if let Some(sig) = sig {
                list.push((format!("/feed/{f}.sig"), 200, sig.as_bytes().to_vec()));
            }
        }
        list
    };
    let wrong_sha = "AAAA".repeat(22);
    // SHA-512 mismatch, size mismatch, a missing and a wrong signature: all
    // `backend`, and nothing is left on disk.
    for (routes_now, what) in [
        (files(&wrong_sha, b"test", Some(UPDATE_SIG)), "sha"),
        (files(TEST_SHA512, b"tests", Some(UPDATE_SIG)), "size"),
        (files(TEST_SHA512, b"test", None), "no signature"),
        (
            files(TEST_SHA512, b"test", Some("garbage")),
            "bad signature",
        ),
    ] {
        set_routes(routes_now);
        let r = invoke(&app, "ow-main", "updater_check", json!({})).unwrap();
        assert_eq!(r["isUpdateAvailable"], true, "{what}");
        let d = invoke(&app, "ow-main", "updater_download", json!({}));
        assert_eq!(d.unwrap_err()["code"], "backend", "{what}");
        let pending = temp_dir_path("updater-closed");
        let leftovers: Vec<PathBuf> = walk(&pending)
            .into_iter()
            .filter(|p| p.to_string_lossy().contains("pending") && p.is_file())
            .collect();
        assert!(leftovers.is_empty(), "{what}: {leftovers:?}");
    }
    let r = invoke(&app, "ow-main", "updater_quit_and_install", json!({}));
    assert_eq!(r.unwrap_err()["code"], "not-found");
    let all = wait_for(&captured, |m| {
        updater_events(m)
            .iter()
            .filter(|e| e["event"] == "error")
            .count()
            >= 10
    });
    let backend = updater_events(&all)
        .into_iter()
        .filter(|e| e["event"] == "error" && e["error"]["code"] == "backend")
        .count();
    assert_eq!(backend, 4);
}

#[test]
fn updater_follows_electron_updater_rules() {
    let routes: Routes = Arc::default();
    let (base, _) = feed_server(Arc::clone(&routes));
    let (app, _) = app_with("updater-rules", json!({}));
    main_and_window(&app);
    invoke(
        &app,
        "ow-main",
        "updater_configure",
        json!({ "provider": "generic", "url": format!("{base}/feed"), "autoDownload": false }),
    )
    .unwrap();
    let set_routes = |list: Vec<(String, u16, Vec<u8>)>| *routes.lock().unwrap() = list;
    // A feed with a rollout creates the staging id.
    set_routes(vec![(
        feed_name().into(),
        200,
        feed("0.2.0", TEST_SHA512, "stagingPercentage: 0\n"),
    )]);
    let r = invoke(&app, "ow-main", "updater_check", json!({})).unwrap();
    assert_eq!(r["isUpdateAvailable"], false);
    // The rollout created the staging id where electron-updater keeps it;
    // the bucket follows it (electron-updater's rule).
    let id_file = updater_id_file("updater-rules").expect(".updaterId");
    for (tail, pct, want) in [("ffffffff", 100, false), ("00000000", 1, true)] {
        std::fs::write(&id_file, format!("12345678-1234-4234-8234-0000{tail}")).unwrap();
        set_routes(vec![(
            feed_name().into(),
            200,
            feed("0.2.0", TEST_SHA512, &format!("stagingPercentage: {pct}\n")),
        )]);
        let r = invoke(&app, "ow-main", "updater_check", json!({})).unwrap();
        assert_eq!(r["isUpdateAvailable"], want, "{tail} {pct}");
    }
    // Prereleases are offered: the generic provider ignores allowPrerelease.
    set_routes(vec![(
        feed_name().into(),
        200,
        feed("0.2.0-beta.1", TEST_SHA512, ""),
    )]);
    let r = invoke(&app, "ow-main", "updater_check", json!({})).unwrap();
    assert_eq!(r["isUpdateAvailable"], true);
    // A minimumSystemVersion above every OS release: not supported.
    set_routes(vec![(
        feed_name().into(),
        200,
        feed("0.2.0", TEST_SHA512, "minimumSystemVersion: 999.0.0\n"),
    )]);
    let r = invoke(&app, "ow-main", "updater_check", json!({})).unwrap();
    assert_eq!(r["isUpdateAvailable"], false);
    assert_eq!(r["updateInfo"]["minimumSystemVersion"], "999.0.0");
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            }
            out.push(p);
        }
    }
    out
}

#[test]
fn updater_disabled_returns_null() {
    let (app, _) = app_with("updater-off", json!({ "updater": { "enabled": false } }));
    main_and_window(&app);
    let r = invoke(&app, "ow-main", "updater_check", json!({})).unwrap();
    assert_eq!(r, Value::Null);
}

/// Regression: the plugin built `ow-main` inside its own setup, while Tauri
/// holds the plugin-store lock that building a webview takes again, so every
/// app with the main webview on hung before it started (found by the Tauri
/// edition of the parity harness). The app must build, and `ow-main` must
/// appear once setup is over.
#[test]
fn main_webview_is_created_after_setup_without_deadlock() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut context = ow_tauri_acl_tests::context();
        context.config_mut().plugins.0.insert(
            "overwolf".into(),
            json!({ "state": { "appDataDir": temp_dir("main-webview") } }),
        );
        let app = mock_builder()
            .plugin(
                Builder::new()
                    .manifest_json(ow_tauri_acl_tests::manifest())
                    .companion_plugins(false)
                    .skip_os_queries()
                    .skip_updater_os_steps()
                    .argv(vec!["acl-fixture".into()])
                    .build(),
            )
            .build(context);
        let Ok(app) = app else {
            let _ = tx.send((false, false));
            return;
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut created = false;
        while Instant::now() < deadline {
            if app.get_webview_window("ow-main").is_some() {
                created = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = tx.send((true, created));
    });
    let (built, created) = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("building the app deadlocked");
    assert!(built, "the app failed to build");
    assert!(created, "ow-main was never created");
}
