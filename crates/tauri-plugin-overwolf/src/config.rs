//! Plugin configuration (CONTRACT A.1).
//!
//! Read from `tauri.conf.json > plugins > overwolf`, then overridden by
//! [`Builder`](crate::Builder) calls, then by environment variables, then by
//! command line switches (last wins). Every field is optional; the defaults
//! are the contract's.
//!
//! ```
//! use tauri_plugin_overwolf::config::Config;
//! let cfg: Config = serde_json::from_str(r#"{ "ads": { "testAd": true }, "ipc": { "invokeTimeoutMs": 5000 } }"#).unwrap();
//! assert!(cfg.ads.test_ad);
//! assert_eq!(cfg.ipc.invoke_timeout_ms, 5000);
//! assert_eq!(cfg.ipc.max_in_flight_invokes, 256);
//! cfg.validate().unwrap();
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::identity::{HashEncoding, is_valid_uid};
use crate::packages::PackagesBackend;

/// Prefix every consent page URL must have (CONTRACT D.6).
pub const CMP_URL_SCOPE: &str = "https://content.overwolf.com/monsdk/electron/";

/// `plugins.overwolf`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Config {
    /// The hidden main webview.
    pub main: MainConfig,
    /// One browser-argument set for every webview (A.1.1).
    pub webview: WebviewConfig,
    /// Console-assigned uid; overrides the computed uid (G.2).
    pub uid: Option<String>,
    /// Which package runtime to use (H.1).
    pub packages_backend: PackagesBackend,
    /// Ads host options.
    pub ads: AdsConfig,
    /// Analytics options.
    pub analytics: AnalyticsConfig,
    /// Consent options.
    pub consent: ConsentConfig,
    /// Email hash options.
    pub email_hashes: EmailHashesConfig,
    /// IPC router limits.
    pub ipc: IpcConfig,
    /// Update client options.
    pub updater: UpdaterConfig,
    /// `shell.openPath` options.
    pub shell: ShellConfig,
    /// Scoped file access options.
    pub fs: FsConfig,
    /// State file options.
    pub state: StateConfig,
}

/// `plugins.overwolf.main`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct MainConfig {
    /// App asset loaded into the hidden main webview.
    pub url: String,
    /// Open devtools for `ow-main` in debug builds.
    pub devtools: bool,
    /// `ow-main` crashes per 60 s before the app exits (A.6).
    pub crash_restart_limit: u32,
}

impl Default for MainConfig {
    fn default() -> Self {
        MainConfig {
            url: "main.html".into(),
            devtools: false,
            crash_restart_limit: 3,
        }
    }
}

/// `plugins.overwolf.webview` (A.1.1).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct WebviewConfig {
    /// `--disable-gpu` (Windows).
    pub disable_gpu: bool,
    /// `--remote-debugging-port=<n>` (Windows, debug builds).
    pub remote_debugging_port: Option<u16>,
    /// Appended verbatim (Windows).
    pub additional_browser_args: Vec<String>,
}

/// `plugins.overwolf.ads`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent contract switches, each documented in CONTRACT A.1"
)]
pub struct AdsConfig {
    /// Test inventory (same as `--test-ad`).
    pub test_ad: bool,
    /// User-gesture window for guest top-level navigation.
    pub gesture_window_ms: u64,
    /// Guest reloads after crashes, per element.
    pub max_recoveries: u32,
    /// Reload delay after a failed guest load.
    pub load_error_retry_ms: u64,
    /// OQ-11.
    pub expose_email_hashes_to_guest: bool,
    /// Extra guest request headers, OQ-05.
    pub request_shaping: bool,
    /// `pageUrl`, `setPageUrl()`, `sendCommand()` (OQ-32).
    pub experimental_element_api: bool,
    /// Per-guest limits (D.4).
    pub guest_limits: GuestLimits,
}

impl Default for AdsConfig {
    fn default() -> Self {
        AdsConfig {
            test_ad: false,
            gesture_window_ms: 1500,
            max_recoveries: 10,
            load_error_retry_ms: 5000,
            expose_email_hashes_to_guest: false,
            request_shaping: false,
            experimental_element_api: false,
            guest_limits: GuestLimits::default(),
        }
    }
}

/// `plugins.overwolf.ads.guestLimits` (D.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct GuestLimits {
    /// Sustained guest messages per second.
    pub events_per_second: u32,
    /// Burst size of the message bucket.
    pub event_burst: u32,
    /// Encoded `data` bytes per second.
    pub bytes_per_second: u64,
    /// System-browser opens per minute.
    pub external_opens_per_minute: u32,
}

