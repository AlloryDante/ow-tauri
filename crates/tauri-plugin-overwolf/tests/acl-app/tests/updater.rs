//! The update client through the compiled ACL (DESIGN §4.14, §7.2, R5,
//! R6, SEC-m4): only `overwolf:updater` grants its commands; without the
//! `updater` cargo feature, or off Windows, they answer `unsupported`. On
//! Windows with the feature (CI runs the workspace with `--all-features`):
//! check, download and verify from a loopback feed, the install at exit,
//! the MSI refusal, the publisher rule and the JavaScript downgrade gate.
//!
//! `install()` from JavaScript exits the app, which Tauri's mock runtime
//! cannot do; the install at exit runs the same verified start.

#![allow(clippy::unwrap_used, reason = "a test fails on any unexpected error")]

mod common;

use serde_json::{Value, json};

use common::{Outcome, code, invoke, origin, outcome};

/// The update client's commands (`overwolf:updater`).
const UPDATER: [&str; 4] = [
    "updater_check",
    "updater_download",
    "updater_install",
    "updater_download_and_install",
];

/// Arguments that reach each handler: an update resource that does not
/// exist.
fn body(cmd: &str) -> Value {
    match cmd {
        "updater_check" => json!({}),
        "updater_install" => json!({ "rid": 4242 }),
        _ => json!({ "rid": 4242, "onEvent": format!("{}1", common::CHANNEL_PREFIX) }),
    }
}

/// Only the opt-in set grants the updater commands: not `overwolf:default`
/// (webviews `main`, `settings/panel`), not another opt-in set.
#[test]
fn only_overwolf_updater_grants_the_updater_commands() {
    let app = common::app(
        "updater-acl",
        &[
            "main",
            "settings/panel",
            "machine-id",
            "analytics",
            "updater",
        ],
    );
    for cmd in UPDATER {
        for label in ["main", "settings/panel", "machine-id", "analytics"] {
            let r = invoke(&app, label, origin(), cmd, body(cmd));
            assert_eq!(outcome(&r), Outcome::Acl, "{cmd} from {label}: {r:?}");
        }
        let r = invoke(&app, "updater", origin(), cmd, body(cmd));
        assert!(
            r.is_err() && !matches!(outcome(&r), Outcome::Acl | Outcome::NotRegistered),
            "{cmd} reached its handler: {r:?}"
        );
        let remote = invoke(&app, "updater", "https://example.com/", cmd, body(cmd));
        assert_eq!(outcome(&remote), Outcome::Acl, "{cmd} from a remote page");
    }
}

/// R6: without the `updater` feature every command answers
/// `unsupported`; with it, off Windows, `check` does (no update resource
/// can exist, so the others find none).
#[cfg(not(all(feature = "updater", windows)))]
#[test]
fn the_updater_is_unsupported_without_the_feature_or_off_windows() {
    let app = common::app("updater-unsupported", &["updater"]);
    for cmd in UPDATER {
        let r = invoke(&app, "updater", origin(), cmd, body(cmd));
        let want = if cmd == "updater_check" || !cfg!(feature = "updater") {
            "unsupported"
        } else {
            "not-found"
        };
        assert_eq!(code(&r), Some(want), "{cmd}: {r:?}");
    }
}

/// The engine on Windows. Compiled everywhere with the feature, run on
/// Windows (the OS layer elsewhere answers `unsupported`).
#[cfg(feature = "updater")]
mod engine {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::{Value, json};
    use tauri::test::MockRuntime;
    use tauri::{App, WebviewUrl};
    use tauri_plugin_overwolf::updater::feed::parse_feed;
    use tauri_plugin_overwolf::updater::verify::{sha512_matches, sha512_reader};
    use tauri_plugin_overwolf::updater::{MSI_UNSUPPORTED, choose_installer};
    use tauri_plugin_overwolf::{Builder, ErrorCode};

    use super::common::{self, CHANNEL_PREFIX, Capture, Channels, Server, code, invoke, origin};

    /// minisign's test key and its signature of `b"test"`.
    const KEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const SIG: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";

    /// The channel id of the download events.
    const EVENTS: u32 = 7;

