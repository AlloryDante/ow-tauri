//! The update engine (DESIGN §4.14, CONTRACT I.2 to I.4): settings, the
//! feed check, the verified download and the install, behind an OS layer
//! ([`UpdateOs`]) so the same code runs in the tests on every OS.
//!
//! Verification fails closed with `verification` (R5): the SHA-512 of the
//! feed entry, then the minisign signature (`updater.pubkey`) and the
//! Authenticode publisher (`updater.publisherNames`). The installer stays
//! open without write sharing from verification until NSIS starts, and is
//! verified again right before it starts (SEC-m3).

use std::fs::File;
use std::io::{Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use sha2::{Digest as _, Sha512};
use tauri::{AppHandle, Manager as _, Resource, ResourceId, Runtime};
use url::Url;

use super::client::{Http, HttpSettings, ProxyChoice, next_chunk, read_capped};
use super::feed::{MAX_FEED_BYTES, UpdateInfo, parse_feed};
use super::install::{WindowsInstall, download_file_name, nsis_command};
use super::verify::{self, Authenticode};
use super::{
    Availability, DevUpdateConfig, check_transport, choose_installer, feed_base, feed_url,
    is_update_available, is_uuid_text, overwolf_feed, parse_version, resolve_file_url,
    staging_bucket, valid_channel,
};
use crate::config::{ConfigError, UpdaterConfig};
use crate::error::{Error, Result};

/// The log target of the update client (DESIGN §4.16).
pub(crate) const LOG_TARGET: &str = "tauri_plugin_overwolf::updater";
/// The largest download without a stated size (4 GiB).
const MAX_UNSIZED_DOWNLOAD: u64 = 4 * 1024 * 1024 * 1024;
/// The largest detached signature file.
const MAX_SIGNATURE_BYTES: usize = 16 * 1024;
/// The message when neither `publisherNames` nor `pubkey` is set (R5).
const NO_PUBLISHER: &str = "set publisherNames (your installer's certificate subject) or pubkey before checking for updates";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the update client needs from the OS. Windows is the only OS with
/// an implementation (R6); tests use a fake.
pub(crate) trait UpdateOs: Send + Sync + 'static {
    /// `Ok` where the client runs; `unsupported` elsewhere (R6).
    fn supported(&self) -> Result<()>;
    /// Node's `os.release()` for `minimumSystemVersion`.
    fn os_release(&self) -> String;
    /// `Get-AuthenticodeSignature` of `file` (blocking). Any failure is a
    /// `verification` error.
    fn authenticode(&self, file: &Path) -> Result<Authenticode>;
    /// Starts the installer (blocking).
    fn launch(&self, plan: &WindowsInstall) -> Result<()>;
}

/// The builder's and JavaScript's choices over the configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Overrides {
    /// The channel.
    pub(crate) channel: Option<String>,
    /// Allow a lower version.
    pub(crate) allow_downgrade: Option<bool>,
    /// Allow prereleases.
    pub(crate) allow_prerelease: Option<bool>,
    /// Feed-origin request headers.
    pub(crate) headers: Vec<(String, String)>,
    /// The connect timeout.
    pub(crate) connect_timeout: Option<Duration>,
    /// The idle read timeout.
    pub(crate) read_timeout: Option<Duration>,
}

/// The effective settings of one updater.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent switches of the updater configuration"
)]
pub(crate) struct Settings {
    /// The feed base URL, with a trailing slash.
    pub(crate) feed: Url,
    /// The channel.
    pub(crate) channel: String,
    /// Offer a lower version.
    pub(crate) allow_downgrade: bool,
    /// Offer a prerelease.
    pub(crate) allow_prerelease: bool,
    /// Feed-origin request headers.
    pub(crate) headers: Vec<(String, String)>,
    /// The connect timeout.
    pub(crate) connect_timeout: Duration,
    /// The idle read timeout.
    pub(crate) read_timeout: Duration,
    /// Accepted Authenticode subjects.
    pub(crate) publisher_names: Vec<String>,
    /// The minisign public key.
    pub(crate) pubkey: Option<String>,
    /// Debug builds only: no publisher check, with a warning per check.
    pub(crate) skip_publisher_check: bool,
    /// `updater.installerArgs`.
    pub(crate) installer_args: Option<Vec<String>>,
    /// Install a downloaded update at exit.
    pub(crate) install_on_exit: bool,
    /// A debug build (loopback `http` feeds, the dev feed, the skip).
    pub(crate) debug: bool,
}

