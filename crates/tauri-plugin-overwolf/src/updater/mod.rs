//! The electron-updater compatible update client (CONTRACT A.2.8, I;
//! [ADR 0008](https://github.com/ow-tauri/ow-tauri/blob/main/docs/adr/0008-updater-client.md)).
//!
//! Overwolf distributes app updates through an electron-updater **generic
//! provider** feed per app. This module reads that feed and does what
//! electron-updater does with it:
//!
//! - [`feed`]: `<channel>.yml` (`-mac.yml`, `-linux.yml`) parsing, with the
//!   field spellings of Overwolf's feed (`IsAdminRightsRequired`);
//! - [`is_update_available`], [`staging_bucket`]: the version and staged
//!   rollout rules;
//! - [`verify`]: size, SHA-512, the detached minisign signature and the
//!   publisher-name rule for Windows Authenticode subjects;
//! - [`install`]: the per-OS install command (NSIS, MSI, `.app`, AppImage).
//!
//! The plugin side (commands, `updater` host messages, the download and the
//! install at exit) lives in `client`.
//!
//! ```
//! use tauri_plugin_overwolf::updater::{feed, is_update_available, Availability};
//! let info = feed::parse_feed("version: 1.2.0\nfiles:\n  - url: setup.exe\n    sha512: abc\n    size: 3\n").unwrap();
//! let current = semver::Version::parse("1.1.0").unwrap();
//! assert_eq!(is_update_available(&current, &info, false, false, Some(10)).unwrap(), Availability::Available);
//! ```

pub mod feed;
pub mod install;
pub mod verify;

#[cfg(feature = "plugin")]
pub(crate) mod client;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::Error;
use crate::paths::TargetOs;

pub use feed::{UpdateFileInfo, UpdateInfo};

#[cfg(feature = "plugin")]
pub use client::Updater;
#[cfg(feature = "plugin")]
pub(crate) use client::UpdaterCore;

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
    /// Offer prerelease versions.
    #[serde(default)]
    pub allow_prerelease: Option<bool>,
    /// Download a found update at once (electron-updater default `true`).
    #[serde(default)]
    pub auto_download: Option<bool>,
    /// Install a downloaded update when the app quits (default `true`).
    #[serde(default)]
    pub auto_install_on_app_quit: Option<bool>,
    /// Read `provider` and `url` from the embedded `dev-app-update.yml`
    /// (debug builds only).
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
                return Err(Error::invalid_argument(
                    "Only the generic update provider is supported.",
                )
                .with_data(serde_json::json!({ "provider": other })));
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
                return Err(
                    Error::invalid_argument("An update request header is invalid.")
                        .with_data(serde_json::json!({ "header": name })),
                );
            }
        }
        Ok(ResolvedConfig {
            url,
            channel,
            allow_downgrade: self.allow_downgrade.unwrap_or(false),
            allow_prerelease: self.allow_prerelease.unwrap_or(false),
            auto_download: self.auto_download.unwrap_or(true),
            auto_install_on_app_quit: self.auto_install_on_app_quit.unwrap_or(true),
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

/// The staged-rollout bucket (0 to 99) of a `stagingId` (I.2 #5). It is
/// electron-updater's rule: the 32-bit big-endian number at bytes 12 to 15
/// of the UUID, as a share of `0xFFFFFFFF`, so the same id lands in the same
/// bucket in both clients. `None` when `id` is not a UUID.
///
/// ```
/// use tauri_plugin_overwolf::updater::staging_bucket;
/// assert_eq!(staging_bucket("00000000-0000-4000-8000-000000000000"), Some(0));
/// assert_eq!(staging_bucket("00000000-0000-4000-8000-0000ffffffff"), Some(99));
/// assert_eq!(staging_bucket("00000000-0000-4000-8000-00007fffffff"), Some(49));
/// assert_eq!(staging_bucket("not a uuid"), None);
/// ```
#[must_use]
pub fn staging_bucket(id: &str) -> Option<u8> {
    let hex: String = id.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(&hex[24..32], 16).ok()?;
    // floor(value / 0xFFFFFFFF * 100), capped at 99 so 100 % always admits.
    let bucket = (u64::from(value) * 100) / u64::from(u32::MAX);
    Some(u8::try_from(bucket.min(99)).unwrap_or(99))
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
    /// The feed version is the running one.
    SameVersion,
    /// The feed version is older and downgrades are off.
    Older,
    /// The feed version is a prerelease and prereleases are off.
    Prerelease,
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
        Error::invalid_argument("The update feed has an invalid version.")
            .with_data(serde_json::json!({ "version": text }))
    })
}

