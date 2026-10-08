//! The plugin configuration, `plugins.overwolf` of `tauri.conf.json`
//! (DESIGN §3.2).
//!
//! The same document is read by the plugin at run time (`app.config()`), by
//! the app's build step (`build::run`, feature `build`) and by the CLI, so a
//! value means the same thing everywhere. Every field is optional; the
//! defaults are ow-electron's behaviour. Apps read the parsed configuration
//! with `app.overwolf().config()`; it never changes at run time.
//!
//! ```
//! use tauri_plugin_overwolf::config::{Config, Validation};
//! let raw = serde_json::json!({ "author": "Example Studio", "ads": { "testAd": true } });
//! let config = Config::from_value(&raw).unwrap();
//! assert!(config.ads.test_ad);
//! assert_eq!(config.analytics.host_label, "tauri");
//! config.validate(Validation::default()).unwrap();
//! ```

use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::identity::{HashEncoding, is_valid_uid};

/// `plugins.overwolf`.
///
/// Deserializing goes through [`Config::from_value`], so Tauri's own parse
/// of the plugin configuration reports the same messages (removed keys with
/// their migration hint, the dot path of a wrong value) as the build step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(
    remote = "Self",
    rename_all = "camelCase",
    deny_unknown_fields,
    default
)]
#[non_exhaustive]
pub struct Config {
    /// The uid formula's author (CONTRACT G.2). Missing or empty: `"unknown"`,
    /// which only debug builds accept unless `uid` is set.
    pub author: Option<String>,
    /// `<PN>`, the ow-electron app name (the uid formula's name, analytics
    /// `app_name`, the guests' `name`). Missing: the Tauri `productName`.
    pub name: Option<String>,
    /// The app uid (1 to 64 ASCII letters or digits after trimming); wins
    /// over the computed one. Set it to the uid the Overwolf console
    /// assigned.
    pub uid: Option<String>,
    /// Ads (`<owadview>`).
    pub ads: AdsConfig,
    /// Anonymous analytics.
    pub analytics: AnalyticsConfig,
    /// Consent (the CMP).
    pub consent: ConsentConfig,
    /// Email hashes.
    pub email_hashes: EmailHashesConfig,
    /// The update client (cargo feature `updater`, Windows).
    pub updater: UpdaterConfig,
    /// Signing with Overwolf (read by the build step and the CLI only).
    pub signing: SigningConfig,
    /// State files (tests only).
    pub state: StateConfig,
}

/// `plugins.overwolf.ads`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
#[non_exhaustive]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent configuration switches of the configuration schema"
)]
pub struct AdsConfig {
    /// Test ads (default `false`). Also on with `--test-ad`,
    /// `OW_TAURI_TEST_AD=1` or `Builder::test_ad(true)`.
    pub test_ad: bool,
    /// `build.overwolf.disableAdOptimization` of ow-electron (default
    /// `false`).
    pub disable_optimization: bool,
    /// Disables ads first-party data before the launch burst (default
    /// `false`).
    pub disable_fpd: bool,
    /// Shapes the ad requests as ow-electron does (default `true`; a debug
    /// switch only).
    pub request_shaping: bool,
    /// Overrides the Overwolf runtime version the guests report.
    pub ow_version_override: Option<String>,
    /// macOS private header API prototype (default `false`).
    pub mac_private_header_api: bool,
    /// Transparent guest backgrounds (default `true`).
    pub transparent_guests: bool,
    /// Most crash recoveries of one guest; `None` (default) is uncapped, as
    /// ow-electron.
    pub max_recoveries: Option<u32>,
    /// Retry delay after a guest's load failure, in ms (default 5000).
    pub load_error_retry_ms: u64,
    /// macOS: recreate a guest when its page asks for a reload (default
    /// `true`); ignored elsewhere.
    pub recreate_on_reload: bool,
    /// A reload sooner than this after the last recreate reloads in place, in
    /// ms (default 30000).
    pub recreate_min_interval_ms: u64,
    /// Recreates per guest and hour; beyond it reloads are in place
    /// (default 30).
    pub recreate_max_per_hour: u32,
    /// Extra app origins allowed to host ads (for example a
    /// `tauri-plugin-localhost` origin); never an Overwolf origin.
    pub allowed_embedder_origins: Vec<String>,
    /// Windows: browser arguments of the ads environment, appended after the
    /// ones ow-electron uses.
    pub browser_args: Vec<String>,
    /// Limits per guest.
    pub guest_limits: GuestLimits,
}

impl Default for AdsConfig {
    fn default() -> Self {
        AdsConfig {
            test_ad: false,
            disable_optimization: false,
            disable_fpd: false,
            request_shaping: true,
            ow_version_override: None,
            mac_private_header_api: false,
            transparent_guests: true,
            max_recoveries: None,
            load_error_retry_ms: 5_000,
            recreate_on_reload: true,
            recreate_min_interval_ms: 30_000,
            recreate_max_per_hour: 30,
            allowed_embedder_origins: Vec::new(),
            browser_args: Vec::new(),
            guest_limits: GuestLimits::default(),
        }
    }
}

/// `plugins.overwolf.ads.guestLimits`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
#[non_exhaustive]
pub struct GuestLimits {
    /// Guest events per second (default 50).
    pub events_per_second: u32,
    /// Event burst (default 100).
    pub event_burst: u32,
    /// Event bytes per second (default 262144).
    pub bytes_per_second: u64,
    /// External opens per minute and guest (default 20).
    pub external_opens_per_minute: u32,
    /// External opens per minute across the app (default 20).
    pub external_opens_per_minute_app: u32,
    /// How long a user activation allows one open, in ms (default 5000).
    pub activation_window_ms: u64,
}

impl Default for GuestLimits {
    fn default() -> Self {
        GuestLimits {
            events_per_second: 50,
            event_burst: 100,
            bytes_per_second: 262_144,
            external_opens_per_minute: 20,
            external_opens_per_minute_app: 20,
            activation_window_ms: 5_000,
        }
    }
}