impl Settings {
    /// Resolves the settings: the configuration, then `overrides`; in a
    /// debug build the embedded `dev-app-update.yml` (`dev`) replaces the
    /// feed (and its channel, unless an override names one).
    ///
    /// # Errors
    ///
    /// `config` without `publisherNames` or `pubkey` (R5; a debug build
    /// may skip with `dangerousSkipPublisherCheck`), `invalid-argument` for
    /// a bad feed URL, channel or key.
    pub(crate) fn resolve(
        config: &UpdaterConfig,
        uid: &str,
        dev: Option<&str>,
        debug: bool,
        overrides: &Overrides,
    ) -> Result<Settings> {
        let dev = match dev.filter(|t| debug && !t.trim().is_empty()) {
            Some(text) => Some(DevUpdateConfig::parse(text)?),
            None => None,
        };
        if let Some(provider) = dev.as_ref().and_then(|d| d.provider.as_deref())
            && provider != "generic"
        {
            return Err(Error::invalid_argument(format!(
                "dev-app-update.yml: only the generic provider is supported (got {provider:?})."
            )));
        }
        let configured = dev
            .as_ref()
            .and_then(|d| d.url.as_deref())
            .or(config.endpoint.as_deref());
        let feed = match configured {
            Some(url) => Url::parse(url.trim())
                .map_err(|_| Error::invalid_argument("The update feed URL does not parse."))?,
            None => overwolf_feed(uid),
        };
        check_transport(&feed, debug)?;
        let channel = overrides
            .channel
            .clone()
            .or_else(|| dev.as_ref().and_then(|d| d.channel.clone()))
            .or_else(|| config.channel.clone())
            .unwrap_or_else(|| "latest".to_owned());
        if !valid_channel(&channel) {
            return Err(Error::invalid_argument(
                "The update channel must be 1 to 64 letters, digits, '.', '_' or '-'.",
            ));
        }
        let publisher_names: Vec<String> = config
            .publisher_names
            .iter()
            .flatten()
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty())
            .collect();
        let pubkey = config.pubkey.clone().filter(|k| !k.trim().is_empty());
        if let Some(key) = &pubkey {
            verify::parse_public_key(key)?;
        }
        let skip_publisher_check = debug && config.dangerous_skip_publisher_check;
        if publisher_names.is_empty() && pubkey.is_none() && !skip_publisher_check {
            return Err(ConfigError::new("updater", NO_PUBLISHER).into());
        }
        let millis = |ms: u64| Duration::from_millis(ms.max(1));
        Ok(Settings {
            feed: feed_base(feed),
            channel,
            allow_downgrade: overrides.allow_downgrade.unwrap_or(config.allow_downgrade),
            allow_prerelease: overrides
                .allow_prerelease
                .unwrap_or(config.allow_prerelease),
            headers: overrides.headers.clone(),
            connect_timeout: overrides
                .connect_timeout
                .unwrap_or_else(|| millis(config.connect_timeout_ms)),
            read_timeout: overrides
                .read_timeout
                .unwrap_or_else(|| millis(config.read_timeout_ms)),
            publisher_names,
            pubkey,
            skip_publisher_check,
            installer_args: config.installer_args.clone(),
            install_on_exit: config.install_on_exit,
            debug,
        })
    }
}

/// A downloaded installer, open without write sharing (Windows) until
/// NSIS starts.
#[derive(Debug)]
pub(crate) struct Staged {
    version: String,
    path: PathBuf,
    handle: File,
    sha512: String,
    signature: Option<String>,
    admin: bool,
}

/// The one place a downloaded installer lives; whoever takes it installs
/// it (`install()`, or the install at exit).
#[derive(Debug, Default)]
pub(crate) struct Slot(Mutex<Option<Staged>>);