impl Default for GuestLimits {
    fn default() -> Self {
        GuestLimits {
            events_per_second: 50,
            event_burst: 100,
            bytes_per_second: 262_144,
            external_opens_per_minute: 5,
        }
    }
}

/// `plugins.overwolf.analytics`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct AnalyticsConfig {
    /// Append `host`, `hostVersion`, `platform` (OQ-03).
    pub host_fields: bool,
    /// How the muid is derived (OQ-02).
    pub muid_strategy: MuidStrategy,
    /// Expose `analytics_set_user_enabled`.
    pub user_switch: bool,
}

/// `analytics.muidStrategy` (E.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MuidStrategy {
    /// Random UUID v4, upper-case, stored in `ow-tauri.json`.
    #[default]
    PerInstall,
    /// Derived from the OS machine identifier; unavailable until Overwolf
    /// specifies the derivation (falls back to `per-install` with a warning).
    MachineId,
}

/// `plugins.overwolf.consent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ConsentConfig {
    /// What `isCMPRequired()` returns (OQ-06).
    pub cmp_required: CmpRequired,
    /// Default consent page URL override (D.6 scope only).
    pub cmp_url: Option<String>,
    /// The first ad mount waits for consent readiness.
    pub gate_ads_on_consent: bool,
    /// The consent page must send `ready` within this time.
    pub ready_timeout_ms: u64,
}

impl Default for ConsentConfig {
    fn default() -> Self {
        ConsentConfig {
            cmp_required: CmpRequired::Always,
            cmp_url: None,
            gate_ads_on_consent: true,
            ready_timeout_ms: 30_000,
        }
    }
}

/// `consent.cmpRequired`: `"always"`, `"never"` or `{ "url": "https://..." }`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CmpRequired {
    /// `isCMPRequired()` resolves `true`.
    #[default]
    Always,
    /// `isCMPRequired()` resolves `false`.
    Never,
    /// Fetched once per session from this HTTPS URL.
    Url(String),
}

impl Serialize for CmpRequired {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            CmpRequired::Always => s.serialize_str("always"),
            CmpRequired::Never => s.serialize_str("never"),
            CmpRequired::Url(url) => {
                use serde::ser::SerializeMap;
                let mut map = s.serialize_map(Some(1))?;
                map.serialize_entry("url", url)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for CmpRequired {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct UrlForm {
            url: String,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Word(String),
            Url(UrlForm),
        }
        match Raw::deserialize(d)? {
            Raw::Word(w) if w == "always" => Ok(CmpRequired::Always),
            Raw::Word(w) if w == "never" => Ok(CmpRequired::Never),
            Raw::Word(w) => Err(serde::de::Error::custom(format!(
                "cmpRequired must be \"always\", \"never\" or {{ \"url\": ... }}, got \"{w}\""
            ))),
            Raw::Url(u) => Ok(CmpRequired::Url(u.url)),
        }
    }
}

/// `plugins.overwolf.emailHashes`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct EmailHashesConfig {
    /// Output encoding (OQ-10).
    pub encoding: HashEncoding,
}

/// `plugins.overwolf.ipc` (C.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct IpcConfig {
    /// 0 = no timeout (Electron behaviour).
    pub invoke_timeout_ms: u64,
    /// Requests buffered before `ow-main` is ready.
    pub startup_queue_max: usize,
    /// A buffered request fails with `not-ready` after this.
    pub startup_timeout_ms: u64,
    /// Encoded size cap per message.
    pub max_message_bytes: usize,
    /// Per sender webview; more reject with `ipc-overloaded`.
    pub max_in_flight_invokes: usize,
    /// Per receiving webview; more reject the sender with `ipc-overloaded`.
    pub max_queued_messages: usize,
}

impl Default for IpcConfig {
    fn default() -> Self {
        IpcConfig {
            invoke_timeout_ms: 0,
            startup_queue_max: 1024,
            startup_timeout_ms: 30_000,
            max_message_bytes: 8_388_608,
            max_in_flight_invokes: 256,
            max_queued_messages: 4096,
        }
    }
}

/// `plugins.overwolf.updater` (I).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct UpdaterConfig {
    /// Whether the update client runs.
    pub enabled: bool,
    /// Installer arguments override.
    pub installer_args: Option<Vec<String>>,
    /// Windows Authenticode subjects; `None` = the running exe's signer.
    pub publisher_names: Option<Vec<String>>,
    /// Minisign public key; required on Linux.
    pub pubkey: Option<String>,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        UpdaterConfig {
            enabled: true,
            installer_args: None,
            publisher_names: None,
            pubkey: None,
        }
    }
}

