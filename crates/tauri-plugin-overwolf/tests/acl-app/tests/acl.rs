//! Mock-runtime tests of the plugin's permission sets, runtime
//! capabilities and caller gate (DESIGN §3.5, §3.6, §4.5, §7.2).
//!
//! The ACL comes from the plugin's permission sets, the runtime
//! capabilities the plugin adds for its own webviews, and `capabilities/`
//! here. Every command is invoked from several webviews; a command is
//! either refused by Tauri's ACL, refused by the plugin's gate
//! (`forbidden`), or reaches its handler.
//!
//! Fixture capabilities: `default.json` (webview `main`), `labels.json`
//! (webviews `settings/panel` and `embedded`), `windows-scoped.json`
//! (window `overlay:hud`), `opt-in.json` (webview `admin`, every opt-in
//! set), one capability per opt-in set (webviews `machine-id`,
//! `email-hashes`, `analytics`, `updater`), `allowed-embedder.json` (the
//! remote page `http://localhost:9527` in webview `localhost-ui`) and
//! `misgranted.json` (plugin webview labels).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test fails on any unexpected error"
)]

mod common;

use serde_json::{Value, json};
use tauri::{WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_overwolf::{Builder, COMMANDS, MachineIds, OverwolfExt};

use common::{ADVIEW_PAGE, CMP_PAGE, Outcome, app, code, invoke, origin, outcome, probe_body};

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
/// The remote app page `allowed-embedder.json` names.
const LOCALHOST_UI: &str = "http://localhost:9527/";

/// The commands webview `label` reaches from the app origin; any other
/// outcome than an ACL refusal or the handler fails the test.
fn granted(app: &tauri::App<tauri::test::MockRuntime>, label: &str) -> Vec<&'static str> {
    let mut granted = Vec::new();
    for cmd in COMMANDS {
        let result = invoke(app, label, origin(), cmd, probe_body(cmd));
        match outcome(&result) {
            Outcome::Acl => {}
            Outcome::Reached => granted.push(*cmd),
            other => panic!("{label} {cmd}: {other:?} {result:?}"),
        }
    }
    granted
}

