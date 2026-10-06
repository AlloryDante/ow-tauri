//! The plugin side of the update client (CONTRACT A.2.8, I.3, I.4): the
//! `updater_*` commands, the `updater` host messages, the verified download
//! and the install at exit.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha512};
use tauri::{Manager, Runtime};
use url::Url;

use super::feed::{MAX_FEED_BYTES, parse_feed};
use super::install::{self, download_file_name, extension};
use super::verify;
use super::{
    Availability, DevUpdateConfig, InstallerKind, ProgressInfo, ResolvedConfig, UpdateCheckResult,
    UpdateInfo, UpdaterConfig, check_transport, choose_file, feed_file_name, is_update_available,
    parse_version, resolve_file_url, staging_bucket,
};
use crate::error::{Error, ErrorCode};
use crate::host::Host;
use crate::ipc::messages::HostMessage;
use crate::paths::TargetOs;
use crate::state::log::LogLevel;

/// The largest download without a stated size (4 GiB).
const MAX_UNSIZED_DOWNLOAD: u64 = 4 * 1024 * 1024 * 1024;
/// The largest detached signature file.
const MAX_SIGNATURE_BYTES: usize = 16 * 1024;
/// At most one `download-progress` per this interval (electron-updater
/// throttles the same way).
const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);

/// A release the last check offered.
#[derive(Debug, Clone)]
struct Found {
    info: UpdateInfo,
}

/// A verified download.
#[derive(Debug, Clone)]
struct Downloaded {
    info: UpdateInfo,
    file: PathBuf,
    kind: InstallerKind,
    admin: bool,
    /// macOS: the unpacked, verified `.app`.
    app: Option<PathBuf>,
}

/// An install the app asked for with `quitAndInstall`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InstallRequest {
    silent: bool,
    force_run_after: bool,
}

/// The update client's state in the host core.
#[derive(Debug, Default)]
pub(crate) struct UpdaterCore {
    config: Option<ResolvedConfig>,
    found: Option<Found>,
    downloaded: Option<Downloaded>,
    request: Option<InstallRequest>,
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "the Authenticode check runs on Windows only")
    )]
    warned_unsigned: bool,
    /// Serialises downloads: a second `updater_download` waits for the first
    /// and reuses its file.
    download_lock: Arc<tokio::sync::Mutex<()>>,
    /// Installs a host without OS access (Tauri's mock runtime) would have
    /// run; tests read them.
    pub(crate) test_installs: Vec<Value>,
}

/// The Rust face of the update client (A.5 `Overwolf::updater`), with the
/// commands' behaviour.
pub struct Updater<R: Runtime>(pub(crate) Weak<Host<R>>);

impl<R: Runtime> std::fmt::Debug for Updater<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Updater").finish_non_exhaustive()
    }
}

impl<R: Runtime> Updater<R> {
    fn host(&self) -> Result<Arc<Host<R>>, Error> {
        self.0
            .upgrade()
            .ok_or_else(|| Error::not_ready("The plugin has shut down."))
    }

    /// `updater_configure` (I.1).
    ///
    /// # Errors
    ///
    /// `invalid-argument` for an unusable configuration.
    pub fn configure(&self, config: &UpdaterConfig) -> Result<(), Error> {
        self.host()?.updater_configure(config)
    }

    /// `updater_check` (I.2): `None` when updates are disabled.
    ///
    /// # Errors
    ///
    /// `network` when the feed cannot be read, `invalid-argument` for a bad
    /// feed or no configuration.
    pub async fn check(&self) -> Result<Option<UpdateCheckResult>, Error> {
        self.host()?.updater_check().await
    }

    /// `updater_download` (I.3): the downloaded, verified files.
    ///
    /// # Errors
    ///
    /// `not-found` without an offered update, `network`, `io`, or `backend`
    /// when verification fails.
    pub async fn download(&self) -> Result<Vec<PathBuf>, Error> {
        self.host()?.updater_download().await
    }