impl Slot {
    fn take(&self) -> Option<Staged> {
        lock(&self.0).take()
    }
}

/// Per-app updater state (managed): the OS layer, the download lock and
/// the pending install at exit.
pub(crate) struct Shared {
    os: Arc<dyn UpdateOs>,
    download_lock: tokio::sync::Mutex<()>,
    pending: Mutex<Option<(Arc<Slot>, ResourceId)>>,
    warned_skip: AtomicBool,
    /// Test hooks.
    #[cfg(test)]
    pub(crate) test: TestHooks,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared").finish_non_exhaustive()
    }
}

/// What the tests change and observe.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct TestHooks {
    /// A fixed proxy for every request.
    pub(crate) proxy: Mutex<Option<String>>,
    /// `AppHandle::exit` calls (the mock runtime cannot exit).
    pub(crate) exits: std::sync::atomic::AtomicUsize,
}

impl Shared {
    /// State over `os`.
    pub(crate) fn new(os: Arc<dyn UpdateOs>) -> Self {
        Shared {
            os,
            download_lock: tokio::sync::Mutex::new(()),
            pending: Mutex::new(None),
            warned_skip: AtomicBool::new(false),
            #[cfg(test)]
            test: TestHooks::default(),
        }
    }
}

/// The managed wrapper of [`Shared`].
pub(crate) struct SharedState(pub(crate) Arc<Shared>);

/// The app's [`Shared`], created on first use with the system OS layer.
pub(crate) fn shared<R: Runtime>(app: &AppHandle<R>) -> Arc<Shared> {
    if let Some(s) = app.try_state::<SharedState>() {
        return Arc::clone(&s.0);
    }
    let fresh = Arc::new(Shared::new(super::os::system()));
    if app.manage(SharedState(Arc::clone(&fresh))) {
        fresh
    } else {
        Arc::clone(&app.state::<SharedState>().0)
    }
}

/// What the engine needs from the app, without its runtime type.
trait AppHooks: Send + Sync {
    /// Exits the app (`AppHandle::exit(0)`).
    fn exit(&self);
    /// Keeps `pending` in the app's resource table until the app exits.
    fn keep_until_exit(&self, pending: PendingInstall) -> ResourceId;
    /// Removes a kept resource (its slot must be empty first).
    fn release(&self, rid: ResourceId);
}

struct Hooks<R: Runtime>(AppHandle<R>);

impl<R: Runtime> AppHooks for Hooks<R> {
    fn exit(&self) {
        #[cfg(test)]
        if let Some(s) = self.0.try_state::<SharedState>() {
            s.0.test.exits.fetch_add(1, Ordering::SeqCst);
            return;
        }
        self.0.exit(0);
    }

    fn keep_until_exit(&self, pending: PendingInstall) -> ResourceId {
        self.0.resources_table().add(pending)
    }

    fn release(&self, rid: ResourceId) {
        let _ = self.0.resources_table().close(rid);
    }
}

/// One updater's context: its settings and the app's facts.
pub(crate) struct Ctx {
    pub(crate) settings: Settings,
    shared: Arc<Shared>,
    hooks: Arc<dyn AppHooks>,
    current_version: String,
    user_data_dir: PathBuf,
    download_dir: PathBuf,
    carried_staging_id: Option<String>,
    http: HttpSettings,
}

impl std::fmt::Debug for Ctx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ctx")
            .field("feed", &self.settings.feed.as_str())
            .field("channel", &self.settings.channel)
            .field("current_version", &self.current_version)
            .finish_non_exhaustive()
    }
}

