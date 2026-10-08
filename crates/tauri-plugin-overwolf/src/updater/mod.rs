//! The electron-updater compatible update client (CONTRACT A.2.8, I;
//! [ADR 0008](https://github.com/ow-tauri/ow-tauri/blob/main/docs/adr/0008-updater-client.md)).
//!
//! Overwolf distributes app updates through an electron-updater **generic
//! provider** feed per app. This module reads that feed and does what
//! electron-updater does with it:
//!
//! - [`feed`]: `<channel>.yml` (`-mac.yml`, `-linux.yml`) parsing, with the
//!   field spellings of Overwolf's feed (`IsAdminRightsRequired`);
//! - [`is_update_available`], [`staging_bucket`], [`os_supports`]: the
//!   version, `minimumSystemVersion` and staged rollout rules;
//! - [`verify`]: size, SHA-512, the detached minisign signature and the
//!   publisher-name rule for Windows Authenticode subjects;
//! - [`install`]: the per-OS install command (NSIS, MSI, `.app`, `AppImage`).
//!
//! The Rust API (`UpdaterBuilder`, `Updater`, `Update`,
//! `DownloadedUpdate`, DESIGN §3.3) is Windows only (R6).
//!
//! ```
//! use tauri_plugin_overwolf::updater::{feed, is_update_available, Availability};
//! let info = feed::parse_feed("version: 1.2.0\nfiles:\n  - url: setup.exe\n    sha512: abc\n    size: 3\n").unwrap();
//! let current = semver::Version::parse("1.1.0").unwrap();
//! assert_eq!(is_update_available(&current, &info, false, "10.0.22631", || Some(10)).unwrap(), Availability::Available);
//! ```

pub mod feed;
pub mod install;
pub mod verify;

#[cfg(windows)]
mod api;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::Error;
use crate::paths::TargetOs;

pub use feed::{UpdateFileInfo, UpdateInfo};

#[cfg(windows)]
pub use api::{DownloadedUpdate, Update, Updater, UpdaterBuilder};

/// `UpdaterConfig` (A.2.8, I.1): what `autoUpdater.setFeedURL()` and the
/// electron-updater properties send to `updater_configure`.
///
/// ```
/// use tauri_plugin_overwolf::updater::UpdaterConfig;
/// let c: UpdaterConfig = serde_json::from_value(serde_json::json!({
///     "provider": "generic",
///     "url": "https://electron-updates.overwolf.com/electron-updates/electron/abc",
///     "channel": "beta"
/// })).unwrap();
/// assert_eq!(c.channel.as_deref(), Some("beta"));
/// assert!(c.auto_download.is_none());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterConfig {
    /// `'generic'`, the only supported provider.
    #[serde(default)]
    pub provider: Option<String>,
    /// The feed URL: `https`, or `http` for localhost in debug builds.
    #[serde(default)]
    pub url: Option<String>,
    /// The feed channel; `latest` when absent.
    #[serde(default)]
    pub channel: Option<String>,
    /// Offer an older version than the running one.
    #[serde(default)]
    pub allow_downgrade: Option<bool>,
    /// electron-updater's `allowPrerelease`. Stored only: the generic
    /// provider never reads it, so prereleases are offered like any other
    /// version (I.2 #4).
    #[serde(default)]
    pub allow_prerelease: Option<bool>,
    /// Download a found update at once (electron-updater default `true`).
    #[serde(default)]
    pub auto_download: Option<bool>,
    /// Install a downloaded update when the app quits (default `true`).
    #[serde(default)]
    pub auto_install_on_app_quit: Option<bool>,
    /// electron-updater's `autoRunAppAfterInstall` (default `true`): a
    /// non-silent `quitAndInstall` starts the app after the install.
    #[serde(default)]
    pub auto_run_app_after_install: Option<bool>,
    /// Read `provider` and `url` from the embedded `dev-app-update.yml`
    /// (debug builds only); without an embedded copy, `url` applies.
    #[serde(default)]
    pub force_dev_update_config: Option<bool>,
    /// Extra request headers for the feed and the download.
    #[serde(default)]
    pub request_headers: Option<BTreeMap<String, String>>,
}