/// `plugins.overwolf.analytics`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
#[non_exhaustive]
pub struct AnalyticsConfig {
    /// The host label of the analytics events and the user agent token
    /// (default `"tauri"`).
    pub host_label: String,
    /// The host version; `None` (default) is the Tauri version.
    pub host_version: Option<String>,
    /// Where the muid comes from (default the machine id, as ow-electron).
    pub muid_strategy: MuidStrategy,
    /// Same as calling `disableAnonymousAnalytics()` before the launch
    /// (default `false`).
    pub disable_anonymous: bool,
    /// Window label globs (`*`, `?`) that are never counted.
    pub exclude_windows: Vec<String>,
    /// Exposes `setAnalyticsUserEnabled` (default `false`).
    pub user_switch: bool,
}

impl Default for AnalyticsConfig {
    fn default() -> Self {
        AnalyticsConfig {
            host_label: "tauri".into(),
            host_version: None,
            muid_strategy: MuidStrategy::default(),
            disable_anonymous: false,
            exclude_windows: Vec::new(),
            user_switch: false,
        }
    }
}

/// Where the muid comes from (CONTRACT E.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum MuidStrategy {
    /// The machine id, shared with every Overwolf app (ow-electron).
    #[default]
    MachineId,
    /// A random id per install, kept in `ow-tauri.json`.
    PerInstall,
}

/// `plugins.overwolf.consent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
#[non_exhaustive]
pub struct ConsentConfig {
    /// Overrides the Overwolf consent page (https only).
    pub cmp_url: Option<String>,
    /// Origins a JavaScript `cmpURL` must match (default
    /// `["https://content.overwolf.com"]`).
    pub allowed_cmp_origins: Vec<String>,
    /// How long the consent page may take to report ready, in ms (default
    /// 30000).
    pub ready_timeout_ms: u64,
    /// How long the EU-only request may take, in ms (default 60000); on
    /// expiry consent counts as required.
    pub eu_only_timeout_ms: u64,
    /// Whether the consent cookies are checked when the page saves nothing.
    pub host_cookie_fallback: CookieFallback,
}

impl Default for ConsentConfig {
    fn default() -> Self {
        ConsentConfig {
            cmp_url: None,
            allowed_cmp_origins: vec!["https://content.overwolf.com".into()],
            ready_timeout_ms: 30_000,
            eu_only_timeout_ms: 60_000,
            host_cookie_fallback: CookieFallback::default(),
        }
    }
}

/// `plugins.overwolf.consent.hostCookieFallback`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum CookieFallback {
    /// Check the consent cookies (default).
    #[default]
    Auto,
    /// Never check them.
    Never,
}

/// `plugins.overwolf.emailHashes`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
#[non_exhaustive]
pub struct EmailHashesConfig {
    /// Hash text encoding (default hex, as ow-electron).
    pub encoding: HashEncoding,
}

/// `plugins.overwolf.updater` (cargo feature `updater`, Windows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
#[non_exhaustive]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent configuration switches of the configuration schema"
)]
pub struct UpdaterConfig {
    /// The feed; `None` (default) is Overwolf's feed for the app's uid.
    pub endpoint: Option<String>,
    /// The channel; `None` is `latest`.
    pub channel: Option<String>,
    /// Accept pre-release versions (default `false`).
    pub allow_prerelease: bool,
    /// Accept a lower version (default `false`).
    pub allow_downgrade: bool,
    /// Let JavaScript `channel` / `allowDowngrade` cause a downgrade (default
    /// `false`).
    pub allow_js_downgrade: bool,
    /// Install a downloaded update when the app exits (default `true`).
    pub install_on_exit: bool,
    /// Installer arguments; `/UPDATE` is always kept.
    pub installer_args: Option<Vec<String>>,
    /// Authenticode subjects the installer must be signed by.
    pub publisher_names: Option<Vec<String>>,
    /// A minisign public key the installer must verify against.
    pub pubkey: Option<String>,
    /// Skips the publisher check (debug builds only).
    pub dangerous_skip_publisher_check: bool,
    /// Connect timeout, in ms (default 30000).
    pub connect_timeout_ms: u64,
    /// Idle read timeout (between bytes), in ms (default 60000).
    pub read_timeout_ms: u64,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        UpdaterConfig {
            endpoint: None,
            channel: None,
            allow_prerelease: false,
            allow_downgrade: false,
            allow_js_downgrade: false,
            install_on_exit: true,
            installer_args: None,
            publisher_names: None,
            pubkey: None,
            dangerous_skip_publisher_check: false,
            connect_timeout_ms: 30_000,
            read_timeout_ms: 60_000,
        }
    }
}

/// `plugins.overwolf.signing` (build step and CLI only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
#[non_exhaustive]
pub struct SigningConfig {
    /// Signing is on (default `false`).
    pub enabled: bool,
    /// A Windows release build without signed output fails (default
    /// `true`; only when `enabled`).
    pub require_signing: bool,
    /// ow-electron's `enableOWCertSigning` (default `false`).
    pub ow_cert_signing: bool,
    /// The `fileHashes` entry; the CLI's `--main` overrides it.
    pub entry: Option<String>,
}

impl Default for SigningConfig {
    fn default() -> Self {
        SigningConfig {
            enabled: false,
            require_signing: true,
            ow_cert_signing: false,
            entry: None,
        }
    }
}

/// `plugins.overwolf.state`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
#[non_exhaustive]
pub struct StateConfig {
    /// Replaces the OS app data directory (tests; refused in release
    /// builds).
    pub app_data_dir: Option<PathBuf>,
}

