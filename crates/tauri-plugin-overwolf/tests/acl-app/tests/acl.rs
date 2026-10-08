//! Mock-runtime tests of the plugin's permission sets, runtime capabilities
//! and caller gate (DESIGN §3.5, §3.6, §4.5, §7.2; W1 minimal suite).
//!
//! The ACL comes from the plugin's permission sets, the runtime
//! capabilities the plugin adds for its own webviews, and `capabilities/`
//! here. Every command is invoked from several webviews; a command is
//! either refused by Tauri's ACL, refused by the plugin's gate
//! (`forbidden`), or reaches its handler.

#![expect(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions fail the test on any unexpected error"
)]

use std::path::PathBuf;

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_overwolf::{Builder, COMMANDS};

/// `overwolf:default` (DESIGN §3.6).
const DEFAULT: [&str; 12] = [
    "adview_mount",
    "adview_update",
    "adview_unmount",
    "adview_command",
    "set_window_name",
    "get_info",
    "is_cmp_required",
    "open_ad_privacy_settings_window",
    "open_cmp_window",
    "disable_anonymous_analytics",
    "disable_ads_optimization",
    "disable_ads_fpd",
];
/// Commands of the plugin's own webviews only (runtime capabilities).
const GUEST_ONLY: [&str; 2] = ["adview_event", "cmp_event"];
/// The ad document, inside the ad guests' capability.
const ADVIEW_PAGE: &str = "https://www.overwolf.com/monsdk/electron/latest/adview.html";
/// A consent page, inside the consent windows' capability.
const CMP_PAGE: &str = "https://content.overwolf.com/monsdk/electron/latest/cmp/ow-cmp-v2.html";

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ow-tauri-acl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A mock app with the plugin and the windows `labels` (app pages).
fn app(name: &str, labels: &[&str]) -> App<MockRuntime> {
    let mut context = ow_tauri_acl_tests::context();
    let plugins = &mut context.config_mut().plugins.0;
    let mut block = plugins
        .get("overwolf")
        .cloned()
        .unwrap_or_else(|| json!({}));
    block["state"] = json!({ "appDataDir": temp_dir(name) });
    plugins.insert("overwolf".into(), block);
    let app = mock_builder()
        .plugin(Builder::new().build())
        .build(context)
        .unwrap();
    for label in labels {
        WebviewWindowBuilder::new(&app, *label, WebviewUrl::default())
            .build()
            .unwrap();
    }
    app
}

fn origin() -> &'static str {
    if cfg!(any(windows, target_os = "android")) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    }
}

fn invoke(
    app: &App<MockRuntime>,
    label: &str,
    url: &str,
    cmd: &str,
    body: Value,
) -> Result<Value, Value> {
    let webview = app.get_webview_window(label).unwrap();
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
    /// Refused by Tauri's ACL.
    Acl,
    /// Allowed by the ACL but no handler (not registered).
    NotRegistered,
    /// Refused by the plugin's caller gate.
    Forbidden,
    /// Reached the handler (which may reject its arguments).
    Reached,
}

fn outcome(result: &Result<Value, Value>) -> Outcome {
    match result {
        Err(Value::String(s)) if s.contains("not allowed") => Outcome::Acl,
        Err(Value::String(s)) if s.contains("not found") => Outcome::NotRegistered,
        Err(e) if e.get("code") == Some(&json!("forbidden")) => Outcome::Forbidden,
        _ => Outcome::Reached,
    }
}

/// Arguments that reach a handler without lasting effect: commands that
/// would wait for `RunEvent::Ready` or send a request get invalid ones.
fn probe_body(cmd: &str) -> Value {
    match cmd {
        "set_window_name" | "adview_event" | "cmp_event" => json!({ "name": "probe" }),
        "set_analytics_user_enabled" | "set_anonymous_analytics_preference" => {
            json!({ "enabled": true })
        }
        "generate_user_email_hashes" => json!({ "email": "user@example.com" }),
        _ => json!({}),
    }
}