    /// `updater_quit_and_install` (I.4): quits and installs at exit.
    ///
    /// # Errors
    ///
    /// `not-found` when nothing is downloaded, `unsupported` on Linux
    /// without `updater.pubkey`.
    pub fn quit_and_install(&self, silent: bool, force_run_after: bool) -> Result<(), Error> {
        self.host()?
            .updater_quit_and_install(silent, force_run_after)
    }
}

fn network(err: &reqwest::Error) -> Error {
    let what = if err.is_timeout() {
        "The update server did not answer in time."
    } else if err.is_redirect() {
        "The update server redirected to a URL that is not allowed."
    } else {
        "The update server could not be reached."
    };
    Error::network(what)
}

fn http_client(debug: bool) -> Result<reqwest::Client, Error> {
    let policy = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= 10 {
            attempt.error("too many redirects")
        } else if check_transport(attempt.url(), debug).is_err() {
            attempt.error("redirect to a URL that is not https")
        } else {
            attempt.follow()
        }
    });
    reqwest::Client::builder()
        .redirect(policy)
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|_| Error::network("The HTTP client could not start."))
}

fn with_headers(
    mut request: reqwest::RequestBuilder,
    config: &ResolvedConfig,
) -> reqwest::RequestBuilder {
    for (name, value) in &config.request_headers {
        request = request.header(name.as_str(), value.as_str());
    }
    request
}

