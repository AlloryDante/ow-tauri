//! Window tracking (DESIGN §4.3, §4.11, §7.1): the name, title and
//! visibility tables, and the tracker and ticker on Tauri's mock runtime.
//!
//! The mock runtime's event loop handles one message per second and never
//! reports window events, so these tests drive the hooks directly
//! (`lifecycle::on_ready`, `poll_now`, `window_event`), as the dispatcher
//! does.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::{App, WebviewUrl, WebviewWindowBuilder};

use super::*;
use crate::analytics::{BoxFuture, CMP_EU_ONLY_URL, HostRequest, HostResponse, Transport};
use crate::host::core_of;

/// Records every request; answers `cmp-eu-only` with `eu_only` (or never,
/// when `None`) and everything else with an empty 200.
pub(crate) struct Capture {
    requests: Mutex<Vec<HostRequest>>,
    eu_only: Option<Vec<u8>>,
}

impl Capture {
    /// `cmp-eu-only` answers `body`.
    pub(crate) fn answering(body: &str) -> Arc<Self> {
        Arc::new(Capture {
            requests: Mutex::new(Vec::new()),
            eu_only: Some(body.as_bytes().to_vec()),
        })
    }

    /// `cmp-eu-only` never answers.
    pub(crate) fn hanging_eu_only() -> Arc<Self> {
        Arc::new(Capture {
            requests: Mutex::new(Vec::new()),
            eu_only: None,
        })
    }

    /// Every request so far.
    pub(crate) fn requests(&self) -> Vec<HostRequest> {
        lock(&self.requests).clone()
    }

    /// The Counter names (`Name=`) sent so far, in order.
    pub(crate) fn counters(&self) -> Vec<String> {
        self.requests()
            .iter()
            .filter_map(|r| {
                let url = Url::parse(&r.url).ok()?;
                let name = url.query_pairs().find(|(k, _)| k == "Name")?.1;
                Some(name.into_owned())
            })
            .collect()
    }

    /// The `Extra` objects of the Counters named `name`.
    pub(crate) fn extras(&self, name: &str) -> Vec<Value> {
        self.requests()
            .iter()
            .filter_map(|r| {
                let url = Url::parse(&r.url).ok()?;
                let pairs: BTreeMap<String, String> = url.query_pairs().into_owned().collect();
                (pairs.get("Name").map(String::as_str) == Some(name))
                    .then(|| serde_json::from_str(pairs.get("Extra")?).ok())?
            })
            .collect()
    }