impl Ctx {
    /// The context of an updater of `app` with `overrides`.
    ///
    /// # Errors
    ///
    /// `unsupported` off Windows (R6), `backend` without the plugin, and
    /// the errors of [`Settings::resolve`].
    pub(crate) fn new<R: Runtime>(app: &AppHandle<R>, overrides: &Overrides) -> Result<Ctx> {
        let shared = shared(app);
        shared.os.supported()?;
        let core = crate::host::core_of(app)
            .ok_or_else(|| Error::backend("tauri-plugin-overwolf is not set up"))?;
        let id = &core.identity;
        let settings = Settings::resolve(
            &id.config.updater,
            &id.app.uid,
            core.options.dev_update_config,
            cfg!(debug_assertions),
            overrides,
        )?;
        let base = if id.config.state.app_data_dir.is_some() {
            id.state_dir.root().join("updater")
        } else {
            app.path()
                .app_cache_dir()
                .map_err(|_| Error::backend("The app cache directory is unknown."))?
                .join("ow-tauri-updater")
        };
        #[cfg(test)]
        let proxy = match lock(&shared.test.proxy).clone() {
            Some(p) => ProxyChoice::Fixed(p),
            None => ProxyChoice::System,
        };
        #[cfg(not(test))]
        let proxy = ProxyChoice::System;
        let http = HttpSettings {
            connect_timeout: settings.connect_timeout,
            read_timeout: settings.read_timeout,
            debug: settings.debug,
            proxy,
        };
        Ok(Ctx {
            settings,
            hooks: Arc::new(Hooks(app.clone())),
            current_version: id.app.version.clone(),
            user_data_dir: crate::paths::user_data_dir(&id.app_data_dir, &id.app.name),
            download_dir: base.join("pending"),
            carried_staging_id: core.state.ow_tauri.get().staging_id,
            http,
            shared,
        })
    }

    /// The running version.
    pub(crate) fn current_version(&self) -> &str {
        &self.current_version
    }

    /// The staged-rollout bucket of this install (I.2 #5). The id lives
    /// where electron-updater keeps it, `<userData>/.updaterId`, so an
    /// install that ran the ow-electron build stays in its bucket. It is
    /// created on first use (only a feed with `stagingPercentage` asks); a
    /// `stagingId` an earlier ow-tauri stored in `ow-tauri.json` is carried
    /// over.
    fn staging_bucket(&self) -> Option<u8> {
        let file = self.user_data_dir.join(".updaterId");
        match std::fs::read_to_string(&file) {
            Ok(id) if is_uuid_text(&id) => return staging_bucket(&id),
            Ok(_) => log::warn!(
                target: LOG_TARGET,
                "the staging user id file exists, but its content is invalid"
            ),
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => log::warn!(
                target: LOG_TARGET,
                "could not read the staging user id, creating a new one: {}",
                e.kind()
            ),
            Err(_) => {}
        }
        let id = self
            .carried_staging_id
            .clone()
            .filter(|id| is_uuid_text(id))
            .unwrap_or_else(|| uuid::Uuid::new_v4().hyphenated().to_string());
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&file, &id));
        if let Err(err) = written {
            log::warn!(
                target: LOG_TARGET,
                "could not store the staging user id: {}",
                err.kind()
            );
        }
        staging_bucket(&id)
    }

    /// Takes any pending install (a newer download replaces it).
    fn drop_pending(&self) {
        let pending = lock(&self.shared.pending).take();
        if let Some((slot, rid)) = pending {
            drop(slot.take());
            self.hooks.release(rid);
        }
    }
}

/// An update the feed offers.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    /// The feed's release.
    pub(crate) info: UpdateInfo,
    /// The installer to download.
    pub(crate) target: Target,
    /// Whether the feed had a rollout share and this install is in it.
    pub(crate) staged: bool,
}

/// The installer of an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// The release version.
    pub(crate) version: String,
    /// Its URL.
    pub(crate) url: Url,
    /// The feed's SHA-512.
    pub(crate) sha512: String,
    /// The feed's size.
    pub(crate) size: Option<u64>,
    /// `IsAdminRightsRequired`.
    pub(crate) admin: bool,
}