/// The effective updater settings after [`UpdaterConfig::resolve`].
#[expect(
    clippy::struct_excessive_bools,
    reason = "the independent electron-updater switches, each read on its own"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConfig {
    /// The feed base URL, with a trailing slash.
    pub url: Url,
    /// The channel (`latest` by default).
    pub channel: String,
    /// See [`UpdaterConfig::allow_downgrade`].
    pub allow_downgrade: bool,
    /// See [`UpdaterConfig::allow_prerelease`].
    pub allow_prerelease: bool,
    /// See [`UpdaterConfig::auto_download`].
    pub auto_download: bool,
    /// See [`UpdaterConfig::auto_install_on_app_quit`].
    pub auto_install_on_app_quit: bool,
    /// See [`UpdaterConfig::auto_run_app_after_install`].
    pub auto_run_app_after_install: bool,
    /// Whether the dev configuration replaced `provider` and `url`.
    pub force_dev_update_config: bool,
    /// See [`UpdaterConfig::request_headers`].
    pub request_headers: BTreeMap<String, String>,
}

/// A `dev-app-update.yml` (I.1 `forceDevUpdateConfig`): its `provider`
/// and `url`.
///
/// ```
/// use tauri_plugin_overwolf::updater::DevUpdateConfig;
/// let d = DevUpdateConfig::parse("provider: generic\nurl: http://localhost:8080/updates\n").unwrap();
/// assert_eq!(d.url.as_deref(), Some("http://localhost:8080/updates"));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DevUpdateConfig {
    /// `provider`.
    pub provider: Option<String>,
    /// `url`.
    pub url: Option<String>,
    /// `channel`.
    pub channel: Option<String>,
}

impl DevUpdateConfig {
    /// Parses a `dev-app-update.yml`.
    ///
    /// # Errors
    ///
    /// `invalid-argument` when the text is not a YAML mapping.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let value = feed::yaml_to_json(text)?;
        let get = |k: &str| {
            value
                .get(k)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        Ok(DevUpdateConfig {
            provider: get("provider"),
            url: get("url"),
            channel: get("channel"),
        })
    }
}

/// Whether `url` is a loopback HTTP URL (`localhost`, `127.0.0.1`, `[::1]`).
#[must_use]
pub fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// The transport rule of I.3: `https`, or `http` on a loopback host in debug
/// builds; never credentials in the URL.
///
/// # Errors
///
/// `invalid-argument` naming the rule the URL breaks.
///
/// ```
/// use tauri_plugin_overwolf::updater::check_transport;
/// let u = |s| url::Url::parse(s).unwrap();
/// assert!(check_transport(&u("https://example.com/feed/"), false).is_ok());
/// assert!(check_transport(&u("http://localhost:8080/"), true).is_ok());
/// assert!(check_transport(&u("http://localhost:8080/"), false).is_err());
/// assert!(check_transport(&u("http://example.com/"), true).is_err());
/// assert!(check_transport(&u("https://user:pw@example.com/"), false).is_err());
/// ```
pub fn check_transport(url: &Url, debug: bool) -> Result<(), Error> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::invalid_argument(
            "Update URLs may not carry credentials.",
        ));
    }
    match url.scheme() {
        "https" => Ok(()),
        "http" if debug && is_loopback(url) => Ok(()),
        _ => Err(Error::invalid_argument(
            "Update URLs must use https (http only for localhost in debug builds).",
        )),
    }
}

/// A channel name: 1 to 64 characters of `[A-Za-z0-9._-]`, so it can name
/// a file on the feed and nothing else.
#[must_use]
pub fn valid_channel(channel: &str) -> bool {
    !channel.is_empty()
        && channel.len() <= 64
        && channel
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !channel.starts_with('.')
}