    /// A feed release document for `file` with `payload`'s SHA-512 (hex)
    /// and size.
    fn release(version: &str, file: &str, payload: &[u8]) -> Vec<u8> {
        let mut digest = String::new();
        for b in sha512_reader(payload).unwrap() {
            let _ = write!(digest, "{b:02x}");
        }
        format!(
            "version: {version}\nfiles:\n  - url: {file}\n    sha512: {digest}\n    size: {}\nreleaseDate: '2026-10-01T00:00:00.000Z'\nreleaseNotes: fixes\n",
            payload.len()
        )
        .into_bytes()
    }

    /// A loopback feed serving `files` (request path, body).
    fn feed(files: Vec<(&str, Vec<u8>)>) -> Server {
        let files: BTreeMap<String, Vec<u8>> =
            files.into_iter().map(|(p, b)| (p.to_owned(), b)).collect();
        Server::start(Arc::new(move |path| files.get(path).cloned()))
    }

    /// An app with webview `updater`, `plugins.overwolf.updater` set to
    /// `updater` over the feed at `<server>/feed/` and no install at
    /// exit. Returns the state directory.
    fn fixture(
        name: &str,
        server: &Server,
        updater: &Value,
        channels: Option<&Arc<Channels>>,
    ) -> (App<MockRuntime>, PathBuf) {
        let mut block = json!({
            "endpoint": format!("{}/feed/", server.base),
            "installOnExit": false
        });
        if let (Some(b), Some(u)) = (block.as_object_mut(), updater.as_object()) {
            b.extend(u.clone());
        }
        let (context, dir) = common::context(name, &json!({ "updater": block }), &[]);
        let app = common::build(
            context,
            Builder::new().analytics_transport(Capture::hanging_eu_only()),
            channels,
        );
        common::window(&app, "updater", WebviewUrl::default());
        (app, dir)
    }

    /// `check(options)` from webview `updater`.
    fn check(app: &App<MockRuntime>, options: &Value) -> Result<Value, Value> {
        invoke(
            app,
            "updater",
            origin(),
            "updater_check",
            json!({ "options": options }),
        )
    }

    /// `Update.download()` of resource `rid`, its events on [`EVENTS`].
    fn download(app: &App<MockRuntime>, rid: &Value) -> Result<Value, Value> {
        invoke(
            app,
            "updater",
            origin(),
            "updater_download",
            json!({ "rid": rid, "onEvent": format!("{CHANNEL_PREFIX}{EVENTS}") }),
        )
    }

