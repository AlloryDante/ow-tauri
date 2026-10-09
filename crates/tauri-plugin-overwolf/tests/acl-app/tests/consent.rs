//! Consent through the compiled ACL on the mock runtime (DESIGN §4.7,
//! §7.2): the startup round and `isCMPRequired()`, the settings window and
//! `cmp_event`, the JavaScript `cmpURL` allowlist (SEC-M1) and the modal
//! privacy window parented to a window that hosts an ad (SEC-M7).
//!
//! Tauri's mock runtime never reports `Destroyed`, so a closed consent
//! window stays in the app's window map; the plugin's own record shows it
//! gone: its `cmp_event` answers `not-found`.
//!
//! What only the plugin's inline tests reach (pushes to ad guests,
//! page-load events): `host::consent::tests` and `host::ads::tests`, named
//! in the lane report.

#![allow(clippy::unwrap_used, reason = "a test fails on any unexpected error")]

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tauri::test::MockRuntime;
use tauri::{App, AppHandle, WebviewUrl};
use tauri_plugin_overwolf::consent::{DEFAULT_CMP_URL, STARTUP_CMP_URL, clear_consent_url};
use tauri_plugin_overwolf::{Builder, OverwolfExt};

use common::{CMP_PAGE, Capture, Outcome, code, invoke, origin, outcome, wait_until};

/// The startup consent window of the first round.
const STARTUP: &str = "ow-cmp-startup";
/// The settings window.
const SETTINGS: &str = "ow-cmp";
/// The hidden default-consent window of the first settings call.
const DEFAULT_CONSENT: &str = "ow-cmp-default";

/// A fixture app with window `main`, `cmp-eu-only` answered by `capture`
/// and `consent` merged into the consent configuration (no cookie
/// fallback, which needs a page). Returns the state directory.
fn fixture(
    name: &str,
    capture: &Arc<Capture>,
    consent: &Value,
) -> (App<MockRuntime>, std::path::PathBuf) {
    let mut block = json!({ "hostCookieFallback": "never" });
    if let (Some(b), Some(c)) = (block.as_object_mut(), consent.as_object()) {
        b.extend(c.clone());
    }
    let (context, dir) = common::context(
        name,
        &json!({
            "analytics": { "muidStrategy": "per-install" },
            "consent": block
        }),
        &[],
    );
    let app = common::build(
        context,
        Builder::new().analytics_transport(capture.clone()),
        None,
    );
    common::window(&app, "main", WebviewUrl::default());
    (app, dir)
}

/// `cmp_event` `name` from consent window `label`'s page.
fn cmp_event<M: tauri::Manager<MockRuntime>>(
    manager: &M,
    label: &str,
    name: &str,
    data: &Value,
) -> Result<Value, Value> {
    invoke(
        manager,
        label,
        CMP_PAGE,
        "cmp_event",
        json!({ "name": name, "data": data }),
    )
}

/// Whether the plugin still owns consent window `label` (a closed one
/// answers `not-found`).
fn open_consent_window(handle: &AppHandle<MockRuntime>, label: &str) -> bool {
    code(&cmp_event(handle, label, "ready", &Value::Null)) != Some("not-found")
}

/// `isCMPRequired()` from webview `main`.
fn is_cmp_required<M: tauri::Manager<MockRuntime>>(manager: &M) -> Result<Value, Value> {
    invoke(manager, "main", origin(), "is_cmp_required", json!({}))
}

/// The URL webview `label` shows (the mock runtime records navigations).
fn url_of(handle: &AppHandle<MockRuntime>, label: &str) -> String {
    common::webview(handle, label)
        .and_then(|w| w.url().ok())
        .map(|u| u.to_string())
        .unwrap_or_default()
}

/// The `cmp` block of `ow-electron.json` under `dir`.
fn stored_cmp(dir: &Path) -> Value {
    fn find(dir: &Path) -> Option<std::path::PathBuf> {
        std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
            let path = e.path();
            if path.is_dir() {
                find(&path)
            } else {
                (e.file_name() == "ow-electron.json").then_some(path)
            }
        })
    }
    find(dir)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .map(|v| v["cmp"].clone())
        .unwrap_or_default()
}

/// What the required-consent worker saw.
struct Seen {
    startup_url: String,
    ready_while_open: Result<Value, Value>,
    first: Result<Value, Value>,
    startup_open_after: bool,
    second: Result<Value, Value>,
    eu_only: usize,
    opened: Result<Value, Value>,
    settings_url: String,
    default_opened: bool,
    saved: Result<Value, Value>,
    from_main: Result<Value, Value>,
    off_scope: Result<Value, Value>,
}