impl UpdaterConfig {
    /// Validates the configuration and fills the electron-updater defaults.
    /// `dev` is the embedded `dev-app-update.yml`, used when
    /// `forceDevUpdateConfig` is set in a debug build.
    ///
    /// # Errors
    ///
    /// `invalid-argument` for a provider other than `generic`, a missing or
    /// unacceptable URL, a bad channel or a header that cannot be sent.
    ///
    /// ```
    /// use tauri_plugin_overwolf::updater::UpdaterConfig;
    /// let c = UpdaterConfig {
    ///     provider: Some("generic".into()),
    ///     url: Some("https://updates.example.com/app".into()),
    ///     ..Default::default()
    /// };
    /// let r = c.resolve(None, false).unwrap();
    /// assert_eq!(r.url.as_str(), "https://updates.example.com/app/");
    /// assert_eq!(r.channel, "latest");
    /// assert!(r.auto_download && r.auto_install_on_app_quit && !r.allow_downgrade);
    /// ```
    pub fn resolve(
        &self,
        dev: Option<&DevUpdateConfig>,
        debug: bool,
    ) -> Result<ResolvedConfig, Error> {
        let force_dev = self.force_dev_update_config.unwrap_or(false) && debug;
        let (provider, url, dev_channel) = match (force_dev, dev) {
            (true, Some(d)) => (d.provider.clone(), d.url.clone(), d.channel.clone()),
            // No embedded copy: the feed from `setFeedURL` still applies, as
            // in electron-updater, where `forceDevUpdateConfig` only lets an
            // unpackaged app check and `setFeedURL` replaces the file.
            (true, None) if self.url.is_some() => (self.provider.clone(), self.url.clone(), None),
            (true, None) => {
                return Err(Error::invalid_argument(
                    "forceDevUpdateConfig is set but no dev-app-update.yml is embedded.",
                ));
            }
            (false, _) => (self.provider.clone(), self.url.clone(), None),
        };
        match provider.as_deref() {
            None | Some("generic") => {}
            Some(other) => {
                return Err(Error::invalid_argument(format!(
                    "Only the generic update provider is supported (got {other:?})."
                )));
            }
        }
        let url = url.ok_or_else(|| Error::invalid_argument("The update feed URL is missing."))?;
        let mut url = Url::parse(url.trim())
            .map_err(|_| Error::invalid_argument("The update feed URL does not parse."))?;
        check_transport(&url, debug)?;
        url.set_query(None);
        url.set_fragment(None);
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        let channel = self
            .channel
            .clone()
            .or(dev_channel)
            .unwrap_or_else(|| "latest".to_owned());
        if !valid_channel(&channel) {
            return Err(Error::invalid_argument(
                "The update channel must be 1 to 64 letters, digits, '.', '_' or '-'.",
            ));
        }
        let request_headers = self.request_headers.clone().unwrap_or_default();
        for (name, value) in &request_headers {
            let ok_name = !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
            let ok_value = value
                .bytes()
                .all(|b| b == b'\t' || (0x20..0x7f).contains(&b));
            if !ok_name || !ok_value {
                return Err(Error::invalid_argument(format!(
                    "The update request header {name:?} is invalid."
                )));
            }
        }
        Ok(ResolvedConfig {
            url,
            channel,
            allow_downgrade: self.allow_downgrade.unwrap_or(false),
            allow_prerelease: self.allow_prerelease.unwrap_or(false),
            auto_download: self.auto_download.unwrap_or(true),
            auto_install_on_app_quit: self.auto_install_on_app_quit.unwrap_or(true),
            auto_run_app_after_install: self.auto_run_app_after_install.unwrap_or(true),
            force_dev_update_config: force_dev,
            request_headers,
        })
    }
}

/// The feed file for `channel` on `os` (I.2 #1): `<channel>.yml` on Windows,
/// `<channel>-mac.yml` on macOS, `<channel>-linux.yml` on Linux.
///
/// ```
/// use tauri_plugin_overwolf::paths::TargetOs;
/// use tauri_plugin_overwolf::updater::feed_file_name;
/// assert_eq!(feed_file_name("latest", TargetOs::Windows), "latest.yml");
/// assert_eq!(feed_file_name("beta", TargetOs::Macos), "beta-mac.yml");
/// assert_eq!(feed_file_name("latest", TargetOs::Linux), "latest-linux.yml");
/// ```
#[must_use]
pub fn feed_file_name(channel: &str, os: TargetOs) -> String {
    match os {
        TargetOs::Windows => format!("{channel}.yml"),
        TargetOs::Macos => format!("{channel}-mac.yml"),
        TargetOs::Linux => format!("{channel}-linux.yml"),
    }
}