/// `plugins.overwolf.shell`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ShellConfig {
    /// Allow `shell.openPath` to open executables and launchers (A.2.3.2).
    pub open_path_allow_executables: bool,
}

/// `plugins.overwolf.fs`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct FsConfig {
    /// Extra read-write directory templates for `fs_*` (A.2.3).
    pub scope: Vec<String>,
}

/// `plugins.overwolf.state`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct StateConfig {
    /// Overrides the OS configuration directory (`<appData>`), for tests.
    pub app_data_dir: Option<PathBuf>,
}

/// A configuration value that fails validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("plugins.overwolf.{path}: {message}")]
pub struct ConfigError {
    /// Dot path below `plugins.overwolf`.
    pub path: String,
    /// What is wrong.
    pub message: String,
}

fn invalid(path: &str, message: impl Into<String>) -> ConfigError {
    ConfigError {
        path: path.into(),
        message: message.into(),
    }
}

/// The variables allowed at the start of an `fs.scope` template.
pub const FS_SCOPE_VARIABLES: [&str; 6] = [
    "$USERDATA",
    "$PICTURES",
    "$VIDEOS",
    "$DOCUMENTS",
    "$DOWNLOADS",
    "$TEMP",
];

/// Whether `url` is an allowed consent page URL (D.6).
///
/// ```
/// use tauri_plugin_overwolf::config::is_allowed_cmp_url;
/// assert!(is_allowed_cmp_url("https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/cmp.html"));
/// assert!(!is_allowed_cmp_url("https://content.overwolf.com.evil.example/monsdk/electron/x"));
/// assert!(!is_allowed_cmp_url("http://content.overwolf.com/monsdk/electron/x"));
/// ```
#[must_use]
pub fn is_allowed_cmp_url(url: &str) -> bool {
    url.starts_with(CMP_URL_SCOPE)
        && !url.contains('\\')
        && !url[CMP_URL_SCOPE.len()..]
            .split(['/', '?', '#'])
            .any(|s| s == "..")
}

impl Config {
    /// Checks every field against the contract's rules.
    ///
    /// # Errors
    ///
    /// The first [`ConfigError`] found, naming the field.
    ///
    /// ```
    /// use tauri_plugin_overwolf::config::Config;
    /// assert!(Config::default().validate().is_ok());
    /// let mut c = Config::default();
    /// c.main.url = "https://example.com/main.html".into();
    /// assert!(c.validate().is_err());
    /// ```
    pub fn validate(&self) -> Result<(), ConfigError> {
        let main_url = self.main.url.trim();
        if main_url.is_empty() {
            return Err(invalid("main.url", "must not be empty"));
        }
        if main_url.contains("://") || main_url.starts_with("//") || main_url.contains('\\') {
            return Err(invalid(
                "main.url",
                "must be a path to an app asset, not a URL",
            ));
        }
        if main_url.split('/').any(|s| s == "..") {
            return Err(invalid("main.url", "must not contain .. segments"));
        }
        if self.main.crash_restart_limit == 0 {
            return Err(invalid("main.crashRestartLimit", "must be at least 1"));
        }
        if self.webview.remote_debugging_port == Some(0) {
            return Err(invalid(
                "webview.remoteDebuggingPort",
                "must be a port between 1 and 65535",
            ));
        }
        for (i, arg) in self.webview.additional_browser_args.iter().enumerate() {
            if !arg.starts_with("--") || arg.chars().any(char::is_whitespace) {
                return Err(invalid(
                    &format!("webview.additionalBrowserArgs[{i}]"),
                    "each entry must be one --switch without whitespace",
                ));
            }
        }
        if let Some(uid) = &self.uid
            && !is_valid_uid(uid)
        {
            return Err(invalid("uid", "must be 1 to 64 ASCII letters or digits"));
        }
        if self.ads.gesture_window_ms > 60_000 {
            return Err(invalid("ads.gestureWindowMs", "must be at most 60000"));
        }
        let limits = &self.ads.guest_limits;
        for (name, value) in [
            (
                "ads.guestLimits.eventsPerSecond",
                u64::from(limits.events_per_second),
            ),
            ("ads.guestLimits.eventBurst", u64::from(limits.event_burst)),
            ("ads.guestLimits.bytesPerSecond", limits.bytes_per_second),
        ] {
            if value == 0 {
                return Err(invalid(name, "must be greater than 0"));
            }
        }
        if let CmpRequired::Url(url) = &self.consent.cmp_required
            && !url.starts_with("https://")
        {
            return Err(invalid("consent.cmpRequired.url", "must be an https URL"));
        }
        if let Some(url) = &self.consent.cmp_url
            && !is_allowed_cmp_url(url)
        {
            return Err(invalid(
                "consent.cmpUrl",
                format!("must start with {CMP_URL_SCOPE}"),
            ));
        }
        if self.consent.ready_timeout_ms == 0 {
            return Err(invalid("consent.readyTimeoutMs", "must be greater than 0"));
        }
        let ipc = &self.ipc;
        for (name, value) in [
            ("ipc.startupQueueMax", ipc.startup_queue_max),
            ("ipc.maxMessageBytes", ipc.max_message_bytes),
            ("ipc.maxInFlightInvokes", ipc.max_in_flight_invokes),
            ("ipc.maxQueuedMessages", ipc.max_queued_messages),
        ] {
            if value == 0 {
                return Err(invalid(name, "must be greater than 0"));
            }
        }
        if ipc.startup_timeout_ms == 0 {
            return Err(invalid("ipc.startupTimeoutMs", "must be greater than 0"));
        }
        if ipc.max_message_bytes > 256 * 1024 * 1024 {
            return Err(invalid("ipc.maxMessageBytes", "must be at most 268435456"));
        }
        if let Some(key) = &self.updater.pubkey
            && key.trim().is_empty()
        {
            return Err(invalid("updater.pubkey", "must not be empty when set"));
        }
        for (i, entry) in self.fs.scope.iter().enumerate() {
            validate_scope_template(entry).map_err(|m| invalid(&format!("fs.scope[{i}]"), m))?;
        }
        Ok(())
    }
}

