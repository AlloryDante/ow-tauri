//! Consent (DESIGN §4.7): the pure rules, and the flow on Tauri's mock
//! runtime driven through the hooks (the mock loop reports no window
//! events and never loads a page, so rounds resolve at their window's
//! `consent.readyTimeoutMs` bound, 300 ms here).

use std::future::Future;
use std::time::Duration;

use serde_json::json;

use super::*;
use crate::host::windows::tests::{Capture, mock_app, wait_until, window};

fn block_on<F: Future>(f: F) -> F::Output {
    tauri::async_runtime::block_on(f)
}

fn scope_url() -> Url {
    Url::parse(&format!("{STARTUP_CMP_URL}?unifiedcmp=")).unwrap()
}

#[test]
fn js_cmp_urls_must_be_allowed() {
    let allowed = vec!["https://content.overwolf.com".to_owned()];
    check_js_cmp_url("https://content.overwolf.com/monsdk/x.html", &allowed).unwrap();
    for bad in [
        "http://content.overwolf.com/x",
        "https://evil.example/x",
        "https://content.overwolf.com.evil.example/x",
        "not a url",
    ] {
        let err = check_js_cmp_url(bad, &allowed).unwrap_err();
        assert_eq!(err.code(), crate::ErrorCode::InvalidArgument, "{bad}");
    }
    let custom = vec!["https://cmp.example.com/".to_owned()];
    check_js_cmp_url("https://cmp.example.com/p?x=1", &custom).unwrap();
    assert!(check_js_cmp_url("https://content.overwolf.com/x", &custom).is_err());
    assert!(ConsentCore::default().last_answer());
}

#[test]
fn labels_and_colors() {
    assert_eq!(startup_label(1), "ow-cmp-startup");
    assert_eq!(startup_label(3), "ow-cmp-startup-3");
    assert_eq!(parse_color("#0D0D0D"), Some(Color(13, 13, 13, 255)));
    assert_eq!(parse_color("#0D0D0D80"), Some(Color(13, 13, 13, 128)));
    assert_eq!(parse_color("red"), None);
    assert!(in_cmp_scope(&scope_url()));
    assert!(!in_cmp_scope(
        &Url::parse("https://evil.example/monsdk/electron/").unwrap()
    ));
    for label in [CMP_SETTINGS_LABEL, CMP_STARTUP_LABEL, CMP_DEFAULT_LABEL] {
        assert!(crate::config::is_reserved_label(label));
    }
}

#[test]
fn navigation_table() {
    let cases: &[(&str, &str, Option<&str>, bool)] = &[
        (CMP_STARTUP_LABEL, STARTUP_CMP_URL, None, true),
        (CMP_SETTINGS_LABEL, "https://www.overwolf.com/x", None, true),
        (
            CMP_SETTINGS_LABEL,
            "https://cmp.example.com/p",
            Some("https://cmp.example.com"),
            true,
        ),
        (
            CMP_STARTUP_LABEL,
            "https://cmp.example.com/p",
            Some("https://cmp.example.com"),
            false,
        ),
        (
            CMP_SETTINGS_LABEL,
            "https://evil.example/",
            Some("https://cmp.example.com"),
            false,
        ),
        (
            CMP_SETTINGS_LABEL,
            "http://content.overwolf.com/x",
            None,
            false,
        ),
        (CMP_SETTINGS_LABEL, "data:text/html,x", None, true),
        (CMP_SETTINGS_LABEL, "about:blank", None, true),
        (CMP_SETTINGS_LABEL, "about:srcdoc", None, false),
        (CMP_SETTINGS_LABEL, "file:///etc/passwd", None, false),
    ];
    for (label, url, origin, ok) in cases {
        assert_eq!(
            navigation_allowed(label, &Url::parse(url).unwrap(), *origin),
            *ok,
            "{label} {url}"
        );
    }
}

