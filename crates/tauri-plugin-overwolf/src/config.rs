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

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// `none` or the reserved `native` (H.2).
    pub packages_backend: PackagesBackend,
    /// Ads host options.
    pub ads: AdsConfig,
    /// Analytics options.
    pub analytics: AnalyticsConfig,
    /// Consent options.
    pub consent: ConsentConfig,
    /// Email hash options.
    pub email_hashes: EmailHashesConfig,
    /// Log file options (F.4).
    pub logging: LoggingConfig,
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
    reason = "independent configuration switches, read from JSON"
)]
pub struct AdsConfig {
    /// Test inventory (same as `--test-ad`).
    pub test_ad: bool,
    /// Request shaping (D.8); always on, the switch exists for debugging.
    pub request_shaping: bool,
    /// Value given to the guest `owVersion` and the consent `oweVersion`
    /// instead of `<owVersion>` (section 0).
    pub ow_version_override: Option<String>,
    /// ow-tauri option: the macOS private header SPI prototype (D.8.3).
    pub mac_private_header_api: bool,
    /// Ad guests are transparent from creation (default `true`), so a slot
    /// without an ad shows the app's own container background and an
    /// interstitial's dim shows the app behind it, as the in-page guest of
    /// ow-electron does (B.3.4). Windows and Linux use the webview's
    /// transparent background; macOS also clears the `WKWebView` background
    /// (`drawsBackground`, a private key-value key Tauri's own transparent
    /// webviews use, and the public `underPageBackgroundColor`) without
    /// Tauri's `macos-private-api` feature. `false` keeps opaque guests.
    pub transparent_guests: bool,
    /// User-gesture window for guest top-level navigation.
    pub gesture_window_ms: u64,
    /// Guest reloads after crashes, per element; `None` = no cap, as
    /// ow-electron (D.7).
    pub max_recoveries: Option<u32>,
    /// Reload interval after a failed main-frame load (D.7).
    pub load_error_retry_ms: u64,
    /// Per-guest limits (D.4).
    pub guest_limits: GuestLimits,
}

impl Default for AdsConfig {
    fn default() -> Self {
        AdsConfig {
            test_ad: false,
            request_shaping: true,
            ow_version_override: None,
            mac_private_header_api: false,
            transparent_guests: true,
            gesture_window_ms: 1500,
            max_recoveries: None,
            load_error_retry_ms: 5000,
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
    /// System-browser opens per minute, per guest (D.7); default 20.
    ///
    /// Every open also needs its own user gesture in the guest (one
    /// gesture, one open), which is the real safeguard; this cap only bounds
    /// a burst. It stays above the 5 opens Overwolf's own ad QA step makes
    /// ("click the ad 5 times", five browser windows).
    pub external_opens_per_minute: u32,
}

impl Default for GuestLimits {
    fn default() -> Self {
        GuestLimits {
            events_per_second: 50,
            event_burst: 100,
            bytes_per_second: 262_144,
            external_opens_per_minute: 20,
        }
    }
}

/// `plugins.overwolf.analytics`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct AnalyticsConfig {
    /// The host label (section 0, "Host label"), default `tauri`.
    pub host_label: String,
    /// The host version; `None` = the Tauri crate version.
    pub host_version: Option<String>,
    /// How the muid is derived (E.4).
    pub muid_strategy: MuidStrategy,
    /// ow-tauri option: expose `analytics_set_user_enabled`.
    pub user_switch: bool,
}

impl Default for AnalyticsConfig {
    fn default() -> Self {
        AnalyticsConfig {
            host_label: "tauri".into(),
            host_version: None,
            muid_strategy: MuidStrategy::MachineId,
            user_switch: false,
        }
    }
}

/// `analytics.muidStrategy` (E.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MuidStrategy {
    /// Derived from the OS machine identifier, as ow-electron (default).
    #[default]
    MachineId,
    /// ow-tauri option, non-parity: a random upper-case UUID v4 stored in
    /// `ow-tauri.json`.
    PerInstall,
}

/// `plugins.overwolf.consent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ConsentConfig {
    /// Settings window page override; any `https:` URL (D.6.4).
    pub cmp_url: Option<String>,
    /// The hidden consent windows are closed after this if still open.
    pub ready_timeout_ms: u64,
    /// Whether the host writes the consent cookies itself (D.6.3).
    pub host_cookie_fallback: CookieFallback,
}

impl Default for ConsentConfig {
    fn default() -> Self {
        ConsentConfig {
            cmp_url: None,
            ready_timeout_ms: 30_000,
            host_cookie_fallback: CookieFallback::Auto,
        }
    }
}