fn validate_scope_template(entry: &str) -> Result<(), &'static str> {
    if entry.trim().is_empty() {
        return Err("must not be empty");
    }
    let rest = if let Some(var) = FS_SCOPE_VARIABLES
        .iter()
        .find(|v| entry == **v || entry.starts_with(&format!("{v}/")))
    {
        &entry[var.len()..]
    } else if std::path::Path::new(entry).is_absolute() {
        entry
    } else if entry.starts_with('$') && !entry.starts_with("$APPNAME") {
        return Err(
            "unknown variable; use $USERDATA, $PICTURES, $VIDEOS, $DOCUMENTS, $DOWNLOADS or $TEMP",
        );
    } else {
        return Err("must be absolute or start with a variable such as $USERDATA");
    };
    if rest.split(['/', '\\']).any(|s| s == "..") {
        return Err("must not contain .. segments");
    }
    Ok(())
}

/// Switches read from the process arguments at setup (A.1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Switches {
    /// `--test-ad`.
    pub test_ad: bool,
    /// `--owepm-package-channel=<pkg>:<channel>[,...]`.
    pub package_channel: Option<String>,
    /// `--force-phased-package[=<pkg>,...]`: `Some(None)` for the bare switch.
    pub force_phased_package: Option<Option<String>>,
    /// `--owepm-packages-url=<url>`.
    pub packages_url: Option<String>,
    /// `--ow-tauri-packages-backend=<value>`.
    pub packages_backend: Option<String>,
    /// `--ow-tauri-relaunch-after=<pid>` (I.4); the process exits early.
    pub relaunch_after: Option<String>,
}

impl Switches {
    /// Parses the switches from process arguments (the first item, the
    /// executable, is skipped). Unknown arguments are ignored.
    ///
    /// ```
    /// use tauri_plugin_overwolf::config::Switches;
    /// let s = Switches::parse(["app", "--test-ad", "--force-phased-package", "--owepm-packages-url=https://x"]);
    /// assert!(s.test_ad);
    /// assert_eq!(s.force_phased_package, Some(None));
    /// assert_eq!(s.packages_url.as_deref(), Some("https://x"));
    /// ```
    pub fn parse<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut out = Switches::default();
        for arg in args.into_iter().skip(1) {
            let arg = arg.as_ref();
            let (name, value) = match arg.split_once('=') {
                Some((n, v)) => (n, Some(v.to_owned())),
                None => (arg, None),
            };
            match name {
                "--test-ad" => out.test_ad = true,
                "--owepm-package-channel" => out.package_channel = value,
                "--force-phased-package" => out.force_phased_package = Some(value),
                "--owepm-packages-url" => out.packages_url = value,
                "--ow-tauri-packages-backend" => out.packages_backend = value,
                "--ow-tauri-relaunch-after" => out.relaunch_after = value,
                _ => {}
            }
        }
        out
    }

    /// Whether `argv` contains `name` as a switch (`--name` or `--name=...`),
    /// as `app.commandLine.hasSwitch()` does.
    ///
    /// ```
    /// use tauri_plugin_overwolf::config::Switches;
    /// assert!(Switches::has_switch(&["a".into(), "--x=1".into()], "x"));
    /// ```
    #[must_use]
    pub fn has_switch(argv: &[String], name: &str) -> bool {
        let flag = format!("--{name}");
        argv.iter()
            .skip(1)
            .any(|a| a == &flag || a.starts_with(&format!("{flag}=")))
    }
}