/// The staged-rollout bucket (0 to 100) of a `stagingId` (I.2 #5). It is
/// electron-updater's rule: the 32-bit big-endian number at bytes 12 to 15
/// of the UUID, as a share of `0xFFFFFFFF`, so the same id lands in the same
/// bucket in both clients. A client is in a rollout of `p` percent when its
/// bucket is below `p` (electron-updater: `value / 0xFFFFFFFF < p / 100`).
/// `None` when `id` is not a UUID in its 36-character text form
/// (electron-updater's `UUID.check`).
///
/// ```
/// use tauri_plugin_overwolf::updater::staging_bucket;
/// assert_eq!(staging_bucket("00000000-0000-4000-8000-000000000000"), Some(0));
/// assert_eq!(staging_bucket("00000000-0000-4000-8000-0000fffffffe"), Some(99));
/// assert_eq!(staging_bucket("00000000-0000-4000-8000-0000ffffffff"), Some(100));
/// assert_eq!(staging_bucket("00000000-0000-4000-8000-00007fffffff"), Some(49));
/// assert_eq!(staging_bucket("not a uuid"), None);
/// ```
#[must_use]
pub fn staging_bucket(id: &str) -> Option<u8> {
    if !is_uuid_text(id) {
        return None;
    }
    let value = u32::from_str_radix(&id[28..36], 16).ok()?;
    // floor(value / 0xFFFFFFFF * 100): `floor(x) < p` is `x < p` for an
    // integer `p`, so the bucket test equals electron-updater's.
    let bucket = (u64::from(value) * 100) / u64::from(u32::MAX);
    u8::try_from(bucket).ok()
}

/// Whether `id` is a UUID in electron-updater's accepted text form:
/// `^[a-f0-9]{8}(-[a-f0-9]{4}){3}-[a-f0-9]{12}$`, any case.
///
/// ```
/// use tauri_plugin_overwolf::updater::is_uuid_text;
/// assert!(is_uuid_text("0F2A1B3C-0000-4000-8000-000000000000"));
/// assert!(!is_uuid_text("0f2a1b3c-0000-4000-8000-000000000000\n"));
/// assert!(!is_uuid_text("0f2a1b3c00004000800000000000000000"));
/// ```
#[must_use]
pub fn is_uuid_text(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// The `stagingPercentage` of a feed as electron-updater reads it: the
/// integer part of the value (`parseInt`), or `None` when it is absent or
/// not a number (then every client is in the rollout).
///
/// ```
/// use tauri_plugin_overwolf::updater::staging_percentage;
/// assert_eq!(staging_percentage(Some(&serde_json::json!(50))), Some(50));
/// assert_eq!(staging_percentage(Some(&serde_json::json!("25.9"))), Some(25));
/// assert_eq!(staging_percentage(Some(&serde_json::json!("soon"))), None);
/// assert_eq!(staging_percentage(None), None);
/// ```
#[must_use]
pub fn staging_percentage(value: Option<&serde_json::Value>) -> Option<i64> {
    let text = match value? {
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.trim().to_owned(),
        _ => return None,
    };
    let digits: String = text
        .char_indices()
        .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+')))
        .map(|(_, c)| c)
        .collect();
    digits.parse().ok()
}

/// The result of comparing the feed with the running version (I.2 #4, #5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// An update the client may offer.
    Available,
    /// The feed version is the running one (build metadata ignored).
    SameVersion,
    /// The feed version is older and downgrades are off.
    Older,
    /// The feed's `minimumSystemVersion` is above this OS release.
    Unsupported,
    /// This client is outside the staged rollout.
    NotInRollout,
}

/// Parses a version as electron-updater does (`semver.parse`, a leading `v`
/// allowed).
///
/// # Errors
///
/// `invalid-argument` when `text` is not a semantic version.
pub fn parse_version(text: &str) -> Result<semver::Version, Error> {
    let t = text.trim();
    semver::Version::parse(t.strip_prefix('v').unwrap_or(t)).map_err(|_| {
        Error::invalid_argument(format!(
            "The update feed has an invalid version ({text:?})."
        ))
    })
}