/// An invalid `plugins.overwolf` value.
///
/// ```
/// use tauri_plugin_overwolf::config::ConfigError;
/// let err = ConfigError::new("uid", "must be 1 to 64 ASCII letters or digits");
/// assert_eq!(err.to_string(), "plugins.overwolf.uid: must be 1 to 64 ASCII letters or digits");
/// assert_eq!(ConfigError::new("", "x").to_string(), "plugins.overwolf: x");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConfigError {
    /// Dot path below `plugins.overwolf` (empty for the object itself).
    pub path: String,
    /// What is wrong.
    pub message: String,
}

impl ConfigError {
    /// An error at `path` (dot path below `plugins.overwolf`).
    #[must_use]
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        ConfigError {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.path.is_empty() {
            write!(f, "plugins.overwolf: {}", self.message)
        } else {
            write!(f, "plugins.overwolf.{}: {}", self.path, self.message)
        }
    }
}

impl std::error::Error for ConfigError {}

/// Which rules [`Config::validate`] applies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Validation {
    /// A release build: the release-only rules apply.
    pub release: bool,
    /// The cargo feature `updater` is on.
    pub updater_feature: bool,
    /// The build step (`build::run`) runs the check: the rules that need the
    /// whole build (the pinned uid) apply too.
    pub build_step: bool,
}

impl Validation {
    /// The rules of the current build at run time: release-only rules in
    /// release builds, the updater rules when the `updater` feature is on.
    #[must_use]
    pub fn runtime() -> Self {
        Validation {
            release: !cfg!(debug_assertions),
            updater_feature: cfg!(feature = "updater"),
            build_step: false,
        }
    }

    /// The rules of an app's build step for a build with these properties.
    #[must_use]
    pub fn build_step(release: bool, updater_feature: bool) -> Self {
        Validation {
            release,
            updater_feature,
            build_step: true,
        }
    }
}

/// Keys of earlier versions, rejected with a migration hint: dot path, the
/// reason, the `docs/MIGRATION.md` anchor.
const REMOVED: [(&str, &str, &str); 9] = [
    (
        "main",
        "there is no hidden main webview; app code runs in the app's own webviews",
        "no-main-webview",
    ),
    (
        "ipc",
        "the app talks to the plugin through Tauri commands",
        "no-ipc-bridge",
    ),
    (
        "shell",
        "use tauri-plugin-opener or tauri-plugin-shell",
        "official-plugins",
    ),
    ("fs", "use tauri-plugin-fs", "official-plugins"),
    ("webview", "use ads.browserArgs", "browser-args"),
    (
        "packagesBackend",
        "packages are not available on Tauri yet",
        "packages",
    ),
    (
        "logging",
        "the plugin logs through the log crate",
        "logging",
    ),
    (
        "ads.gestureWindowMs",
        "click-outs use the platform's user activation",
        "gesture-window",
    ),
    (
        "ads.guestHeartbeatTimeoutMs",
        "guest crashes are detected with the web content terminate hook",
        "crash-recovery",
    ),
];

/// The value at dot path `path` of `raw`.
fn at<'a>(raw: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(raw, |v, key| v.get(key))
}

/// Parses one config section at its dotted path, for its error.
type SectionCheck = fn(&str, &Value) -> Result<(), ConfigError>;

/// A serde message with JSON-style quotes and without its position suffix.
fn serde_message(err: &serde_json::Error) -> String {
    err.to_string()
        .split(" at line ")
        .next()
        .unwrap_or_default()
        .replace('`', "\"")
}

/// Deserializes `value` as `T`; on failure names the first key below `path`
/// whose value alone fails, so the error carries the leaf's dot path.
fn parse_at<T: DeserializeOwned>(path: &str, value: &Value) -> Result<T, ConfigError> {
    serde_json::from_value::<T>(value.clone()).map_err(|whole| {
        let join = |key: &str| {
            if path.is_empty() {
                key.to_owned()
            } else {
                format!("{path}.{key}")
            }
        };
        if let Value::Object(object) = value {
            for (key, v) in object {
                let mut one = Map::new();
                one.insert(key.clone(), v.clone());
                if let Err(err) = serde_json::from_value::<T>(Value::Object(one)) {
                    let message = serde_message(&err);
                    // An unknown key is reported at the object that has it.
                    let leaf = if message.starts_with("unknown field") {
                        path.to_owned()
                    } else {
                        join(key)
                    };
                    return ConfigError::new(leaf, message);
                }
            }
        }
        ConfigError::new(path, serde_message(&whole))
    })
}

/// The derived (`remote = "Self"`) parse of [`Config`], without the
/// checks of [`Config::from_value`].
struct Root(Config);

impl<'de> Deserialize<'de> for Root {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Config::deserialize(d).map(Root)
    }
}

impl<'de> Deserialize<'de> for Config {
    /// Parses through [`Config::from_value`].
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Value::deserialize(d)?;
        Config::from_value(&raw).map_err(serde::de::Error::custom)
    }
}

impl Serialize for Config {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Config::serialize(self, s)
    }
}