    /// Every file under `dir` named `name`.
    fn find(dir: &Path, name: &str) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(find(&path, name));
            } else if entry.file_name() == name {
                out.push(path);
            }
        }
        out
    }

    /// Whether the feed was asked for `path` (any query).
    fn requested(server: &Server, path: &str) -> bool {
        server.seen().iter().any(|line| {
            line.split(' ')
                .nth(1)
                .is_some_and(|p| p.split('?').next() == Some(path))
        })
    }

    /// The fixture feed is what the client reads: its release, installer
    /// entry and digest parse on every OS (the client runs on Windows).
    #[test]
    fn the_fixture_feed_parses() {
        let parse = |file: &str| {
            parse_feed(&String::from_utf8(release("2.0.0", file, b"test")).unwrap()).unwrap()
        };
        let info = parse("setup.exe");
        assert_eq!(info.version, "2.0.0");
        let file = choose_installer(&info).unwrap();
        assert_eq!((file.url.as_str(), file.size), ("setup.exe", Some(4)));
        assert!(sha512_matches(
            &file.sha512,
            &sha512_reader(&b"test"[..]).unwrap()
        ));
        assert_eq!(
            choose_installer(&parse("App.msi"))
                .map_err(|e| e.code())
                .err(),
            Some(ErrorCode::Unsupported)
        );
    }

    /// I.2, I.3: `check` reads `<endpoint>/latest.yml`, `download` streams
    /// the installer with Started / Progress / Finished events, fetches
    /// `setup.exe.sig` and verifies SHA-512 and the minisign signature;
    /// `install` before a download finds nothing.
    #[test]
    #[cfg_attr(not(windows), ignore = "the update client runs on Windows only (R6)")]
    fn check_download_and_verify_from_a_loopback_feed() {
        let server = feed(vec![
            ("/feed/latest.yml", release("2.0.0", "setup.exe", b"test")),
            ("/feed/setup.exe", b"test".to_vec()),
            ("/feed/setup.exe.sig", SIG.as_bytes().to_vec()),
        ]);
        let channels = Arc::new(Channels::default());
        let (app, dir) = fixture(
            "updater-flow",
            &server,
            &json!({ "pubkey": KEY }),
            Some(&channels),
        );
        let meta = check(&app, &json!({})).unwrap();
        assert_eq!(meta["version"], "2.0.0");
        assert_eq!(meta["currentVersion"], "0.1.0");
        assert_eq!(meta["body"], "fixes");
        assert_eq!(meta["raw"]["version"], "2.0.0");
        assert!(
            requested(&server, "/feed/latest.yml"),
            "{:?}",
            server.seen()
        );
        let rid = meta["rid"].clone();
        let early = invoke(
            &app,
            "updater",
            origin(),
            "updater_install",
            json!({ "rid": rid }),
        );
        assert_eq!(code(&early), Some("not-found"), "install before download");
        assert_eq!(download(&app, &rid), Ok(Value::Null));
        let events = channels.messages("updater", EVENTS);
        assert_eq!(
            events.first(),
            Some(&json!({ "event": "Started", "data": { "contentLength": 4 } })),
            "{events:?}"
        );
        assert_eq!(events.last(), Some(&json!({ "event": "Finished" })));
        let streamed: u64 = events
            .iter()
            .filter(|e| e["event"] == "Progress")
            .filter_map(|e| e["data"]["chunkLength"].as_u64())
            .sum();
        assert_eq!(streamed, 4);
        assert!(requested(&server, "/feed/setup.exe"));
        assert!(requested(&server, "/feed/setup.exe.sig"));
        let staged = find(&dir, "setup.exe");
        assert_eq!(staged.len(), 1, "the verified installer waits: {staged:?}");
        assert_eq!(std::fs::read(&staged[0]).unwrap(), b"test");
        let other = download(&app, &json!(9999));
        assert_eq!(code(&other), Some("not-found"), "another resource id");
    }

    /// I.3, R5: a signature that does not match the bytes is a
    /// `verification` error and the download is deleted.
    #[test]
    #[cfg_attr(not(windows), ignore = "the update client runs on Windows only (R6)")]
    fn a_bad_signature_fails_and_removes_the_download() {
        let server = feed(vec![
            ("/feed/latest.yml", release("2.0.0", "setup.exe", b"tesT")),
            ("/feed/setup.exe", b"tesT".to_vec()),
            ("/feed/setup.exe.sig", SIG.as_bytes().to_vec()),
        ]);
        let (app, dir) = fixture("updater-bad-sig", &server, &json!({ "pubkey": KEY }), None);
        let meta = check(&app, &json!({})).unwrap();
        let r = download(&app, &meta["rid"]);
        assert_eq!(code(&r), Some("verification"), "{r:?}");
        assert!(find(&dir, "setup.exe").is_empty());
        assert!(find(&dir, "temp-setup.exe").is_empty());
    }

    /// DESIGN §4.14: a release with only an `.msi` is refused with
    /// [`MSI_UNSUPPORTED`].
    #[test]
    #[cfg_attr(not(windows), ignore = "the update client runs on Windows only (R6)")]
    fn an_msi_only_release_is_unsupported() {
        let server = feed(vec![(
            "/feed/latest.yml",
            release("2.0.0", "App.msi", b"test"),
        )]);
        let (app, _dir) = fixture("updater-msi", &server, &json!({ "pubkey": KEY }), None);
        let r = check(&app, &json!({}));
        assert_eq!(code(&r), Some("unsupported"), "{r:?}");
        let message = r.unwrap_err()["message"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        assert!(message.contains(MSI_UNSUPPORTED), "{message}");
    }

    /// R5: nothing is trusted by default. Without `publisherNames` or
    /// `pubkey` a check is a `config` error before any request; with
    /// `publisherNames`, an unsigned installer fails the Authenticode
    /// publisher check and is deleted.
    #[test]
    #[cfg_attr(not(windows), ignore = "the update client runs on Windows only (R6)")]
    fn the_publisher_rule() {
        let server = feed(vec![
            ("/feed/latest.yml", release("2.0.0", "setup.exe", b"test")),
            ("/feed/setup.exe", b"test".to_vec()),
        ]);
        let (bare, _dir) = fixture("updater-r5-none", &server, &json!({}), None);
        let r = check(&bare, &json!({}));
        assert_eq!(code(&r), Some("config"), "{r:?}");
        assert!(
            server.seen().is_empty(),
            "no request without a publisher rule"
        );

        let (named, dir) = fixture(
            "updater-r5-names",
            &server,
            &json!({ "publisherNames": ["Example Studio"] }),
            None,
        );
        let meta = check(&named, &json!({})).unwrap();
        let r = download(&named, &meta["rid"]);
        assert_eq!(code(&r), Some("verification"), "{r:?}");
        assert!(
            find(&dir, "setup.exe").is_empty(),
            "the unsigned file is deleted"
        );
    }

    /// SEC-m4: JavaScript's `channel` or `allowDowngrade` allow a lower
    /// version only with `updater.allowJsDowngrade`; `false` always turns
    /// it off. JavaScript sends no headers (SEC-M10).
    #[test]
    #[cfg_attr(not(windows), ignore = "the update client runs on Windows only (R6)")]
    fn the_javascript_downgrade_gate() {
        let older = release("0.0.1", "setup.exe", b"test");
        let server = feed(vec![
            ("/feed/latest.yml", older.clone()),
            ("/feed/beta.yml", older),
        ]);
        let (gated, _dir) = fixture(
            "updater-downgrade",
            &server,
            &json!({ "pubkey": KEY }),
            None,
        );
        for options in [
            json!({}),
            json!({ "channel": "beta" }),
            json!({ "allowDowngrade": true }),
        ] {
            assert_eq!(check(&gated, &options), Ok(Value::Null), "{options}");
        }
        assert!(requested(&server, "/feed/beta.yml"), "{:?}", server.seen());
        assert!(
            check(&gated, &json!({ "headers": { "x-a": "b" } })).is_err(),
            "no headers from JavaScript"
        );

        let (open, _dir) = fixture(
            "updater-downgrade-allowed",
            &server,
            &json!({ "pubkey": KEY, "allowJsDowngrade": true }),
            None,
        );
        assert_eq!(
            check(&open, &json!({ "channel": "beta" })).unwrap()["version"],
            "0.0.1"
        );
        assert_eq!(
            check(&open, &json!({ "allowDowngrade": true })).unwrap()["version"],
            "0.0.1"
        );
        for options in [
            json!({}),
            json!({ "channel": "beta", "allowDowngrade": false }),
        ] {
            assert_eq!(check(&open, &options), Ok(Value::Null), "{options}");
        }
    }

    /// I.4, `updater.installOnExit`: a verified download starts at the
    /// app's exit (Tauri clears its resources; `restart()` does too) as
    /// `setup.exe /S /UPDATE`.
    #[test]
    #[cfg_attr(not(windows), ignore = "the update client runs on Windows only (R6)")]
    fn a_downloaded_update_installs_at_exit() {
        let installer = std::fs::read(env!("CARGO_BIN_EXE_fake-installer")).unwrap();
        let server = feed(vec![
            (
                "/feed/latest.yml",
                release("2.0.0", "setup.exe", &installer),
            ),
            ("/feed/setup.exe", installer),
        ]);
        let (app, dir) = fixture(
            "updater-at-exit",
            &server,
            &json!({ "dangerousSkipPublisherCheck": true, "installOnExit": true }),
            None,
        );
        let meta = check(&app, &json!({})).unwrap();
        assert_eq!(download(&app, &meta["rid"]), Ok(Value::Null));
        assert!(
            find(&dir, "setup.args").is_empty(),
            "nothing starts before the exit"
        );
        app.handle().cleanup_before_exit();
        assert!(
            common::wait_until(Duration::from_secs(30), || !find(&dir, "setup.args")
                .is_empty()),
            "the installer started at exit"
        );
        let args = std::fs::read_to_string(&find(&dir, "setup.args")[0]).unwrap();
        assert_eq!(args.lines().collect::<Vec<_>>(), ["/S", "/UPDATE"]);
    }
}
