//! Engine tests on Tauri's mock runtime, against a loopback feed and a
//! fake OS layer (DESIGN §4.14, §7.4): the verification rules (R5), the
//! re-verification before launch (SEC-m3), `/UPDATE`, the install at exit
//! and the staged rollout.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use serde_json::{Value, json};
use sha2::Sha512;
use tauri::App;
use tauri::test::MockRuntime;

use super::*;
use crate::error::ErrorCode;
use crate::host::windows::tests::{Capture, mock_app};
use crate::updater::client::tests::{Server, respond};
use crate::updater::{Update, UpdaterBuilder};

/// A test public key and a prehashed minisign signature of the four bytes
/// `test` (the minisign-verify crate's own test vector).
const KEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
const SIG: &str = "untrusted comment: signature from minisign secret key
RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=
trusted comment: timestamp:1556193335\tfile:test
y/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";

/// What the fake Authenticode check reports.
#[derive(Debug, Clone)]
pub(crate) enum Signer {
    /// A valid signature by this subject.
    Subject(&'static str),
    /// PowerShell blocked: the check cannot run.
    Blocked,
}

/// A Windows stand-in: records launches with the installer's bytes at
/// launch time.
pub(crate) struct FakeOs {
    pub(crate) signer: Mutex<Signer>,
    pub(crate) launches: Mutex<Vec<(WindowsInstall, Vec<u8>)>>,
    pub(crate) authenticode_calls: std::sync::atomic::AtomicUsize,
}

impl FakeOs {
    pub(crate) fn new(signer: Signer) -> Arc<Self> {
        Arc::new(FakeOs {
            signer: Mutex::new(signer),
            launches: Mutex::new(Vec::new()),
            authenticode_calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    pub(crate) fn launches(&self) -> Vec<(WindowsInstall, Vec<u8>)> {
        lock(&self.launches).clone()
    }
}

impl UpdateOs for FakeOs {
    fn supported(&self) -> Result<()> {
        Ok(())
    }

    fn os_release(&self) -> String {
        "10.0.22631".into()
    }

    fn authenticode(&self, file: &Path) -> Result<Authenticode> {
        self.authenticode_calls.fetch_add(1, Ordering::SeqCst);
        match lock(&self.signer).clone() {
            Signer::Subject(s) => Ok(Authenticode {
                status: 0,
                status_message: "Valid".into(),
                subject: Some(s.into()),
                path: file.to_string_lossy().into_owned(),
            }),
            Signer::Blocked => Err(Error::verification("The signature check could not run.")),
        }
    }

    fn launch(&self, plan: &WindowsInstall) -> Result<()> {
        let bytes = std::fs::read(&plan.program).unwrap_or_default();
        lock(&self.launches).push((plan.clone(), bytes));
        Ok(())
    }
}

/// A feed server: `/feed/<channel>.yml` lists `/files/setup.exe` with
/// `payload`'s SHA-512; the installer and its `.sig` are served too.
pub(crate) struct Feed {
    pub(crate) server: Server,
    pub(crate) yaml: Arc<Mutex<String>>,
}

pub(crate) fn sha512_b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(Sha512::digest(data))
}

pub(crate) fn start_feed(version: &str, payload: &'static [u8]) -> Feed {
    let yaml = Arc::new(Mutex::new(format!(
        "version: {version}\nfiles:\n  - url: ../files/setup.exe\n    sha512: {}\n    size: {}\nreleaseDate: '2026-10-01T00:00:00.000Z'\nreleaseNotes: fixes\n",
        sha512_b64(payload),
        payload.len()
    )));
    let feed_text = Arc::clone(&yaml);
    let server = Server::start(Arc::new(move |stream, req| {
        let s = stream;
        let path = req.path().split('?').next().unwrap_or_default().to_owned();
        if Path::new(&path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("yml"))
        {
            let body = lock(&feed_text).clone();
            respond(s, "200 OK", &[], body.as_bytes());
        } else if path == "/files/setup.exe" {
            respond(s, "200 OK", &[], payload);
        } else if path == "/files/setup.exe.sig" {
            respond(s, "200 OK", &[], SIG.as_bytes());
        } else {
            respond(s, "404 Not Found", &[], b"");
        }
    }));
    Feed { server, yaml }
}

/// A mock app with `updater` config (endpoint = `feed`) and `os`.
pub(crate) fn mock(
    name: &str,
    feed: &Feed,
    updater: &Value,
    os: Arc<FakeOs>,
) -> (App<MockRuntime>, PathBuf, Arc<Shared>) {
    let mut config = json!({ "endpoint": format!("{}/feed", feed.server.base) });
    if let (Some(c), Some(u)) = (config.as_object_mut(), updater.as_object()) {
        c.extend(u.clone());
    }
    let (app, dir, _core) = mock_app(
        name,
        &json!({ "updater": config }),
        &[],
        Capture::answering("{}"),
    );
    let shared = Arc::new(Shared::new(os));
    assert!(app.manage(SharedState(Arc::clone(&shared))));
    (app, dir, shared)
}

fn block<T>(f: impl Future<Output = T>) -> T {
    tauri::async_runtime::block_on(f)
}

fn check(app: &App<MockRuntime>) -> Result<Option<Update>> {
    block(async { UpdaterBuilder::new(app.handle().clone()).build()?.check().await })
}

fn signed() -> Value {
    json!({ "publisherNames": ["Example Studio"] })
}

#[test]
fn check_download_install() {
    let feed = start_feed("2.0.0", b"test");
    let os = FakeOs::new(Signer::Subject("CN=Example Studio, C=PT"));
    let (app, dir, shared) = mock("updater-flow", &feed, &signed(), Arc::clone(&os));
    let update = check(&app).unwrap().expect("2.0.0 is newer than the mock app");
    assert_eq!(update.version, "2.0.0");
    assert!(!update.staged);
    assert_eq!(update.body.as_deref(), Some("fixes"));
    assert_eq!(update.date.as_deref(), Some("2026-10-01T00:00:00.000Z"));
    assert!(update.download_url.as_str().ends_with("/files/setup.exe"));
    let seen = feed.server.seen();
    let line = &seen[0].line;
    assert!(line.starts_with("GET /feed/latest.yml?noCache="), "{line}");
    let mut chunks = 0;
    let mut finished = false;
    let downloaded = block(update.download(
        |n, total| {
            chunks += n;
            assert_eq!(total, Some(4));
        },
        || finished = true,
    ))
    .unwrap();
    assert_eq!((chunks, finished), (4, true));
    assert!(os.launches().is_empty());
    update.install(downloaded).unwrap();
    let launches = os.launches();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].0.args, ["/UPDATE", "/R"]);
    assert_eq!(launches[0].1, b"test");
    assert!(!launches[0].0.elevate);
    // Verified at download and again before the start.
    assert_eq!(os.authenticode_calls.load(Ordering::SeqCst), 2);
    assert_eq!(shared.test.exits.load(Ordering::SeqCst), 1);
    // Nothing is pending at exit any more.
    app.handle().cleanup_before_exit();
    assert_eq!(os.launches().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `updater.installOnExit` (default): a downloaded update starts silently
/// with `/S /UPDATE` when Tauri clears the resources at exit.
#[test]
fn install_at_exit() {
    let feed = start_feed("2.0.0", b"test");
    let os = FakeOs::new(Signer::Subject("CN=Example Studio"));
    let (app, dir, shared) = mock(
        "updater-exit",
        &feed,
        &json!({ "publisherNames": ["Example Studio"], "installerArgs": ["/S"] }),
        Arc::clone(&os),
    );
    let update = check(&app).unwrap().unwrap();
    let downloaded = block(update.download(|_, _| {}, || {})).unwrap();
    drop(downloaded);
    assert!(os.launches().is_empty());
    app.handle().cleanup_before_exit();
    let launches = os.launches();
    assert_eq!(launches.len(), 1);
    // installerArgs keep /UPDATE (DX-minor-8).
    assert_eq!(launches[0].0.args, ["/S", "/UPDATE"]);
    assert_eq!(shared.test.exits.load(Ordering::SeqCst), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_install_at_exit_when_off() {
    let feed = start_feed("2.0.0", b"test");
    let os = FakeOs::new(Signer::Subject("CN=Example Studio"));
    let (app, dir, _shared) = mock(
        "updater-exit-off",
        &feed,
        &json!({ "publisherNames": ["Example Studio"], "installOnExit": false }),
        Arc::clone(&os),
    );
    let update = check(&app).unwrap().unwrap();
    drop(block(update.download(|_, _| {}, || {})).unwrap());
    app.handle().cleanup_before_exit();
    assert!(os.launches().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// §7.4, SEC-m3: an installer changed after the download fails the check
/// before launch; nothing starts and the file is gone.
#[test]
fn swapped_installer_is_not_launched() {
    let feed = start_feed("2.0.0", b"test");
    let os = FakeOs::new(Signer::Subject("CN=Example Studio"));
    let (app, dir, shared) = mock("updater-swap", &feed, &signed(), Arc::clone(&os));
    let update = check(&app).unwrap().unwrap();
    let downloaded = block(update.download(|_, _| {}, || {})).unwrap();
    let core = crate::host::core_of(app.handle()).unwrap();
    let file = core
        .identity
        .state_dir
        .root()
        .join("updater/pending/setup.exe");
    assert!(file.is_file());
    let swap = std::fs::OpenOptions::new().write(true).open(&file);
    if cfg!(windows) {
        // The open handle denies writers until NSIS starts.
        assert!(swap.is_err(), "the installer must be locked");
    } else {
        use std::io::Write as _;
        swap.unwrap().write_all(b"evil").unwrap();
        let err = update.install(downloaded).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Verification);
        assert!(os.launches().is_empty());
        assert!(!file.exists());
        assert_eq!(shared.test.exits.load(Ordering::SeqCst), 0);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// §7.4: PowerShell blocked → `verification`, no install.
#[test]
fn blocked_signature_check_fails_closed() {
    let feed = start_feed("2.0.0", b"test");
    let os = FakeOs::new(Signer::Blocked);
    let (app, dir, _shared) = mock("updater-blocked", &feed, &signed(), Arc::clone(&os));
    let update = check(&app).unwrap().unwrap();
    let err = block(update.download(|_, _| {}, || {})).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Verification);
    app.handle().cleanup_before_exit();
    assert!(os.launches().is_empty());
    // A signer that turns bad between download and install also stops it.
    let os = FakeOs::new(Signer::Subject("CN=Example Studio"));
    let (app2, dir2, _) = mock("updater-blocked-late", &feed, &signed(), Arc::clone(&os));
    let update = check(&app2).unwrap().unwrap();
    let downloaded = block(update.download(|_, _| {}, || {})).unwrap();
    *lock(&os.signer) = Signer::Subject("CN=Someone Else");
    assert_eq!(
        update.install(downloaded).unwrap_err().code(),
        ErrorCode::Verification
    );
    assert!(os.launches().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

/// R5: another publisher, a bad hash and a bad size all fail with
/// `verification`.
#[test]
fn verification_failures() {
    let feed = start_feed("2.0.0", b"test");
    let os = FakeOs::new(Signer::Subject("CN=Other Studio"));
    let (app, dir, _shared) = mock("updater-verify", &feed, &signed(), Arc::clone(&os));
    let update = check(&app).unwrap().unwrap();
    let err = block(update.download(|_, _| {}, || {})).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Verification);
    *lock(&os.signer) = Signer::Subject("CN=Example Studio");
    *lock(&feed.yaml) = format!(
        "version: 2.0.0\nfiles:\n  - url: ../files/setup.exe\n    sha512: {}\n    size: 4\n",
        sha512_b64(b"nope")
    );
    let update = check(&app).unwrap().unwrap();
    let err = block(update.download(|_, _| {}, || {})).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Verification);
    assert!(err.to_string().contains("SHA-512"), "{err}");
    *lock(&feed.yaml) = format!(
        "version: 2.0.0\nfiles:\n  - url: ../files/setup.exe\n    sha512: {}\n    size: 3\n",
        sha512_b64(b"test")
    );
    let update = check(&app).unwrap().unwrap();
    let err = block(update.download(|_, _| {}, || {})).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Verification);
    assert!(os.launches().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// R5: a minisign key alone is enough; a wrong signature fails.
#[test]
fn minisign_only() {
    let feed = start_feed("2.0.0", b"test");
    let os = FakeOs::new(Signer::Blocked);
    let (app, dir, _shared) = mock(
        "updater-minisign",
        &feed,
        &json!({ "pubkey": KEY }),
        Arc::clone(&os),
    );
    let update = check(&app).unwrap().unwrap();
    let downloaded = block(update.download(|_, _| {}, || {})).unwrap();
    update.install(downloaded).unwrap();
    assert_eq!(os.launches().len(), 1);
    // No Authenticode check without publisherNames.
    assert_eq!(os.authenticode_calls.load(Ordering::SeqCst), 0);
    // A payload the signature does not cover.
    let other = start_feed("2.0.0", b"Test");
    let (app2, dir2, _) = mock(
        "updater-minisign-bad",
        &other,
        &json!({ "pubkey": KEY }),
        FakeOs::new(Signer::Blocked),
    );
    let update = check(&app2).unwrap().unwrap();
    let err = block(update.download(|_, _| {}, || {})).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Verification);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

/// R5: neither publisherNames nor pubkey is a `config` error at build;
/// a debug build may skip with dangerousSkipPublisherCheck.
#[test]
fn publisher_data_is_required() {
    let feed = start_feed("2.0.0", b"test");
    let os = FakeOs::new(Signer::Blocked);
    let (app, dir, _shared) = mock("updater-r5", &feed, &json!({}), Arc::clone(&os));
    let err = UpdaterBuilder::new(app.handle().clone()).build().unwrap_err();
    assert_eq!(err.code(), ErrorCode::Config);
    assert!(err.to_string().contains("publisherNames"), "{err}");
    let (app2, dir2, _) = mock(
        "updater-r5-skip",
        &feed,
        &json!({ "dangerousSkipPublisherCheck": true }),
        Arc::clone(&os),
    );
    let update = check(&app2).unwrap().unwrap();
    let downloaded = block(update.download(|_, _| {}, || {})).unwrap();
    update.install(downloaded).unwrap();
    assert_eq!(os.launches().len(), 1);
    // The release rule: the skip never applies outside debug builds.
    let config: UpdaterConfig =
        serde_json::from_value(json!({ "dangerousSkipPublisherCheck": true })).unwrap();
    let err = Settings::resolve(&config, "abc", None, false, &Overrides::default()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Config);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

/// D24: an MSI-only release is `unsupported` with the documented message.
#[test]
fn msi_release_is_refused() {
    let feed = start_feed("2.0.0", b"test");
    *lock(&feed.yaml) =
        "version: 2.0.0\nfiles:\n  - url: App-2.0.0.msi\n    sha512: abc\n    size: 4\n".into();
    let (app, dir, _shared) = mock(
        "updater-msi",
        &feed,
        &signed(),
        FakeOs::new(Signer::Subject("CN=Example Studio")),
    );
    let err = check(&app).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Unsupported);
    assert!(err.to_string().contains(crate::updater::MSI_UNSUPPORTED));
    let _ = std::fs::remove_dir_all(&dir);
}

/// SEC-m4 is the commands' rule; here: the Rust `channel()` allows a
/// downgrade, as electron-updater, and `allow_downgrade(false)` after it
/// turns it off.
#[test]
fn rust_channel_allows_downgrade() {
    let feed = start_feed("0.0.1", b"test");
    let os = FakeOs::new(Signer::Subject("CN=Example Studio"));
    let (app, dir, _shared) = mock("updater-channel", &feed, &signed(), os);
    assert!(check(&app).unwrap().is_none());
    let found = block(async {
        UpdaterBuilder::new(app.handle().clone())
            .channel("beta")
            .build()?
            .check()
            .await
    })
    .unwrap();
    assert_eq!(found.unwrap().version, "0.0.1");
    assert!(feed.server.seen().last().unwrap().line.contains("/feed/beta.yml"));
    let none = block(async {
        UpdaterBuilder::new(app.handle().clone())
            .channel("beta")
            .allow_downgrade(false)
            .build()?
            .check()
            .await
    })
    .unwrap();
    assert!(none.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// I.2 #5: the staging id lives in `<userData>/.updaterId`, created only
/// when the feed has a percentage; `staged` reports the passed gate.
#[test]
fn staged_rollout() {
    let feed = start_feed("2.0.0", b"test");
    let (app, dir, _shared) = mock(
        "updater-staging",
        &feed,
        &signed(),
        FakeOs::new(Signer::Subject("CN=Example Studio")),
    );
    let core = crate::host::core_of(app.handle()).unwrap();
    let id_file = crate::paths::user_data_dir(&core.identity.app_data_dir, &core.identity.app.name)
        .join(".updaterId");
    assert!(!check(&app).unwrap().unwrap().staged);
    assert!(!id_file.exists());
    std::fs::create_dir_all(id_file.parent().unwrap()).unwrap();
    std::fs::write(&id_file, "00000000-0000-4000-8000-000000000000").unwrap();
    lock(&feed.yaml).push_str("stagingPercentage: 10\n");
    assert!(check(&app).unwrap().unwrap().staged);
    std::fs::write(&id_file, "00000000-0000-4000-8000-0000ffffffff").unwrap();
    assert!(check(&app).unwrap().is_none());
    std::fs::remove_file(&id_file).unwrap();
    let _ = check(&app).unwrap();
    assert!(is_uuid_text(&std::fs::read_to_string(&id_file).unwrap()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// SEC-M10: builder headers reach the feed, never the installer download.
#[test]
fn headers_reach_the_feed_only() {
    let feed = start_feed("2.0.0", b"test");
    let (app, dir, _shared) = mock(
        "updater-headers",
        &feed,
        &signed(),
        FakeOs::new(Signer::Subject("CN=Example Studio")),
    );
    let update = block(async {
        UpdaterBuilder::new(app.handle().clone())
            .header("X-Token", "secret")?
            .build()?
            .check()
            .await
    })
    .unwrap()
    .unwrap();
    drop(block(update.download(|_, _| {}, || {})).unwrap());
    let seen = feed.server.seen();
    let feed_req = seen.iter().find(|r| r.path().contains(".yml")).unwrap();
    assert_eq!(feed_req.header("x-token"), Some("secret"));
    let file_req = seen.iter().find(|r| r.path() == "/files/setup.exe").unwrap();
    assert_eq!(file_req.header("x-token"), None);
    assert!(
        UpdaterBuilder::new(app.handle().clone())
            .header("Bad\nName", "x")
            .is_err()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// §7.4: a refusing proxy is a `network` error, no panic.
#[test]
fn refusing_proxy_is_a_network_error() {
    let feed = start_feed("2.0.0", b"test");
    let (app, dir, shared) = mock(
        "updater-proxy",
        &feed,
        &signed(),
        FakeOs::new(Signer::Subject("CN=Example Studio")),
    );
    *lock(&shared.test.proxy) = Some("http://127.0.0.1:9".into());
    assert_eq!(check(&app).unwrap_err().code(), ErrorCode::Network);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The dev feed (debug builds) replaces the configured one.
#[test]
fn settings_resolution() {
    let config: UpdaterConfig = serde_json::from_value(json!({
        "publisherNames": ["  ", "Studio"],
        "channel": "stable",
        "connectTimeoutMs": 0
    }))
    .unwrap();
    let s = Settings::resolve(&config, "abc", None, false, &Overrides::default()).unwrap();
    assert_eq!(
        s.feed.as_str(),
        "https://electron-updates.overwolf.com/electron-updates/electron/abc/"
    );
    assert_eq!(s.channel, "stable");
    assert_eq!(s.publisher_names, ["Studio"]);
    assert_eq!(s.connect_timeout, Duration::from_millis(1));
    assert_eq!(s.read_timeout, Duration::from_secs(60));
    assert!(s.install_on_exit && !s.allow_downgrade && !s.skip_publisher_check);
    let dev = "provider: generic\nurl: http://127.0.0.1:8080/updates\nchannel: beta\n";
    let d = Settings::resolve(&config, "abc", Some(dev), true, &Overrides::default()).unwrap();
    assert_eq!(
        (d.feed.as_str(), d.channel.as_str()),
        ("http://127.0.0.1:8080/updates/", "beta")
    );
    // Ignored in release builds.
    let r = Settings::resolve(&config, "abc", Some(dev), false, &Overrides::default()).unwrap();
    assert!(r.feed.as_str().starts_with("https://electron-updates.overwolf.com/"));
    let github = "provider: github\nurl: https://x.example/\n";
    assert!(Settings::resolve(&config, "abc", Some(github), true, &Overrides::default()).is_err());
    let o = Overrides {
        channel: Some("../x".into()),
        ..Overrides::default()
    };
    assert!(Settings::resolve(&config, "abc", None, false, &o).is_err());
    let bad_key: UpdaterConfig = serde_json::from_value(json!({ "pubkey": "nope" })).unwrap();
    assert!(Settings::resolve(&bad_key, "abc", None, false, &Overrides::default()).is_err());
    assert_eq!(
        signature_url(&Url::parse("https://cdn.example.com/1.0/setup.exe?t=1").unwrap()).as_str(),
        "https://cdn.example.com/1.0/setup.exe.sig?t=1"
    );
}