/// electron-updater's default `isUpdateSupported`: `false` when the feed's
/// `minimumSystemVersion` (a string) is above `os_release` (Node's
/// `os.release()`), `true` otherwise. `None` when either does not parse as
/// a semantic version; electron-updater then logs a warning and treats the
/// update as supported.
///
/// ```
/// use tauri_plugin_overwolf::updater::os_supports;
/// let min = serde_json::json!("10.0.22000");
/// assert_eq!(os_supports(Some(&min), "10.0.19045"), Some(false));
/// assert_eq!(os_supports(Some(&min), "10.0.22631"), Some(true));
/// assert_eq!(os_supports(Some(&min), "unknown"), None);
/// assert_eq!(os_supports(None, "10.0.19045"), Some(true));
/// ```
#[must_use]
pub fn os_supports(minimum: Option<&serde_json::Value>, os_release: &str) -> Option<bool> {
    let Some(minimum) = minimum else {
        return Some(true);
    };
    let truthy = match minimum {
        serde_json::Value::Null | serde_json::Value::Bool(false) => false,
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Number(n) => n.as_f64() != Some(0.0),
        _ => true,
    };
    if !truthy {
        return Some(true);
    }
    // `semver.lt` throws on anything but a version string.
    let minimum = parse_version(minimum.as_str()?).ok()?;
    let current = parse_version(os_release).ok()?;
    Some(current.cmp_precedence(&minimum) != std::cmp::Ordering::Less)
}

/// electron-updater 6's `isUpdateAvailable` (I.2 #4, #5), in its order:
///
/// 1. an equal version (`semver.eq`, build metadata ignored) is never an
///    update;
/// 2. `minimumSystemVersion` above `os_release` is not supported;
/// 3. the staged rollout: `bucket` is called only when the feed has a
///    numeric `stagingPercentage`, so the staging id is created lazily, as
///    electron-updater does;
/// 4. newer (`semver.gt`), or older (`semver.lt`) with `allow_downgrade`.
///
/// Prereleases are not filtered: the generic provider never reads
/// `allowPrerelease`.
///
/// # Errors
///
/// `invalid-argument` when the feed version is not a semantic version.
///
/// ```
/// use tauri_plugin_overwolf::updater::{feed::UpdateInfo, is_update_available, Availability};
/// let v = |s: &str| semver::Version::parse(s).unwrap();
/// let mut info = UpdateInfo { version: "2.0.0".into(), ..Default::default() };
/// let check = |cur: &str, info: &UpdateInfo, down: bool, bucket: u8| {
///     is_update_available(&v(cur), info, down, "10.0.22631", || Some(bucket)).unwrap()
/// };
/// assert_eq!(check("1.0.0", &info, false, 5), Availability::Available);
/// assert_eq!(check("2.0.0", &info, false, 5), Availability::SameVersion);
/// assert_eq!(check("3.0.0", &info, false, 5), Availability::Older);
/// assert_eq!(check("3.0.0", &info, true, 5), Availability::Available);
/// // Build metadata is ignored, as node-semver ignores it.
/// info.version = "2.0.0+20261001".into();
/// assert_eq!(check("2.0.0+20260901", &info, true, 5), Availability::SameVersion);
/// info.staging_percentage = Some(serde_json::json!(5));
/// assert_eq!(check("1.0.0", &info, false, 5), Availability::NotInRollout);
/// assert_eq!(check("1.0.0", &info, false, 4), Availability::Available);
/// ```
pub fn is_update_available(
    current: &semver::Version,
    info: &UpdateInfo,
    allow_downgrade: bool,
    os_release: &str,
    bucket: impl FnOnce() -> Option<u8>,
) -> Result<Availability, Error> {
    use std::cmp::Ordering;
    let latest = parse_version(&info.version)?;
    let order = latest.cmp_precedence(current);
    if order == Ordering::Equal {
        return Ok(Availability::SameVersion);
    }
    if os_supports(info.minimum_system_version.as_ref(), os_release) == Some(false) {
        return Ok(Availability::Unsupported);
    }
    if let Some(pct) = staging_percentage(info.staging_percentage.as_ref())
        && let Some(bucket) = bucket()
        && i64::from(bucket) >= pct
    {
        return Ok(Availability::NotInRollout);
    }
    match order {
        Ordering::Greater => Ok(Availability::Available),
        _ if allow_downgrade => Ok(Availability::Available),
        _ => Ok(Availability::Older),
    }
}

/// The installer kinds of I.2 #6 / I.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallerKind {
    /// Windows NSIS `setup.exe`.
    Nsis,
    /// Windows `.msi`.
    Msi,
    /// macOS `.zip` holding the `.app`.
    MacZip,
    /// Linux `.AppImage`.
    AppImage,
}

