//! `setWindowName` through the compiled ACL (DESIGN §3.5, SEC-m1, §7.2):
//! a page names only its own window. The command takes no label, so a page
//! cannot name another window; the Rust API names any app window.
//!
//! Which window got the name is not readable from outside the crate (the
//! override feeds the ad guests' `windowName`, CR-2 in the lane report);
//! `host::windows::tests::window_names` covers the override itself.

#![allow(clippy::unwrap_used, reason = "a test fails on any unexpected error")]

mod common;

use serde_json::json;
use tauri::WebviewUrl;
use tauri_plugin_overwolf::{ErrorCode, OverwolfExt};

use common::{Outcome, code, invoke, origin, outcome};

/// The call names the caller's window: label arguments are not part of
/// the command and change nothing; a remote page is refused.
#[test]
fn set_window_name_only_names_the_callers_window() {
    let app = common::app("window-name", &["main", "settings/panel", "overlay:hud"]);
    for (label, url) in [
        ("main", origin().to_owned()),
        ("settings/panel", format!("{}/settings.html", origin())),
        ("overlay:hud", format!("{}/hud.html", origin())),
    ] {
        let r = invoke(
            &app,
            label,
            &url,
            "set_window_name",
            json!({ "name": "home" }),
        );
        assert_eq!(r, Ok(serde_json::Value::Null), "{label}");
        // Another window's label in the arguments: ignored, the caller's
        // window is named.
        let r = invoke(
            &app,
            label,
            &url,
            "set_window_name",
            json!({ "name": "home", "label": "missing", "windowLabel": "missing" }),
        );
        assert_eq!(
            r,
            Ok(serde_json::Value::Null),
            "{label} with a label argument"
        );
    }
    // A remote page in the window: the local-only capability refuses it
    // (the plugin gate behind it is in `acl.rs`).
    let remote = invoke(
        &app,
        "main",
        "https://example.com/",
        "set_window_name",
        json!({ "name": "home" }),
    );
    assert_eq!(outcome(&remote), Outcome::Acl, "{remote:?}");
}

/// A child webview names the window it lives in.
#[cfg(any(windows, target_os = "macos"))]
#[test]
fn a_child_webview_names_its_window() {
    let app = common::app("window-name-child", &["overlay:hud"]);
    common::child_webview(
        &app,
        "overlay:hud",
        "embedded",
        WebviewUrl::App("hud.html".into()),
    );
    let r = invoke(
        &app,
        "embedded",
        origin(),
        "set_window_name",
        json!({ "name": "hud" }),
    );
    assert_eq!(r, Ok(serde_json::Value::Null));
}

/// Names are 1 to 128 printable ASCII characters (they become the
/// `x-ow-window` header): anything else is `invalid-argument`.
#[test]
fn window_names_are_short_printable_ascii() {
    let app = common::app("window-name-invalid", &["main"]);
    let long = "x".repeat(129);
    for name in [
        "",
        "a\nb",
        "a\r\nx-evil: 1",
        "caf\u{e9}",
        "tab\there",
        &long,
    ] {
        let r = invoke(
            &app,
            "main",
            origin(),
            "set_window_name",
            json!({ "name": name }),
        );
        assert_eq!(code(&r), Some("invalid-argument"), "{name:?}: {r:?}");
    }
    let r = invoke(
        &app,
        "main",
        origin(),
        "set_window_name",
        json!({ "name": "x".repeat(128) }),
    );
    assert_eq!(r, Ok(serde_json::Value::Null), "128 characters");
}

/// The Rust API names any app window, never a plugin window or an
/// unknown one.
#[test]
fn the_rust_api_names_app_windows_only() {
    let app = common::app("window-name-rust", &["main"]);
    common::window(&app, "settings/panel", WebviewUrl::default());
    let ow = app.overwolf();
    assert!(ow.set_window_name("settings/panel", "settings").is_ok());
    assert_eq!(
        ow.set_window_name("missing", "x").map_err(|e| e.code()),
        Err(ErrorCode::NotFound)
    );
    for reserved in ["owad-1", "ow-cmp", "ow-cmp-startup"] {
        assert_eq!(
            ow.set_window_name(reserved, "x").map_err(|e| e.code()),
            Err(ErrorCode::Forbidden),
            "{reserved}"
        );
    }
    assert_eq!(
        ow.set_window_name("main", "").map_err(|e| e.code()),
        Err(ErrorCode::InvalidArgument)
    );
}