/// The required-consent sequence after `RunEvent::Ready`.
fn required_sequence(handle: &AppHandle<MockRuntime>, capture: &Capture) -> Seen {
    assert!(
        wait_until(Duration::from_secs(20), || common::webview(handle, STARTUP)
            .is_some()),
        "the startup window opened after cmp-eu-only"
    );
    let startup_url = url_of(handle, STARTUP);
    let ready_while_open = cmp_event(handle, STARTUP, "ready", &Value::Null);
    // The round resolves when the startup window is done (its page load,
    // or `readyTimeoutMs` on the mock runtime, which loads no page).
    let first = is_cmp_required(handle);
    let startup_open_after = open_consent_window(handle, STARTUP);
    let second = is_cmp_required(handle);
    let eu_only = capture.eu_only_count();
    let opened = invoke(
        handle,
        "main",
        origin(),
        "open_ad_privacy_settings_window",
        json!({}),
    );
    let settings_url = url_of(handle, SETTINGS);
    let default_opened = common::webview(handle, DEFAULT_CONSENT).is_some();
    let saved = cmp_event(
        handle,
        SETTINGS,
        "saveConsent",
        &json!({ "consent": "CQTEST" }),
    );
    let from_main = invoke(
        handle,
        "main",
        origin(),
        "cmp_event",
        json!({ "name": "saveConsent", "data": { "consent": "CQMAIN" } }),
    );
    let off_scope = invoke(
        handle,
        SETTINGS,
        "https://evil.example/cmp.html",
        "cmp_event",
        json!({ "name": "saveConsent", "data": { "consent": "CQEVIL" } }),
    );
    // The default-consent window closes at its bound; it must be gone
    // before the runner destroys the windows.
    assert!(
        wait_until(Duration::from_secs(20), || !open_consent_window(
            handle,
            DEFAULT_CONSENT
        )),
        "the default-consent window closed at its bound"
    );
    Seen {
        startup_url,
        ready_while_open,
        first,
        startup_open_after,
        second,
        eu_only,
        opened,
        settings_url,
        default_opened,
        saved,
        from_main,
        off_scope,
    }
}