async fn get_ok(
    client: &reqwest::Client,
    config: &ResolvedConfig,
    url: &Url,
) -> Result<reqwest::Response, Error> {
    let response = with_headers(client.get(url.clone()), config)
        .send()
        .await
        .map_err(|e| network(&e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(Error::network(format!(
            "The update server answered HTTP {}.",
            status.as_u16()
        ))
        .with_data(json!({ "status": status.as_u16() })));
    }
    Ok(response)
}

/// Reads a small body, refusing more than `cap` bytes.
async fn read_capped(mut response: reqwest::Response, cap: usize) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| network(&e))? {
        if out.len() + chunk.len() > cap {
            return Err(Error::invalid_argument(
                "The update server sent too much data.",
            ));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// The feed URL with electron-updater's cache-busting query.
fn feed_url(config: &ResolvedConfig, os: TargetOs) -> Result<Url, Error> {
    let mut url = resolve_file_url(&config.url, &feed_file_name(&config.channel, os))?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    url.query_pairs_mut().append_pair("noCache", &token[..12]);
    Ok(url)
}

fn signature_url(file: &Url) -> Url {
    let mut sig = file.clone();
    let path = format!("{}.sig", file.path());
    sig.set_path(&path);
    sig
}

fn remove_quietly(path: &Path) {
    let _ = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
}

impl<R: Runtime> Host<R> {
    /// Sends one `updater` message (A.3, I.3).
    fn updater_emit(self: &Arc<Self>, event: &str, field: Option<(&str, Value)>) {
        let mut body = Map::new();
        body.insert("event".into(), Value::String(event.into()));
        if let Some((k, v)) = field {
            body.insert(k.into(), v);
        }
        self.send_main(HostMessage::Updater { body });
    }

    /// Reports a failure as an `error` event and returns it. `unsupported`
    /// is not emitted: the `autoUpdater` facade emits it for the call.
    fn updater_fail(self: &Arc<Self>, err: Error) -> Error {
        self.log(LogLevel::Warn, &format!("updater: {}", err.message()));
        if err.code() != ErrorCode::Unsupported {
            let wire = serde_json::to_value(&err).unwrap_or(Value::Null);
            self.updater_emit("error", Some(("error", wire)));
        }
        err
    }

    /// `updater_configure` (I.1).
    pub(crate) fn updater_configure(self: &Arc<Self>, config: &UpdaterConfig) -> Result<(), Error> {
        let dev = self
            .options
            .dev_app_update
            .filter(|t| !t.trim().is_empty())
            .map(DevUpdateConfig::parse)
            .transpose()?;
        if config.force_dev_update_config == Some(true) && !self.info.debug {
            self.log(
                LogLevel::Warn,
                "updater: forceDevUpdateConfig is ignored in release builds",
            );
        }
        let resolved = config.resolve(dev.as_ref(), self.info.debug)?;
        self.with_core(|c| {
            let changed_feed = c
                .updater
                .config
                .as_ref()
                .is_some_and(|old| old.url != resolved.url || old.channel != resolved.channel);
            if changed_feed {
                c.updater.found = None;
            }
            c.updater.config = Some(resolved);
        });
        Ok(())
    }

    fn updater_config(self: &Arc<Self>) -> Result<ResolvedConfig, Error> {
        self.with_core(|c| c.updater.config.clone()).ok_or_else(|| {
            Error::invalid_argument("No update feed is configured; call autoUpdater.setFeedURL().")
        })
    }

    /// The staged-rollout bucket of this install, creating `stagingId` in
    /// `ow-tauri.json` when it is missing or not a UUID (I.2 #5).
    fn staging_bucket(&self) -> Option<u8> {
        let stored = self.ow_tauri.get().staging_id;
        if let Some(bucket) = stored.as_deref().and_then(staging_bucket) {
            return Some(bucket);
        }
        let id = uuid::Uuid::new_v4().hyphenated().to_string();
        let bucket = staging_bucket(&id);
        let value = id.clone();
        if let Err(err) = self.ow_tauri.update(|s| s.staging_id = Some(value)) {
            self.log(
                LogLevel::Warn,
                &format!("updater: could not store the staging id: {}", err.kind()),
            );
        }
        bucket
    }

    /// `updater_check` (I.2).
    pub(crate) async fn updater_check(
        self: &Arc<Self>,
    ) -> Result<Option<UpdateCheckResult>, Error> {
        if !self.info.config.updater.enabled {
            return Ok(None);
        }
        let config = self.updater_config().map_err(|e| self.updater_fail(e))?;
        self.updater_emit("checking-for-update", None);
        let (info, available) = self
            .updater_fetch(&config)
            .await
            .map_err(|e| self.updater_fail(e))?;
        let wire = serde_json::to_value(&info).unwrap_or(Value::Null);
        self.with_core(|c| {
            c.updater.found = available.then(|| Found { info: info.clone() });
        });
        if available {
            self.updater_emit("update-available", Some(("info", wire)));
            if config.auto_download {
                let host = Arc::clone(self);
                tauri::async_runtime::spawn(async move {
                    // Failures are reported as `error` events.
                    let _ = host.updater_download().await;
                });
            }
        } else {
            self.updater_emit("update-not-available", Some(("info", wire)));
        }
        Ok(Some(UpdateCheckResult {
            version_info: info.clone(),
            update_info: info,
            is_update_available: available,
        }))
    }

    async fn updater_fetch(
        self: &Arc<Self>,
        config: &ResolvedConfig,
    ) -> Result<(UpdateInfo, bool), Error> {
        let client = http_client(self.info.debug)?;
        let url = feed_url(config, self.info.os)?;
        let response = get_ok(&client, config, &url).await?;
        let body = read_capped(response, MAX_FEED_BYTES).await?;
        let text = String::from_utf8(body)
            .map_err(|_| Error::invalid_argument("The update feed is not UTF-8 text."))?;
        let info = parse_feed(&text)?;
        let current = parse_version(&self.info.manifest.version)?;
        let availability = is_update_available(
            &current,
            &info,
            config.allow_downgrade,
            config.allow_prerelease,
            self.staging_bucket(),
        )?;
        if availability == Availability::NotInRollout {
            self.log(
                LogLevel::Info,
                "updater: this install is outside the staged rollout",
            );
        }
        Ok((info, availability == Availability::Available))
    }

    /// The update download folder: the app cache directory, or the state
    /// folder when `state.appDataDir` overrides the OS folders (tests).
    fn updater_dir(&self) -> Result<PathBuf, Error> {
        if self.info.config.state.app_data_dir.is_some() {
            return Ok(self.info.state_dir.root().join("updater"));
        }
        self.app
            .path()
            .app_cache_dir()
            .map(|d| d.join("ow-tauri-updater"))
            .map_err(|_| Error::io("The app cache directory is unknown."))
    }

    /// `updater_download` (I.3).
    pub(crate) async fn updater_download(self: &Arc<Self>) -> Result<Vec<PathBuf>, Error> {
        let lock = self.with_core(|c| Arc::clone(&c.updater.download_lock));
        let _guard = lock.lock().await;
        let (config, found, downloaded) = self.with_core(|c| {
            (
                c.updater.config.clone(),
                c.updater.found.clone(),
                c.updater.downloaded.clone(),
            )
        });
        let Some(config) = config else {
            return Err(self.updater_fail(Error::invalid_argument(
                "No update feed is configured; call autoUpdater.setFeedURL().",
            )));
        };
        let Some(found) = found else {
            return Err(self.updater_fail(Error::not_found(
                "No update is available; call checkForUpdates() first.",
            )));
        };
        if let Some(d) = downloaded
            && d.info.version == found.info.version
            && d.file.is_file()
        {
            return Ok(vec![d.file]);
        }
        match self.updater_fetch_file(&config, found.info).await {
            Ok(done) => {
                let file = done.file.clone();
                let wire = serde_json::to_value(&done.info).unwrap_or(Value::Null);
                self.with_core(|c| c.updater.downloaded = Some(done));
                self.updater_emit("update-downloaded", Some(("info", wire)));
                Ok(vec![file])
            }
            Err(err) => Err(self.updater_fail(err)),
        }
    }

    async fn updater_fetch_file(
        self: &Arc<Self>,
        config: &ResolvedConfig,
        mut info: UpdateInfo,
    ) -> Result<Downloaded, Error> {
        let os = self.info.os;
        let (file, kind) = choose_file(&info, os)
            .map(|(f, k)| (f.clone(), k))
            .ok_or_else(|| Error::backend("The update feed has no file for this platform."))?;
        if file.sha512.trim().is_empty() {
            return Err(Error::backend("The update feed entry has no SHA-512."));
        }
        let url = resolve_file_url(&config.url, &file.url)?;
        check_transport(&url, self.info.debug)
            .map_err(|_| Error::backend("The update file URL must use https."))?;
        let dir = self.updater_dir()?.join("pending");
        // Only one pending update is kept.
        remove_quietly(&dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| Error::from_io("Creating the update folder", &e))?;
        let name = download_file_name(url.as_str(), extension(kind));
        let part = dir.join(format!("{name}.part"));
        let target = dir.join(&name);
        let result = self
            .updater_stream(config, &url, file.size, &file.sha512, &part)
            .await;
        if let Err(err) = result {
            remove_quietly(&part);
            return Err(err);
        }
        let checked = self.updater_verify(config, &url, &part, kind).await;
        if let Err(err) = checked {
            remove_quietly(&part);
            return Err(err);
        }
        std::fs::rename(&part, &target).map_err(|e| {
            remove_quietly(&part);
            Error::from_io("Storing the update", &e)
        })?;
        let app = if kind == InstallerKind::MacZip && self.options.os_queries {
            match self.updater_unpack_mac(&target, &dir).await {
                Ok(app) => Some(app),
                Err(err) => {
                    remove_quietly(&dir);
                    return Err(err);
                }
            }
        } else {
            None
        };
        info.downloaded_file = Some(target.to_string_lossy().into_owned());
        Ok(Downloaded {
            info,
            file: target,
            kind,
            admin: file.is_admin_rights_required == Some(true),
            app,
        })
    }

    /// Streams `url` to `part`, checking size and SHA-512, with progress.
    async fn updater_stream(
        self: &Arc<Self>,
        config: &ResolvedConfig,
        url: &Url,
        size: Option<u64>,
        sha512: &str,
        part: &Path,
    ) -> Result<(), Error> {
        let client = http_client(self.info.debug)?;
        let mut response = get_ok(&client, config, url).await?;
        let total = size.or(response.content_length()).unwrap_or(0);
        if let (Some(want), Some(got)) = (size, response.content_length())
            && want != got
        {
            return Err(Error::backend(
                "The update file size does not match the feed.",
            ));
        }
        let cap = size.unwrap_or(MAX_UNSIZED_DOWNLOAD);
        let mut out = std::fs::File::create(part)
            .map_err(|e| Error::from_io("Creating the update file", &e))?;
        let mut hasher = Sha512::new();
        let mut transferred: u64 = 0;
        let started = Instant::now();
        let mut last_emit: Option<Instant> = None;
        let mut emitted: Option<u64> = None;
        while let Some(chunk) = response.chunk().await.map_err(|e| network(&e))? {
            transferred += chunk.len() as u64;
            if transferred > cap {
                return Err(Error::backend(
                    "The update file is larger than the feed states.",
                ));
            }
            hasher.update(&chunk);
            out.write_all(&chunk)
                .map_err(|e| Error::from_io("Writing the update file", &e))?;
            if last_emit.is_none_or(|t| t.elapsed() >= PROGRESS_INTERVAL) {
                last_emit = Some(Instant::now());
                emitted = Some(transferred);
                self.updater_progress(transferred, total.max(transferred), started);
            }
        }
        out.sync_all()
            .map_err(|e| Error::from_io("Writing the update file", &e))?;
        drop(out);
        if emitted != Some(transferred) {
            self.updater_progress(transferred, total.max(transferred), started);
        }
        if size.is_some_and(|s| s != transferred) {
            return Err(Error::backend(
                "The update file size does not match the feed.",
            ));
        }
        if !verify::sha512_matches(sha512, &hasher.finalize()) {
            return Err(Error::backend(
                "The update file's SHA-512 does not match the feed.",
            ));
        }
        Ok(())
    }

    fn updater_progress(self: &Arc<Self>, transferred: u64, total: u64, started: Instant) {
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let progress = ProgressInfo::new(transferred, total, elapsed);
        let wire = serde_json::to_value(progress).unwrap_or(Value::Null);
        self.updater_emit("download-progress", Some(("progress", wire)));
    }

    /// The detached signature and the OS publisher check (I.3).
    async fn updater_verify(
        self: &Arc<Self>,
        config: &ResolvedConfig,
        url: &Url,
        part: &Path,
        kind: InstallerKind,
    ) -> Result<(), Error> {
        if let Some(pubkey) = self.info.config.updater.pubkey.as_deref() {
            let key = verify::parse_public_key(pubkey)?;
            let client = http_client(self.info.debug)?;
            let response = get_ok(&client, config, &signature_url(url))
                .await
                .map_err(|_| Error::backend("The update signature could not be downloaded."))?;
            let sig = read_capped(response, MAX_SIGNATURE_BYTES)
                .await
                .map_err(|_| Error::backend("The update signature could not be downloaded."))?;
            let sig = String::from_utf8(sig)
                .map_err(|_| Error::backend("The update signature file does not parse."))?;
            let path = part.to_path_buf();
            tauri::async_runtime::spawn_blocking(move || {
                verify::verify_minisign(&key, &sig, &path)
            })
            .await
            .map_err(|_| Error::backend("The update signature check failed."))??;
        }
        if !self.options.os_queries {
            // Tauri's mock runtime: no OS signature tools.
            return Ok(());
        }
        match kind {
            InstallerKind::Nsis | InstallerKind::Msi => {
                #[cfg(windows)]
                let checked = self.updater_authenticode(part).await;
                #[cfg(not(windows))]
                let checked = Err(Error::backend(
                    "Windows installers can only be verified on Windows.",
                ));
                checked
            }
            InstallerKind::MacZip | InstallerKind::AppImage => Ok(()),
        }
    }

    #[cfg(windows)]
    async fn updater_authenticode(self: &Arc<Self>, file: &Path) -> Result<(), Error> {
        let file = file.to_path_buf();
        let names = self.info.config.updater.publisher_names.clone();
        let (decision, own_unsigned) = tauri::async_runtime::spawn_blocking(move || {
            let own = std::env::current_exe()
                .map_err(|e| Error::from_io("Finding the running app", &e))
                .and_then(|exe| authenticode(&exe))?;
            let installer = authenticode(&file)?;
            if !same_path(&installer.path, &file) {
                return Err(Error::backend("The signature check read another file."));
            }
            let decision = verify::decide_publisher(&own, &installer, names.as_deref());
            Ok((decision, !own.is_valid()))
        })
        .await
        .map_err(|_| Error::backend("The update signature check failed."))??;
        match decision {
            verify::PublisherDecision::Accept => Ok(()),
            verify::PublisherDecision::SkipUnsigned => {
                let first =
                    self.with_core(|c| !std::mem::replace(&mut c.updater.warned_unsigned, true));
                if first && own_unsigned {
                    self.log(
                        LogLevel::Warn,
                        "updater: the running app is not signed, so the installer's publisher is not checked; sign the app",
                    );
                }
                Ok(())
            }
            verify::PublisherDecision::Reject(why) => Err(Error::backend(why)),
        }
    }

    /// macOS: unpacks the zip and checks the bundle's code signature and
    /// team id against the running app (I.3, I.4).
    async fn updater_unpack_mac(
        self: &Arc<Self>,
        zip: &Path,
        dir: &Path,
    ) -> Result<PathBuf, Error> {
        let zip = zip.to_path_buf();
        let out = dir.join("unpacked");
        tauri::async_runtime::spawn_blocking(move || mac_unpack_and_check(&zip, &out))
            .await
            .map_err(|_| Error::backend("The update bundle check failed."))?
    }

    /// `updater_quit_and_install` (I.4).
    pub(crate) fn updater_quit_and_install(
        self: &Arc<Self>,
        silent: bool,
        force_run_after: bool,
    ) -> Result<(), Error> {
        let downloaded = self.with_core(|c| c.updater.downloaded.clone());
        let Some(downloaded) = downloaded.filter(|d| d.file.is_file()) else {
            return Err(self.updater_fail(Error::not_found(
                "No update has been downloaded; quitAndInstall has nothing to install.",
            )));
        };
        if downloaded.kind == InstallerKind::AppImage && self.info.config.updater.pubkey.is_none() {
            return Err(self.updater_fail(Error::unsupported(
                "AppImage updates need updater.pubkey; update the app manually.",
            )));
        }
        self.with_core(|c| {
            c.updater.request = Some(InstallRequest {
                silent,
                force_run_after,
            });
        });
        self.log(LogLevel::Info, "updater: quitting to install the update");
        self.begin_quit(0);
        Ok(())
    }

    /// Runs at exit, before the process ends (A.6 step 5, I.4): the install
    /// `quitAndInstall` asked for, or the `autoInstallOnAppQuit` one. A
    /// crash relaunch skips the automatic install.
    pub(crate) async fn updater_install_at_exit(self: &Arc<Self>) {
        let (request, downloaded, auto, crash_relaunch) = self.with_core(|c| {
            (
                c.updater.request.take(),
                c.updater.downloaded.clone(),
                c.updater
                    .config
                    .as_ref()
                    .is_some_and(|cfg| cfg.auto_install_on_app_quit),
                c.relaunch_args.is_some(),
            )
        });
        let Some(downloaded) = downloaded.filter(|d| d.file.is_file()) else {
            return;
        };
        let (silent, force_run_after, explicit) = match request {
            Some(r) => (r.silent, r.force_run_after, true),
            None if auto && !crash_relaunch => (true, false, false),
            None => return,
        };
        if downloaded.kind == InstallerKind::AppImage && self.info.config.updater.pubkey.is_none() {
            self.log(
                LogLevel::Warn,
                "updater: AppImage updates need updater.pubkey; not installing",
            );
            return;
        }
        let relaunch = explicit
            && matches!(
                downloaded.kind,
                InstallerKind::MacZip | InstallerKind::AppImage
            );
        if !self.options.os_queries {
            let record = json!({
                "file": downloaded.file.to_string_lossy(),
                "kind": format!("{:?}", downloaded.kind),
                "silent": silent,
                "forceRunAfter": force_run_after,
                "relaunch": relaunch,
                "args": install::windows_install(
                    downloaded.kind,
                    &downloaded.file,
                    silent,
                    force_run_after,
                    self.info.config.updater.installer_args.as_deref(),
                    downloaded.admin,
                ).args,
            });
            self.with_core(|c| c.updater.test_installs.push(record));
            return;
        }
        let host = Arc::clone(self);
        let job = downloaded.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            host.updater_run_install(&job, silent, force_run_after)
        })
        .await;
        match result {
            Ok(Ok(())) => {
                self.log(LogLevel::Info, "updater: the update installer started");
                if relaunch {
                    self.with_core(|c| c.relaunch_args = Some(Vec::new()));
                }
            }
            Ok(Err(err)) => self.log(
                LogLevel::Error,
                &format!("updater: the install failed: {}", err.message()),
            ),
            Err(_) => self.log(LogLevel::Error, "updater: the install failed"),
        }
    }

    fn updater_run_install(
        &self,
        job: &Downloaded,
        silent: bool,
        force_run_after: bool,
    ) -> Result<(), Error> {
        match job.kind {
            InstallerKind::Nsis | InstallerKind::Msi => {
                let plan = install::windows_install(
                    job.kind,
                    &job.file,
                    silent,
                    force_run_after,
                    self.info.config.updater.installer_args.as_deref(),
                    job.admin,
                );
                run_windows_install(&plan)
            }
            InstallerKind::MacZip => {
                let app = job
                    .app
                    .as_deref()
                    .ok_or_else(|| Error::backend("The update bundle was not unpacked."))?;
                let exe = std::env::current_exe()
                    .map_err(|e| Error::from_io("Finding the running app", &e))?;
                let bundle = install::bundle_of(&exe)
                    .ok_or_else(|| Error::unsupported("The running app is not an .app bundle."))?;
                let staged = install::sibling(&bundle, "ow-tauri-new");
                remove_quietly(&staged);
                run_tool(
                    std::process::Command::new("/usr/bin/ditto")
                        .arg(app)
                        .arg(&staged),
                    "Copying the update bundle",
                )?;
                install::swap_into_place(&staged, &bundle).inspect_err(|_| remove_quietly(&staged))
            }
            InstallerKind::AppImage => {
                let target = std::env::var_os("APPIMAGE")
                    .map(PathBuf::from)
                    .ok_or_else(|| Error::unsupported("The app is not running as an AppImage."))?;
                install::replace_file(&job.file, &target)
            }
        }
    }
}