/// Values taken from the environment (A.1), applied after the builder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvOverrides {
    /// `OW_TAURI_TEST_AD=1`.
    pub test_ad: bool,
    /// `OW_TAURI_PACKAGES_BACKEND`.
    pub packages_backend: Option<String>,
    /// `OW_TAURI_PACKAGE_RUNTIME`: path of a sidecar or shared library (H).
    pub package_runtime: Option<PathBuf>,
    /// `OW_TAURI_REMOTE_DEBUGGING_PORT` (debug builds only).
    pub remote_debugging_port: Option<u16>,
    /// `OW_CLI_EMAIL`, passed to the native runtime only.
    pub dev_email: Option<String>,
    /// `OW_CLI_API_KEY`, passed to the native runtime only.
    pub dev_api_key: Option<String>,
    /// `OW_DEV_KEY`, passed to the native runtime only.
    pub dev_key: Option<String>,
}

impl EnvOverrides {
    /// Reads the variables through `get` (`std::env::var` in production).
    /// `debug` gates `OW_TAURI_REMOTE_DEBUGGING_PORT`. Returns the overrides
    /// and warnings for values that could not be used.
    ///
    /// ```
    /// use tauri_plugin_overwolf::config::EnvOverrides;
    /// let (env, warnings) = EnvOverrides::read(|k| (k == "OW_TAURI_TEST_AD").then(|| "1".to_owned()), true);
    /// assert!(env.test_ad);
    /// assert!(warnings.is_empty());
    /// ```
    pub fn read(get: impl Fn(&str) -> Option<String>, debug: bool) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let non_empty = |k: &str| get(k).filter(|v| !v.trim().is_empty());
        let port = match non_empty("OW_TAURI_REMOTE_DEBUGGING_PORT") {
            Some(_) if !debug => {
                warnings
                    .push("OW_TAURI_REMOTE_DEBUGGING_PORT is ignored in release builds".to_owned());
                None
            }
            Some(v) => match v.trim().parse::<u16>() {
                Ok(p) if p > 0 => Some(p),
                _ => {
                    warnings.push(format!(
                        "OW_TAURI_REMOTE_DEBUGGING_PORT=\"{v}\" is not a port; ignored"
                    ));
                    None
                }
            },
            None => None,
        };
        let env = EnvOverrides {
            test_ad: get("OW_TAURI_TEST_AD").is_some_and(|v| v.trim() == "1"),
            packages_backend: non_empty("OW_TAURI_PACKAGES_BACKEND"),
            package_runtime: non_empty("OW_TAURI_PACKAGE_RUNTIME").map(PathBuf::from),
            remote_debugging_port: port,
            dev_email: non_empty("OW_CLI_EMAIL"),
            dev_api_key: non_empty("OW_CLI_API_KEY"),
            dev_key: non_empty("OW_DEV_KEY"),
        };
        (env, warnings)
    }
}

/// Applies environment variables and command line switches to `config` (in
/// that order, last wins) and returns warnings for unusable values.
///
/// ```
/// use tauri_plugin_overwolf::config::{apply_overrides, Config, EnvOverrides, Switches};
/// use tauri_plugin_overwolf::PackagesBackend;
/// let mut cfg = Config::default();
/// let env = EnvOverrides { packages_backend: Some("none".into()), ..Default::default() };
/// let sw = Switches::parse(["app", "--ow-tauri-packages-backend=simulated"]);
/// let warnings = apply_overrides(&mut cfg, &env, &sw);
/// assert!(warnings.is_empty());
/// assert_eq!(cfg.packages_backend, PackagesBackend::Simulated);
/// ```
pub fn apply_overrides(
    config: &mut Config,
    env: &EnvOverrides,
    switches: &Switches,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if env.test_ad {
        config.ads.test_ad = true;
    }
    if let Some(port) = env.remote_debugging_port {
        config.webview.remote_debugging_port = Some(port);
    }
    for (source, value) in [
        ("OW_TAURI_PACKAGES_BACKEND", env.packages_backend.as_deref()),
        (
            "--ow-tauri-packages-backend",
            switches.packages_backend.as_deref(),
        ),
    ] {
        if let Some(value) = value {
            match value.trim().parse::<PackagesBackend>() {
                Ok(b) => config.packages_backend = b,
                Err(_) => warnings.push(format!(
                    "{source}=\"{value}\" is not auto, native, simulated or none; ignored"
                )),
            }
        }
    }
    if switches.test_ad {
        config.ads.test_ad = true;
    }
    warnings
}