fn path_ends_with(url: &str, ext: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or_default();
    path.len() >= ext.len()
        && path.is_char_boundary(path.len() - ext.len())
        && path[path.len() - ext.len()..].eq_ignore_ascii_case(ext)
}

/// The file to download on `os` (I.2 #6): Windows `.exe` (NSIS) else
/// `.msi`; macOS `.zip`; Linux `.AppImage`.
///
/// ```
/// use tauri_plugin_overwolf::paths::TargetOs;
/// use tauri_plugin_overwolf::updater::{choose_file, feed::{UpdateFileInfo, UpdateInfo}, InstallerKind};
/// let file = |u: &str| UpdateFileInfo { url: u.into(), sha512: "x".into(), ..Default::default() };
/// let info = UpdateInfo { version: "1.0.0".into(), files: vec![file("App.msi"), file("setup.EXE")], ..Default::default() };
/// let (f, kind) = choose_file(&info, TargetOs::Windows).unwrap();
/// assert_eq!((f.url.as_str(), kind), ("setup.EXE", InstallerKind::Nsis));
/// assert!(choose_file(&info, TargetOs::Macos).is_none());
/// ```
#[must_use]
pub fn choose_file(info: &UpdateInfo, os: TargetOs) -> Option<(&UpdateFileInfo, InstallerKind)> {
    let find = |ext: &str| info.files.iter().find(|f| path_ends_with(&f.url, ext));
    match os {
        TargetOs::Windows => find(".exe")
            .map(|f| (f, InstallerKind::Nsis))
            .or_else(|| find(".msi").map(|f| (f, InstallerKind::Msi))),
        TargetOs::Macos => find(".zip").map(|f| (f, InstallerKind::MacZip)),
        TargetOs::Linux => find(".appimage").map(|f| (f, InstallerKind::AppImage)),
    }
}

/// Resolves a feed file `url` against the feed base (I.2 #6), as
/// electron-updater's `new URL(file, baseUrl)`.
///
/// # Errors
///
/// `invalid-argument` when the result does not parse.
///
/// ```
/// use tauri_plugin_overwolf::updater::resolve_file_url;
/// let base = url::Url::parse("https://feed.example.com/apps/abc/").unwrap();
/// assert_eq!(resolve_file_url(&base, "setup.exe").unwrap().as_str(), "https://feed.example.com/apps/abc/setup.exe");
/// assert_eq!(
///     resolve_file_url(&base, "https://cdn.example.com/1.0/setup.exe").unwrap().as_str(),
///     "https://cdn.example.com/1.0/setup.exe"
/// );
/// ```
pub fn resolve_file_url(base: &Url, file: &str) -> Result<Url, Error> {
    base.join(file)
        .map_err(|_| Error::invalid_argument("An update file URL does not parse."))
}

/// `UpdateCheckResult` (I.5): what `updater_check` returns.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    /// The feed's update info.
    pub update_info: UpdateInfo,
    /// The same value (electron-updater keeps it for compatibility).
    pub version_info: UpdateInfo,
    /// Whether the client offers the update (I.2 #4, #5).
    pub is_update_available: bool,
}

/// `ProgressInfo` of `download-progress` (I.3), with electron-updater's
/// fields in its order: `{ total, delta, transferred, percent,
/// bytesPerSecond }`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressInfo {
    /// Bytes to download.
    pub total: u64,
    /// Bytes downloaded since the previous `download-progress`.
    pub delta: u64,
    /// Bytes downloaded so far.
    pub transferred: u64,
    /// `transferred / total * 100`.
    pub percent: f64,
    /// Average speed since the download started, rounded.
    pub bytes_per_second: u64,
}