/// Reads the feed (I.2): `None` when no update is offered.
///
/// # Errors
///
/// `unsupported` off Windows and for an `.msi` release, `network` for the
/// request, `invalid-argument` for a bad feed, `backend` for a release
/// without a usable installer entry.
pub(crate) async fn check(ctx: &Ctx) -> Result<Option<Found>> {
    ctx.shared.os.supported()?;
    if ctx.settings.skip_publisher_check {
        log::warn!(
            target: LOG_TARGET,
            "dangerousSkipPublisherCheck: the update installer's publisher is not checked (debug build)"
        );
    }
    let http = Http::new(&ctx.http)?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let url = feed_url(&ctx.settings.feed, &ctx.settings.channel, &token[..12])?;
    let response = http.get(&url, &ctx.settings.headers).await?;
    let body = read_capped(response, MAX_FEED_BYTES).await?;
    let text = String::from_utf8(body)
        .map_err(|_| Error::invalid_argument("The update feed is not UTF-8 text."))?;
    let info = parse_feed(&text)?;
    let current = parse_version(&ctx.current_version)?;
    let os_release = ctx.shared.os.os_release();
    let asked = AtomicBool::new(false);
    let availability = is_update_available(
        &current,
        &info,
        ctx.settings.allow_downgrade,
        ctx.settings.allow_prerelease,
        &os_release,
        || {
            asked.store(true, Ordering::Relaxed);
            ctx.staging_bucket()
        },
    )?;
    match availability {
        Availability::Available => {}
        Availability::NotInRollout => {
            log::info!(target: LOG_TARGET, "this install is outside the staged rollout");
            return Ok(None);
        }
        Availability::Unsupported => {
            log::info!(
                target: LOG_TARGET,
                "the OS version {os_release} is below the minimum OS version of version {}",
                info.version
            );
            return Ok(None);
        }
        Availability::Prerelease => {
            log::info!(
                target: LOG_TARGET,
                "version {} is a prerelease and prereleases are off",
                info.version
            );
            return Ok(None);
        }
        Availability::SameVersion | Availability::Older => return Ok(None),
    }
    let file = choose_installer(&info)?;
    let url = resolve_file_url(&ctx.settings.feed, &file.url)?;
    check_transport(&url, ctx.settings.debug)
        .map_err(|_| Error::backend("The update file URL must use https."))?;
    let target = Target {
        version: info.version.clone(),
        url,
        sha512: file.sha512.clone(),
        size: file.size,
        admin: file.is_admin_rights_required == Some(true),
    };
    Ok(Some(Found {
        staged: asked.load(Ordering::Relaxed),
        target,
        info,
    }))
}

fn remove_quietly(path: &Path) {
    let _ = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
}

fn signature_url(file: &Url) -> Url {
    let mut sig = file.clone();
    let path = format!("{}.sig", file.path());
    sig.set_path(&path);
    sig
}

/// Opens the installer for reading; on Windows without write or delete
/// sharing, so its bytes cannot change until the handle closes (SEC-m3).
fn open_locked(path: &Path) -> std::io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        /// `FILE_SHARE_READ`.
        const FILE_SHARE_READ: u32 = 1;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
    }
    #[cfg(not(windows))]
    {
        File::open(path)
    }
}

