//! `<owadview>` through the compiled ACL on the mock runtime (DESIGN §3.5,
//! §4.4, §4.5, §7.2): elements belong to the webview that mounted them,
//! Channel events reach only the mounting element, a child webview can
//! embed ads, the guests' first navigation waits for consent at most 3 s,
//! and a capability that names a window (not a webview) also matches the
//! ad guests in that window, where the plugin's gate refuses them.
//!
//! Fixture windows: `main`, `settings/panel` and `overlay:hud` (Tauri's
//! label charset), the child webview `embedded` in `overlay:hud`.
//! `capabilities/default.json` grants `overwolf:default` to the webview
//! `main`, `labels.json` to the webviews `settings/panel` and `embedded`,
//! and `windows-scoped.json` to every webview of the window `overlay:hud`.

#![allow(clippy::unwrap_used, reason = "a test fails on any unexpected error")]

mod common;

use serde_json::json;

use common::{code, invoke, origin};

/// An element nobody mounted is `not-found` for every element command,
/// before and without `RunEvent::Ready`.
#[test]
fn unmounted_elements_are_not_found() {
    let app = common::app("adview-unmounted", &["main"]);
    for (cmd, body) in [
        (
            "adview_update",
            json!({ "request": { "elementId": "e9", "visible": false } }),
        ),
        ("adview_unmount", json!({ "elementId": "e9" })),
        (
            "adview_command",
            json!({ "elementId": "e9", "command": "reload" }),
        ),
    ] {
        let r = invoke(&app, "main", origin(), cmd, body);
        assert_eq!(code(&r), Some("not-found"), "{cmd}: {r:?}");
    }
}

/// Linux has no ad guests: the mount answers `unsupported` once the
/// plugin started.
#[cfg(not(any(windows, target_os = "macos")))]
#[test]
fn linux_mounts_answer_unsupported() {
    use common::{Capture, mount_body};
    use tauri::WebviewUrl;
    use tauri_plugin_overwolf::Builder;
    let (context, _dir) = common::context("adview-linux", &json!({}), &[]);
    let app = common::build(
        context,
        Builder::new().analytics_transport(Capture::hanging_eu_only()),
        None,
    );
    common::window(&app, "main", WebviewUrl::default());
    let result = common::run(app, |handle| {
        invoke(
            handle,
            "main",
            origin(),
            "adview_mount",
            mount_body("e1", 1),
        )
    });
    assert_eq!(code(&result), Some("unsupported"), "{result:?}");
}

