//! The guest host on Tauri's mock runtime (DESIGN §7.3): forged guest
//! events, isolation between embedders, window close, hide and minimize,
//! recreates.
//!
//! The mock runtime runs no page and never reports window events, so these
//! tests drive the driver's hooks as the dispatcher, the window ticker, the
//! platform callbacks and the command layer do, and read what the host sent
//! and did from its trace (recorded by a host without OS queries).

use std::sync::Mutex;

use tauri::App;
use tauri::Manager;
use tauri::ipc::InvokeResponseBody;
use tauri::test::MockRuntime;

use super::*;
use crate::host::windows::tests::{Capture, mock_app, wait_until, window};
use crate::identity::EmailHashes;

type Events = Arc<Mutex<Vec<Value>>>;
type Mock = Arc<Core<MockRuntime>>;

const AD_LINK: &str = "https://advertiser.example/landing";

/// A channel that keeps every message it is sent.
fn collector() -> (Channel<ChannelMessage>, Events) {
    let events: Events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let channel = Channel::new(move |body: InvokeResponseBody| {
        if let InvokeResponseBody::Json(json) = body {
            sink.lock()
                .unwrap()
                .push(serde_json::from_str(&json).unwrap());
        }
        Ok(())
    });
    (channel, events)
}

fn request(element_id: &str, performance: bool) -> AdviewMount {
    serde_json::from_value(json!({
        "elementId": element_id,
        "attributes": {
            "cid": "example-cid",
            "slotsize": "300x250",
            "adstyle": "default",
            "performance": performance
        },
        "rect": { "x": 10.0, "y": 20.0, "width": 300.0, "height": 250.0 },
        "visible": true,
        "devicePixelRatio": 1.0,
        "innerWidth": 800.0
    }))
    .unwrap()
}

/// Mounts `element_id` in webview `embedder`; returns the guest label and
/// the element's events.
fn mount_on(
    core: &Mock,
    app: &App<MockRuntime>,
    embedder: &str,
    element_id: &str,
) -> (String, Events) {
    let webview = app.get_webview(embedder).unwrap();
    let (channel, events) = collector();
    let label =
        tauri::async_runtime::block_on(mount(core, &webview, request(element_id, false), channel))
            .unwrap();
    (label, events)
}