/// Downloads and verifies `target` (I.3). `on_chunk(length, total)` per
/// chunk, `on_finish()` once the bytes are in.
///
/// # Errors
///
/// `network`, `io` or `verification`; the file is deleted on any failure.
pub(crate) async fn download<C: FnMut(usize, Option<u64>), D: FnOnce()>(
    ctx: &Arc<Ctx>,
    target: &Target,
    mut on_chunk: C,
    on_finish: D,
) -> Result<Arc<Slot>> {
    ctx.shared.os.supported()?;
    let _guard = ctx.shared.download_lock.lock().await;
    ctx.drop_pending();
    let dir = &ctx.download_dir;
    // Only one pending update is kept.
    remove_quietly(dir);
    std::fs::create_dir_all(dir).map_err(|e| Error::from_io("Creating the update folder", &e))?;
    let name = download_file_name(target.url.as_str(), ".exe");
    // electron-updater's `temp-<name>`: the file keeps its extension, which
    // the Authenticode check needs.
    let part = dir.join(format!("temp-{name}"));
    let path = dir.join(&name);
    if let Err(err) = stream(ctx, target, &part, &mut on_chunk).await {
        remove_quietly(&part);
        return Err(err);
    }
    on_finish();
    let signature = match &ctx.settings.pubkey {
        Some(_) => match fetch_signature(ctx, &target.url).await {
            Ok(sig) => Some(sig),
            Err(err) => {
                remove_quietly(&part);
                return Err(err);
            }
        },
        None => None,
    };
    std::fs::rename(&part, &path).map_err(|e| {
        remove_quietly(&part);
        Error::from_io("Storing the update", &e)
    })?;
    let handle = open_locked(&path).map_err(|e| {
        remove_quietly(&path);
        Error::from_io("Opening the update", &e)
    })?;
    let mut staged = Staged {
        version: target.version.clone(),
        path,
        handle,
        sha512: target.sha512.clone(),
        signature,
        admin: target.admin,
    };
    let job = Arc::clone(ctx);
    let (checked, staged) = tauri::async_runtime::spawn_blocking(move || {
        let checked = verify_staged(&job, &mut staged);
        (checked, staged)
    })
    .await
    .map_err(|_| Error::verification("The update check stopped."))?;
    if let Err(err) = checked {
        let path = staged.path.clone();
        drop(staged);
        remove_quietly(&path);
        return Err(err);
    }
    let slot = Arc::new(Slot(Mutex::new(Some(staged))));
    if ctx.settings.install_on_exit {
        let rid = ctx.hooks.keep_until_exit(PendingInstall {
            slot: Arc::clone(&slot),
            ctx: Arc::clone(ctx),
        });
        *lock(&ctx.shared.pending) = Some((Arc::clone(&slot), rid));
    }
    Ok(slot)
}

async fn stream<C: FnMut(usize, Option<u64>)>(
    ctx: &Ctx,
    target: &Target,
    part: &Path,
    on_chunk: &mut C,
) -> Result<()> {
    let http = Http::new(&ctx.http)?;
    // Custom headers never go to the download host (SEC-M10).
    let mut response = http.get(&target.url, &[]).await?;
    let length = response.content_length();
    if let (Some(want), Some(got)) = (target.size, length)
        && want != got
    {
        return Err(Error::verification(
            "The update file size does not match the feed.",
        ));
    }
    let total = length.or(target.size);
    let cap = target.size.unwrap_or(MAX_UNSIZED_DOWNLOAD);
    let mut out =
        File::create(part).map_err(|e| Error::from_io("Creating the update file", &e))?;
    let mut hasher = Sha512::new();
    let mut transferred: u64 = 0;
    while let Some(chunk) = next_chunk(&mut response).await? {
        transferred += chunk.len() as u64;
        if transferred > cap {
            return Err(Error::verification(
                "The update file is larger than the feed states.",
            ));
        }
        hasher.update(&chunk);
        out.write_all(&chunk)
            .map_err(|e| Error::from_io("Writing the update file", &e))?;
        on_chunk(chunk.len(), total);
    }
    out.sync_all()
        .map_err(|e| Error::from_io("Writing the update file", &e))?;
    drop(out);
    if target.size.is_some_and(|s| s != transferred) {
        return Err(Error::verification(
            "The update file size does not match the feed.",
        ));
    }
    if !verify::sha512_matches(&target.sha512, &hasher.finalize()) {
        return Err(Error::verification(
            "The update file's SHA-512 does not match the feed.",
        ));
    }
    Ok(())
}

async fn fetch_signature(ctx: &Ctx, file: &Url) -> Result<String> {
    let failed = |_| Error::verification("The update signature could not be downloaded.");
    let http = Http::new(&ctx.http)?;
    let response = http.get(&signature_url(file), &[]).await.map_err(failed)?;
    let sig = read_capped(response, MAX_SIGNATURE_BYTES)
        .await
        .map_err(failed)?;
    String::from_utf8(sig).map_err(|_| Error::verification("The update signature file does not parse."))
}