fn run_tool(
    command: &mut std::process::Command,
    what: &str,
) -> Result<std::process::Output, Error> {
    let out = command
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| Error::from_io(what, &e))?;
    if out.status.success() {
        Ok(out)
    } else {
        Err(Error::backend(format!("{what} failed.")))
    }
}

/// Unpacks a macOS update zip with `ditto` and checks the bundle (I.3).
fn mac_unpack_and_check(zip: &Path, out: &Path) -> Result<PathBuf, Error> {
    remove_quietly(out);
    std::fs::create_dir_all(out).map_err(|e| Error::from_io("Creating the unpack folder", &e))?;
    run_tool(
        std::process::Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(zip)
            .arg(out),
        "Unpacking the update",
    )?;
    let app = install::find_app_bundle(out)?;
    run_tool(
        std::process::Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&app),
        "Checking the update's code signature",
    )
    .map_err(|_| Error::backend("The update bundle's code signature is not valid."))?;
    let team_of = |path: &Path| -> Result<Option<String>, Error> {
        let out = std::process::Command::new("/usr/bin/codesign")
            .args(["-dv", "--verbose=2"])
            .arg(path)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| Error::from_io("Reading a code signature", &e))?;
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(verify::team_identifier(&text))
    };
    let exe = std::env::current_exe().map_err(|e| Error::from_io("Finding the running app", &e))?;
    let running = install::bundle_of(&exe)
        .ok_or_else(|| Error::unsupported("The running app is not an .app bundle."))?;
    let own = team_of(&running)?.ok_or_else(|| {
        Error::backend("The running app has no team id, so updates cannot be verified.")
    })?;
    match team_of(&app)? {
        Some(team) if team == own => Ok(app),
        _ => Err(Error::backend(
            "The update bundle is signed by another team.",
        )),
    }
}