/// Mounts through `adview_mount` on Tauri's mock runtime. Ignored until
/// CR-1 lands: with the public `Builder` the plugin reads the monitors for
/// the guest's `systemInfo`, and the mock runtime's `available_monitors()`
/// is `unimplemented!()` (the plugin's own tests turn `os_queries` off,
/// which only crate-internal tests can). Verified against a copy of the
/// plugin with CR-1 applied; see the lane report.
#[cfg(any(windows, target_os = "macos"))]
mod guests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};
    use tauri::test::MockRuntime;
    use tauri::{AppHandle, WebviewUrl};
    use tauri_plugin_overwolf::Builder;
    use tauri_plugin_overwolf::ads::CONSENT_WAIT_MS;

    use super::common::{
        self, ADVIEW_PAGE, Capture, Channels, Outcome, code, invoke, mount_body, origin, outcome,
        wait_until,
    };

    /// The fixture windows (`main`, `settings/panel`, `overlay:hud` with the
    /// child webview `embedded`), `requestShaping` off so a guest's first
    /// navigation is a plain `navigate` the mock runtime records.
    fn fixture(name: &str, channels: &Arc<Channels>) -> tauri::App<MockRuntime> {
        let (context, _dir) = common::context(
            name,
            &json!({
                "ads": { "requestShaping": false },
                "analytics": { "muidStrategy": "per-install" }
            }),
            &[],
        );
        let app = common::build(
            context,
            Builder::new().analytics_transport(Capture::hanging_eu_only()),
            Some(channels),
        );
        common::window(&app, "main", WebviewUrl::default());
        common::window(
            &app,
            "settings/panel",
            WebviewUrl::App("settings.html".into()),
        );
        common::window(&app, "overlay:hud", WebviewUrl::App("hud.html".into()));
        common::child_webview(
            &app,
            "overlay:hud",
            "embedded",
            WebviewUrl::App("hud.html".into()),
        );
        app
    }

    /// Mounts `element_id` from webview `embedder` with the page's Channel
    /// `channel`; returns the guest label (or the failure, which no guest
    /// label matches).
    fn mount(
        handle: &AppHandle<MockRuntime>,
        embedder: &str,
        element_id: &str,
        channel: u32,
    ) -> String {
        let r = invoke(
            handle,
            embedder,
            origin(),
            "adview_mount",
            mount_body(element_id, channel),
        );
        r.as_ref()
            .ok()
            .and_then(|v| v["guestLabel"].as_str())
            .map_or_else(
                || format!("mount from {embedder} failed: {r:?}"),
                str::to_owned,
            )
    }

    /// Whether guest `label` shows the ad document.
    fn on_ad_page(handle: &AppHandle<MockRuntime>, label: &str) -> bool {
        common::webview(handle, label)
            .and_then(|w| w.url().ok())
            .is_some_and(|u| u.as_str() == ADVIEW_PAGE)
    }

    /// What the worker saw, asserted after the app exited.
    struct Seen {
        a: String,
        b: String,
        navigated_after: Duration,
        cross: Vec<(&'static str, Result<Value, Value>)>,
        guests_after_cross: (bool, bool),
        events: Vec<(String, Result<Value, Value>)>,
        guest_calls: Vec<(String, bool, Outcome)>,
        after_unmount: (bool, bool),
    }

    /// The worker: two mounts of element `e1` (webview `main`, child webview
    /// `embedded`), webview `settings/panel` tries to drive it, each guest
    /// sends an event, the guests call app commands, `embedded` unmounts.
    fn exercise(handle: &AppHandle<MockRuntime>) -> Seen {
        let a = mount(handle, "main", "e1", 101);
        let mounted_at = Instant::now();
        let b = mount(handle, "embedded", "e1", 202);
        let cross = [
            (
                "adview_update",
                json!({ "request": { "elementId": "e1", "visible": false } }),
            ),
            (
                "adview_command",
                json!({ "elementId": "e1", "command": "reload" }),
            ),
            ("adview_unmount", json!({ "elementId": "e1" })),
        ]
        .into_iter()
        .map(|(cmd, body)| (cmd, invoke(handle, "settings/panel", origin(), cmd, body)))
        .collect();
        let guests_after_cross = (
            common::webview(handle, &a).is_some(),
            common::webview(handle, &b).is_some(),
        );
        // No startup consent window opened (`cmp-eu-only` hangs): the gate
        // stays closed and the first navigation waits for its bound.
        let limit = Duration::from_millis(CONSENT_WAIT_MS) + Duration::from_secs(5);
        assert!(
            wait_until(limit, || on_ad_page(handle, &a)),
            "{a} navigated"
        );
        let navigated_after = mounted_at.elapsed();
        assert!(
            wait_until(limit, || on_ad_page(handle, &b)),
            "{b} navigated"
        );
        let events = [(&a, "a"), (&b, "b")]
            .into_iter()
            .map(|(guest, data)| {
                let body = json!({ "slotId": guest, "name": "impression", "data": data });
                (
                    guest.clone(),
                    invoke(handle, guest, ADVIEW_PAGE, "adview_event", body),
                )
            })
            .collect();
        // A page inside a guest that reaches the app's origin (a local
        // frame, SEC-B1), and the ad page itself.
        let mut guest_calls = Vec::new();
        for guest in [&a, &b] {
            for (url, cmd, body) in [
                (origin(), "get_info", json!({})),
                (
                    origin(),
                    "adview_update",
                    json!({ "request": { "elementId": "e1" } }),
                ),
                (ADVIEW_PAGE, "get_info", json!({})),
            ] {
                let r = invoke(handle, guest, url, cmd, body);
                guest_calls.push((format!("{guest} {url} {cmd}"), url == origin(), outcome(&r)));
            }
        }
        let unmounted = invoke(
            handle,
            "embedded",
            origin(),
            "adview_unmount",
            json!({ "elementId": "e1" }),
        );
        assert_eq!(unmounted, Ok(Value::Null));
        let gone = wait_until(Duration::from_secs(5), || {
            common::webview(handle, &b).is_none()
        });
        let after_unmount = (common::webview(handle, &a).is_some(), gone);
        Seen {
            a,
            b,
            navigated_after,
            cross,
            guests_after_cross,
            events,
            guest_calls,
            after_unmount,
        }
    }

    /// DESIGN §4.5 rule 4 and §7.2: webview A's element is `not-found` for
    /// webview B (update, command, unmount); Channel events reach only the
    /// element that mounted the guest; a child webview embeds an ad; the
    /// first navigation of a guest waits for the consent gate at most
    /// `CONSENT_WAIT_MS` (D.6.5); a `windows` capability matches a guest's
    /// local frame and the gate refuses it.
    #[test]
    #[ignore = "CR-1: adview_mount needs Builder::os_queries(false) on the mock runtime"]
    fn elements_and_channel_events_stay_with_their_embedder() {
        let channels = Arc::new(Channels::default());
        let app = fixture("adview-isolation", &channels);
        let seen = common::run(app, exercise);
        assert!(
            seen.a.starts_with("owad-") && seen.b.starts_with("owad-") && seen.a != seen.b,
            "{} {}",
            seen.a,
            seen.b
        );
        for (cmd, r) in &seen.cross {
            assert_eq!(
                code(r),
                Some("not-found"),
                "{cmd} from settings/panel: {r:?}"
            );
        }
        assert_eq!(
            seen.guests_after_cross,
            (true, true),
            "another webview cannot unmount"
        );
        let bound = Duration::from_millis(CONSENT_WAIT_MS);
        assert!(
            seen.navigated_after + Duration::from_millis(250) >= bound
                && seen.navigated_after < bound + Duration::from_secs(3),
            "first navigation at the consent bound: {:?}",
            seen.navigated_after
        );
        for (guest, r) in &seen.events {
            assert_eq!(r, &Ok(Value::Null), "adview_event from {guest}");
        }
        let impressions = |webview: &str, id: u32| -> Vec<Value> {
            channels
                .messages(webview, id)
                .into_iter()
                .filter(|m| m["name"] == "impression")
                .map(|m| m["data"].clone())
                .collect()
        };
        assert_eq!(impressions("main", 101), [json!("a")]);
        assert_eq!(impressions("embedded", 202), [json!("b")]);
        for (webview, id) in [("main", 101), ("embedded", 202)] {
            assert_eq!(
                channels.names(webview, id).first().map(String::as_str),
                Some("did-attach"),
                "{webview}"
            );
        }
        assert_eq!(
            channels.receivers(),
            [("embedded".to_owned(), 202), ("main".to_owned(), 101)],
            "no other webview or channel heard an element event"
        );
        for (call, local, got) in &seen.guest_calls {
            // `windows-scoped.json` names the window `overlay:hud`, so it
            // matches a local frame of B's guest too, and the plugin's gate
            // refuses it. `default.json` names the webview `main`, which no
            // guest is; the guest capability grants `adview_event` only.
            let want = if *local && call.starts_with(&seen.b) {
                Outcome::Forbidden
            } else {
                Outcome::Acl
            };
            assert_eq!(*got, want, "{call}");
        }
        assert_eq!(
            seen.after_unmount,
            (true, true),
            "B's unmount closed B's guest only"
        );
        assert!(
            !channels.names("main", 101).iter().any(|n| n == "destroyed"),
            "the element's runtime reports its own unmount"
        );
    }
}