/// Verifies the open installer (R5): SHA-512 from the handle, then the
/// minisign signature from the handle, then the Authenticode publisher.
/// Blocking.
fn verify_staged(ctx: &Ctx, staged: &mut Staged) -> Result<()> {
    let rewind = |f: &mut File| {
        f.seek(SeekFrom::Start(0))
            .map(drop)
            .map_err(|e| Error::from_io("Reading the update file", &e))
    };
    rewind(&mut staged.handle)?;
    let digest = verify::sha512_reader(&mut staged.handle)?;
    if !verify::sha512_matches(&staged.sha512, &digest) {
        return Err(Error::verification(
            "The update file's SHA-512 does not match the feed.",
        ));
    }
    let mut checked = false;
    if let Some(key) = &ctx.settings.pubkey {
        let sig = staged
            .signature
            .as_deref()
            .ok_or_else(|| Error::verification("The update signature is missing."))?;
        let key = verify::parse_public_key(key)?;
        rewind(&mut staged.handle)?;
        verify::verify_minisign_reader(&key, sig, &mut staged.handle)?;
        checked = true;
    }
    if !ctx.settings.publisher_names.is_empty() {
        let report = ctx.shared.os.authenticode(&staged.path)?;
        verify::check_publisher(&report, &staged.path, &ctx.settings.publisher_names)?;
        checked = true;
    }
    if !checked {
        if !ctx.settings.skip_publisher_check {
            return Err(Error::verification(NO_PUBLISHER));
        }
        if !ctx.shared.warned_skip.swap(true, Ordering::SeqCst) {
            log::warn!(
                target: LOG_TARGET,
                "dangerousSkipPublisherCheck: installing an update whose publisher is not checked (debug build)"
            );
        }
    }
    Ok(())
}

/// Verifies `staged` again and starts NSIS (SEC-m3). The handle closes
/// after the installer started; on a failed check the file is deleted and
/// nothing starts.
fn launch(ctx: &Ctx, mut staged: Staged, silent: bool) -> Result<()> {
    if let Err(err) = verify_staged(ctx, &mut staged) {
        let path = staged.path.clone();
        drop(staged);
        remove_quietly(&path);
        return Err(err);
    }
    let plan = nsis_command(
        &staged.path,
        ctx.settings.installer_args.as_deref(),
        silent,
        staged.admin,
    );
    let started = ctx.shared.os.launch(&plan);
    drop(staged);
    started
}

/// `Update::install` (I.4): re-verifies, starts `setup.exe /UPDATE /R`
/// (or `updater.installerArgs` with `/UPDATE`) and exits the app.
///
/// # Errors
///
/// `not-found` when the download was installed or replaced, and the
/// errors of the verification and the start.
pub(crate) fn install(ctx: &Ctx, slot: &Slot, version: &str) -> Result<()> {
    ctx.shared.os.supported()?;
    let staged = slot.take().ok_or_else(|| {
        Error::not_found("The update was already installed or replaced by a newer download.")
    })?;
    if staged.version != version {
        return Err(Error::invalid_argument(
            "The downloaded update belongs to another version.",
        ));
    }
    {
        let mut pending = lock(&ctx.shared.pending);
        if let Some((_, rid)) = pending.take() {
            ctx.hooks.release(rid);
        }
    }
    launch(ctx, staged, false)?;
    log::info!(target: LOG_TARGET, "the update installer started; exiting");
    ctx.hooks.exit();
    Ok(())
}

/// The install at exit (`updater.installOnExit`, electron-updater's
/// `autoInstallOnAppQuit`): a resource in the app's table, dropped when
/// Tauri clears the table on exit (after the plugin's exit sentinel, which
/// was added first). An `AppHandle::restart()` clears the table too, so a
/// pending update also installs then, as electron-updater installs on any
/// quit.
pub(crate) struct PendingInstall {
    slot: Arc<Slot>,
    ctx: Arc<Ctx>,
}

impl Resource for PendingInstall {}

impl Drop for PendingInstall {
    fn drop(&mut self) {
        let Some(staged) = self.slot.take() else {
            return;
        };
        match launch(&self.ctx, staged, true) {
            Ok(()) => log::info!(target: LOG_TARGET, "the update installer started at exit"),
            Err(err) => log::error!(target: LOG_TARGET, "the update was not installed at exit: {err}"),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