/// A mock app with ads configuration `ads` and windows `labels`.
fn app_with(
    name: &str,
    ads: &Value,
    labels: &[&str],
) -> (App<MockRuntime>, std::path::PathBuf, Mock, Arc<Capture>) {
    let capture = Capture::answering(r#"{"params":[]}"#);
    let (app, dir, core) = mock_app(name, &json!({ "ads": ads }), &[], capture.clone());
    for l in labels {
        window(&app, l);
    }
    (app, dir, core, capture)
}

fn trace(core: &Mock) -> Vec<Value> {
    lock(&core.ads.state).trace.clone()
}

fn clear_trace(core: &Mock) {
    lock(&core.ads.state).trace.clear();
}

/// The arguments of every `function` call into guest `label`, in order.
fn calls(core: &Mock, label: &str, function: &str) -> Vec<Value> {
    trace(core)
        .into_iter()
        .filter(|e| e["via"] == "guest-call" && e["label"] == label && e["function"] == function)
        .map(|e| e["args"].clone())
        .collect()
}

/// Every host message of type `kind` delivered to guest `label`.
fn delivered(core: &Mock, label: &str, kind: &str) -> Vec<Value> {
    trace(core)
        .into_iter()
        .filter(|e| {
            e["via"] == "private-message" && e["label"] == label && e["message"]["type"] == kind
        })
        .map(|e| e["message"].clone())
        .collect()
}

/// The trace entries of `kind` for guest `label`.
fn of_kind(core: &Mock, label: &str, kind: &str) -> Vec<Value> {
    trace(core)
        .into_iter()
        .filter(|e| e["kind"] == kind && e["label"] == label)
        .collect()
}

/// The position of the first trace entry matching `f`.
fn position(core: &Mock, f: impl Fn(&Value) -> bool) -> Option<usize> {
    trace(core).iter().position(f)
}

fn names(events: &Events) -> Vec<String> {
    events
        .lock()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn count(events: &Events, name: &str) -> usize {
    names(events).iter().filter(|n| *n == name).count()
}

/// How many analytics requests carry Kind `kind`.
fn kind_count(capture: &Capture, kind: u32) -> usize {
    let needle = format!("\"Kind\":{kind}");
    capture
        .requests()
        .iter()
        .filter(|r| {
            r.body
                .as_ref()
                .is_some_and(|b| String::from_utf8_lossy(b).contains(&needle))
        })
        .count()
}

fn guest_exists(core: &Mock, label: &str) -> bool {
    lock(&core.ads.state).guests.contains_key(label)
}

fn observation(label: &str, visible: bool, minimized: bool) -> WindowObservation {
    WindowObservation {
        label: label.to_owned(),
        visible,
        minimized,
        changed: true,
    }
}

fn link() -> Url {
    Url::parse(AD_LINK).unwrap()
}

#[test]
fn a_mount_creates_a_hidden_muted_guest_and_attaches_it() {
    let (app, dir, core, _) = app_with("ads-mount", &json!({}), &["main"]);
    let (label, events) = mount_on(&core, &app, "main", "e1");
    assert_eq!(label, "owad-1");
    assert!(
        webview_of(&core, &label).is_some(),
        "the guest webview exists"
    );
    assert_eq!(
        core.ads.mount_of("main", "e1").map(|m| m.guest),
        Some(label.clone())
    );
    assert_eq!(names(&events), ["did-attach"]);
    let created = of_kind(&core, &label, "created");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["embedder"], "main");
    assert_eq!(created[0]["bounds"], json!([10.0, 20.0, 300.0, 250.0]));
    let muted = trace(&core)
        .into_iter()
        .filter(|e| e["via"] == "set-muted" && e["label"] == label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(muted.len(), 1, "muted at creation");
    assert_eq!(muted[0]["muted"], true);
    // A second mount of the same element replaces the guest.
    let (again, _) = mount_on(&core, &app, "main", "e1");
    assert_eq!(again, "owad-2");
    assert!(!guest_exists(&core, &label));
    let _ = std::fs::remove_dir_all(&dir);
}

/// §7.3: a forged `__host:gesture` opens nothing; native activation opens
/// once per activation, and the guest and the element hear `ad-clicked`.
#[test]
fn a_forged_gesture_never_opens_a_link() {
    let (app, dir, core, _) = app_with("ads-forged-gesture", &json!({}), &["main"]);
    let (label, events) = mount_on(&core, &app, "main", "e1");
    guest_event(&core, &label, None, "__host:gesture", None).unwrap();
    open_external(&core, &label, &link(), None, "popup");
    assert_eq!(of_kind(&core, &label, "open-refused").len(), 1);
    assert!(of_kind(&core, &label, "open-external").is_empty());
    // WebView2's own flag false: refused too.
    let reports = Reports(Arc::downgrade(&core));
    reports.popup(&label, AD_LINK, false);
    reports.top_navigation(&label, AD_LINK, false, None);
    assert!(of_kind(&core, &label, "open-external").is_empty());

    arm(&core, &label);
    open_external(&core, &label, &link(), None, "popup");
    assert_eq!(of_kind(&core, &label, "open-external").len(), 1);
    open_external(&core, &label, &link(), None, "popup");
    assert_eq!(
        of_kind(&core, &label, "open-external").len(),
        1,
        "one activation, one open"
    );
    assert_eq!(
        delivered(&core, &label, "ad-clicked"),
        [json!({ "type": "ad-clicked", "data": AD_LINK })]
    );
    let clicked: Vec<Value> = events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e["name"] == "ad-clicked")
        .cloned()
        .collect();
    assert_eq!(
        clicked,
        [json!({ "name": "ad-clicked", "data": { "url": AD_LINK }, "source": "host" })]
    );
    // A native input shortly before a script navigation counts.
    reports.top_navigation(&label, AD_LINK, false, Some(100));
    assert_eq!(of_kind(&core, &label, "open-external").len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// §7.3: 30 activated opens across two guests within a minute open 20 (the
/// per-app cap).
#[test]
fn activated_opens_are_capped_across_the_app() {
    let (app, dir, core, _) = app_with("ads-open-cap", &json!({}), &["main"]);
    let (a, _) = mount_on(&core, &app, "main", "e1");
    let (b, _) = mount_on(&core, &app, "main", "e2");
    for _ in 0..15 {
        for l in [&a, &b] {
            arm(&core, l);
            open_external(&core, l, &link(), None, "popup");
        }
    }
    let opened =
        of_kind(&core, &a, "open-external").len() + of_kind(&core, &b, "open-external").len();
    let refused =
        of_kind(&core, &a, "open-refused").len() + of_kind(&core, &b, "open-refused").len();
    assert_eq!((opened, refused), (20, 10));
    let _ = std::fs::remove_dir_all(&dir);
}

/// §7.3: forged crashes recover up to `ads.maxRecoveries`, send no
/// analytics within 10 s of the last load, and then destroy the guest.
#[test]
fn forged_crashes_end_at_max_recoveries() {
    let (app, dir, core, capture) = app_with(
        "ads-forged-crash",
        &json!({ "maxRecoveries": 2 }),
        &["main"],
    );
    crate::host::lifecycle::on_ready(&core);
    let (label, events) = mount_on(&core, &app, "main", "e1");
    for _ in 0..3 {
        guest_event(&core, &label, None, "__host:crash", None).unwrap();
    }
    assert_eq!(count(&events, "render-process-gone"), 3);
    assert_eq!(count(&events, "destroyed"), 1);
    assert_eq!(names(&events).last().map(String::as_str), Some("destroyed"));
    assert!(!guest_exists(&core, &label));
    let gone: Vec<Value> = events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e["name"] == "render-process-gone")
        .map(|e| e["data"]["details"]["reason"].clone())
        .collect();
    assert_eq!(gone, [json!("killed"), json!("killed"), json!("killed")]);
    // The guest is gone: its next event finds nothing.
    assert!(guest_event(&core, &label, None, "__host:crash", None).is_err());
    std::thread::sleep(Duration::from_millis(200));
    assert!(!capture.counters().iter().any(|c| c == "owadview_crashed"));
    assert_eq!(kind_count(&capture, 400_024), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// §7.3: a flood is dropped over the limit; 10 s over it reloads the guest,
/// 10 s more closes it.
#[test]
fn a_flood_is_dropped_then_reloads_then_closes() {
    let (app, dir, core, _) = app_with(
        "ads-flood",
        &json!({
            "recreateOnReload": false,
            "guestLimits": { "eventsPerSecond": 1, "eventBurst": 2 }
        }),
        &["main"],
    );
    let (label, events) = mount_on(&core, &app, "main", "e1");
    with_guest(&core, &label, |g| g.navigated = true);
    let start = core.now();
    for _ in 0..5 {
        guest_event_at(&core, &label, None, "impression", None, start).unwrap();
    }
    assert_eq!(count(&events, "impression"), 2, "the burst, then drops");
    let mut t = start;
    while of_kind(&core, &label, "reload").is_empty() && t < start + 11_000 {
        t += 50;
        guest_event_at(&core, &label, None, "impression", None, t).unwrap();
    }
    assert!(
        t >= start + 10_000,
        "reloaded only after 10 s over the limit"
    );
    assert_eq!(of_kind(&core, &label, "reload").len(), 1);
    assert!(guest_exists(&core, &label));
    let reloaded = t;
    while guest_exists(&core, &label) && t < reloaded + 11_000 {
        t += 50;
        let _ = guest_event_at(&core, &label, None, "impression", None, t);
    }
    assert!(!guest_exists(&core, &label), "closed after a second period");
    assert!(t >= reloaded + 10_000);
    assert_eq!(names(&events).last().map(String::as_str), Some("destroyed"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn oversized_data_reaches_the_element_truncated() {
    let (app, dir, core, _) = app_with("ads-oversized", &json!({}), &["main"]);
    let (label, events) = mount_on(&core, &app, "main", "e1");
    guest_event(
        &core,
        &label,
        None,
        "impression",
        Some(json!("x".repeat(20_000))),
    )
    .unwrap();
    guest_event(&core, &label, None, "impression", Some(json!({ "a": 1 }))).unwrap();
    let got = events.lock().unwrap().clone();
    let impressions: Vec<&Value> = got.iter().filter(|e| e["name"] == "impression").collect();
    assert_eq!(impressions.len(), 2);
    assert_eq!(
        impressions[0]["data"],
        json!({ "truncated": true, "bytes": 20_002 })
    );
    assert_eq!(impressions[0]["source"], "guest");
    assert_eq!(impressions[1]["data"], json!({ "a": 1 }));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn internal_and_reserved_names_never_reach_the_element() {
    let (app, dir, core, _) = app_with("ads-reserved", &json!({}), &["main"]);
    let (label, events) = mount_on(&core, &app, "main", "e1");
    for name in [
        "__host:gesture",
        "__host:focus",
        "__host:unknown",
        "__host:applySetting",
    ] {
        guest_event(&core, &label, None, name, None).unwrap();
    }
    assert!(guest_event(&core, &label, None, "", None).is_err());
    // A claimed slot id is only logged: the caller label decides.
    guest_event(&core, &label, Some("owad-99"), "impression", None).unwrap();
    assert_eq!(names(&events), ["did-attach", "impression"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// §4.4.9: the local-frame invariant table.
#[test]
fn guest_navigation_refuses_local_frames() {
    let (app, dir, core, _) = app_with(
        "ads-local-frames",
        &json!({ "allowedEmbedderOrigins": ["http://127.0.0.1:1430"] }),
        &["main"],
    );
    let (label, _) = mount_on(&core, &app, "main", "e1");
    let app_origin = core.identity.app_origin.to_string();
    let off_overwolf = !cfg!(windows);
    let table: [(&str, bool); 11] = [
        (ADVIEW_URL, true),
        ("https://cdn.overwolf.com/ads/frame.html", true),
        ("about:blank", true),
        ("https://securepubads.example/frame", off_overwolf),
        ("tauri://localhost/index.html", false),
        ("http://tauri.localhost/index.html", false),
        ("http://localhost:1420/", false),
        ("http://app.localhost/", false),
        ("http://127.0.0.1:1430/x", false),
        ("file:///etc/hosts", false),
        ("ipc://localhost/cmd", false),
    ];
    for (url, allowed) in table {
        let url = Url::parse(url).unwrap();
        assert_eq!(guest_navigation(&core, &label, &url), allowed, "{url}");
    }
    assert!(
        !guest_navigation(&core, &label, &Url::parse(&app_origin).unwrap()),
        "{app_origin}"
    );
    assert!(
        !guest_navigation(&core, "owad-999", &Url::parse(ADVIEW_URL).unwrap()),
        "an unknown guest"
    );
    assert!(!of_kind(&core, &label, "navigation-refused").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// §7.3 isolation: an element belongs to the webview that mounted it, and
/// a guest's events reach only its own element.
#[test]
fn elements_and_events_stay_with_their_embedder() {
    let (app, dir, core, _) = app_with("ads-isolation", &json!({}), &["main", "second"]);
    let (a, events_a) = mount_on(&core, &app, "main", "e1");
    let second = app.get_webview("second").unwrap();
    assert!(core.ads.mount_of("second", "e1").is_none());
    let update: AdviewUpdate =
        serde_json::from_value(json!({ "elementId": "e1", "visible": false })).unwrap();
    assert!(update_guest(&core, &second, update).is_err());
    assert!(command(&core, "second", "e1", AdviewCommandName::Reload, &[]).is_err());
    unmount(&core, "second", "e1");
    assert!(guest_exists(&core, &a), "another webview cannot unmount it");

    let (b, events_b) = mount_on(&core, &app, "second", "e1");
    assert_ne!(a, b);
    guest_event(&core, &a, None, "impression", Some(json!("a"))).unwrap();
    guest_event(&core, &b, None, "impression", Some(json!("b"))).unwrap();
    let data = |events: &Events| -> Vec<Value> {
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["name"] == "impression")
            .map(|e| e["data"].clone())
            .collect()
    };
    assert_eq!(data(&events_a), [json!("a")]);
    assert_eq!(data(&events_b), [json!("b")]);
    unmount(&core, "main", "e1");
    assert!(!guest_exists(&core, &a) && guest_exists(&core, &b));
    assert_eq!(
        count(&events_a, "destroyed"),
        0,
        "the element's runtime reports its own unmount"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn update_guest(core: &Mock, embedder: &Webview<MockRuntime>, request: AdviewUpdate) -> Result<()> {
    update(core, embedder, request)
}

#[test]
fn e_hashes_and_consent_reach_every_existing_guest() {
    let (app, dir, core, _) = app_with("ads-ehashes", &json!({}), &["main"]);
    let hashes = EmailHashes {
        sha1: Some("a".into()),
        md5: Some("b".into()),
        sha256: Some("c".into()),
    };
    core.ads.set_email_hashes(Some(hashes.clone()));
    let (a, _) = mount_on(&core, &app, "main", "e1");
    let (b, _) = mount_on(&core, &app, "main", "e2");
    assert!(
        delivered(&core, &a, "eHashes").is_empty(),
        "nothing was waiting"
    );
    core.ads.set_email_hashes(Some(hashes));
    for l in [&a, &b] {
        assert_eq!(
            delivered(&core, l, "eHashes"),
            [json!({ "type": "eHashes", "data": { "sha1": "a", "md5": "b", "sha256": "c" } })]
        );
    }
    deliver_to_all(&core, "consent", Some(&json!("TCF-STRING")));
    assert_eq!(delivered(&core, &b, "consent").len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// W0c ruling 2: a close the app prevents leaves the guests visible.
#[test]
fn a_prevented_close_keeps_guests_visible() {
    let (app, dir, core, _) = app_with("ads-prevented-close", &json!({}), &["main"]);
    let (label, _) = mount_on(&core, &app, "main", "e1");
    clear_trace(&core);
    close_requested(&core, "main");
    if CLOSE_HIDE == CloseHide::AtCloseRequested {
        assert!(wait_until(Duration::from_secs(2), || calls(
            &core,
            &label,
            "setVisibility"
        )
        .first()
            == Some(&json!("hidden"))));
    }
    std::thread::sleep(Duration::from_millis(CLOSE_GRACE_MS + 300));
    assert!(guest_exists(&core, &label));
    assert_eq!(with_guest(&core, &label, |g| g.sent_visible), Some(true));
    let sent = calls(&core, &label, "setVisibility");
    if CLOSE_HIDE == CloseHide::AtCloseRequested {
        assert_eq!(sent, [json!("hidden"), json!("visible")]);
    } else {
        assert!(sent.is_empty(), "{sent:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// W0c ruling 2: a close that goes through hides the guests' documents,
/// then closes the guests (no `destroyed`: the page is going away).
#[test]
fn a_closed_window_hides_its_guests_then_closes_them() {
    let (app, dir, core, _) = app_with("ads-close", &json!({}), &["main"]);
    let (label, events) = mount_on(&core, &app, "main", "e1");
    clear_trace(&core);
    close_requested(&core, "main");
    if CLOSE_HIDE == CloseHide::AtCloseRequested {
        assert!(wait_until(Duration::from_secs(2), || !calls(
            &core,
            &label,
            "setVisibility"
        )
        .is_empty()));
    }
    window_event(&core, "main", &WindowEvent::Destroyed);
    assert!(!guest_exists(&core, &label));
    let hidden = position(&core, |e| {
        e["via"] == "guest-call" && e["function"] == "setVisibility" && e["args"] == "hidden"
    })
    .expect("the document was told it is hidden");
    let closed = position(&core, |e| e["kind"] == "closed").expect("closed");
    assert!(hidden < closed);
    assert_eq!(count(&events, "destroyed"), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// D.5: a window hidden to the tray sends `window-hidden`, then the
/// visibility; shown again, only the visibility.
#[test]
fn close_to_tray_sends_window_hidden() {
    let (app, dir, core, _) = app_with("ads-tray", &json!({}), &["main"]);
    let (label, _) = mount_on(&core, &app, "main", "e1");
    clear_trace(&core);
    on_poll(&core, &[observation("main", false, false)], core.now());
    assert_eq!(delivered(&core, &label, "window-hidden").len(), 1);
    let message = position(&core, |e| e["message"]["type"] == "window-hidden").unwrap();
    let hidden = position(&core, |e| {
        e["function"] == "setVisibility" && e["args"] == "hidden"
    })
    .expect("visibility hidden");
    assert!(message < hidden);
    clear_trace(&core);
    on_poll(&core, &[observation("main", true, false)], core.now());
    assert!(delivered(&core, &label, "window-hidden").is_empty());
    assert_eq!(with_guest(&core, &label, |g| g.sent_visible), Some(true));
    let _ = std::fs::remove_dir_all(&dir);
}

/// D.5: a minimize sends the visibility, `window-minimized`, then (not on
/// Windows) `window-hidden`; Windows also hides the webview natively.
#[test]
fn a_minimize_sends_window_minimized() {
    let (app, dir, core, _) = app_with("ads-minimize", &json!({}), &["main"]);
    let (label, _) = mount_on(&core, &app, "main", "e1");
    clear_trace(&core);
    on_poll(&core, &[observation("main", false, true)], core.now());
    let hidden = position(&core, |e| {
        e["function"] == "setVisibility" && e["args"] == "hidden"
    })
    .expect("visibility hidden");
    let minimized = position(&core, |e| e["message"]["type"] == WINDOW_MINIMIZED).unwrap();
    assert!(hidden < minimized);
    let window_hidden = delivered(&core, &label, "window-hidden");
    assert_eq!(
        window_hidden.len(),
        usize::from(MINIMIZE_SENDS_WINDOW_HIDDEN)
    );
    if MINIMIZE_SENDS_WINDOW_HIDDEN {
        let after = position(&core, |e| e["message"]["type"] == "window-hidden").unwrap();
        assert!(minimized < after);
    }
    let native = of_kind(&core, &label, "native-visibility");
    assert_eq!(native.len(), usize::from(MINIMIZE_HIDES_NATIVELY));
    let _ = std::fs::remove_dir_all(&dir);
}

/// §4.4.6.3: events of a retired native instance are dropped.
#[test]
fn a_retired_generation_is_not_heard() {
    let (app, dir, core, _) = app_with("ads-generation", &json!({}), &["main"]);
    let (label, events) = mount_on(&core, &app, "main", "e1");
    let generation = with_guest(&core, &label, |g| g.generations.retire()).unwrap();
    guest_event(&core, &label, None, "impression", None).unwrap();
    guest_event(&core, &label, None, "__host:crash", None).unwrap();
    assert_eq!(names(&events), ["did-attach"]);
    assert!(with_guest(&core, &label, |g| g.generations.go_live(generation)).unwrap());
    guest_event(&core, &label, None, "impression", None).unwrap();
    assert_eq!(names(&events), ["did-attach", "impression"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// §4.4.6: a reload recreates the guest on macOS (same label, no
/// `did-attach`, no 400025) within the rate guard; other reloads are in
/// place.
#[test]
fn reloads_recreate_within_the_guard_and_never_reattach() {
    let (app, dir, core, capture) = app_with("ads-recreate", &json!({}), &["main"]);
    crate::host::lifecycle::on_ready(&core);
    let (label, events) = mount_on(&core, &app, "main", "e1");
    assert!(wait_until(Duration::from_secs(6), || !of_kind(
        &core,
        &label,
        "first-navigation"
    )
    .is_empty()));
    assert!(wait_until(Duration::from_secs(5), || kind_count(
        &capture, 400_025
    ) == 1));
    for _ in 0..5 {
        guest_event(&core, &label, None, "__host:reload", None).unwrap();
        std::thread::sleep(Duration::from_millis(RELOAD_DELAY_MS + 80));
        assert!(wait_until(Duration::from_secs(2), || with_guest(
            &core,
            &label,
            |g| g.generations.admits()
        ) == Some(true)));
    }
    let reloads = of_kind(&core, &label, "reload");
    assert_eq!(reloads.len(), 5, "{reloads:?}");
    let recreated: Vec<bool> = reloads
        .iter()
        .map(|r| r["recreate"].as_bool().unwrap())
        .collect();
    let mut expected = vec![false; 5];
    expected[0] = RECREATE_PLATFORM;
    assert_eq!(recreated, expected, "one recreate per 30 s");
    if RECREATE_PLATFORM {
        assert!(wait_until(Duration::from_secs(2), || !of_kind(
            &core,
            &label,
            "recreated"
        )
        .is_empty()));
    }
    assert!(webview_of(&core, &label).is_some(), "the label lives on");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(count(&events, "did-attach"), 1);
    assert_eq!(count(&events, "destroyed"), 0);
    assert_eq!(kind_count(&capture, 400_025), 1, "no 400025 on a recreate");
    let _ = std::fs::remove_dir_all(&dir);
}

/// D.5: a reload the page asks for while hidden waits for the hold or for
/// the guest to be shown again; it is never dropped.
#[test]
fn a_hidden_reload_waits_for_visibility() {
    let (app, dir, core, _) = app_with(
        "ads-hidden-reload",
        &json!({ "recreateOnReload": false }),
        &["main"],
    );
    let (label, _) = mount_on(&core, &app, "main", "e1");
    with_guest(&core, &label, |g| g.navigated = true);
    let update: AdviewUpdate =
        serde_json::from_value(json!({ "elementId": "e1", "visible": false })).unwrap();
    update_guest(&core, &app.get_webview("main").unwrap(), update).unwrap();
    assert_eq!(
        calls(&core, &label, "setVisibility").last(),
        Some(&json!("hidden"))
    );
    guest_event(&core, &label, None, "__host:reload", None).unwrap();
    std::thread::sleep(Duration::from_millis(RELOAD_DELAY_MS + 200));
    assert!(
        of_kind(&core, &label, "reload").is_empty(),
        "held while hidden"
    );
    assert_eq!(with_guest(&core, &label, |g| g.reload_held), Some(true));
    let update: AdviewUpdate =
        serde_json::from_value(json!({ "elementId": "e1", "visible": true })).unwrap();
    update_guest(&core, &app.get_webview("main").unwrap(), update).unwrap();
    tick(&core, core.now());
    assert_eq!(of_kind(&core, &label, "reload").len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// B.3.4: a performance guest lets input through until its first
/// `performance_ad_loaded`, which reaches the element after the switch.
#[test]
fn a_performance_guest_turns_modal_at_its_first_load_event() {
    let (app, dir, core, _) = app_with("ads-performance", &json!({}), &["main"]);
    let webview = app.get_webview("main").unwrap();
    let (channel, events) = collector();
    let label =
        tauri::async_runtime::block_on(mount(&core, &webview, request("p1", true), channel))
            .unwrap();
    assert_eq!(with_guest(&core, &label, |g| g.passthrough), Some(true));
    assert_eq!(of_kind(&core, &label, "zorder").len(), 1, "raised at mount");
    guest_event(&core, &label, None, crate::ads::MODAL_EVENT, None).unwrap();
    assert_eq!(with_guest(&core, &label, |g| g.passthrough), Some(false));
    let switched = position(&core, |e| e["kind"] == "passthrough" && e["on"] == false).unwrap();
    assert!(switched > 0);
    assert_eq!(count(&events, crate::ads::MODAL_EVENT), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A new document in the embedder, or the embedder gone, closes its
/// guests without `destroyed`.
#[test]
fn guests_follow_their_embedder_document() {
    let (app, dir, core, _) = app_with("ads-embedder-load", &json!({}), &["main"]);
    let (label, events) = mount_on(&core, &app, "main", "e1");
    let webview = app.get_webview("main").unwrap();
    let url = Url::parse("tauri://localhost/other.html").unwrap();
    page_load(&core, &webview, PageLoadEvent::Finished, &url);
    assert!(guest_exists(&core, &label), "only a started load counts");
    page_load(&core, &webview, PageLoadEvent::Started, &url);
    assert!(!guest_exists(&core, &label));
    assert_eq!(count(&events, "destroyed"), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// D.6.5, D.5: the first navigation waits for the consent gate; a finished
/// load mutes again, re-sends the visibility and focus, and
/// `did-finish-load` follows `dom-ready`.
#[test]
fn the_first_load_follows_the_consent_gate() {
    let (app, dir, core, _) = app_with("ads-first-load", &json!({}), &["main"]);
    let (label, events) = mount_on(&core, &app, "main", "e1");
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        of_kind(&core, &label, "first-navigation").is_empty(),
        "the gate is closed"
    );
    crate::host::lifecycle::on_ready(&core);
    assert!(wait_until(Duration::from_secs(6), || !of_kind(
        &core,
        &label,
        "first-navigation"
    )
    .is_empty()));
    let shaped: Vec<Value> = trace(&core)
        .into_iter()
        .filter(|e| e["url"] == ADVIEW_URL && e["label"] == label.as_str())
        .collect();
    assert_eq!(shaped.len(), 1);
    assert!(webview_of(&core, &label).is_some());
    let url = Url::parse(ADVIEW_URL).unwrap();
    clear_trace(&core);
    guest_page_load(&core, &label, PageLoadEvent::Finished, &url);
    assert_eq!(count(&events, "did-finish-load"), 0, "waits for dom-ready");
    guest_event(&core, &label, None, "__host:domReady", None).unwrap();
    let tail: Vec<String> = names(&events).into_iter().skip(1).collect();
    assert_eq!(tail, ["dom-ready", "did-finish-load"]);
    assert_eq!(calls(&core, &label, "setVisibility"), [json!("visible")]);
    assert_eq!(calls(&core, &label, "setEmbedderFocus").len(), 1);
    assert!(
        trace(&core)
            .iter()
            .any(|e| e["via"] == "set-muted" && e["cause"] == "load")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `render-process-gone` reasons an element heard, in order.
#[cfg(target_os = "macos")]
fn gone_reasons(events: &Events) -> Vec<Value> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e["name"] == "render-process-gone")
        .map(|e| e["data"]["details"]["reason"].clone())
        .collect()
}

/// W0c ruling 1, §4.4.7: on macOS the app's terminate hook recovers an ad
/// guest as after any crash: `render-process-gone` ("crashed"), a recreate
/// on the §4.4.6 path under the same label, and after
/// `ads.maxRecoveries` the guest closes.
#[cfg(target_os = "macos")]
#[test]
fn the_terminate_hook_recovers_a_guest() {
    use crate::platform::terminate::handle_web_content_process_terminate;
    let (app, dir, core, _) = app_with("ads-terminate", &json!({ "maxRecoveries": 1 }), &["main"]);
    let (label, events) = mount_on(&core, &app, "main", "e1");
    with_guest(&core, &label, |g| g.navigated = true);
    handle_web_content_process_terminate(&app.get_webview(&label).unwrap());
    assert_eq!(gone_reasons(&events), [json!("crashed")]);
    let reloads = of_kind(&core, &label, "reload");
    assert_eq!(reloads.len(), 1, "{reloads:?}");
    assert_eq!(reloads[0]["recreate"], true, "recovered by a recreate");
    assert!(wait_until(Duration::from_secs(2), || !of_kind(
        &core,
        &label,
        "recreated"
    )
    .is_empty()));
    assert!(wait_until(Duration::from_secs(2), || with_guest(
        &core,
        &label,
        |g| g.generations.admits()
    ) == Some(true)));
    assert!(guest_exists(&core, &label));
    assert_eq!(
        count(&events, "did-attach"),
        1,
        "a recovery never reattaches"
    );
    // The recreated guest's process ends too: over ads.maxRecoveries.
    handle_web_content_process_terminate(&app.get_webview(&label).unwrap());
    assert_eq!(gone_reasons(&events), [json!("crashed"), json!("crashed")]);
    assert_eq!(count(&events, "destroyed"), 1);
    assert!(!guest_exists(&core, &label));
    assert!(
        app.get_webview("main").is_some(),
        "the embedder is never reloaded for a guest"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// After `RunEvent::Exit` a guest's ended process is neither reported nor
/// recovered, and a reload asked for then does nothing.
#[test]
fn no_recovery_after_exit() {
    let (app, dir, core, _) = app_with("ads-after-exit", &json!({}), &["main"]);
    let (label, events) = mount_on(&core, &app, "main", "e1");
    with_guest(&core, &label, |g| g.navigated = true);
    crate::host::lifecycle::on_exit(&core);
    guest_crashed(&core, &label, GoneReason::Crashed, 0);
    reload_guest(&core, &label);
    assert_eq!(count(&events, "render-process-gone"), 0);
    assert!(of_kind(&core, &label, "reload").is_empty());
    #[cfg(target_os = "macos")]
    crate::platform::terminate::handle_web_content_process_terminate(
        &app.get_webview(&label).unwrap(),
    );
    assert_eq!(count(&events, "render-process-gone"), 0);
    assert!(of_kind(&core, &label, "reload").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