impl ProgressInfo {
    /// The progress after `transferred` of `total` bytes in `elapsed_ms`,
    /// `delta` of them since the previous event.
    ///
    /// ```
    /// use tauri_plugin_overwolf::updater::ProgressInfo;
    /// let p = ProgressInfo::new(50, 20, 200, 1000);
    /// assert_eq!((p.percent, p.bytes_per_second, p.delta), (25.0, 50, 20));
    /// assert_eq!(ProgressInfo::new(5, 5, 10, 3000).bytes_per_second, 2);
    /// ```
    #[must_use]
    pub fn new(transferred: u64, delta: u64, total: u64, elapsed_ms: u64) -> Self {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a percentage of byte counts below 2^52"
        )]
        let percent = if total == 0 {
            0.0
        } else {
            transferred as f64 * 100.0 / total as f64
        };
        let elapsed = elapsed_ms.max(1);
        ProgressInfo {
            total,
            delta,
            transferred,
            percent,
            // Math.round(transferred / seconds).
            bytes_per_second: transferred.saturating_mul(1000).saturating_add(elapsed / 2)
                / elapsed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    #[test]
    fn resolve_rules() {
        let base = UpdaterConfig {
            provider: Some("generic".into()),
            url: Some("https://feed.example.com/app?x=1#y".into()),
            ..Default::default()
        };
        let r = base.resolve(None, false).unwrap();
        assert_eq!(r.url.as_str(), "https://feed.example.com/app/");
        let bad = |c: UpdaterConfig, debug: bool| c.resolve(None, debug).unwrap_err().code();
        assert_eq!(
            bad(
                UpdaterConfig {
                    provider: Some("github".into()),
                    ..base.clone()
                },
                false
            ),
            crate::ErrorCode::InvalidArgument
        );
        assert!(
            UpdaterConfig {
                url: None,
                ..base.clone()
            }
            .resolve(None, false)
            .is_err()
        );
        for ch in ["", "../x", ".hidden", "a/b", &"x".repeat(65)] {
            assert!(
                UpdaterConfig {
                    channel: Some(ch.to_owned()),
                    ..base.clone()
                }
                .resolve(None, false)
                .is_err(),
                "{ch:?}"
            );
        }
        let mut headers = BTreeMap::new();
        headers.insert("X-Bad\n".to_owned(), "v".to_owned());
        assert!(
            UpdaterConfig {
                request_headers: Some(headers),
                ..base.clone()
            }
            .resolve(None, false)
            .is_err()
        );
        // forceDevUpdateConfig is a debug-build feature.
        let dev = DevUpdateConfig {
            provider: Some("generic".into()),
            url: Some("http://127.0.0.1:9/feed".into()),
            channel: Some("beta".into()),
        };
        let forced = UpdaterConfig {
            force_dev_update_config: Some(true),
            ..base.clone()
        };
        let r = forced.resolve(Some(&dev), true).unwrap();
        assert_eq!(
            (r.url.as_str(), r.channel.as_str()),
            ("http://127.0.0.1:9/feed/", "beta")
        );
        let r = forced.resolve(Some(&dev), false).unwrap();
        assert_eq!(r.url.as_str(), "https://feed.example.com/app/");
        // Without an embedded copy the feed from setFeedURL is checked (the
        // sample sets both); with neither there is nothing to check.
        let r = forced.resolve(None, true).unwrap();
        assert_eq!(r.url.as_str(), "https://feed.example.com/app/");
        let no_feed = UpdaterConfig {
            url: None,
            ..forced.clone()
        };
        assert!(no_feed.resolve(None, true).is_err());
    }

    fn check(current: &str, info: &UpdateInfo, downgrade: bool, bucket: u8) -> Availability {
        is_update_available(&v(current), info, downgrade, "10.0.22631", || Some(bucket)).unwrap()
    }

    #[test]
    fn availability_rules() {
        // electron-updater's generic provider offers prereleases: it never
        // reads allowPrerelease.
        let mut info = UpdateInfo {
            version: "1.1.0-beta.1".into(),
            ..Default::default()
        };
        assert_eq!(check("1.0.0", &info, false, 0), Availability::Available);
        assert_eq!(check("1.1.0", &info, false, 0), Availability::Older);
        info.version = "v1.0.1".into();
        assert_eq!(check("1.0.0", &info, false, 0), Availability::Available);
        info.version = "1.0".into();
        assert!(is_update_available(&v("1.0.0"), &info, false, "1.0.0", || None).is_err());
        // Build metadata: semver.eq / gt ignore it, so the same release is
        // never offered again, also with downgrades on.
        info.version = "1.2.3+20261001".into();
        assert_eq!(
            check("1.2.3+20260901", &info, true, 0),
            Availability::SameVersion
        );
        assert_eq!(check("1.2.2+x", &info, false, 0), Availability::Available);
        // A non-numeric staging percentage admits everyone (electron-updater).
        info.version = "2.0.0".into();
        info.staging_percentage = Some(json!("half"));
        assert_eq!(check("1.0.0", &info, false, 99), Availability::Available);
        info.staging_percentage = Some(json!(100));
        assert_eq!(check("1.0.0", &info, false, 99), Availability::Available);
        assert_eq!(
            check("1.0.0", &info, false, 100),
            Availability::NotInRollout
        );
        info.staging_percentage = Some(json!(0));
        assert_eq!(check("1.0.0", &info, false, 0), Availability::NotInRollout);
        // The equal check runs before the rollout, and the staging id is
        // only asked for when the feed has a percentage.
        assert_eq!(check("2.0.0", &info, false, 0), Availability::SameVersion);
        info.staging_percentage = None;
        let asked = std::cell::Cell::new(false);
        let r = is_update_available(&v("1.0.0"), &info, false, "1.0.0", || {
            asked.set(true);
            Some(0)
        });
        assert_eq!(r.unwrap(), Availability::Available);
        assert!(!asked.get());
    }

    #[test]
    fn minimum_system_version() {
        let mut info = UpdateInfo {
            version: "2.0.0".into(),
            minimum_system_version: Some(json!("10.0.22000")),
            staging_percentage: Some(json!(0)),
            ..Default::default()
        };
        // Checked before the rollout, as electron-updater does.
        let r = is_update_available(&v("1.0.0"), &info, false, "10.0.19045", || Some(0));
        assert_eq!(r.unwrap(), Availability::Unsupported);
        info.staging_percentage = None;
        for (os, want) in [
            ("10.0.22631", Availability::Available),
            ("25.5.0", Availability::Available),
            // Not comparable: supported, with electron-updater's warning.
            ("unknown", Availability::Available),
        ] {
            let r = is_update_available(&v("1.0.0"), &info, false, os, || None);
            assert_eq!(r.unwrap(), want, "{os}");
        }
        // A number throws in semver.lt: supported.
        info.minimum_system_version = Some(json!(99));
        assert_eq!(
            os_supports(info.minimum_system_version.as_ref(), "1.0.0"),
            None
        );
        assert_eq!(os_supports(Some(&json!("")), "1.0.0"), Some(true));
        // A prerelease OS release (Linux kernels) compares by precedence.
        assert_eq!(
            os_supports(Some(&json!("6.8.0")), "6.8.0-45-generic"),
            Some(false)
        );
    }

    #[test]
    fn staging_matches_electron_updater() {
        // electron-updater: UUID.parse(id).readUInt32BE(12) / 0xffffffff
        // < stagingPercentage / 100. The bucket is that value's integer part.
        for (tail, want) in [
            ("00000000", 0_u8),
            ("028f5c28", 0),
            ("028f5c29", 1),
            ("80000000", 50),
            ("fffffffe", 99),
            ("ffffffff", 100),
        ] {
            let id = format!("12345678-1234-4234-8234-0000{tail}");
            assert_eq!(staging_bucket(&id), Some(want), "{id}");
        }
        for bad in [
            "12345678-1234-4234-8234-0000ffffffffff",
            "123456781234423482340000ffffffff",
            "12345678-1234-4234-8234-0000fffffff\n",
            "12345678-1234-4234-8234-0000fffffffg",
        ] {
            assert_eq!(staging_bucket(bad), None, "{bad}");
        }
    }

    #[test]
    fn file_choice_per_os() {
        let file = |u: &str| UpdateFileInfo {
            url: u.into(),
            sha512: "x".into(),
            ..Default::default()
        };
        let info = UpdateInfo {
            version: "1.0.0".into(),
            files: vec![
                file("App-1.0.0.msi"),
                file("App-1.0.0-mac.zip?sig=1"),
                file("App-1.0.0.AppImage"),
            ],
            ..Default::default()
        };
        assert_eq!(
            choose_file(&info, TargetOs::Windows).unwrap().1,
            InstallerKind::Msi
        );
        assert_eq!(
            choose_file(&info, TargetOs::Macos).unwrap().1,
            InstallerKind::MacZip
        );
        assert_eq!(
            choose_file(&info, TargetOs::Linux).unwrap().1,
            InstallerKind::AppImage
        );
        assert!(path_ends_with("x.exe", ".EXE"));
        assert!(!path_ends_with("é", ".exe"));
    }
}