/// electron-updater's `isUpdateAvailable` (I.2 #4, #5): an equal version is
/// never an update; then the staged rollout; then newer, or older with
/// `allow_downgrade`. A prerelease needs `allow_prerelease`. `bucket` is
/// [`staging_bucket`] of this install.
///
/// # Errors
///
/// `invalid-argument` when the feed version is not a semantic version.
///
/// ```
/// use tauri_plugin_overwolf::updater::{feed::UpdateInfo, is_update_available, Availability};
/// let v = |s: &str| semver::Version::parse(s).unwrap();
/// let mut info = UpdateInfo { version: "2.0.0".into(), ..Default::default() };
/// assert_eq!(is_update_available(&v("1.0.0"), &info, false, false, Some(5)).unwrap(), Availability::Available);
/// assert_eq!(is_update_available(&v("2.0.0"), &info, false, false, Some(5)).unwrap(), Availability::SameVersion);
/// assert_eq!(is_update_available(&v("3.0.0"), &info, false, false, Some(5)).unwrap(), Availability::Older);
/// assert_eq!(is_update_available(&v("3.0.0"), &info, true, false, Some(5)).unwrap(), Availability::Available);
/// info.staging_percentage = Some(serde_json::json!(5));
/// assert_eq!(is_update_available(&v("1.0.0"), &info, false, false, Some(5)).unwrap(), Availability::NotInRollout);
/// assert_eq!(is_update_available(&v("1.0.0"), &info, false, false, Some(4)).unwrap(), Availability::Available);
/// ```
pub fn is_update_available(
    current: &semver::Version,
    info: &UpdateInfo,
    allow_downgrade: bool,
    allow_prerelease: bool,
    bucket: Option<u8>,
) -> Result<Availability, Error> {
    let latest = parse_version(&info.version)?;
    if latest == *current {
        return Ok(Availability::SameVersion);
    }
    if let (Some(pct), Some(bucket)) =
        (staging_percentage(info.staging_percentage.as_ref()), bucket)
        && i64::from(bucket) >= pct
    {
        return Ok(Availability::NotInRollout);
    }
    if !latest.pre.is_empty() && !allow_prerelease {
        return Ok(Availability::Prerelease);
    }
    if latest > *current || allow_downgrade {
        Ok(Availability::Available)
    } else {
        Ok(Availability::Older)
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

/// `ProgressInfo` of `download-progress` (I.3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressInfo {
    /// `transferred / total * 100`.
    pub percent: f64,
    /// Average speed since the download started.
    pub bytes_per_second: u64,
    /// Bytes to download.
    pub total: u64,
    /// Bytes downloaded so far.
    pub transferred: u64,
}

impl ProgressInfo {
    /// The progress after `transferred` of `total` bytes in `elapsed_ms`.
    ///
    /// ```
    /// use tauri_plugin_overwolf::updater::ProgressInfo;
    /// let p = ProgressInfo::new(50, 200, 1000);
    /// assert_eq!((p.percent, p.bytes_per_second), (25.0, 50));
    /// ```
    #[must_use]
    pub fn new(transferred: u64, total: u64, elapsed_ms: u64) -> Self {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a percentage of byte counts below 2^52"
        )]
        let percent = if total == 0 {
            0.0
        } else {
            transferred as f64 * 100.0 / total as f64
        };
        ProgressInfo {
            percent,
            bytes_per_second: transferred.saturating_mul(1000) / elapsed_ms.max(1),
            total,
            transferred,
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
        assert!(forced.resolve(None, true).is_err());
    }

    #[test]
    fn availability_rules() {
        let mut info = UpdateInfo {
            version: "1.1.0-beta.1".into(),
            ..Default::default()
        };
        assert_eq!(
            is_update_available(&v("1.0.0"), &info, false, false, None).unwrap(),
            Availability::Prerelease
        );
        assert_eq!(
            is_update_available(&v("1.0.0"), &info, false, true, None).unwrap(),
            Availability::Available
        );
        info.version = "v1.0.1".into();
        assert_eq!(
            is_update_available(&v("1.0.0"), &info, false, false, None).unwrap(),
            Availability::Available
        );
        info.version = "1.0".into();
        assert!(is_update_available(&v("1.0.0"), &info, false, false, None).is_err());
        // A non-numeric staging percentage admits everyone (electron-updater).
        info.version = "2.0.0".into();
        info.staging_percentage = Some(json!("half"));
        assert_eq!(
            is_update_available(&v("1.0.0"), &info, false, false, Some(99)).unwrap(),
            Availability::Available
        );
        info.staging_percentage = Some(json!(100));
        assert_eq!(
            is_update_available(&v("1.0.0"), &info, false, false, Some(99)).unwrap(),
            Availability::Available
        );
        info.staging_percentage = Some(json!(0));
        assert_eq!(
            is_update_available(&v("1.0.0"), &info, false, false, Some(0)).unwrap(),
            Availability::NotInRollout
        );
    }

    #[test]
    fn staging_matches_electron_updater() {
        // electron-updater: UUID.parse(id).readUInt32BE(12) / 0xffffffff * 100
        // < stagingPercentage. The bucket is that value's integer part.
        for (tail, want) in [
            ("00000000", 0_u8),
            ("028f5c28", 0),
            ("028f5c29", 1),
            ("80000000", 50),
            ("ffffffff", 99),
        ] {
            let id = format!("12345678-1234-4234-8234-0000{tail}");
            assert_eq!(staging_bucket(&id), Some(want), "{id}");
        }
        assert_eq!(
            staging_bucket("12345678-1234-4234-8234-0000ffffffffff"),
            None
        );
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