impl Config {
    /// Parses a raw `plugins.overwolf` value (`null` is the defaults).
    ///
    /// Keys of earlier versions are rejected with a migration hint, unknown
    /// keys with the list of known ones, and wrong types with the key's dot
    /// path.
    ///
    /// # Errors
    ///
    /// The first [`ConfigError`] found.
    ///
    /// ```
    /// use tauri_plugin_overwolf::config::Config;
    /// let err = Config::from_value(&serde_json::json!({ "main": { "url": "x" } })).unwrap_err();
    /// assert!(err.to_string().starts_with("plugins.overwolf.main: removed in ow-tauri 1.0"));
    /// let err = Config::from_value(&serde_json::json!({ "ads": { "testAd": "yes" } })).unwrap_err();
    /// assert_eq!(err.path, "ads.testAd");
    /// ```
    pub fn from_value(raw: &Value) -> Result<Config, ConfigError> {
        if raw.is_null() {
            return Ok(Config::default());
        }
        for (path, reason, anchor) in REMOVED {
            if at(raw, path).is_some() {
                return Err(ConfigError::new(
                    path,
                    format!("removed in ow-tauri 1.0 ({reason}); see docs/MIGRATION.md#{anchor}"),
                ));
            }
        }
        if at(raw, "ads.recreateOnReload").is_some_and(Value::is_string) {
            return Err(ConfigError::new(
                "ads.recreateOnReload",
                "removed in ow-tauri 1.0 (\"auto\" and \"never\" are now true and false); see docs/MIGRATION.md#recreate-on-reload",
            ));
        }
        // Nested objects first, so an error names the deepest path.
        if let Some(v) = at(raw, "ads.guestLimits") {
            parse_at::<GuestLimits>("ads.guestLimits", v)?;
        }
        let sections: [(&str, SectionCheck); 7] = [
            ("ads", |p, v| parse_at::<AdsConfig>(p, v).map(drop)),
            ("analytics", |p, v| {
                parse_at::<AnalyticsConfig>(p, v).map(drop)
            }),
            ("consent", |p, v| parse_at::<ConsentConfig>(p, v).map(drop)),
            ("emailHashes", |p, v| {
                parse_at::<EmailHashesConfig>(p, v).map(drop)
            }),
            ("updater", |p, v| parse_at::<UpdaterConfig>(p, v).map(drop)),
            ("signing", |p, v| parse_at::<SigningConfig>(p, v).map(drop)),
            ("state", |p, v| parse_at::<StateConfig>(p, v).map(drop)),
        ];
        for (path, check) in sections {
            if let Some(v) = at(raw, path) {
                check(path, v)?;
            }
        }
        parse_at::<Root>("", raw).map(|Root(config)| config)
    }

    /// Checks every value against the rules of DESIGN §3.2. `validation`
    /// selects the release-only and build-step rules.
    ///
    /// # Errors
    ///
    /// The first [`ConfigError`] found, naming the value's dot path.
    ///
    /// ```
    /// use tauri_plugin_overwolf::config::{Config, Validation};
    /// let config = Config::from_value(&serde_json::json!({ "uid": "not valid!" })).unwrap();
    /// let err = config.validate(Validation::default()).unwrap_err();
    /// assert_eq!(err.path, "uid");
    /// ```
    #[expect(
        clippy::too_many_lines,
        reason = "one rule per row of the validation table, in its order"
    )]
    pub fn validate(&self, validation: Validation) -> Result<(), ConfigError> {
        let Validation {
            release,
            updater_feature,
            build_step,
        } = validation;
        if let Some(uid) = &self.uid
            && !is_valid_uid(uid.trim())
        {
            return Err(ConfigError::new(
                "uid",
                "must be 1 to 64 ASCII letters or digits",
            ));
        }
        let set = |v: &Option<String>| v.as_deref().is_some_and(|s| !s.is_empty());
        if release && build_step && !set(&self.uid) && !(set(&self.author) && set(&self.name)) {
            return Err(ConfigError::new(
                "",
                "set \"uid\", or both \"author\" and \"name\", before a release build (the uid must not depend on defaults)",
            ));
        }
        if !is_valid_host_label(&self.analytics.host_label) {
            return Err(ConfigError::new(
                "analytics.hostLabel",
                "must be 1 to 32 lower-case letters, digits or _ starting with a letter",
            ));
        }
        for (i, glob) in self.analytics.exclude_windows.iter().enumerate() {
            if glob.is_empty() || is_reserved_label(glob) {
                return Err(ConfigError::new(
                    format!("analytics.excludeWindows[{i}]"),
                    "must be a non-empty label glob",
                ));
            }
        }
        if let Some(url) = &self.consent.cmp_url
            && !is_https_url(url)
        {
            return Err(ConfigError::new("consent.cmpUrl", "must be an https URL"));
        }
        for (i, origin) in self.consent.allowed_cmp_origins.iter().enumerate() {
            if !is_origin(origin, &["https"]) {
                return Err(ConfigError::new(
                    format!("consent.allowedCmpOrigins[{i}]"),
                    "must be an https origin (scheme://host[:port], no path)",
                ));
            }
        }
        for (i, origin) in self.ads.allowed_embedder_origins.iter().enumerate() {
            if !is_origin(origin, &["http", "https"]) || is_overwolf_origin(origin) {
                return Err(ConfigError::new(
                    format!("ads.allowedEmbedderOrigins[{i}]"),
                    "must be an http(s) origin that is not an Overwolf origin",
                ));
            }
        }
        for (path, value) in [
            ("consent.readyTimeoutMs", self.consent.ready_timeout_ms),
            ("consent.euOnlyTimeoutMs", self.consent.eu_only_timeout_ms),
        ] {
            if !(1..=600_000).contains(&value) {
                return Err(ConfigError::new(path, "must be 1 to 600000"));
            }
        }
        let limits = &self.ads.guest_limits;
        for (path, value) in [
            (
                "ads.guestLimits.eventsPerSecond",
                u64::from(limits.events_per_second),
            ),
            ("ads.guestLimits.eventBurst", u64::from(limits.event_burst)),
            ("ads.guestLimits.bytesPerSecond", limits.bytes_per_second),
            (
                "ads.guestLimits.externalOpensPerMinute",
                u64::from(limits.external_opens_per_minute),
            ),
            (
                "ads.guestLimits.externalOpensPerMinuteApp",
                u64::from(limits.external_opens_per_minute_app),
            ),
            (
                "ads.guestLimits.activationWindowMs",
                limits.activation_window_ms,
            ),
            (
                "ads.recreateMaxPerHour",
                u64::from(self.ads.recreate_max_per_hour),
            ),
        ] {
            if value == 0 {
                return Err(ConfigError::new(path, "must be greater than 0"));
            }
        }
        for (i, arg) in self.ads.browser_args.iter().enumerate() {
            let name = arg.split('=').next().unwrap_or_default();
            let remote_debugging = name.starts_with("--remote-debugging")
                || name.starts_with("--remote-allow-origins");
            if !arg.starts_with("--")
                || arg.len() < 3
                || name == "--user-data-dir"
                || (release && remote_debugging)
            {
                return Err(ConfigError::new(
                    format!("ads.browserArgs[{i}]"),
                    "must be a --switch other than --user-data-dir (and no remote debugging in release builds)",
                ));
            }
        }
        if let Some(endpoint) = &self.updater.endpoint {
            let loopback_ok = !release && is_loopback_http(endpoint);
            if !is_https_url(endpoint) && !loopback_ok {
                return Err(ConfigError::new("updater.endpoint", "must be an https URL"));
            }
        }
        if self
            .updater
            .pubkey
            .as_deref()
            .is_some_and(|k| k.trim().is_empty())
        {
            return Err(ConfigError::new(
                "updater.pubkey",
                "must not be empty when set",
            ));
        }
        if release && updater_feature {
            let publishers = self
                .updater
                .publisher_names
                .as_ref()
                .is_some_and(|names| names.iter().any(|n| !n.trim().is_empty()));
            if !publishers && self.updater.pubkey.is_none() {
                return Err(ConfigError::new(
                    "updater",
                    "set publisherNames (your installer's certificate subject) or pubkey before a release build",
                ));
            }
        }
        if release && self.updater.dangerous_skip_publisher_check {
            return Err(ConfigError::new(
                "updater.dangerousSkipPublisherCheck",
                "not allowed in release builds",
            ));
        }
        if release && self.state.app_data_dir.is_some() {
            return Err(ConfigError::new(
                "state.appDataDir",
                "only allowed in debug builds",
            ));
        }
        Ok(())
    }

    /// Normalises values that have a fixed rule instead of an error and
    /// returns one warning per change: `updater.installerArgs` with a silent
    /// `/S` always carries `/UPDATE` (an update must not run the uninstall
    /// hooks).
    ///
    /// ```
    /// use tauri_plugin_overwolf::config::Config;
    /// let mut config = Config::from_value(&serde_json::json!({ "updater": { "installerArgs": ["/S"] } })).unwrap();
    /// let warnings = config.normalize();
    /// assert_eq!(config.updater.installer_args.as_deref(), Some(&["/S".to_owned(), "/UPDATE".to_owned()][..]));
    /// assert_eq!(warnings.len(), 1);
    /// ```
    pub fn normalize(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();
        if let Some(args) = &mut self.updater.installer_args
            && args.iter().any(|a| a.eq_ignore_ascii_case("/S"))
            && !args.iter().any(|a| a.eq_ignore_ascii_case("/UPDATE"))
        {
            args.push("/UPDATE".into());
            warnings.push(
                "plugins.overwolf.updater.installerArgs: added /UPDATE (an update must not run the uninstall hooks)"
                    .into(),
            );
        }
        warnings
    }
}