/// The browser arguments for every webview on Windows (A.1.1), as one string.
///
/// `pending` are the switches app code recorded during the previous session
/// (`--disable-gpu`, `--remote-debugging-port=<n>`), applied now.
///
/// ```
/// use tauri_plugin_overwolf::config::{browser_args, WebviewConfig};
/// let args = browser_args(&WebviewConfig { disable_gpu: true, ..Default::default() }, &[], false);
/// assert!(args.starts_with("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection"));
/// assert!(args.ends_with("--disable-gpu"));
/// ```
#[must_use]
pub fn browser_args(webview: &WebviewConfig, pending: &[String], debug: bool) -> String {
    let mut args: Vec<String> = vec![
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection".into(),
        "--disable-background-timer-throttling".into(),
        "--disable-renderer-backgrounding".into(),
        "--disable-backgrounding-occluded-windows".into(),
    ];
    let pending_gpu = pending.iter().any(|a| a == "--disable-gpu");
    if webview.disable_gpu || pending_gpu {
        args.push("--disable-gpu".into());
    }
    let pending_port = pending
        .iter()
        .find_map(|a| a.strip_prefix("--remote-debugging-port="))
        .and_then(|p| p.parse::<u16>().ok())
        .filter(|p| *p > 0);
    if debug && let Some(port) = webview.remote_debugging_port.or(pending_port) {
        args.push(format!("--remote-debugging-port={port}"));
    }
    args.extend(webview.additional_browser_args.iter().cloned());
    args.join(" ")
}