/// Valid bodies of the element commands, so a refusal comes from the gate
/// and not from argument parsing. The mount's gate runs before it waits
/// for `RunEvent::Ready`.
fn element_body(cmd: &str) -> Value {
    match cmd {
        "adview_mount" => common::mount_body("e1", 1),
        "adview_update" => json!({ "request": { "elementId": "e1" } }),
        "adview_unmount" => json!({ "elementId": "e1" }),
        "adview_command" => json!({ "elementId": "e1", "command": "reload" }),
        _ => probe_body(cmd),
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
    assert_eq!(granted(&app, "main"), DEFAULT);
}

/// Labels with `/` and `:` (Tauri's label charset) work in capabilities,
/// by webview label (`labels.json`) and by window label
/// (`windows-scoped.json`); an unnamed webview gets nothing.
#[test]
fn labels_with_slashes_and_colons_get_overwolf_default() {
    let app = app("labels", &["settings/panel", "overlay:hud", "other"]);
    assert_eq!(granted(&app, "settings/panel"), DEFAULT);
    assert_eq!(granted(&app, "overlay:hud"), DEFAULT);
    assert_eq!(granted(&app, "other"), Vec::<&str>::new());
}

/// Each opt-in set grants exactly its commands (DESIGN §3.6, R7), and only
/// that set grants them.
#[test]
fn opt_in_sets_grant_exactly_their_commands() {
    let sets: [(&str, &[&str]); 4] = [
        ("machine-id", &["get_machine_ids"]),
        (
            "email-hashes",
            &[
                "generate_user_email_hashes",
                "set_user_email_hashes",
                "clear_user_email_hashes",
            ],
        ),
        (
            "analytics",
            &[
                "set_external_payment_user_id",
                "set_analytics_user_enabled",
                "set_anonymous_analytics_preference",
            ],
        ),
        (
            "updater",
            &[
                "updater_check",
                "updater_download",
                "updater_install",
                "updater_download_and_install",
            ],
        ),
    ];
    let labels: Vec<&str> = sets.iter().map(|(l, _)| *l).collect();
    let app = app("opt-in-sets", &labels);
    for (label, commands) in sets {
        assert_eq!(granted(&app, label), commands, "{label}");
    }
    // Together with `overwolf:default` (`opt-in.json`): every app command.
    let all = common::app("opt-in-all", &["admin"]);
    let admin = granted(&all, "admin");
    let app_commands: Vec<&str> = COMMANDS
        .iter()
        .copied()
        .filter(|c| !GUEST_ONLY.contains(c))
        .collect();
    assert_eq!(admin, app_commands);
}

/// `overwolf:machine-id`: `getMachineIds()` answers both ids, which
/// `getInfo()` never carries (R7).
#[test]
fn machine_ids_need_their_own_set() {
    let app = app("machine-ids", &["machine-id", "main"]);
    let ids = invoke(&app, "machine-id", origin(), "get_machine_ids", json!({})).unwrap();
    let ow = app.overwolf();
    // `muid` is `muidV2` when present (ow-electron's `app.overwolf.muid`);
    // the first-generation id differs from it on Windows when the shared
    // registry values do.
    let want = MachineIds::new(ow.muid(), ow.muid_v2());
    assert_eq!(ids, serde_json::to_value(&want).unwrap());
    assert_eq!(ids["muidV2"], ow.muid_v2());
    assert!(!ids["muid"].as_str().unwrap().is_empty());
    let refused = invoke(&app, "main", origin(), "get_machine_ids", json!({}));
    assert_eq!(outcome(&refused), Outcome::Acl, "{refused:?}");
    let info = invoke(&app, "main", origin(), "get_info", json!({})).unwrap();
    assert_eq!(info["name"], "ACL Fixture");
    assert_eq!(info["uid"].as_str().unwrap().len(), 40);
    assert!(info.get("muid").is_none() && info.get("muidV2").is_none());
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

/// A guest is a child webview of the app window that hosts it. A
/// capability naming the webview `main` never matches it: from the ad page
/// it reaches `adview_event` only, from a local frame nothing. A capability
/// naming the window (`windows-scoped.json`, window `overlay:hud`) matches
/// every webview of that window, the guest's local frames included; the
/// plugin's gate refuses those (DESIGN §4.5 rule 2).
#[cfg(any(windows, target_os = "macos"))]
#[test]
fn window_capabilities_also_match_guests_and_the_gate_refuses_them() {
    let app = app("windows-vs-webviews", &["main", "overlay:hud"]);
    let ad = WebviewUrl::External(ADVIEW_PAGE.parse().unwrap());
    common::child_webview(&app, "main", "owad-7", ad.clone());
    common::child_webview(&app, "overlay:hud", "owad-8", ad);
    common::child_webview(
        &app,
        "overlay:hud",
        "embedded",
        WebviewUrl::App("hud.html".into()),
    );
    for cmd in COMMANDS {
        let from_page = invoke(&app, "owad-7", ADVIEW_PAGE, cmd, probe_body(cmd));
        let want = if *cmd == "adview_event" {
            Outcome::Reached
        } else {
            Outcome::Acl
        };
        assert_eq!(
            outcome(&from_page),
            want,
            "owad-7 page {cmd}: {from_page:?}"
        );
        let local_frame = invoke(&app, "owad-7", origin(), cmd, element_body(cmd));
        assert_eq!(
            outcome(&local_frame),
            Outcome::Acl,
            "owad-7 local {cmd}: {local_frame:?}"
        );
    }
    for cmd in DEFAULT {
        let local_frame = invoke(&app, "owad-8", origin(), cmd, element_body(cmd));
        assert_eq!(
            outcome(&local_frame),
            Outcome::Forbidden,
            "owad-8 local {cmd}: {local_frame:?}"
        );
        // The window capability is local only: the ad page gets nothing.
        let from_page = invoke(&app, "owad-8", ADVIEW_PAGE, cmd, probe_body(cmd));
        assert_eq!(
            outcome(&from_page),
            Outcome::Acl,
            "owad-8 page {cmd}: {from_page:?}"
        );
    }
    // The app's own child webview in that window (the child-webview
    // embedder) gets `overwolf:default`.
    assert_eq!(granted(&app, "embedded"), DEFAULT);
}

/// Labels starting `owad-` / `ow-cmp` are the plugin's (D10): a capability
/// that names them hands them nothing, every app command (the element
/// commands with valid arguments too) is `forbidden`.
#[test]
fn reserved_labels_are_refused_by_the_gate() {
    let app = app("misgranted", &["owad-misgranted", "ow-cmp-misgranted"]);
    for label in ["owad-misgranted", "ow-cmp-misgranted"] {
        for cmd in DEFAULT {
            let r = invoke(&app, label, origin(), cmd, element_body(cmd));
            assert_eq!(outcome(&r), Outcome::Forbidden, "{label} {cmd}: {r:?}");
        }
    }
    // An app window that takes a plugin window's label is not one: it
    // cannot save consent, and the Rust API cannot rename it.
    let app = self::app("reserved-cmp", &["ow-cmp"]);
    let save = invoke(
        &app,
        "ow-cmp",
        CMP_PAGE,
        "cmp_event",
        json!({ "name": "saveConsent", "data": { "consent": "CQ" } }),
    );
    assert_eq!(code(&save), Some("not-found"), "{save:?}");
    let rename = app.overwolf().set_window_name("ow-cmp", "x").unwrap_err();
    assert_eq!(rename.code(), tauri_plugin_overwolf::ErrorCode::Forbidden);
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

/// DESIGN §4.5 rule 3: a non-app origin is refused unless it is listed in
/// `ads.allowedEmbedderOrigins` (an app served by
/// `tauri-plugin-localhost`); the capability grants the remote URL.
#[test]
fn a_remote_origin_needs_allowed_embedder_origins() {
    let page = WebviewUrl::External(LOCALHOST_UI.parse().unwrap());
    let plain = app("embedder-plain", &[]);
    common::window(&plain, "localhost-ui", page.clone());
    let refused = invoke(&plain, "localhost-ui", LOCALHOST_UI, "get_info", json!({}));
    assert_eq!(outcome(&refused), Outcome::Forbidden, "{refused:?}");

    let (context, _dir) = common::context(
        "embedder-allowed",
        &json!({ "ads": { "allowedEmbedderOrigins": ["http://localhost:9527"] } }),
        &[],
    );
    let allowed = common::build(context, Builder::new(), None);
    common::window(&allowed, "localhost-ui", page);
    common::window(
        &allowed,
        "other",
        WebviewUrl::External("http://localhost:9528/".parse().unwrap()),
    );
    assert_eq!(
        granted_from(&allowed, "localhost-ui", LOCALHOST_UI),
        DEFAULT
    );
    let info = invoke(
        &allowed,
        "localhost-ui",
        LOCALHOST_UI,
        "get_info",
        json!({}),
    )
    .unwrap();
    assert_eq!(info["name"], "ACL Fixture");
    // Another port is another origin; the capability does not name it.
    let other = invoke(
        &allowed,
        "other",
        "http://localhost:9528/",
        "get_info",
        json!({}),
    );
    assert_eq!(outcome(&other), Outcome::Acl, "{other:?}");
    // The listed origin may not reach the guest or consent commands.
    for cmd in GUEST_ONLY {
        let r = invoke(&allowed, "localhost-ui", LOCALHOST_UI, cmd, probe_body(cmd));
        assert_eq!(outcome(&r), Outcome::Acl, "{cmd}: {r:?}");
    }
}

/// [`granted`] for a page at `url`.
fn granted_from(
    app: &tauri::App<tauri::test::MockRuntime>,
    label: &str,
    url: &str,
) -> Vec<&'static str> {
    COMMANDS
        .iter()
        .copied()
        .filter(|cmd| {
            let r = invoke(app, label, url, cmd, probe_body(cmd));
            assert_ne!(outcome(&r), Outcome::Forbidden, "{label} {cmd}: {r:?}");
            outcome(&r) == Outcome::Reached
        })
        .collect()
}