/// The label prefix of ad guest webviews (`owad-<n>`).
pub const ADVIEW_LABEL_PREFIX: &str = "owad-";
/// The label prefix of consent windows (`ow-cmp…`).
pub const CMP_LABEL_PREFIX: &str = "ow-cmp";

/// Whether `label` starts with a prefix reserved for the plugin's own
/// webviews and windows: [`ADVIEW_LABEL_PREFIX`] (ad guests) or
/// [`CMP_LABEL_PREFIX`] (consent windows).
///
/// ```
/// use tauri_plugin_overwolf::config::is_reserved_label;
/// assert!(is_reserved_label("owad-1"));
/// assert!(is_reserved_label("ow-cmp-default"));
/// assert!(!is_reserved_label("main"));
/// ```
#[must_use]
pub fn is_reserved_label(label: &str) -> bool {
    label.starts_with(ADVIEW_LABEL_PREFIX) || label.starts_with(CMP_LABEL_PREFIX)
}

/// Whether `url` parses as an absolute `https:` URL with a host.
///
/// ```
/// use tauri_plugin_overwolf::config::is_https_url;
/// assert!(is_https_url("https://example.com/cmp.html"));
/// assert!(!is_https_url("http://example.com/"));
/// assert!(!is_https_url("cmp.html"));
/// ```
#[must_use]
pub fn is_https_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| u.scheme() == "https" && u.host().is_some())
}

/// Whether `url` is an `http:` URL on a loopback host (debug-only updater
/// endpoints).
fn is_loopback_http(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "http"
            && match u.host() {
                Some(url::Host::Domain(d)) => d == "localhost",
                Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
                Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
                None => false,
            }
    })
}

/// Whether `text` is exactly an origin (`scheme://host[:port]`, no path,
/// query, fragment or credentials) with one of `schemes`.
fn is_origin(text: &str, schemes: &[&str]) -> bool {
    url::Url::parse(text).is_ok_and(|u| {
        schemes.contains(&u.scheme())
            && u.host().is_some()
            && u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none()
            && u.path() == "/"
            && !text.trim_end_matches('/').ends_with(':')
            && text.matches('/').count() == 2
    })
}

/// Whether origin `text` is `overwolf.com` or one of its subdomains.
fn is_overwolf_origin(text: &str) -> bool {
    url::Url::parse(text).is_ok_and(|u| {
        u.host_str().is_some_and(|h| {
            let h = h.trim_end_matches('.').to_ascii_lowercase();
            h == "overwolf.com" || h.ends_with(".overwolf.com")
        })
    })
}