/// D.6.1, D.6.2, D.6.4, D.6.6: before `RunEvent::Ready` the answer is
/// `true` at once; at Ready the round sends `cmp-eu-only` and opens the
/// startup window, and `isCMPRequired()` resolves when that window is
/// done; a cacheable answer is not asked again. The settings window and
/// the hidden default-consent window open; `cmp_event` from the settings
/// page saves the consent to `ow-electron.json`; an app webview and a page
/// outside the consent scope cannot call it.
#[test]
fn a_required_round_resolves_and_the_settings_window_saves() {
    let capture = Capture::answering(r#"{"params":[]}"#);
    let (app, dir) = fixture(
        "consent-required",
        &capture,
        &json!({ "readyTimeoutMs": 1500 }),
    );
    assert_eq!(is_cmp_required(&app), Ok(json!(true)), "before Ready");
    assert_eq!(capture.eu_only_count(), 0, "nothing is sent before Ready");
    let worker_capture = Arc::clone(&capture);
    let seen = common::run(app, move |handle| {
        required_sequence(handle, &worker_capture)
    });
    assert!(
        seen.startup_url.starts_with(STARTUP_CMP_URL) && !seen.startup_url.contains("clear=true"),
        "the consent page: {}",
        seen.startup_url
    );
    assert_eq!(seen.ready_while_open, Ok(Value::Null));
    assert_eq!(seen.first, Ok(json!(true)));
    assert!(
        !seen.startup_open_after,
        "the round resolved with its window"
    );
    assert_eq!(seen.second, Ok(json!(true)));
    assert_eq!(seen.eu_only, 1, "a cacheable answer is not asked again");
    assert_eq!(seen.opened, Ok(Value::Null));
    assert!(
        seen.settings_url.starts_with(DEFAULT_CMP_URL),
        "{}",
        seen.settings_url
    );
    assert!(
        seen.default_opened,
        "consent is required: the default window opened"
    );
    assert_eq!(seen.saved, Ok(Value::Null));
    assert_eq!(stored_cmp(&dir)["cmpString"], "CQTEST");
    assert_eq!(
        outcome(&seen.from_main),
        Outcome::Acl,
        "{:?}",
        seen.from_main
    );
    assert_eq!(
        outcome(&seen.off_scope),
        Outcome::Acl,
        "{:?}",
        seen.off_scope
    );
}

/// D.6.2: a `no-cmp` answer opens the clearing page, `isCMPRequired()`
/// answers `false` (cached) and the settings call opens no default-consent
/// window.
#[test]
fn a_no_cmp_round_answers_false_and_is_cached() {
    let capture = Capture::answering(r#"{"params":["no-cmp"]}"#);
    let (app, _dir) = fixture(
        "consent-no-cmp",
        &capture,
        &json!({ "readyTimeoutMs": 1500 }),
    );
    let worker_capture = Arc::clone(&capture);
    let (url, first, second, eu_only, opened, default_opened) = common::run(app, move |handle| {
        assert!(
            wait_until(Duration::from_secs(20), || common::webview(handle, STARTUP)
                .is_some()),
            "the startup window opened"
        );
        let url = url_of(handle, STARTUP);
        let first = is_cmp_required(handle);
        let second = is_cmp_required(handle);
        let eu_only = worker_capture.eu_only_count();
        let opened = invoke(
            handle,
            "main",
            origin(),
            "open_ad_privacy_settings_window",
            json!({}),
        );
        let default_opened = common::webview(handle, DEFAULT_CONSENT).is_some();
        (url, first, second, eu_only, opened, default_opened)
    });
    assert_eq!(url, clear_consent_url());
    assert_eq!(first, Ok(json!(false)));
    assert_eq!(second, Ok(json!(false)));
    assert_eq!(eu_only, 1);
    assert_eq!(opened, Ok(Value::Null));
    assert!(!default_opened, "no default-consent window without consent");
}

/// SEC-M1: a JavaScript `cmpURL` must match `consent.allowedCmpOrigins`
/// (default `https://content.overwolf.com`) and be `https:`; anything
/// else is `invalid-argument` and opens nothing. A listed origin opens.
#[test]
fn js_cmp_url_off_the_allowlist_is_invalid_argument() {
    let capture = Capture::hanging_eu_only();
    let (app, _dir) = fixture(
        "consent-cmp-url",
        &capture,
        &json!({ "readyTimeoutMs": 600_000 }),
    );
    for cmd in ["open_ad_privacy_settings_window", "open_cmp_window"] {
        for url in [
            "https://evil.example/cmp.html",
            "http://content.overwolf.com/cmp.html",
            "https://content.overwolf.com.evil.example/cmp.html",
            "not a url",
        ] {
            let r = invoke(
                &app,
                "main",
                origin(),
                cmd,
                json!({ "options": { "cmpURL": url } }),
            );
            assert_eq!(code(&r), Some("invalid-argument"), "{cmd} {url}: {r:?}");
        }
    }
    assert!(common::webview(&app, SETTINGS).is_none(), "nothing opened");

    let (listed, _dir) = fixture(
        "consent-cmp-url-listed",
        &capture,
        &json!({
            "readyTimeoutMs": 600_000,
            "allowedCmpOrigins": ["https://cmp.example.com"]
        }),
    );
    let refused = invoke(
        &listed,
        "main",
        origin(),
        "open_cmp_window",
        json!({ "options": { "cmpURL": DEFAULT_CMP_URL } }),
    );
    assert_eq!(
        code(&refused),
        Some("invalid-argument"),
        "the list replaces the default"
    );
    let opened = invoke(
        &listed,
        "main",
        origin(),
        "open_cmp_window",
        json!({ "options": { "cmpURL": "https://cmp.example.com/consent.html" } }),
    );
    assert_eq!(opened, Ok(Value::Null));
    assert!(
        url_of(listed.handle(), SETTINGS).starts_with("https://cmp.example.com/consent.html"),
        "{}",
        url_of(listed.handle(), SETTINGS)
    );
}

/// SEC-M7, DESIGN §4.7.4: a modal privacy window from the page of a window
/// that hosts an ad guest is parented to that window (found by its Tauri
/// label, the guest beside it); a named parent must be an app window.
#[cfg(any(windows, target_os = "macos"))]
#[test]
fn the_modal_privacy_window_is_parented_to_an_ad_hosting_window() {
    let capture = Capture::hanging_eu_only();
    let (app, _dir) = fixture(
        "consent-modal",
        &capture,
        &json!({ "readyTimeoutMs": 600_000 }),
    );
    common::child_webview(
        &app,
        "main",
        "owad-1",
        WebviewUrl::External(common::ADVIEW_PAGE.parse().unwrap()),
    );
    let modal = invoke(
        &app,
        "main",
        origin(),
        "open_ad_privacy_settings_window",
        json!({ "options": { "modal": true } }),
    );
    assert_eq!(modal, Ok(Value::Null));
    assert!(common::webview(&app, SETTINGS).is_some());
    assert!(
        common::webview(&app, DEFAULT_CONSENT).is_some(),
        "consent counts as required before the first round"
    );
    let named = invoke(
        &app,
        "main",
        origin(),
        "open_ad_privacy_settings_window",
        json!({ "options": { "modal": true, "parent": "main" } }),
    );
    assert_eq!(named, Ok(Value::Null), "the open window is reused");
    for parent in ["missing", STARTUP, SETTINGS, "owad-1"] {
        let r = invoke(
            &app,
            "main",
            origin(),
            "open_ad_privacy_settings_window",
            json!({ "options": { "modal": true, "parent": parent } }),
        );
        assert_eq!(code(&r), Some("not-found"), "parent {parent}: {r:?}");
    }
    // The ad guest's page cannot open it at all.
    let from_guest = invoke(
        &app,
        "owad-1",
        common::ADVIEW_PAGE,
        "open_ad_privacy_settings_window",
        json!({}),
    );
    assert_eq!(outcome(&from_guest), Outcome::Acl, "{from_guest:?}");
    // Rust callers name any app window.
    let options = serde_json::from_value(json!({ "parent": "main" })).unwrap();
    let rust =
        tauri::async_runtime::block_on(app.overwolf().open_ad_privacy_settings_window(options));
    assert!(rust.is_ok(), "{rust:?}");
}