#[cfg(windows)]
fn same_path(reported: &str, file: &Path) -> bool {
    let norm = |s: &str| s.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    norm(reported) == norm(&file.to_string_lossy())
}

/// `Get-AuthenticodeSignature` through PowerShell, as electron-updater runs it.
#[cfg(windows)]
fn authenticode(file: &Path) -> Result<verify::Authenticode, Error> {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let literal = file.to_string_lossy().replace('\'', "''");
    let script = format!(
        "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; Get-AuthenticodeSignature -LiteralPath '{literal}' | ConvertTo-Json -Compress"
    );
    let out = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-InputFormat",
            "None",
            "-Command",
            &script,
        ])
        .env("PSModulePath", "")
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| Error::from_io("Starting the signature check", &e))?;
    verify::parse_authenticode(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(windows)]
fn run_windows_install(plan: &install::WindowsInstall) -> Result<(), Error> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    if !plan.elevate {
        std::process::Command::new(&plan.program)
            .args(&plan.args)
            .spawn()
            .map_err(|e| Error::from_io("Starting the update installer", &e))?;
        return Ok(());
    }
    let wide = |s: &std::ffi::OsStr| {
        s.encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let params = plan
        .args
        .iter()
        .map(|a| quote_windows_arg(a))
        .collect::<Vec<_>>()
        .join(" ");
    let verb = wide(std::ffi::OsStr::new("runas"));
    let file = wide(plan.program.as_os_str());
    let params = wide(std::ffi::OsStr::new(&params));
    // SAFETY: every pointer is a NUL-terminated UTF-16 buffer that outlives
    // the call; a null window handle and directory are allowed.
    let code = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(Error::io("The elevated update installer could not start."))
    }
}