    /// How many `cmp-eu-only` requests were sent.
    pub(crate) fn eu_only_count(&self) -> usize {
        self.requests()
            .iter()
            .filter(|r| r.url.starts_with(CMP_EU_ONLY_URL))
            .count()
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

/// A mock app with the plugin (`extra` merged into its configuration) and
/// the config window entries `config_windows` (`[label, title]`); returns
/// the app, its state directory and the plugin core. No window is created.
pub(crate) fn mock_app(
    name: &str,
    extra: &Value,
    config_windows: &[(&str, &str)],
    transport: Arc<dyn Transport>,
) -> (App<MockRuntime>, PathBuf, Arc<Core<MockRuntime>>) {
    let dir = crate::state::test_dir(name);
    let mut config = json!({
        "author": "Example Studio",
        "name": "Example App",
        "state": { "appDataDir": dir },
        "analytics": { "muidStrategy": "per-install" },
        "consent": { "readyTimeoutMs": 300 }
    });
    if let (Some(base), Some(more)) = (config.as_object_mut(), extra.as_object()) {
        for (k, v) in more {
            match (base.get_mut(k), v) {
                (Some(Value::Object(b)), Value::Object(m)) => {
                    b.extend(m.clone());
                }
                _ => {
                    base.insert(k.clone(), v.clone());
                }
            }
        }
    }
    let mut context = mock_context(noop_assets());
    context
        .config_mut()
        .plugins
        .0
        .insert("overwolf".into(), config);
    for (label, title) in config_windows {
        let window: tauri::utils::config::WindowConfig = serde_json::from_value(json!({
            "label": label,
            "title": title,
            "create": false
        }))
        .unwrap();
        context.config_mut().app.windows.push(window);
    }
    let mut builder = crate::Builder::new();
    let options = builder.options_mut();
    options.transport = Some(transport);
    options.os_queries = false;
    options.runtime_capabilities = false;
    options.argv = Some(vec!["app".into()]);
    let app = mock_builder()
        .plugin(builder.build())
        .build(context)
        .unwrap();
    let core = core_of(app.handle()).expect("the plugin ran its setup");
    (app, dir, core)
}

/// A window `label` with one webview of the same label.
pub(crate) fn window(app: &App<MockRuntime>, label: &str) {
    let _mock = mock_windows();
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .unwrap();
}

/// Serialises window creation and destruction in tests: Tauri's mock
/// runtime keeps its windows in a `RefCell`, and a consent window built or
/// destroyed from a runtime task while the test thread builds one panics
/// with "already borrowed" (the real runtimes hop to the event loop).
pub(crate) fn mock_windows() -> MutexGuard<'static, ()> {
    static MOCK_WINDOWS: Mutex<()> = Mutex::new(());
    MOCK_WINDOWS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Waits until `done` holds, at most `limit`.
pub(crate) fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    done()
}

#[test]
fn globs() {
    assert!(glob_matches("tray*", "tray"));
    assert!(glob_matches("tray*", "tray-menu"));
    assert!(glob_matches("a?c", "abc"));
    assert!(!glob_matches("a?c", "ac"));
    assert!(!glob_matches("tray", "tray2"));
    assert!(glob_matches("*", ""));
}

#[test]
fn counted_windows() {
    let w = AppWindows::new(&["tray*".to_owned()]);
    assert!(w.is_counted("main"));
    assert!(!w.is_counted("tray-menu"));
    assert!(!w.is_counted("ow-cmp-default"));
    assert!(!w.is_counted("owad-1"));
}

#[test]
fn window_names() {
    assert!(valid_window_name("settings"));
    assert!(!valid_window_name(""));
    assert!(!valid_window_name("a\r\nx-evil: 1"));
    assert!(!valid_window_name(&"x".repeat(129)));
    assert!(!valid_window_name("caf\u{e9}"));
    let w = AppWindows::new(&[]);
    w.set_name("main", "home");
    assert_eq!(w.name_override("main").as_deref(), Some("home"));
}

/// DESIGN §7.1: titles (D5, PAR-minor-2).
#[test]
fn title_table() {
    let cases: &[(Option<&str>, Option<&str>, &str)] = &[
        (Some("Main"), Some("Set in setup"), "Main"),
        (None, Some("Native"), "Native"),
        (None, Some(PLACEHOLDER_TITLE), "My App"),
        (None, Some(""), "My App"),
        (Some(PLACEHOLDER_TITLE), Some("Native"), "My App"),
        (Some(""), None, "My App"),
        (None, None, "My App"),
    ];
    for (configured, native, want) in cases {
        assert_eq!(
            window_title(*configured, *native, "My App"),
            *want,
            "{configured:?} {native:?}"
        );
    }
}

/// DESIGN §7.1: names (§4.3.3): the app origin's `index.html` rule for app
/// pages, remote URLs unchanged.
#[test]
fn name_table() {
    let cases: &[(&str, bool, &str)] = &[
        ("tauri://localhost/", true, "index"),
        ("tauri://localhost", true, "index"),
        ("http://tauri.localhost/", true, "index"),
        ("https://tauri.localhost/", true, "index"),
        ("tauri://localhost/settings.html", true, "settings"),
        ("tauri://localhost/nested/", true, "index"),
        ("http://localhost:1420/", true, "index"),
        ("tauri://localhost/route", true, "route"),
        ("tauri://localhost/#/hash", true, "index"),
        ("https://example.com/", false, "example.com"),
        ("https://example.com/page.html?x=1", false, "page"),
        ("about:blank", false, "blank"),
    ];
    for (url, app_page, want) in cases {
        assert_eq!(
            crate::analytics::app_window_name(url, *app_page),
            *want,
            "{url}"
        );
    }
    let (_app, dir, core) = mock_app("windows-names", &json!({}), &[], Capture::answering("{}"));
    for (url, want) in [
        ("tauri://localhost/", "index"),
        ("http://tauri.localhost/about.html", "about"),
        ("https://example.com/", "example.com"),
    ] {
        assert_eq!(url_name(&core, &Url::parse(url).unwrap()), want, "{url}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// DESIGN §7.1: the visible-period state machine (E.2 #7).
#[test]
fn visibility_table() {
    use VisibilityChange::{None as Nothing, Shown};
    let ended = |name: &str, ms: u64| {
        VisibilityChange::Ended(VisiblePeriod {
            name: name.to_owned(),
            title: "T".to_owned(),
            visible_ms: ms,
        })
    };
    let entry = || WindowEntry {
        title: Some("T".into()),
        ..WindowEntry::new(true)
    };

    // Shown, then hidden.
    let mut e = entry();
    e.page_finished("index".into());
    assert_eq!(e.observe(true, false, 0), Shown);
    assert_eq!(e.observe(true, false, 250), Nothing);
    assert_eq!(e.observe(false, false, 1500), ended("index", 1500));
    assert_eq!(e.observe(false, false, 1750), Nothing);

    // A minimize ends the period; the restore starts none; a show after a
    // hide does.
    let mut e = entry();
    assert_eq!(e.observe(true, false, 0), Shown);
    assert_eq!(e.observe(true, true, 2000), ended("blank", 2000));
    assert_eq!(e.observe(true, false, 3000), Nothing, "restore");
    assert_eq!(e.observe(false, false, 4000), Nothing, "hide");
    assert_eq!(e.observe(true, false, 5000), Shown, "show after hide");

    // macOS: during the minimize animation the window is neither visible
    // nor minimized; that is part of the minimize, not a hide.
    let mut e = entry();
    assert_eq!(e.observe(true, false, 0), Shown);
    e.minimizing = true;
    assert_eq!(e.observe(false, false, 100), ended("blank", 100));
    assert!(e.minimizing && e.minimize_ended);
    assert_eq!(e.observe(false, true, 300), Nothing);
    assert!(!e.minimizing);
    assert_eq!(e.observe(true, false, 900), Nothing, "restore");

    // The name is fixed when the window is first visible with a loaded
    // page; later loads (SPA routes, navigations) never rename it.
    let mut e = entry();
    assert_eq!(e.observe(true, false, 0), Shown);
    assert_eq!(e.name, None);
    e.page_finished("late".into());
    assert_eq!(e.name.as_deref(), Some("late"));
    e.page_finished("other".into());
    assert_eq!(e.observe(false, false, 1000), ended("late", 1000));
    e.page_finished("third".into());
    assert_eq!(e.observe(true, false, 2000), Shown);
    assert_eq!(e.observe(false, false, 3000), ended("late", 1000));
}

#[test]
fn the_naming_webview_is_labelled_like_its_window_or_the_first() {
    let mut e = WindowEntry::new(true);
    assert_eq!(e.naming_webview("main"), None);
    e.webviews = vec!["child".into(), "main".into()];
    assert_eq!(e.naming_webview("main"), Some("main"));
    e.webviews = vec!["a".into(), "b".into()];
    assert_eq!(e.naming_webview("main"), Some("a"));
}

/// Registration, titles, #5 once after the burst, names, `window_closed`.
#[test]
fn tracked_windows_feed_the_analytics() {
    let capture = Capture::answering("{}");
    let (app, dir, core) = mock_app(
        "windows-tracked",
        &json!({ "analytics": { "excludeWindows": ["tray*"] } }),
        &[("settings", "Settings Title")],
        capture.clone(),
    );
    window(&app, "main");
    window(&app, "settings");
    window(&app, "tray-menu");
    assert!(core.windows.contains("main") && core.windows.contains("tray-menu"));
    assert_eq!(core.windows.first_app_webview().as_deref(), Some("main"));

    // Before Ready: polls see the windows, #5 waits for the burst.
    core.windows.poll_now(&core);
    assert!(capture.requests().is_empty());
    crate::host::lifecycle::on_ready(&core);
    assert!(wait_until(Duration::from_secs(5), || capture
        .counters()
        .iter()
        .filter(|c| c.ends_with("_app_heartbeat"))
        .count()
        == 2));
    core.windows.poll_now(&core);
    core.windows.poll_now(&core);
    std::thread::sleep(Duration::from_millis(100));
    let beats = capture.extras("tauri_app_heartbeat");
    assert_eq!(
        beats
            .iter()
            .map(|e| e["hasVisibleWindow"].clone())
            .collect::<Vec<_>>(),
        [json!(false), json!(true)],
        "#5 once, after #4"
    );
    // D5: the mock window has no native title.
    assert_eq!(core.windows.title("main").as_deref(), Some("Example App"));
    assert_eq!(
        core.windows.title("settings").as_deref(),
        Some("Settings Title")
    );
    assert_eq!(core.windows.is_visible("main"), Some(true));

    // Names before the first loaded page follow the current URL.
    let url = Url::parse("tauri://localhost/").unwrap();
    assert_eq!(core.windows.window_name(&core, "main", Some(&url)), "index");
    assert_eq!(core.windows.window_name(&core, "main", None), "blank");
    core.windows.set_name("main", "home");
    assert_eq!(core.windows.window_name(&core, "main", Some(&url)), "home");

    // A destroyed counted window ends its period (≥ 1 s); an excluded one
    // sends nothing.
    std::thread::sleep(Duration::from_millis(1100));
    for label in ["main", "tray-menu"] {
        core.windows
            .window_event(&core, label, &WindowEvent::Destroyed);
    }
    assert!(!core.windows.contains("main"));
    assert_eq!(core.windows.name_override("main"), None);
    assert!(wait_until(Duration::from_secs(5), || !capture
        .extras("tauri_window_closed")
        .is_empty()));
    let closed = capture.extras("tauri_window_closed");
    assert_eq!(closed.len(), 1, "{closed:?}");
    assert_eq!(closed[0]["name"], "blank");
    assert_eq!(closed[0]["title"], "Example App");
    assert!(closed[0]["length"].as_u64().unwrap() >= 1);

    // At exit the open period of `settings` ends.
    end_all_periods(&core);
    assert!(wait_until(Duration::from_secs(5), || capture
        .extras("tauri_window_closed")
        .len()
        == 2));
    assert_eq!(
        capture.extras("tauri_window_closed")[1]["title"],
        "Settings Title"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reserved_labels_are_never_tracked() {
    let (app, dir, core) = mock_app(
        "windows-reserved",
        &json!({}),
        &[],
        Capture::answering("{}"),
    );
    window(&app, "owad-mine");
    window(&app, "ow-cmp-fake");
    assert!(core.windows.is_empty());
    assert_eq!(core.windows.first_app_webview(), None);
    window(&app, "main");
    assert!(core.windows.contains("main"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// DESIGN §4.11: 250 ms ticks of one main-thread hop each while a window
/// exists, never a second hop while one is queued, parked (no hop at all)
/// without windows. (The mock runtime runs a hop inline when its loop is
/// not running.)
#[test]
fn the_ticker_hops_once_per_tick_and_parks_without_windows() {
    let (app, dir, core) = mock_app("windows-ticker", &json!({}), &[], Capture::answering("{}"));
    crate::host::lifecycle::on_ready(&core);
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(core.ticker.hops(), 0, "parked: a tray app polls nothing");
    let wait = core.analytics.until_next_check(core.now());
    assert!(
        wait > Duration::from_secs(3500) && wait <= Duration::from_secs(3600),
        "parked until the hourly check: {wait:?}"
    );

    window(&app, "late");
    std::thread::sleep(Duration::from_millis(1100));
    let (hops, polls) = (core.ticker.hops(), core.ticker.polls());
    assert!((4..=7).contains(&hops), "about one hop per 250 ms: {hops}");
    assert_eq!(polls, hops, "one poll per hop");

    // A queued hop blocks the next one.
    core.ticker.hop_pending.store(true, Ordering::SeqCst);
    request_poll(&core);
    assert_eq!(core.ticker.hops(), core.ticker.polls());
    core.ticker.hop_pending.store(false, Ordering::SeqCst);

    // The last window gone: parked again.
    core.windows
        .window_event(&core, "late", &WindowEvent::Destroyed);
    std::thread::sleep(Duration::from_millis(300));
    let parked = core.ticker.hops();
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(core.ticker.hops(), parked);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `set_window_name` names the caller's own window only: the command takes
/// no label (DESIGN §3.5), and the Rust call refuses plugin and unknown
/// windows.
#[test]
fn set_window_name_is_own_window_only() {
    let source = include_str!("../../commands/info.rs");
    let start = source.find("async fn set_window_name").unwrap();
    let signature = &source[start..start + source[start..].find('{').unwrap()];
    assert!(signature.contains("name: String"), "{signature}");
    assert!(!signature.contains("label"), "{signature}");
    assert!(source[start..].contains("webview.window().label()"));

    let (app, dir, core) = mock_app("windows-setname", &json!({}), &[], Capture::answering("{}"));
    window(&app, "main");
    let ow = crate::ext::Overwolf(core.clone());
    ow.set_window_name("main", "home").unwrap();
    assert_eq!(core.windows.name_override("main").as_deref(), Some("home"));
    let error_code = |r: crate::Result<()>| r.unwrap_err().code();
    assert_eq!(
        error_code(ow.set_window_name("owad-1", "x")),
        crate::ErrorCode::Forbidden
    );
    assert_eq!(
        error_code(ow.set_window_name("missing", "x")),
        crate::ErrorCode::NotFound
    );
    assert_eq!(
        error_code(ow.set_window_name("main", "a\nb")),
        crate::ErrorCode::InvalidArgument
    );
    let _ = std::fs::remove_dir_all(&dir);
}
