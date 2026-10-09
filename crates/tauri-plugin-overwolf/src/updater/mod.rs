//! The Overwolf update client (CONTRACT I; DESIGN §4.14;
//! [ADR 0008](https://github.com/AlloryDante/ow-tauri/blob/main/docs/adr/0008-updater-client.md)).
//!
//! Overwolf distributes app updates through an electron-updater **generic
//! provider** feed per app,
//! `https://electron-updates.overwolf.com/electron-updates/electron/<uid>/<channel>.yml`.
//! This module reads that feed and does what electron-updater does with it:
//!
//! - [`feed`]: `<channel>.yml` parsing, with the field spellings of
//!   Overwolf's feed (`IsAdminRightsRequired`);
//! - [`is_update_available`], [`staging_bucket`], [`os_supports`]: the
//!   version, prerelease, `minimumSystemVersion` and staged rollout rules;
//! - [`choose_installer`]: the NSIS `setup.exe` of the release (an `.msi` is
//!   refused);
//! - [`verify`]: SHA-512, the detached minisign signature and the publisher
//!   rule for Authenticode subjects (R5: `publisherNames` or `pubkey` is
//!   required; nothing is trusted by default);
//! - [`install`]: the installer command line, which always carries
//!   `/UPDATE`.
//!
//! The client runs on **Windows only** (R6). The Rust API
//! (`UpdaterBuilder`, `Updater`, `Update`, `DownloadedUpdate`, DESIGN §3.3)
//! is exported on Windows; elsewhere the commands answer `unsupported` and
//! apps use `tauri-plugin-updater`. The functions here are pure and build
//! on every OS.
//!
//! ```
//! use tauri_plugin_overwolf::updater::{feed, is_update_available, Availability};
//! let info = feed::parse_feed("version: 1.2.0\nfiles:\n  - url: setup.exe\n    sha512: abc\n    size: 3\n").unwrap();
//! let current = semver::Version::parse("1.1.0").unwrap();
//! assert_eq!(
//!     is_update_available(&current, &info, false, false, "10.0.22631", || Some(10)).unwrap(),
//!     Availability::Available
//! );
//! ```

pub mod feed;
pub mod install;
pub mod verify;

// The engine builds on every OS so the commands, and the tests, share one
// code path; only its OS layer (`os`) is Windows code. The public Rust API
// is exported on Windows only (R6).
#[cfg_attr(
    not(windows),
    allow(
        unreachable_pub,
        dead_code,
        reason = "the Rust API is exported on Windows only (R6); the commands use part of it everywhere"
    )
)]
mod api;
pub(crate) mod client;
pub(crate) mod engine;
mod os;

use url::Url;

use crate::error::Error;

pub use feed::{UpdateFileInfo, UpdateInfo};

#[cfg(windows)]
pub use api::{DownloadedUpdate, Update, Updater, UpdaterBuilder};
#[cfg(not(windows))]
pub(crate) use api::{DownloadedUpdate, Update, UpdaterBuilder};

/// Overwolf's update feed of the app `uid` (CONTRACT I.1), with the
/// trailing slash the channel file name is joined to.
///
/// ```
/// use tauri_plugin_overwolf::updater::overwolf_feed;
/// assert_eq!(
///     overwolf_feed("abc").as_str(),
///     "https://electron-updates.overwolf.com/electron-updates/electron/abc/"
/// );
/// ```
#[must_use]
pub fn overwolf_feed(uid: &str) -> Url {
    let mut url = Url::parse("https://electron-updates.overwolf.com/electron-updates/electron/")
        .unwrap_or_else(|_| unreachable!("a valid literal URL"));
    if let Ok(mut path) = url.path_segments_mut() {
        path.pop_if_empty().push(uid).push("");
    }
    url
}

/// The feed base URL as the client joins file names to it: no query or
/// fragment, and a trailing slash.
///
/// ```
/// use tauri_plugin_overwolf::updater::feed_base;
/// let u = url::Url::parse("https://feed.example.com/app?x=1#y").unwrap();
/// assert_eq!(feed_base(u).as_str(), "https://feed.example.com/app/");
/// ```
#[must_use]
pub fn feed_base(mut url: Url) -> Url {
    url.set_query(None);
    url.set_fragment(None);
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    url
}