/// Keeps the runtime-recorded switches that take effect from the next
/// launch (A.1.1, B.2.1) and drops everything else.
///
/// ```
/// use tauri_plugin_overwolf::config::filter_pending_browser_args;
/// let kept = filter_pending_browser_args(&["--disable-gpu".into(), "--foo".into(), "--remote-debugging-port=9222".into()]);
/// assert_eq!(kept, ["--disable-gpu", "--remote-debugging-port=9222"]);
/// ```
#[must_use]
pub fn filter_pending_browser_args(args: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for arg in args {
        let keep = arg == "--disable-gpu"
            || arg
                .strip_prefix("--remote-debugging-port=")
                .and_then(|p| p.parse::<u16>().ok())
                .is_some_and(|p| p > 0);
        if keep && !out.contains(arg) {
            out.push(arg.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_contract() {
        let cfg = Config::default();
        assert_eq!(cfg.main.url, "main.html");
        assert_eq!(cfg.main.crash_restart_limit, 3);
        assert_eq!(cfg.packages_backend, PackagesBackend::Auto);
        assert_eq!(cfg.ads.gesture_window_ms, 1500);
        assert_eq!(cfg.ads.guest_limits.bytes_per_second, 262_144);
        assert_eq!(cfg.consent.cmp_required, CmpRequired::Always);
        assert!(cfg.consent.gate_ads_on_consent);
        assert_eq!(cfg.ipc.max_message_bytes, 8 * 1024 * 1024);
        assert_eq!(cfg.ipc.max_queued_messages, 4096);
        assert!(cfg.updater.enabled);
        assert_eq!(cfg.email_hashes.encoding, HashEncoding::Hex);
        cfg.validate().unwrap();
        // An empty object and the full serialised default both deserialise to the default.
        assert_eq!(serde_json::from_str::<Config>("{}").unwrap(), cfg);
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(serde_json::from_value::<Config>(json).unwrap(), cfg);
    }

    #[test]
    fn full_contract_example_parses() {
        let text = r#"{
          "main": { "url": "main.html", "devtools": false, "crashRestartLimit": 3 },
          "webview": { "disableGpu": false, "remoteDebuggingPort": null, "additionalBrowserArgs": [] },
          "uid": null,
          "packagesBackend": "auto",
          "ads": { "testAd": false, "gestureWindowMs": 1500, "maxRecoveries": 10, "loadErrorRetryMs": 5000,
                   "exposeEmailHashesToGuest": false, "requestShaping": false, "experimentalElementApi": false,
                   "guestLimits": { "eventsPerSecond": 50, "eventBurst": 100, "bytesPerSecond": 262144, "externalOpensPerMinute": 5 } },
          "analytics": { "hostFields": false, "muidStrategy": "per-install", "userSwitch": false },
          "consent": { "cmpRequired": "always", "cmpUrl": null, "gateAdsOnConsent": true, "readyTimeoutMs": 30000 },
          "emailHashes": { "encoding": "hex" },
          "ipc": { "invokeTimeoutMs": 0, "startupQueueMax": 1024, "startupTimeoutMs": 30000, "maxMessageBytes": 8388608,
                   "maxInFlightInvokes": 256, "maxQueuedMessages": 4096 },
          "updater": { "enabled": true, "installerArgs": null, "publisherNames": null, "pubkey": null },
          "shell": { "openPathAllowExecutables": false },
          "fs": { "scope": [] },
          "state": { "appDataDir": null }
        }"#;
        let cfg: Config = serde_json::from_str(text).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(serde_json::from_str::<Config>(r#"{"ads":{"testAdd":true}}"#).is_err());
        assert!(serde_json::from_str::<Config>(r#"{"bogus":1}"#).is_err());
    }

    #[test]
    fn cmp_required_forms() {
        let parse = |s: &str| serde_json::from_str::<CmpRequired>(s);
        assert_eq!(parse(r#""never""#).unwrap(), CmpRequired::Never);
        assert_eq!(
            parse(r#"{"url":"https://example.com/x"}"#).unwrap(),
            CmpRequired::Url("https://example.com/x".into())
        );
        assert!(parse(r#""sometimes""#).is_err());
        assert!(parse(r#"{"url":"x","extra":1}"#).is_err());
        let back = serde_json::to_string(&CmpRequired::Url("https://a".into())).unwrap();
        assert_eq!(back, r#"{"url":"https://a"}"#);
    }

    #[test]
    fn validation_rejects_bad_values() {
        let check = |json: &str, path: &str| {
            let cfg: Config = serde_json::from_str(json).unwrap();
            let e = cfg.validate().unwrap_err();
            assert_eq!(e.path, path, "{json}");
            assert!(e.to_string().starts_with("plugins.overwolf."));
        };
        check(r#"{"main":{"url":""}}"#, "main.url");
        check(
            r#"{"main":{"url":"https://evil.example/main.html"}}"#,
            "main.url",
        );
        check(r#"{"main":{"url":"../outside.html"}}"#, "main.url");
        check(
            r#"{"main":{"crashRestartLimit":0}}"#,
            "main.crashRestartLimit",
        );
        check(
            r#"{"webview":{"remoteDebuggingPort":0}}"#,
            "webview.remoteDebuggingPort",
        );
        check(
            r#"{"webview":{"additionalBrowserArgs":["--a b"]}}"#,
            "webview.additionalBrowserArgs[0]",
        );
        check(
            r#"{"webview":{"additionalBrowserArgs":["--ok","nope"]}}"#,
            "webview.additionalBrowserArgs[1]",
        );
        check(r#"{"uid":"bad uid"}"#, "uid");
        check(
            r#"{"ads":{"gestureWindowMs":60001}}"#,
            "ads.gestureWindowMs",
        );
        check(
            r#"{"ads":{"guestLimits":{"eventBurst":0}}}"#,
            "ads.guestLimits.eventBurst",
        );
        check(
            r#"{"consent":{"cmpRequired":{"url":"http://x"}}}"#,
            "consent.cmpRequired.url",
        );
        check(
            r#"{"consent":{"cmpUrl":"https://example.com/cmp.html"}}"#,
            "consent.cmpUrl",
        );
        check(
            r#"{"consent":{"readyTimeoutMs":0}}"#,
            "consent.readyTimeoutMs",
        );
        check(
            r#"{"ipc":{"maxInFlightInvokes":0}}"#,
            "ipc.maxInFlightInvokes",
        );
        check(r#"{"ipc":{"startupTimeoutMs":0}}"#, "ipc.startupTimeoutMs");
        check(
            r#"{"ipc":{"maxMessageBytes":300000000}}"#,
            "ipc.maxMessageBytes",
        );
        check(r#"{"updater":{"pubkey":" "}}"#, "updater.pubkey");
        check(r#"{"fs":{"scope":["relative/dir"]}}"#, "fs.scope[0]");
        check(r#"{"fs":{"scope":["$HOME/x"]}}"#, "fs.scope[0]");
        check(r#"{"fs":{"scope":["$PICTURES/../x"]}}"#, "fs.scope[0]");
    }

    #[test]
    fn scope_templates() {
        for ok in [
            "$PICTURES/Overwolf/$APPNAME",
            "$USERDATA",
            "$TEMP/x",
            "/abs/path",
        ] {
            validate_scope_template(ok).unwrap();
        }
        assert!(validate_scope_template("$PICTURESX").is_err());
    }

    #[test]
    fn cmp_url_scope() {
        assert!(is_allowed_cmp_url(
            "https://content.overwolf.com/monsdk/electron/latest/cmp/x.html?a=b"
        ));
        assert!(!is_allowed_cmp_url(
            "https://content.overwolf.com/monsdk/electron/../../x"
        ));
        assert!(!is_allowed_cmp_url("https://content.overwolf.com/other/"));
    }

    #[test]
    fn switches() {
        let s = Switches::parse([
            "exe",
            "--owepm-package-channel=gep:beta,overlay:qa",
            "--force-phased-package=gep",
            "--ow-tauri-packages-backend=none",
            "--ow-tauri-relaunch-after=123",
            "--unrelated",
        ]);
        assert!(!s.test_ad);
        assert_eq!(s.package_channel.as_deref(), Some("gep:beta,overlay:qa"));
        assert_eq!(s.force_phased_package, Some(Some("gep".into())));
        assert_eq!(s.packages_backend.as_deref(), Some("none"));
        assert_eq!(s.relaunch_after.as_deref(), Some("123"));
        // The executable itself is never a switch.
        assert!(!Switches::parse(["--test-ad"]).test_ad);
        let argv: Vec<String> = vec!["exe".into(), "--test-ad".into()];
        assert!(Switches::has_switch(&argv, "test-ad"));
        assert!(!Switches::has_switch(&argv, "test"));
    }

    #[test]
    fn env_overrides() {
        let vars = [
            ("OW_TAURI_TEST_AD", "0"),
            ("OW_TAURI_PACKAGES_BACKEND", "native"),
            ("OW_TAURI_PACKAGE_RUNTIME", "/opt/runtime"),
            ("OW_TAURI_REMOTE_DEBUGGING_PORT", "9222"),
            ("OW_CLI_EMAIL", "dev@example.com"),
        ];
        let get = |k: &str| {
            vars.iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| (*v).to_owned())
        };
        let (env, warnings) = EnvOverrides::read(get, true);
        assert!(!env.test_ad, "only 1 enables test ads");
        assert_eq!(env.remote_debugging_port, Some(9222));
        assert_eq!(env.package_runtime, Some(PathBuf::from("/opt/runtime")));
        assert_eq!(env.dev_email.as_deref(), Some("dev@example.com"));
        assert!(warnings.is_empty());
        let (env, warnings) = EnvOverrides::read(get, false);
        assert_eq!(env.remote_debugging_port, None);
        assert_eq!(warnings.len(), 1);
        let (_, warnings) = EnvOverrides::read(
            |k| (k == "OW_TAURI_REMOTE_DEBUGGING_PORT").then(|| "x".into()),
            true,
        );
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn override_order_and_bad_values() {
        let mut cfg = Config::default();
        let env = EnvOverrides {
            test_ad: false,
            packages_backend: Some("bogus".into()),
            ..EnvOverrides::default()
        };
        let sw = Switches::parse(["exe", "--test-ad"]);
        let warnings = apply_overrides(&mut cfg, &env, &sw);
        assert_eq!(warnings.len(), 1);
        assert!(cfg.ads.test_ad);
        assert_eq!(cfg.packages_backend, PackagesBackend::Auto);
    }

    #[test]
    fn browser_arguments() {
        let base = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows";
        assert_eq!(browser_args(&WebviewConfig::default(), &[], true), base);
        let cfg = WebviewConfig {
            disable_gpu: false,
            remote_debugging_port: Some(9222),
            additional_browser_args: vec!["--lang=en".into()],
        };
        assert_eq!(
            browser_args(&cfg, &[], true),
            format!("{base} --remote-debugging-port=9222 --lang=en")
        );
        assert_eq!(
            browser_args(&cfg, &[], false),
            format!("{base} --lang=en"),
            "no port in release"
        );
        let pending = vec![
            "--disable-gpu".to_owned(),
            "--remote-debugging-port=9333".to_owned(),
        ];
        assert_eq!(
            browser_args(&WebviewConfig::default(), &pending, true),
            format!("{base} --disable-gpu --remote-debugging-port=9333")
        );
        assert_eq!(
            filter_pending_browser_args(&["--remote-debugging-port=0".into()]),
            Vec::<String>::new()
        );
    }
}