#[cfg(not(windows))]
fn run_windows_install(_plan: &install::WindowsInstall) -> Result<(), Error> {
    Err(Error::unsupported(
        "Windows installers run on Windows only.",
    ))
}

/// Quotes one argument for a Windows command line (`CommandLineToArgvW`
/// rules).
#[cfg_attr(not(any(windows, test)), expect(dead_code, reason = "Windows only"))]
fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_quoting() {
        assert_eq!(quote_windows_arg("/S"), "/S");
        assert_eq!(quote_windows_arg(""), "\"\"");
        assert_eq!(
            quote_windows_arg("/D=C:\\Program Files\\A"),
            "\"/D=C:\\Program Files\\A\""
        );
        assert_eq!(quote_windows_arg("a \"b\""), "\"a \\\"b\\\"\"");
        assert_eq!(quote_windows_arg("x y\\"), "\"x y\\\\\"");
    }

    #[test]
    fn feed_and_signature_urls() {
        let config = UpdaterConfig {
            url: Some("https://feed.example.com/apps/abc".into()),
            channel: Some("beta".into()),
            ..Default::default()
        }
        .resolve(None, false)
        .unwrap();
        let url = feed_url(&config, TargetOs::Macos).unwrap();
        assert!(
            url.as_str()
                .starts_with("https://feed.example.com/apps/abc/beta-mac.yml?noCache=")
        );
        let file = Url::parse("https://cdn.example.com/1.0/setup.exe?t=1").unwrap();
        assert_eq!(
            signature_url(&file).as_str(),
            "https://cdn.example.com/1.0/setup.exe.sig?t=1"
        );
    }
}