/// `consent.hostCookieFallback` (D.6.3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CookieFallback {
    /// Write the cookies when the page did not (default).
    #[default]
    Auto,
    /// Never write cookies from the host.
    Never,
}

/// `plugins.overwolf.logging` (F.4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct LoggingConfig {
    /// Append to `logs/ow-tauri.log`; off by default, as ow-electron writes
    /// no log.
    pub enabled: bool,
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
    /// Windows Authenticode publishers of the installer (a CN, or a DN
    /// whose every attribute must match), always enforced when set, as
    /// electron-updater enforces `publisherName`. `None`: the running exe's
    /// signer, unless the app exe is signed with Overwolf's certificate
    /// (`enableOWCertSigning`), which skips the check with a warning.
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

/// Whether `url` parses as an absolute `https:` URL.
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

/// Whether `label` is a valid `analytics.hostLabel`: 1 to 32 lower-case ASCII
/// letters, digits or `_`, starting with a letter. It becomes part of
/// Counter names and the user agent token.
///
/// ```
/// use tauri_plugin_overwolf::config::is_valid_host_label;
/// assert!(is_valid_host_label("tauri"));
/// assert!(is_valid_host_label("electron"));
/// assert!(!is_valid_host_label("Tauri"));
/// assert!(!is_valid_host_label("a b"));
/// ```
#[must_use]
pub fn is_valid_host_label(label: &str) -> bool {
    (1..=32).contains(&label.len())
        && label.starts_with(|c: char| c.is_ascii_lowercase())
        && label
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Configuration keys removed by the parity revision (A.1), as dot paths
/// below `plugins.overwolf`. The plugin rejects them; the build helper
/// names them in a build warning.
pub const REMOVED_KEYS: [&str; 6] = [
    "analytics.hostFields",
    "ads.experimentalElementApi",
    "ads.exposeEmailHashesToGuest",
    "ads.legacyHostMessages",
    "consent.gateAdsOnConsent",
    "consent.cmpRequired",
];

/// Warnings for removed keys and removed `packagesBackend` values in a raw
/// `plugins.overwolf` object (A.1).
///
/// ```
/// use tauri_plugin_overwolf::config::removed_key_warnings;
/// let raw = serde_json::json!({ "packagesBackend": "auto", "consent": { "cmpRequired": "always" } });
/// let w = removed_key_warnings(&raw);
/// assert_eq!(w.len(), 2);
/// assert!(w[0].contains("consent.cmpRequired"));
/// ```
#[must_use]
pub fn removed_key_warnings(raw: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for path in REMOVED_KEYS {
        let mut cur = Some(raw);
        for part in path.split('.') {
            cur = cur.and_then(|v| v.get(part));
        }
        if cur.is_some() {
            out.push(format!(
                "plugins.overwolf.{path} was removed by the parity revision; remove it (the plugin rejects it)"
            ));
        }
    }
    if let Some(v) = raw.get("packagesBackend").and_then(Value::as_str)
        && (v == "auto" || v == "simulated")
    {
        out.push(format!(
            "plugins.overwolf.packagesBackend \"{v}\" was removed; use \"none\" (the plugin rejects it)"
        ));
    }
    out
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
    #[expect(clippy::too_many_lines, reason = "one check per configuration field")]
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
        if let Some(url) = &self.consent.cmp_url
            && !is_https_url(url)
        {
            return Err(invalid("consent.cmpUrl", "must be an https URL"));
        }
        if !is_valid_host_label(&self.analytics.host_label) {
            return Err(invalid(
                "analytics.hostLabel",
                "must be 1 to 32 lower-case ASCII letters, digits or _",
            ));
        }
        for (name, value) in [
            (
                "analytics.hostVersion",
                self.analytics.host_version.as_ref(),
            ),
            (
                "ads.owVersionOverride",
                self.ads.ow_version_override.as_ref(),
            ),
        ] {
            if let Some(v) = value
                && (v.is_empty() || v.len() > 64 || !v.chars().all(|c| c.is_ascii_graphic()))
            {
                return Err(invalid(name, "must be 1 to 64 printable ASCII characters"));
            }
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
/// let sw = Switches::parse(["app", "--ow-tauri-packages-backend=native"]);
/// let warnings = apply_overrides(&mut cfg, &env, &sw);
/// assert!(warnings.is_empty());
/// assert_eq!(cfg.packages_backend, PackagesBackend::Native);
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
                    "{source}=\"{value}\" is not none or native; ignored"
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
        assert_eq!(cfg.packages_backend, PackagesBackend::None);
        assert_eq!(cfg.ads.gesture_window_ms, 1500);
        assert!(cfg.ads.request_shaping);
        assert_eq!(cfg.ads.max_recoveries, None);
        assert_eq!(cfg.ads.guest_limits.bytes_per_second, 262_144);
        assert_eq!(cfg.analytics.host_label, "tauri");
        assert_eq!(cfg.analytics.host_version, None);
        assert_eq!(cfg.analytics.muid_strategy, MuidStrategy::MachineId);
        assert_eq!(cfg.consent.host_cookie_fallback, CookieFallback::Auto);
        assert!(!cfg.logging.enabled);
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
          "packagesBackend": "none",
          "ads": { "testAd": false, "requestShaping": true, "owVersionOverride": null, "macPrivateHeaderApi": false,
                   "transparentGuests": true,
                   "gestureWindowMs": 1500, "maxRecoveries": null, "loadErrorRetryMs": 5000,
                   "guestLimits": { "eventsPerSecond": 50, "eventBurst": 100, "bytesPerSecond": 262144, "externalOpensPerMinute": 20 } },
          "analytics": { "hostLabel": "tauri", "hostVersion": null, "muidStrategy": "machine-id", "userSwitch": false },
          "consent": { "cmpUrl": null, "readyTimeoutMs": 30000, "hostCookieFallback": "auto" },
          "logging": { "enabled": false },
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
    fn guests_are_transparent_by_default() {
        assert!(Config::default().ads.transparent_guests);
        let cfg: Config =
            serde_json::from_str(r#"{ "ads": { "transparentGuests": false } }"#).unwrap();
        assert!(!cfg.ads.transparent_guests);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(serde_json::from_str::<Config>(r#"{"ads":{"testAdd":true}}"#).is_err());
        assert!(serde_json::from_str::<Config>(r#"{"bogus":1}"#).is_err());
    }

    #[test]
    fn removed_keys_are_rejected_and_named() {
        for json in [
            r#"{"analytics":{"hostFields":true}}"#,
            r#"{"ads":{"experimentalElementApi":true}}"#,
            r#"{"ads":{"exposeEmailHashesToGuest":true}}"#,
            r#"{"ads":{"legacyHostMessages":true}}"#,
            r#"{"consent":{"gateAdsOnConsent":true}}"#,
            r#"{"consent":{"cmpRequired":"always"}}"#,
            r#"{"packagesBackend":"auto"}"#,
            r#"{"packagesBackend":"simulated"}"#,
        ] {
            assert!(serde_json::from_str::<Config>(json).is_err(), "{json}");
            let raw: Value = serde_json::from_str(json).unwrap();
            assert_eq!(removed_key_warnings(&raw).len(), 1, "{json}");
        }
        assert!(removed_key_warnings(&serde_json::json!({"ads":{"testAd":true}})).is_empty());
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
            r#"{"consent":{"cmpUrl":"http://example.com/cmp.html"}}"#,
            "consent.cmpUrl",
        );
        check(
            r#"{"analytics":{"hostLabel":"Tauri"}}"#,
            "analytics.hostLabel",
        );
        check(r#"{"analytics":{"hostLabel":""}}"#, "analytics.hostLabel");
        check(
            r#"{"analytics":{"hostVersion":"1 2"}}"#,
            "analytics.hostVersion",
        );
        check(
            r#"{"ads":{"owVersionOverride":""}}"#,
            "ads.owVersionOverride",
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
        // An absolute path needs a drive or UNC prefix on Windows.
        let absolute = if cfg!(windows) {
            r"C:\abs\path"
        } else {
            "/abs/path"
        };
        for ok in [
            "$PICTURES/Overwolf/$APPNAME",
            "$USERDATA",
            "$TEMP/x",
            absolute,
        ] {
            validate_scope_template(ok).unwrap();
        }
        assert!(validate_scope_template("$PICTURESX").is_err());
        assert_eq!(
            validate_scope_template("/abs/path").is_ok(),
            cfg!(not(windows))
        );
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
        assert_eq!(cfg.packages_backend, PackagesBackend::None);
        let env = EnvOverrides {
            packages_backend: Some("simulated".into()),
            ..EnvOverrides::default()
        };
        let warnings = apply_overrides(&mut cfg, &env, &Switches::default());
        assert_eq!(
            warnings,
            ["OW_TAURI_PACKAGES_BACKEND=\"simulated\" is not none or native; ignored"]
        );
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