/// The feed file URL of `channel` (CONTRACT I.2 #1): `<base><channel>.yml`
/// with electron-updater's cache-busting `noCache` query of 12 hex
/// characters (`token`).
///
/// # Errors
///
/// `invalid-argument` for a channel [`valid_channel`] refuses.
///
/// ```
/// use tauri_plugin_overwolf::updater::feed_url;
/// let base = url::Url::parse("https://feed.example.com/apps/abc/").unwrap();
/// let u = feed_url(&base, "beta", "0123456789ab").unwrap();
/// assert_eq!(u.as_str(), "https://feed.example.com/apps/abc/beta.yml?noCache=0123456789ab");
/// assert!(feed_url(&base, "../x", "0").is_err());
/// ```
pub fn feed_url(base: &Url, channel: &str, token: &str) -> Result<Url, Error> {
    if !valid_channel(channel) {
        return Err(Error::invalid_argument(
            "The update channel must be 1 to 64 letters, digits, '.', '_' or '-'.",
        ));
    }
    let mut url = resolve_file_url(base, &format!("{channel}.yml"))?;
    url.query_pairs_mut().append_pair("noCache", token);
    Ok(url)
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
    /// The feed version is a prerelease and prereleases are off.
    Prerelease,
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
/// 2. a prerelease is offered only with `allow_prerelease` (CONTRACT I.2
///    #4; electron-updater's generic provider ignores the switch);
/// 3. `minimumSystemVersion` above `os_release` is not supported;
/// 4. the staged rollout: `bucket` is called only when the feed has a
///    numeric `stagingPercentage`, so the staging id is created lazily, as
///    electron-updater does;
/// 5. newer (`semver.gt`), or older (`semver.lt`) with `allow_downgrade`.
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
///     is_update_available(&v(cur), info, down, false, "10.0.22631", || Some(bucket)).unwrap()
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
    allow_prerelease: bool,
    os_release: &str,
    bucket: impl FnOnce() -> Option<u8>,
) -> Result<Availability, Error> {
    use std::cmp::Ordering;
    let latest = parse_version(&info.version)?;
    let order = latest.cmp_precedence(current);
    if order == Ordering::Equal {
        return Ok(Availability::SameVersion);
    }
    if !latest.pre.is_empty() && !allow_prerelease {
        return Ok(Availability::Prerelease);
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

/// The message of the `unsupported` error for a feed that offers only an
/// `.msi` (D24).
pub const MSI_UNSUPPORTED: &str =
    "MSI is not supported for Overwolf distribution (docs/PRODUCTION-CHECKLIST.md#installers)";

fn path_ends_with(url: &str, ext: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or_default();
    path.len() >= ext.len()
        && path.is_char_boundary(path.len() - ext.len())
        && path[path.len() - ext.len()..].eq_ignore_ascii_case(ext)
}

/// The installer to download (I.2 #6, D24): the release's NSIS `.exe`.
///
/// # Errors
///
/// `unsupported` with [`MSI_UNSUPPORTED`] when the release has only an
/// `.msi`; `backend` when it has no Windows installer or the entry has no
/// SHA-512.
///
/// ```
/// use tauri_plugin_overwolf::updater::{choose_installer, feed::{UpdateFileInfo, UpdateInfo}};
/// let file = |u: &str| UpdateFileInfo { url: u.into(), sha512: "x".into(), ..Default::default() };
/// let info = UpdateInfo { version: "1.0.0".into(), files: vec![file("App.msi"), file("setup.EXE")], ..Default::default() };
/// assert_eq!(choose_installer(&info).unwrap().url, "setup.EXE");
/// let msi = UpdateInfo { files: vec![file("App.msi")], ..info };
/// assert_eq!(choose_installer(&msi).unwrap_err().code(), tauri_plugin_overwolf::ErrorCode::Unsupported);
/// ```
pub fn choose_installer(info: &UpdateInfo) -> Result<&UpdateFileInfo, Error> {
    let find = |ext: &str| info.files.iter().find(|f| path_ends_with(&f.url, ext));
    let Some(file) = find(".exe") else {
        return Err(if find(".msi").is_some() {
            Error::unsupported(MSI_UNSUPPORTED)
        } else {
            Error::backend("The update feed has no Windows installer (.exe).")
        });
    };
    if file.sha512.trim().is_empty() {
        return Err(Error::backend("The update feed entry has no SHA-512."));
    }
    Ok(file)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    fn check(current: &str, info: &UpdateInfo, downgrade: bool, bucket: u8) -> Availability {
        is_update_available(&v(current), info, downgrade, false, "10.0.22631", || {
            Some(bucket)
        })
        .unwrap()
    }

    #[test]
    fn availability_rules() {
        // Prereleases only with allowPrerelease (CONTRACT I.2 #4).
        let mut info = UpdateInfo {
            version: "1.1.0-beta.1".into(),
            ..Default::default()
        };
        assert_eq!(check("1.0.0", &info, false, 0), Availability::Prerelease);
        let pre = |cur: &str, info: &UpdateInfo, down: bool| {
            is_update_available(&v(cur), info, down, true, "10.0.22631", || None).unwrap()
        };
        assert_eq!(pre("1.0.0", &info, false), Availability::Available);
        assert_eq!(pre("1.1.0", &info, false), Availability::Older);
        assert_eq!(pre("1.1.0-beta.1", &info, false), Availability::SameVersion);
        info.version = "v1.0.1".into();
        assert_eq!(check("1.0.0", &info, false, 0), Availability::Available);
        info.version = "1.0".into();
        assert!(is_update_available(&v("1.0.0"), &info, false, false, "1.0.0", || None).is_err());
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
        let r = is_update_available(&v("1.0.0"), &info, false, false, "1.0.0", || {
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
        let r = is_update_available(&v("1.0.0"), &info, false, false, "10.0.19045", || Some(0));
        assert_eq!(r.unwrap(), Availability::Unsupported);
        info.staging_percentage = None;
        for (os, want) in [
            ("10.0.22631", Availability::Available),
            ("25.5.0", Availability::Available),
            // Not comparable: supported, with electron-updater's warning.
            ("unknown", Availability::Available),
        ] {
            let r = is_update_available(&v("1.0.0"), &info, false, false, os, || None);
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
    fn installer_choice() {
        let file = |u: &str, sha: &str| UpdateFileInfo {
            url: u.into(),
            sha512: sha.into(),
            ..Default::default()
        };
        let info = |files| UpdateInfo {
            version: "1.0.0".into(),
            files,
            ..Default::default()
        };
        // NSIS only (D24): an MSI-only release is `unsupported`, with the
        // documented message.
        let msi = choose_installer(&info(vec![file("App-1.0.0.msi", "x")])).unwrap_err();
        assert_eq!(msi.code(), crate::ErrorCode::Unsupported);
        assert!(msi.to_string().contains(MSI_UNSUPPORTED), "{msi}");
        let none = choose_installer(&info(vec![file("App.zip", "x")])).unwrap_err();
        assert_eq!(none.code(), crate::ErrorCode::Backend);
        let no_hash = choose_installer(&info(vec![file("setup.exe?sig=1", " ")])).unwrap_err();
        assert_eq!(no_hash.code(), crate::ErrorCode::Backend);
        let both = info(vec![file("a.msi", "x"), file("b.Exe?sig=1", "y")]);
        assert_eq!(choose_installer(&both).unwrap().url, "b.Exe?sig=1");
        assert!(path_ends_with("x.exe", ".EXE"));
        assert!(!path_ends_with("é", ".exe"));
    }

    #[test]
    fn feed_urls() {
        let base = feed_base(Url::parse("https://feed.example.com/apps/abc?x=1").unwrap());
        assert_eq!(base.as_str(), "https://feed.example.com/apps/abc/");
        let url = feed_url(&base, "latest", "aaaaaaaaaaaa").unwrap();
        assert_eq!(
            url.as_str(),
            "https://feed.example.com/apps/abc/latest.yml?noCache=aaaaaaaaaaaa"
        );
        for ch in ["", "../x", ".hidden", "a/b", &"x".repeat(65)] {
            assert!(feed_url(&base, ch, "t").is_err(), "{ch:?}");
        }
        assert_eq!(
            overwolf_feed("abcdefghijklmnopabcdefghijklmnop").as_str(),
            "https://electron-updates.overwolf.com/electron-updates/electron/abcdefghijklmnopabcdefghijklmnop/"
        );
    }
}