/// Whether `label` is a valid `analytics.hostLabel`: 1 to 32 lower-case ASCII
/// letters, digits or `_`, starting with a letter.
///
/// ```
/// use tauri_plugin_overwolf::config::is_valid_host_label;
/// assert!(is_valid_host_label("tauri"));
/// assert!(!is_valid_host_label("Tauri"));
/// ```
#[must_use]
pub fn is_valid_host_label(label: &str) -> bool {
    (1..=32).contains(&label.len())
        && label.starts_with(|c: char| c.is_ascii_lowercase())
        && label
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse(raw: &Value) -> Config {
        Config::from_value(raw).expect("valid config")
    }

    fn error(raw: &Value, validation: Validation) -> String {
        match Config::from_value(raw) {
            Err(err) => err.to_string(),
            Ok(config) => config
                .validate(validation)
                .expect_err("invalid config")
                .to_string(),
        }
    }

    const DEBUG: Validation = Validation {
        release: false,
        updater_feature: false,
        build_step: false,
    };
    const RELEASE: Validation = Validation {
        release: true,
        updater_feature: false,
        build_step: false,
    };
    const RELEASE_BUILD_STEP: Validation = Validation {
        release: true,
        updater_feature: false,
        build_step: true,
    };
    const RELEASE_UPDATER: Validation = Validation {
        release: true,
        updater_feature: true,
        build_step: false,
    };

    #[test]
    fn defaults() {
        let c = parse(&Value::Null);
        assert_eq!(c, Config::default());
        assert!(c.ads.request_shaping && c.ads.transparent_guests && c.ads.recreate_on_reload);
        assert_eq!(c.ads.load_error_retry_ms, 5_000);
        assert_eq!(c.ads.recreate_min_interval_ms, 30_000);
        assert_eq!(c.ads.recreate_max_per_hour, 30);
        assert_eq!(c.ads.guest_limits.events_per_second, 50);
        assert_eq!(c.ads.guest_limits.event_burst, 100);
        assert_eq!(c.ads.guest_limits.bytes_per_second, 262_144);
        assert_eq!(c.ads.guest_limits.external_opens_per_minute, 20);
        assert_eq!(c.ads.guest_limits.external_opens_per_minute_app, 20);
        assert_eq!(c.ads.guest_limits.activation_window_ms, 5_000);
        assert_eq!(c.analytics.host_label, "tauri");
        assert_eq!(c.analytics.muid_strategy, MuidStrategy::MachineId);
        assert_eq!(
            c.consent.allowed_cmp_origins,
            ["https://content.overwolf.com"]
        );
        assert_eq!(c.consent.ready_timeout_ms, 30_000);
        assert_eq!(c.consent.eu_only_timeout_ms, 60_000);
        assert!(c.updater.install_on_exit);
        assert_eq!(c.updater.connect_timeout_ms, 30_000);
        assert_eq!(c.updater.read_timeout_ms, 60_000);
        assert!(!c.signing.enabled && c.signing.require_signing);
        c.validate(DEBUG).unwrap();
        c.validate(RELEASE).unwrap();
    }

    #[test]
    fn every_key_round_trips() {
        let raw = json!({
            "author": "A", "name": "N", "uid": "abc",
            "ads": {
                "testAd": true, "disableOptimization": true, "disableFpd": true,
                "requestShaping": false, "owVersionOverride": "1.2", "macPrivateHeaderApi": true,
                "transparentGuests": false, "maxRecoveries": 3, "loadErrorRetryMs": 1,
                "recreateOnReload": false, "recreateMinIntervalMs": 2, "recreateMaxPerHour": 3,
                "allowedEmbedderOrigins": ["http://localhost:1430"], "browserArgs": ["--x"],
                "guestLimits": {
                    "eventsPerSecond": 1, "eventBurst": 2, "bytesPerSecond": 3,
                    "externalOpensPerMinute": 4, "externalOpensPerMinuteApp": 5,
                    "activationWindowMs": 6
                }
            },
            "analytics": {
                "hostLabel": "electron", "hostVersion": "42", "muidStrategy": "per-install",
                "disableAnonymous": true, "excludeWindows": ["tray*"], "userSwitch": true
            },
            "consent": {
                "cmpUrl": "https://example.com/cmp.html", "allowedCmpOrigins": ["https://example.com"],
                "readyTimeoutMs": 1, "euOnlyTimeoutMs": 2, "hostCookieFallback": "never"
            },
            "emailHashes": { "encoding": "base64" },
            "updater": {
                "endpoint": "https://example.com/feed", "channel": "beta", "allowPrerelease": true,
                "allowDowngrade": true, "allowJsDowngrade": true, "installOnExit": false,
                "installerArgs": ["/S", "/UPDATE"], "publisherNames": ["CN=Example"], "pubkey": "k",
                "dangerousSkipPublisherCheck": true, "connectTimeoutMs": 1, "readTimeoutMs": 2
            },
            "signing": { "enabled": true, "requireSigning": false, "owCertSigning": true, "entry": "index.html" },
            "state": { "appDataDir": "/tmp/x" }
        });
        let c = parse(&raw);
        assert_eq!(serde_json::to_value(&c).unwrap(), raw);
        c.validate(DEBUG).unwrap();
    }

    #[test]
    fn removed_keys_carry_a_migration_hint() {
        for (raw, path) in [
            (json!({ "main": {} }), "main"),
            (json!({ "ipc": {} }), "ipc"),
            (json!({ "shell": {} }), "shell"),
            (json!({ "fs": { "scope": [] } }), "fs"),
            (json!({ "webview": { "disableGpu": true } }), "webview"),
            (json!({ "packagesBackend": "none" }), "packagesBackend"),
            (json!({ "logging": { "enabled": true } }), "logging"),
            (
                json!({ "ads": { "gestureWindowMs": 1 } }),
                "ads.gestureWindowMs",
            ),
            (
                json!({ "ads": { "guestHeartbeatTimeoutMs": 1 } }),
                "ads.guestHeartbeatTimeoutMs",
            ),
            (
                json!({ "ads": { "recreateOnReload": "auto" } }),
                "ads.recreateOnReload",
            ),
        ] {
            let err = Config::from_value(&raw).unwrap_err();
            assert_eq!(err.path, path);
            let text = err.to_string();
            assert!(
                text.starts_with(&format!(
                    "plugins.overwolf.{path}: removed in ow-tauri 1.0 ("
                )),
                "{text}"
            );
            assert!(text.contains("; see docs/MIGRATION.md#"), "{text}");
        }
    }

    /// Regression (W1 gate): Tauri parses `plugins.overwolf` with
    /// `serde_json::from_value`, which used to bypass `from_value` and lose
    /// the migration hints.
    #[test]
    fn tauris_own_parse_keeps_the_hints() {
        let err = serde_json::from_value::<Config>(json!({ "main": {} })).unwrap_err();
        let text = err.to_string();
        assert!(
            text.starts_with("plugins.overwolf.main: removed in ow-tauri 1.0 ("),
            "{text}"
        );
        assert!(text.contains("docs/MIGRATION.md#no-main-webview"), "{text}");
        let none = serde_json::from_value::<Option<Config>>(Value::Null).unwrap();
        assert!(none.is_none());
        let some = serde_json::from_value::<Option<Config>>(json!({ "author": "A" })).unwrap();
        assert_eq!(some.unwrap().author.as_deref(), Some("A"));
    }

    #[test]
    fn unknown_keys_name_their_object() {
        let err = Config::from_value(&json!({ "nope": 1 })).unwrap_err();
        assert_eq!(err.path, "");
        assert!(
            err.message
                .starts_with("unknown field \"nope\", expected one of"),
            "{}",
            err.message
        );
        let err = Config::from_value(&json!({ "ads": { "guestLimits": { "x": 1 } } })).unwrap_err();
        assert_eq!(err.path, "ads.guestLimits");
        let err = Config::from_value(&json!({ "consent": { "cmpURL": "x" } })).unwrap_err();
        assert_eq!(err.path, "consent");
        assert!(err.to_string().contains("unknown field \"cmpURL\""));
    }

    #[test]
    fn wrong_types_name_the_leaf() {
        let err = Config::from_value(&json!({ "analytics": { "userSwitch": "yes" } })).unwrap_err();
        assert_eq!(err.path, "analytics.userSwitch");
        let err = Config::from_value(&json!({ "uid": 7 })).unwrap_err();
        assert_eq!(err.path, "uid");
        let err = Config::from_value(&json!({ "ads": { "guestLimits": { "eventBurst": -1 } } }))
            .unwrap_err();
        assert_eq!(err.path, "ads.guestLimits.eventBurst");
    }

    /// Every row of the DESIGN §3.2 validation table.
    #[test]
    #[expect(clippy::too_many_lines, reason = "one row per table entry")]
    fn validation_table() {
        let rows: Vec<(Value, Validation, &str)> = vec![
            (
                json!({ "uid": "  " }),
                DEBUG,
                "plugins.overwolf.uid: must be 1 to 64 ASCII letters or digits",
            ),
            (
                json!({ "uid": "a".repeat(65) }),
                DEBUG,
                "plugins.overwolf.uid: must be 1 to 64 ASCII letters or digits",
            ),
            (
                json!({ "author": "A" }),
                RELEASE_BUILD_STEP,
                "plugins.overwolf: set \"uid\", or both \"author\" and \"name\", before a release build (the uid must not depend on defaults)",
            ),
            (
                json!({ "analytics": { "hostLabel": "Tauri" } }),
                DEBUG,
                "plugins.overwolf.analytics.hostLabel: must be 1 to 32 lower-case letters, digits or _ starting with a letter",
            ),
            (
                json!({ "analytics": { "excludeWindows": ["ok", ""] } }),
                DEBUG,
                "plugins.overwolf.analytics.excludeWindows[1]: must be a non-empty label glob",
            ),
            (
                json!({ "analytics": { "excludeWindows": ["owad-*"] } }),
                DEBUG,
                "plugins.overwolf.analytics.excludeWindows[0]: must be a non-empty label glob",
            ),
            (
                json!({ "consent": { "cmpUrl": "http://example.com/" } }),
                DEBUG,
                "plugins.overwolf.consent.cmpUrl: must be an https URL",
            ),
            (
                json!({ "consent": { "allowedCmpOrigins": ["https://example.com/path"] } }),
                DEBUG,
                "plugins.overwolf.consent.allowedCmpOrigins[0]: must be an https origin (scheme://host[:port], no path)",
            ),
            (
                json!({ "consent": { "allowedCmpOrigins": ["http://example.com"] } }),
                DEBUG,
                "plugins.overwolf.consent.allowedCmpOrigins[0]: must be an https origin (scheme://host[:port], no path)",
            ),
            (
                json!({ "ads": { "allowedEmbedderOrigins": ["https://www.overwolf.com"] } }),
                DEBUG,
                "plugins.overwolf.ads.allowedEmbedderOrigins[0]: must be an http(s) origin that is not an Overwolf origin",
            ),
            (
                json!({ "ads": { "allowedEmbedderOrigins": ["file:///x"] } }),
                DEBUG,
                "plugins.overwolf.ads.allowedEmbedderOrigins[0]: must be an http(s) origin that is not an Overwolf origin",
            ),
            (
                json!({ "consent": { "readyTimeoutMs": 0 } }),
                DEBUG,
                "plugins.overwolf.consent.readyTimeoutMs: must be 1 to 600000",
            ),
            (
                json!({ "consent": { "euOnlyTimeoutMs": 600_001 } }),
                DEBUG,
                "plugins.overwolf.consent.euOnlyTimeoutMs: must be 1 to 600000",
            ),
            (
                json!({ "ads": { "guestLimits": { "eventBurst": 0 } } }),
                DEBUG,
                "plugins.overwolf.ads.guestLimits.eventBurst: must be greater than 0",
            ),
            (
                json!({ "ads": { "recreateMaxPerHour": 0 } }),
                DEBUG,
                "plugins.overwolf.ads.recreateMaxPerHour: must be greater than 0",
            ),
            (
                json!({ "ads": { "browserArgs": ["disable-gpu"] } }),
                DEBUG,
                "plugins.overwolf.ads.browserArgs[0]: must be a --switch other than --user-data-dir (and no remote debugging in release builds)",
            ),
            (
                json!({ "ads": { "browserArgs": ["--user-data-dir=/x"] } }),
                DEBUG,
                "plugins.overwolf.ads.browserArgs[0]: must be a --switch other than --user-data-dir (and no remote debugging in release builds)",
            ),
            (
                json!({ "ads": { "browserArgs": ["--remote-debugging-port=9222"] } }),
                RELEASE,
                "plugins.overwolf.ads.browserArgs[0]: must be a --switch other than --user-data-dir (and no remote debugging in release builds)",
            ),
            (
                json!({ "ads": { "browserArgs": ["--ok", "--remote-allow-origins=*"] } }),
                RELEASE,
                "plugins.overwolf.ads.browserArgs[1]: must be a --switch other than --user-data-dir (and no remote debugging in release builds)",
            ),
            (
                json!({ "updater": { "endpoint": "http://example.com/feed" } }),
                DEBUG,
                "plugins.overwolf.updater.endpoint: must be an https URL",
            ),
            (
                json!({ "updater": { "endpoint": "http://127.0.0.1:8080/feed", "pubkey": "k" } }),
                RELEASE,
                "plugins.overwolf.updater.endpoint: must be an https URL",
            ),
            (
                json!({ "updater": { "pubkey": " " } }),
                DEBUG,
                "plugins.overwolf.updater.pubkey: must not be empty when set",
            ),
            (
                json!({}),
                RELEASE_UPDATER,
                "plugins.overwolf.updater: set publisherNames (your installer's certificate subject) or pubkey before a release build",
            ),
            (
                json!({ "updater": { "publisherNames": [" "] } }),
                RELEASE_UPDATER,
                "plugins.overwolf.updater: set publisherNames (your installer's certificate subject) or pubkey before a release build",
            ),
            (
                json!({ "updater": { "dangerousSkipPublisherCheck": true, "pubkey": "k" } }),
                RELEASE_UPDATER,
                "plugins.overwolf.updater.dangerousSkipPublisherCheck: not allowed in release builds",
            ),
            (
                json!({ "state": { "appDataDir": "/tmp/x" } }),
                RELEASE,
                "plugins.overwolf.state.appDataDir: only allowed in debug builds",
            ),
        ];
        for (raw, validation, expected) in rows {
            assert_eq!(error(&raw, validation), expected, "{raw}");
        }
    }

    /// The release-only rules accept the same values in debug builds, and
    /// the pinned-uid rule only runs in the build step.
    #[test]
    fn release_only_rules_pass_in_debug() {
        for raw in [
            json!({ "author": "A" }),
            json!({ "ads": { "browserArgs": ["--remote-debugging-port=9222"] } }),
            json!({ "updater": { "endpoint": "http://127.0.0.1:8080/feed" } }),
            json!({ "updater": { "endpoint": "http://localhost:8080/feed" } }),
            json!({ "updater": { "dangerousSkipPublisherCheck": true } }),
            json!({ "state": { "appDataDir": "/tmp/x" } }),
        ] {
            parse(&raw).validate(DEBUG).unwrap();
        }
        // The pinned-uid rule is the build step's; at run time the uid is
        // already fixed by the build.
        parse(&json!({ "author": "A" })).validate(RELEASE).unwrap();
        for raw in [
            json!({ "uid": "abc" }),
            json!({ "author": "A", "name": "N" }),
        ] {
            parse(&raw).validate(RELEASE_BUILD_STEP).unwrap();
        }
        // Either publisher data satisfies the updater rule.
        for raw in [
            json!({ "updater": { "publisherNames": ["CN=Example"] } }),
            json!({ "updater": { "pubkey": "k" } }),
        ] {
            parse(&raw).validate(RELEASE_UPDATER).unwrap();
        }
    }

    #[test]
    fn accepted_values() {
        for raw in [
            json!({ "uid": "  abc  " }),
            json!({ "ads": { "allowedEmbedderOrigins": ["http://localhost:1430", "https://app.example.com:8443"] } }),
            json!({ "consent": { "allowedCmpOrigins": ["https://content.overwolf.com", "https://cmp.example.com:444"] } }),
            json!({ "consent": { "readyTimeoutMs": 600_000, "euOnlyTimeoutMs": 1 } }),
            json!({ "analytics": { "excludeWindows": ["tray", "hud-?", "*-overlay"] } }),
            json!({ "ads": { "browserArgs": ["--disable-gpu", "--lang=en"] } }),
        ] {
            parse(&raw).validate(DEBUG).unwrap();
        }
    }

    #[test]
    fn normalize_adds_update_once() {
        let mut c = parse(&json!({ "updater": { "installerArgs": ["/S", "/update"] } }));
        assert!(c.normalize().is_empty());
        let mut c = parse(&json!({ "updater": { "installerArgs": ["--force-run"] } }));
        assert!(c.normalize().is_empty());
        assert_eq!(
            c.updater.installer_args.as_deref(),
            Some(&["--force-run".to_owned()][..])
        );
    }

    #[test]
    fn origins() {
        assert!(is_origin("https://a.example", &["https"]));
        assert!(is_origin("https://a.example:8443", &["https"]));
        assert!(!is_origin("https://a.example/", &["https"]));
        assert!(!is_origin("https://user@a.example", &["https"]));
        assert!(!is_origin("https://a.example?x", &["https"]));
        assert!(is_overwolf_origin("https://overwolf.com"));
        assert!(is_overwolf_origin("https://content.overwolf.com"));
        assert!(!is_overwolf_origin("https://notoverwolf.com"));
        assert!(is_loopback_http("http://[::1]:1/x"));
        assert!(!is_loopback_http("http://example.com/x"));
    }
}