#[test]
fn twenty_five_commands_are_registered() {
    assert_eq!(COMMANDS.len(), 25);
    let app = app("registered", &["admin", "owad-1", "ow-cmp-1"]);
    for cmd in COMMANDS {
        let result = match *cmd {
            "adview_event" => invoke(&app, "owad-1", ADVIEW_PAGE, cmd, probe_body(cmd)),
            "cmp_event" => invoke(&app, "ow-cmp-1", CMP_PAGE, cmd, probe_body(cmd)),
            _ => invoke(&app, "admin", origin(), cmd, probe_body(cmd)),
        };
        assert_eq!(outcome(&result), Outcome::Reached, "{cmd}: {result:?}");
    }
}

#[test]
fn overwolf_default_grants_exactly_twelve_commands() {
    let app = app("default", &["main"]);
    let mut granted = Vec::new();
    for cmd in COMMANDS {
        let result = invoke(&app, "main", origin(), cmd, probe_body(cmd));
        match outcome(&result) {
            Outcome::Acl => {}
            Outcome::Reached => granted.push(*cmd),
            other => panic!("{cmd}: {other:?} {result:?}"),
        }
    }
    assert_eq!(granted, DEFAULT);
}

#[test]
fn guests_get_their_one_command_only() {
    let app = app("guests", &["owad-1", "ow-cmp-1"]);
    for cmd in COMMANDS {
        let guest = invoke(&app, "owad-1", ADVIEW_PAGE, cmd, probe_body(cmd));
        let cmp = invoke(&app, "ow-cmp-1", CMP_PAGE, cmd, probe_body(cmd));
        let want = |allowed: &str| {
            if *cmd == allowed {
                Outcome::Reached
            } else {
                Outcome::Acl
            }
        };
        assert_eq!(
            outcome(&guest),
            want("adview_event"),
            "owad-1 {cmd}: {guest:?}"
        );
        assert_eq!(outcome(&cmp), want("cmp_event"), "ow-cmp-1 {cmd}: {cmp:?}");
    }
    // The guests' capability is bound to the Overwolf pages.
    let elsewhere = invoke(
        &app,
        "owad-1",
        "https://example.com/",
        "adview_event",
        probe_body("adview_event"),
    );
    assert_eq!(outcome(&elsewhere), Outcome::Acl, "{elsewhere:?}");
    // An app webview cannot call the guest commands.
    let app2 = self::app("guests-main", &["main"]);
    for cmd in GUEST_ONLY {
        let r = invoke(&app2, "main", origin(), cmd, probe_body(cmd));
        assert_eq!(outcome(&r), Outcome::Acl, "{cmd}: {r:?}");
    }
}

#[test]
fn a_misgranted_plugin_label_is_refused_by_the_gate() {
    let app = app("misgranted", &["owad-misgranted", "ow-cmp-misgranted"]);
    for label in ["owad-misgranted", "ow-cmp-misgranted"] {
        for cmd in DEFAULT {
            let r = invoke(&app, label, origin(), cmd, probe_body(cmd));
            if cmd.starts_with("adview_") {
                // Argument errors come first for these (empty body).
                assert_ne!(outcome(&r), Outcome::Acl, "{label} {cmd}: {r:?}");
                continue;
            }
            assert_eq!(outcome(&r), Outcome::Forbidden, "{label} {cmd}: {r:?}");
        }
    }
}

#[test]
fn a_remote_page_in_an_app_webview_is_refused_by_the_gate() {
    let app = app("remote", &[]);
    WebviewWindowBuilder::new(
        &app,
        "main",
        WebviewUrl::External("https://example.com/".parse().unwrap()),
    )
    .build()
    .unwrap();
    let r = invoke(&app, "main", origin(), "get_info", json!({}));
    assert_eq!(outcome(&r), Outcome::Forbidden, "{r:?}");
    assert!(
        r.unwrap_err()["message"]
            .as_str()
            .unwrap()
            .contains("main is not a local app webview")
    );
}

#[test]
fn get_info_answers_without_machine_ids() {
    let app = app("info", &["main"]);
    let info = invoke(&app, "main", origin(), "get_info", json!({})).unwrap();
    assert_eq!(info["name"], "ACL Fixture");
    assert_eq!(info["uid"].as_str().unwrap().len(), 40);
    assert!(info.get("muid").is_none() && info.get("muidV2").is_none());
}