/// Regression (Windows lab): ow-electron's consent page read
/// `getIsAdOptimizationEnabled()` as `true` on Windows.
#[test]
fn ad_optimization_defaults_per_platform_until_stored() {
    assert_eq!(crate::consent::ad_optimization(None), cfg!(windows));
    assert!(!crate::consent::ad_optimization(Some(false)));
    assert!(crate::consent::ad_optimization(Some(true)));
}

#[test]
fn the_shim_carries_its_configuration_token() {
    assert!(CMP_JS.contains(CMP_CONFIG_TOKEN));
    let script = crate::ads::splice_config(CMP_JS, CMP_CONFIG_TOKEN, &cmp_config(true)).unwrap();
    assert!(script.contains(r#"{"adOptimization":true}"#));
    assert!(script.contains("plugin:overwolf|cmp_event"));
}

#[test]
fn the_ads_environment_arguments() {
    use crate::ads::{ADS_PARITY_ARGS, ads_browser_args};
    assert_eq!(ads_browser_args(&[]), ADS_PARITY_ARGS);
    assert_eq!(
        ads_browser_args(&["--lang=de".into()]),
        format!("{ADS_PARITY_ARGS} --lang=de")
    );
    assert!(ADS_PARITY_ARGS.contains("--disable-web-security"));
}

/// DESIGN §4.4.9: consent windows never load a local-origin document.
#[test]
fn consent_windows_refuse_local_origins() {
    for url in [
        "tauri://localhost/index.html",
        "http://tauri.localhost/",
        "http://localhost:1420/",
        "http://asset.localhost/x",
    ] {
        let url: Url = url.parse().unwrap();
        for label in [CMP_SETTINGS_LABEL, CMP_STARTUP_LABEL, CMP_DEFAULT_LABEL] {
            assert!(!navigation_allowed(label, &url, None), "{label} {url}");
        }
        assert!(!crate::ads::frame_url_allowed(&url, &[]), "{url}");
    }
}

#[test]
fn resolve_takes_round_waiters() {
    let mut c = ConsentState::default();
    let (a, mut ra) = oneshot::channel();
    let (b, mut rb) = oneshot::channel();
    c.waiters.push((1, a));
    c.waiters.push((2, b));
    for tx in c.resolve(1) {
        tx.send(()).unwrap();
    }
    assert!(ra.try_recv().is_ok());
    assert!(rb.try_recv().is_err());
    assert_eq!(c.waiters.len(), 1);
    assert_eq!(c.resolved, vec![1]);
}

/// D.6.2: `no-cmp` opens the gate at once and `isCMPRequired()` is `false`,
/// cached for the launch; the request carries `cache-control: no-cache`.
#[test]
fn no_cmp_opens_the_gate_and_is_cached() {
    let capture = Capture::answering(r#"{"params":["no-cmp"]}"#);
    let (app, dir, core) = mock_app("consent-nocmp", &json!({}), &[], capture.clone());
    window(&app, "main");
    let gate = core.consent.subscribe_gate();
    assert!(!*gate.borrow());
    assert!(
        block_on(core.consent.is_cmp_required(&core)),
        "unknown before Ready"
    );
    assert_eq!(capture.eu_only_count(), 0);
    crate::host::lifecycle::on_ready(&core);
    assert!(!block_on(core.consent.is_cmp_required(&core)));
    assert!(core.consent.is_gate_open());
    assert!(!block_on(core.consent.is_cmp_required(&core)), "cached");
    assert_eq!(capture.eu_only_count(), 1);
    let eu = capture
        .requests()
        .into_iter()
        .find(|r| r.url.starts_with(crate::analytics::CMP_EU_ONLY_URL))
        .unwrap();
    assert!(
        eu.headers
            .iter()
            .any(|(k, v)| *k == "cache-control" && v == "no-cache")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// D.6.1: consent required by default; the gate opens when the startup
/// window closes, and the window is the plugin's own while it lives.
#[test]
fn required_and_the_gate_follows_the_startup_window() {
    let capture = Capture::answering(r#"{"params":[]}"#);
    let (app, dir, core) = mock_app("consent-required", &json!({}), &[], capture.clone());
    window(&app, "main");
    crate::host::lifecycle::on_ready(&core);
    assert!(wait_until(Duration::from_secs(5), || core
        .consent
        .owns_window(CMP_STARTUP_LABEL)));
    assert!(!core.consent.is_gate_open());
    assert!(block_on(core.consent.is_cmp_required(&core)));
    assert!(wait_until(Duration::from_secs(5), || core
        .consent
        .is_gate_open()));
    assert!(!core.consent.owns_window(CMP_STARTUP_LABEL));
    assert!(block_on(core.consent.is_cmp_required(&core)));
    assert_eq!(capture.eu_only_count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// D.6.2: a `{}` body is not cached: every call runs a round of its own.
#[test]
fn an_empty_answer_runs_a_round_per_call() {
    let capture = Capture::answering("{}");
    let (app, dir, core) = mock_app("consent-uncached", &json!({}), &[], capture.clone());
    window(&app, "main");
    crate::host::lifecycle::on_ready(&core);
    // The startup round resolves when its window gives up
    // (`readyTimeoutMs`); the answer is not cached.
    assert!(wait_until(Duration::from_secs(5), || lock(
        &core.consent.state
    )
    .resolved
    .contains(&1)));
    assert_eq!(lock(&core.consent.state).mode, CacheMode::Uncached);
    assert_eq!(capture.eu_only_count(), 1);
    assert!(block_on(core.consent.is_cmp_required(&core)));
    assert_eq!(capture.eu_only_count(), 2);
    assert!(block_on(core.consent.is_cmp_required(&core)));
    assert_eq!(capture.eu_only_count(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

/// DESIGN §4.7.1: a request without an answer counts as failed at
/// `consent.euOnlyTimeoutMs`; the startup window still opens.
#[test]
fn an_eu_only_timeout_counts_as_required() {
    let capture = Capture::hanging_eu_only();
    let (app, dir, core) = mock_app(
        "consent-timeout",
        &json!({ "consent": { "euOnlyTimeoutMs": 100 } }),
        &[],
        capture.clone(),
    );
    window(&app, "main");
    crate::host::lifecycle::on_ready(&core);
    assert!(wait_until(Duration::from_secs(5), || core
        .consent
        .owns_window(CMP_STARTUP_LABEL)));
    assert!(block_on(core.consent.is_cmp_required(&core)));
    assert_eq!(capture.eu_only_count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The ow-tauri user switch off: no `cmp-eu-only`, consent required.
#[test]
fn the_user_switch_off_sends_no_eu_only_request() {
    let capture = Capture::answering(r#"{"params":["no-cmp"]}"#);
    let (app, dir, core) = mock_app(
        "consent-switch",
        &json!({ "analytics": { "userSwitch": true } }),
        &[],
        capture.clone(),
    );
    window(&app, "main");
    crate::ext::Overwolf(core.clone())
        .set_analytics_user_enabled(false)
        .unwrap();
    crate::host::lifecycle::on_ready(&core);
    assert!(block_on(core.consent.is_cmp_required(&core)));
    assert!(capture.requests().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// DESIGN §4.7.4: a modal privacy window from a window that hosts an ad
/// (several webviews) finds its parent; unknown and plugin windows are
/// `not-found`. The first call also opens the hidden default window.
#[test]
fn modal_privacy_window_on_an_ad_hosting_window() {
    let capture = Capture::answering(r#"{"params":[]}"#);
    let (app, dir, core) = mock_app(
        "consent-modal",
        &json!({ "consent": { "readyTimeoutMs": 10000 } }),
        &[],
        capture,
    );
    window(&app, "main");
    #[cfg(ow_tauri_ads)]
    {
        use tauri::Manager as _;
        let main = app.get_window("main").unwrap();
        let mock = crate::host::windows::tests::mock_windows();
        main.add_child(
            tauri::webview::WebviewBuilder::new(
                "owad-1",
                WebviewUrl::External("https://content.overwolf.com/".parse().unwrap()),
            ),
            tauri::LogicalPosition::new(0.0, 0.0),
            tauri::LogicalSize::new(300.0, 250.0),
        )
        .unwrap();
        drop(mock);
        assert!(
            app.get_webview_window("main").is_none(),
            "an ad-hosting window"
        );
    }
    let modal = CmpWindowOptions {
        modal: Some(true),
        ..CmpWindowOptions::default()
    };
    let unknown = CmpWindowOptions {
        parent: Some("missing".into()),
        ..CmpWindowOptions::default()
    };
    let plugin_parent = CmpWindowOptions {
        parent: Some(CMP_STARTUP_LABEL.into()),
        ..CmpWindowOptions::default()
    };
    let error_code = |o: &CmpWindowOptions, caller: Option<&str>| {
        core.consent
            .open_settings_window(&core, o, caller)
            .unwrap_err()
            .code()
    };
    assert_eq!(
        error_code(&unknown, Some("main")),
        crate::ErrorCode::NotFound
    );
    assert_eq!(error_code(&plugin_parent, None), crate::ErrorCode::NotFound);
    let not_https = CmpWindowOptions {
        cmp_url: Some("http://content.overwolf.com/x".into()),
        ..CmpWindowOptions::default()
    };
    assert_eq!(
        error_code(&not_https, None),
        crate::ErrorCode::InvalidArgument
    );
    core.consent
        .open_settings_window(&core, &modal, Some("main"))
        .unwrap();
    assert!(core.consent.owns_window(CMP_SETTINGS_LABEL));
    assert!(core.consent.owns_window(CMP_DEFAULT_LABEL));
    assert!(crate::compat::window(&core.app, CMP_SETTINGS_LABEL).is_some());
    // A second call reuses the window.
    core.consent
        .open_settings_window(&core, &CmpWindowOptions::default(), None)
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// D.6.6: saves go to `ow-electron.json` and to the consent listeners;
/// pages outside the scope and bad strings are refused.
#[test]
fn cmp_events_save_and_notify() {
    let (_app, dir, core) = mock_app("consent-events", &json!({}), &[], Capture::answering("{}"));
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let s = seen.clone();
    core.consent
        .add_consent_listener(Arc::new(move |v: &str| lock(&s).push(v.to_owned())));
    let url = scope_url();
    let event = |name, consent: Option<&str>, enabled: Option<bool>| {
        core.consent.cmp_event(
            &core,
            CMP_STARTUP_LABEL,
            Some(&url),
            name,
            Some(CmpEventData {
                consent: consent.map(str::to_owned),
                enabled,
            }),
        )
    };
    event(CmpEventName::SaveConsent, Some("CQTEST"), None).unwrap();
    event(
        CmpEventName::SaveUnifiedConsent,
        Some("cmp=CQTEST&ac=2~1"),
        None,
    )
    .unwrap();
    event(CmpEventName::EnableAdOptimization, None, Some(true)).unwrap();
    event(CmpEventName::Ready, None, None).unwrap();
    let shared = core.state.ow_electron.read().state.cmp.unwrap();
    assert_eq!(shared.cmp_string.as_deref(), Some("CQTEST"));
    assert_eq!(
        shared.unified_consent_string.as_deref(),
        Some("cmp%3DCQTEST%26ac%3D2~1")
    );
    assert!(shared.time_stamp.unwrap() > 0);
    assert_eq!(core.state.ow_tauri.get().ad_optimization, Some(true));
    assert_eq!(*lock(&seen), ["CQTEST", "cmp%3DCQTEST%26ac%3D2~1"]);

    // A cleared string stores timeStamp 0 (observed).
    event(CmpEventName::SaveConsent, Some(""), None).unwrap();
    assert_eq!(
        core.state.ow_electron.read().state.cmp.unwrap().time_stamp,
        Some(0)
    );
    let error_code = |r: Result<()>| r.unwrap_err().code();
    assert_eq!(
        error_code(event(CmpEventName::SaveConsent, Some("a b"), None)),
        crate::ErrorCode::InvalidArgument
    );
    assert_eq!(
        error_code(event(CmpEventName::EnableAdOptimization, None, None)),
        crate::ErrorCode::InvalidArgument
    );
    let outside = Url::parse("https://www.overwolf.com/elsewhere").unwrap();
    assert_eq!(
        error_code(core.consent.cmp_event(
            &core,
            CMP_STARTUP_LABEL,
            Some(&outside),
            CmpEventName::SaveConsent,
            None
        )),
        crate::ErrorCode::Forbidden
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// DESIGN §4.2: when the last app window is destroyed, the settings and
/// default windows close at once, a startup window as soon as its page
/// has saved.
#[test]
fn the_last_window_rule() {
    let capture = Capture::answering(r#"{"params":[]}"#);
    let (app, dir, core) = mock_app(
        "consent-last-window",
        &json!({ "consent": { "readyTimeoutMs": 10000 } }),
        &[],
        capture,
    );
    window(&app, "main");
    crate::host::lifecycle::on_ready(&core);
    assert!(wait_until(Duration::from_secs(5), || core
        .consent
        .owns_window(CMP_STARTUP_LABEL)));
    core.consent
        .open_settings_window(&core, &CmpWindowOptions::default(), None)
        .unwrap();
    // As the dispatcher delivers `Destroyed`: windows, then consent.
    core.windows
        .window_event(&core, "main", &WindowEvent::Destroyed);
    core.consent
        .window_event(&core, "main", &WindowEvent::Destroyed);
    assert!(wait_until(Duration::from_secs(5), || !core
        .consent
        .owns_window(CMP_DEFAULT_LABEL)));
    assert!(!core.consent.owns_window(CMP_SETTINGS_LABEL));
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        core.consent.owns_window(CMP_STARTUP_LABEL),
        "the startup page may still save"
    );
    assert!(!core.consent.is_gate_open());
    core.consent
        .cmp_event(
            &core,
            CMP_STARTUP_LABEL,
            Some(&scope_url()),
            CmpEventName::SaveConsent,
            Some(CmpEventData {
                consent: Some("CQ".into()),
                enabled: None,
            }),
        )
        .unwrap();
    assert!(wait_until(Duration::from_secs(5), || !core
        .consent
        .owns_window(CMP_STARTUP_LABEL)));
    assert!(core.consent.is_gate_open());
    let _ = std::fs::remove_dir_all(&dir);
}

/// W0c ruling 1: on macOS the app's terminate hook closes a hidden consent
/// window as after any crash (its round resolves); any other consent
/// window is left to reload in place.
#[cfg(target_os = "macos")]
#[test]
fn the_terminate_hook_closes_a_hidden_consent_window() {
    let capture = Capture::answering(r#"{"params":[]}"#);
    let (app, dir, core) = mock_app(
        "consent-terminate",
        &json!({ "consent": { "readyTimeoutMs": 60_000 } }),
        &[],
        capture,
    );
    window(&app, "main");
    let (tx, rx) = oneshot::channel();
    {
        let mut s = lock(&core.consent.state);
        s.rounds = 7;
        s.waiters.push((7, tx));
    }
    open_hidden_window(&core, CMP_DEFAULT_LABEL, &default_consent_url(), Some(7)).unwrap();
    assert!(core.consent.owns_window(CMP_DEFAULT_LABEL));
    let webview = crate::compat::webview(&app, CMP_DEFAULT_LABEL).unwrap();
    crate::platform::terminate::handle_web_content_process_terminate(&webview);
    assert!(wait_until(Duration::from_secs(5), || !core
        .consent
        .owns_window(CMP_DEFAULT_LABEL)));
    assert!(block_on(rx).is_ok(), "the round resolved");
    assert!(lock(&core.consent.state).resolved.contains(&7));
    // Not a hidden consent window: left to the caller's in-place reload.
    assert!(!web_content_terminated(&core, CMP_SETTINGS_LABEL));
    assert!(!web_content_terminated(&core, CMP_DEFAULT_LABEL));
    let _ = std::fs::remove_dir_all(&dir);
}
