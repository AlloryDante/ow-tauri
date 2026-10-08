//! Anonymous app analytics: `Counter` and `InsertStats` requests (CONTRACT E;
//! ADR 0006, `docs/adr/0006-analytics-labelling.md`).
//!
//! The pieces are pure so the wire bytes can be tested exactly:
//!
//! - [`HostLabel`]: the `<label>` / `<owVersion>` rule of CONTRACT section 0;
//! - [`compose_user_agent`]: `<UA>` (E.1);
//! - [`safari_ua_version`]: Safari's `Version/` token (E.1);
//! - [`Reporter`]: builds every host request ([`HostRequest`]) with the
//!   header order and encoding ow-electron uses (observed);
//! - [`window_analytics_name`]: the `name` field of `<label>_window_closed`;
//! - [`session::Session`]: which events a session sends, and when (E.2, E.3).
//!
//! The plugin sends the requests through a [`Transport`] (a `hyper`
//! client by default; tests capture them).
//!
//! ```
//! use tauri_plugin_overwolf::analytics::HostLabel;
//! let label = HostLabel::new("tauri", "2.12.1");
//! assert_eq!(label.ow_version(), "tauri-2.12.1");
//! assert_eq!(label.insert_stats_owver(), "tauri-2_12_1");
//! assert_eq!(label.counter_name("app_start"), "tauri_app_start");
//! assert_eq!(label.ua_token(), "Tauri/2.12.1");
//! ```

pub mod session;
#[cfg(feature = "plugin")]
pub(crate) mod transport;
pub mod user_agent;

use std::time::Duration;

use serde_json::{Map, Value};

#[cfg(feature = "plugin")]
pub use transport::{BoxFuture, Transport};

/// **Tests only.** Endpoint overrides for failure injection (DESIGN §7.4):
/// a host request whose URL starts with one of the production endpoints is
/// sent to the override instead (path and query kept after the endpoint).
/// Set through the hidden `Builder::endpoints` (feature `test-util`). Not a
/// stable API.
///
/// ```
/// use tauri_plugin_overwolf::analytics::{TestEndpoints, COUNTER_URL};
/// let e = TestEndpoints { counter: Some("http://127.0.0.1:9/c".into()), ..TestEndpoints::default() };
/// assert_eq!(e.rewrite(&format!("{COUNTER_URL}?a=1")), "http://127.0.0.1:9/c?a=1");
/// assert_eq!(e.rewrite("https://example.com/"), "https://example.com/");
/// ```
#[doc(hidden)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestEndpoints {
    /// Replaces [`COUNTER_URL`].
    pub counter: Option<String>,
    /// Replaces [`INSERT_STATS_URL`].
    pub insert_stats: Option<String>,
    /// Replaces [`CMP_EU_ONLY_URL`].
    pub cmp_eu_only: Option<String>,
    /// Replaces the configured update feed (the update client, W3).
    pub update_feed: Option<String>,
}

impl TestEndpoints {
    /// `url` with a production endpoint prefix replaced by its override.
    #[must_use]
    pub fn rewrite(&self, url: &str) -> String {
        let pairs = [
            (COUNTER_URL, &self.counter),
            (INSERT_STATS_URL, &self.insert_stats),
            (CMP_EU_ONLY_URL, &self.cmp_eu_only),
        ];
        for (endpoint, replacement) in pairs {
            if let (Some(rest), Some(to)) = (url.strip_prefix(endpoint), replacement) {
                return format!("{to}{rest}");
            }
        }
        url.to_owned()
    }
}

/// The longest an exit waits for queued analytics requests (A.6, E.1).
pub const DRAIN_LIMIT: Duration = Duration::from_millis(1500);
/// Timeout of every host request except `cmp-eu-only` (E.1).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// The Counter endpoint (E.1).
pub const COUNTER_URL: &str = "https://analyticsnew.overwolf.com/analytics/Counter";
/// The `InsertStats` endpoint (E.1).
pub const INSERT_STATS_URL: &str = "https://tracking.overwolf.com/tracking/InsertStats";
/// The consent experiment endpoint (D.6.2).
pub const CMP_EU_ONLY_URL: &str = "https://features.overwolf.com/experiments/cmp-eu-only";

/// `InsertStats` kinds (E.2).
pub mod kind {
    /// First launch.
    pub const FIRST_LAUNCH: u32 = 400_022;
    /// Heartbeat.
    pub const HEARTBEAT: u32 = 400_023;
    /// Ad guest crash (reason first in `Extra`).
    pub const GUEST_CRASH: u32 = 400_024;
    /// Ad guest attached.
    pub const GUEST_ATTACH: u32 = 400_025;
}

/// The host label and version (CONTRACT section 0, "Host label").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLabel {
    label: String,
    version: String,
}

impl HostLabel {
    /// A label (`analytics.hostLabel`) and version (`analytics.hostVersion`,
    /// else the Tauri crate version).
    ///
    /// ```
    /// let ow = tauri_plugin_overwolf::analytics::HostLabel::new("electron", "42.11.4");
    /// assert_eq!(ow.ow_version(), "42.11.4");
    /// assert_eq!(ow.ua_token(), "Electron/42.11.4");
    /// ```
    #[must_use]
    pub fn new(label: impl Into<String>, version: impl Into<String>) -> Self {
        HostLabel {
            label: label.into(),
            version: version.into(),
        }
    }

    /// `<label>`.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The host version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// `<owVersion>`: `<label>-<version>`, or the bare version when the label
    /// is `electron`.
    ///
    /// ```
    /// use tauri_plugin_overwolf::analytics::HostLabel;
    /// assert_eq!(HostLabel::new("tauri", "2.12.1").ow_version(), "tauri-2.12.1");
    /// ```
    #[must_use]
    pub fn ow_version(&self) -> String {
        if self.label == "electron" {
            self.version.clone()
        } else {
            format!("{}-{}", self.label, self.version)
        }
    }

    /// The `InsertStats` `owver`: `<owVersion>` with `.` replaced by `_`.
    ///
    /// ```
    /// use tauri_plugin_overwolf::analytics::HostLabel;
    /// assert_eq!(HostLabel::new("electron", "42.11.4").insert_stats_owver(), "42_11_4");
    /// ```
    #[must_use]
    pub fn insert_stats_owver(&self) -> String {
        self.ow_version().replace('.', "_")
    }

    /// The Counter `Name` of an event: `<label>_<event>`.
    ///
    /// ```
    /// use tauri_plugin_overwolf::analytics::HostLabel;
    /// assert_eq!(HostLabel::new("tauri", "2").counter_name("sub_info"), "tauri_sub_info");
    /// ```
    #[must_use]
    pub fn counter_name(&self, event: &str) -> String {
        format!("{}_{event}", self.label)
    }

    /// The installer's uninstall Counter name: `ow_<label>_app_uninstall`
    /// (I.6).
    ///
    /// ```
    /// use tauri_plugin_overwolf::analytics::HostLabel;
    /// assert_eq!(HostLabel::new("tauri", "2").uninstall_counter_name(), "ow_tauri_app_uninstall");
    /// ```
    #[must_use]
    pub fn uninstall_counter_name(&self) -> String {
        format!("ow_{}_app_uninstall", self.label)
    }

    /// The user agent token: `<Label>/<version>` with the first letter of
    /// the label upper-cased.
    ///
    /// ```
    /// use tauri_plugin_overwolf::analytics::HostLabel;
    /// assert_eq!(HostLabel::new("tauri", "2.12.1").ua_token(), "Tauri/2.12.1");
    /// ```
    #[must_use]
    pub fn ua_token(&self) -> String {
        let mut chars = self.label.chars();
        let first: String = chars
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        format!("{first}{}/{}", chars.as_str(), self.version)
    }
}

/// Composes `<UA>` from the platform webview's default user agent (E.1).
///
/// With a ` Chrome/<x>` token (WebView2), `<PNNS>/<ver> ` goes immediately
/// before `Chrome/` and ` <Label>/<v>` immediately after the Chrome token,
/// where Electron places its tokens. Otherwise (`WebKit`) both are appended.
///
/// The WKWebView default carries no browser product tokens, and ad stacks
/// rate such a user agent as an unknown browser and serve it no demand. When
/// the default has an `AppleWebKit/<w>` token but no `Safari/` token and
/// `safari_version` is known, Safari's own tokens are added the way Electron
/// keeps Chromium's: `<PNNS>/<ver> Version/<safari> <Label>/<v> Safari/<w>`.
/// The engine part is never changed: a `WebKit` user agent stays `WebKit`.
///
/// ```
/// use tauri_plugin_overwolf::analytics::{compose_user_agent, HostLabel};
/// let label = HostLabel::new("tauri", "2.12.1");
/// assert_eq!(
///     compose_user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0", "My App", "1.0.0", &label, None),
///     "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) MyApp/1.0.0 Chrome/141.0.0.0 Tauri/2.12.1 Safari/537.36 Edg/141.0.0.0"
/// );
/// assert_eq!(
///     compose_user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)", "My App", "1.0.0", &label, Some("26.5")),
///     "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) MyApp/1.0.0 Version/26.5 Tauri/2.12.1 Safari/605.1.15"
/// );
/// assert_eq!(
///     compose_user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)", "My App", "1.0.0", &label, None),
///     "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) MyApp/1.0.0 Tauri/2.12.1"
/// );
/// ```
#[must_use]
pub fn compose_user_agent(
    default_ua: &str,
    product_name: &str,
    version: &str,
    label: &HostLabel,
    safari_version: Option<&str>,
) -> String {
    let ua = default_ua.trim();
    let app = format!("{}/{version}", product_name.replace(' ', ""));
    let host = label.ua_token();
    if let Some(start) = ua.find(" Chrome/") {
        let chrome_start = start + 1;
        let chrome_end = ua[chrome_start..]
            .find(' ')
            .map_or(ua.len(), |i| chrome_start + i);
        format!(
            "{} {app} {} {host}{}",
            &ua[..start],
            &ua[chrome_start..chrome_end],
            &ua[chrome_end..]
        )
    } else if ua.is_empty() {
        format!("{app} {host}")
    } else if let (Some(webkit), Some(safari), false) = (
        token_version(ua, "AppleWebKit/"),
        safari_version.map(str::trim).filter(|v| !v.is_empty()),
        ua.contains(" Safari/"),
    ) {
        format!("{ua} {app} Version/{safari} {host} Safari/{webkit}")
    } else {
        format!("{ua} {app} {host}")
    }
}

/// The version of the first `<name><version>` token of `ua`.
fn token_version<'a>(ua: &'a str, name: &str) -> Option<&'a str> {
    let rest = &ua[ua.find(name)? + name.len()..];
    let version = rest.split([' ', ';', ')']).next()?;
    (!version.is_empty()).then_some(version)
}

/// Safari's `Version/` token value from its bundle version: major and
/// minor only, as Safari sends it (`26.5.2` -> `26.5`).
///
/// ```
/// use tauri_plugin_overwolf::analytics::safari_ua_version;
/// assert_eq!(safari_ua_version("26.5.2").as_deref(), Some("26.5"));
/// assert_eq!(safari_ua_version("18").as_deref(), Some("18.0"));
/// assert_eq!(safari_ua_version("x"), None);
/// ```
#[must_use]
pub fn safari_ua_version(bundle_version: &str) -> Option<String> {
    let mut parts = bundle_version.trim().split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next().map_or(Some(0), |m| m.parse().ok())?;
    Some(format!("{major}.{minor}"))
}

/// The app locale in Chromium's `accept-language` form (`en-US`), from a
/// POSIX or BCP 47 locale string.
///
/// ```
/// use tauri_plugin_overwolf::analytics::accept_language;
/// assert_eq!(accept_language("en_US.UTF-8"), "en-US");
/// assert_eq!(accept_language("de-DE"), "de-DE");
/// assert_eq!(accept_language(""), "en-US");
/// ```
#[must_use]
pub fn accept_language(locale: &str) -> String {
    let base = locale
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .trim()
        .replace('_', "-");
    if base.is_empty() || base == "C" || base == "POSIX" {
        "en-US".into()
    } else {
        base
    }
}

/// HTTP method of a host request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
}

/// One host request, exactly as it goes on the wire (E.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRequest {
    /// The method.
    pub method: Method,
    /// The full URL, query included.
    pub url: String,
    /// Headers in wire order, `content-length` included when there is a
    /// body (first, as ow-electron sends it, observed). The transport adds
    /// nothing.
    pub headers: Vec<(&'static str, String)>,
    /// The body (`POST` only).
    pub body: Option<Vec<u8>>,
    /// `None` = no client timeout (`cmp-eu-only`, D.6.2).
    pub timeout: Option<Duration>,
}

/// A host response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostResponse {
    /// HTTP status.
    pub status: u16,
    /// The decoded body.
    pub body: Vec<u8>,
    /// Every `Set-Cookie` header value, in response order. Only reported:
    /// host requests neither send nor store cookies, as ow-electron's do
    /// not (observed: every cookie excluded by the request's credentials
    /// mode).
    pub set_cookies: Vec<String>,
}

/// Encodes like `URLSearchParams`: space as `+`, every byte other than ASCII
/// alphanumerics and `*-._` percent-encoded.
///
/// ```
/// assert_eq!(tauri_plugin_overwolf::analytics::form_encode(r#"{"a":"b c"}"#), "%7B%22a%22%3A%22b+c%22%7D");
/// ```
#[must_use]
pub fn form_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Everything a host request carries about the app and the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reporter {
    /// The host label.
    pub label: HostLabel,
    /// `<ver>`.
    pub app_version: String,
    /// `<uid>`.
    pub uid: String,
    /// `<cuid>`.
    pub cuid: String,
    /// `darwin`, `win32` or `linux`.
    pub os: String,
    /// Node's `os.release()`.
    pub os_version: String,
    /// `<PN>`.
    pub app_name: String,
    /// `<muid>`.
    pub muid: String,
    /// `<muidV2>`.
    pub muid_v2: String,
    /// `<UA>`.
    pub user_agent: String,
    /// `accept-language`.
    pub locale: String,
}

impl Reporter {
    /// The headers every host request ends with (E.1).
    fn common_headers(&self, head: Vec<(&'static str, String)>) -> Vec<(&'static str, String)> {
        let mut out = head;
        out.extend([
            ("sec-fetch-site", "none".to_owned()),
            ("sec-fetch-mode", "no-cors".to_owned()),
            ("sec-fetch-dest", "empty".to_owned()),
            ("user-agent", self.user_agent.clone()),
            ("accept-encoding", "gzip, deflate, br, zstd".to_owned()),
            ("accept-language", self.locale.clone()),
            ("priority", "u=4, i".to_owned()),
        ]);
        out
    }

    /// The six fields every Counter `Extra` starts with, in order.
    #[must_use]
    pub fn base_extra(&self) -> Map<String, Value> {
        let mut m = Map::new();
        for (k, v) in [
            ("app_ver", &self.app_version),
            ("app_id", &self.uid),
            ("os", &self.os),
            ("os_ver", &self.os_version),
            ("app_name", &self.app_name),
            ("app_cuid", &self.cuid),
        ] {
            m.insert(k.into(), Value::String(v.clone()));
        }
        m
    }

    /// A Counter request for `<label>_<event>` with the event fields in
    /// order after the six base fields (E.1).
    ///
    /// ```
    /// # use tauri_plugin_overwolf::analytics::*;
    /// # let r = Reporter { label: HostLabel::new("tauri", "2.12.1"), app_version: "1.0.0".into(), uid: "u".into(), cuid: "c".into(), os: "darwin".into(), os_version: "25.5.0".into(), app_name: "My App".into(), muid: "m".into(), muid_v2: "m2".into(), user_agent: "UA".into(), locale: "en-US".into() };
    /// let req = r.counter("app_start", &[]);
    /// assert_eq!(req.url, "https://analyticsnew.overwolf.com/analytics/Counter?Name=tauri_app_start&MUID=m&MUIDV2=m2&owver=tauri-2.12.1&Extra=%7B%22app_ver%22%3A%221.0.0%22%2C%22app_id%22%3A%22u%22%2C%22os%22%3A%22darwin%22%2C%22os_ver%22%3A%2225.5.0%22%2C%22app_name%22%3A%22My+App%22%2C%22app_cuid%22%3A%22c%22%7D");
    /// ```
    #[must_use]
    pub fn counter(&self, event: &str, fields: &[(String, Value)]) -> HostRequest {
        let mut extra = self.base_extra();
        for (k, v) in fields {
            extra.insert(k.clone(), v.clone());
        }
        let extra = Value::Object(extra).to_string();
        let url = format!(
            "{COUNTER_URL}?Name={}&MUID={}&MUIDV2={}&owver={}&Extra={}",
            form_encode(&self.label.counter_name(event)),
            form_encode(&self.muid),
            form_encode(&self.muid_v2),
            form_encode(&self.label.ow_version()),
            form_encode(&extra)
        );
        HostRequest {
            method: Method::Get,
            url,
            headers: self.common_headers(Vec::new()),
            body: None,
            timeout: Some(REQUEST_TIMEOUT),
        }
    }

    /// An `InsertStats` request (E.1). `reason` goes first in `Extra`
    /// (Kind 400024).
    ///
    /// ```
    /// # use tauri_plugin_overwolf::analytics::*;
    /// # let r = Reporter { label: HostLabel::new("tauri", "2.12.1"), app_version: "1.0.0".into(), uid: "u".into(), cuid: "c".into(), os: "darwin".into(), os_version: "25.5.0".into(), app_name: "My App".into(), muid: "m".into(), muid_v2: "m".into(), user_agent: "UA".into(), locale: "en-US".into() };
    /// let req = r.insert_stats(kind::GUEST_CRASH, Some("killed"));
    /// assert_eq!(req.url, "https://tracking.overwolf.com/tracking/InsertStats?Stats=true&owver=tauri-2_12_1");
    /// assert_eq!(req.headers[0], ("content-length", "56".to_owned()));
    /// assert_eq!(req.body.unwrap(), br#"{"Kind":400024,"Extra":"killed.1_0_0.u.darwin.My App.c"}"#);
    /// ```
    #[must_use]
    pub fn insert_stats(&self, kind: u32, reason: Option<&str>) -> HostRequest {
        let clean = |s: &str| s.replace(['.', ':'], "_");
        let mut parts: Vec<String> = Vec::with_capacity(6);
        if let Some(r) = reason {
            parts.push(clean(r));
        }
        for v in [
            &self.app_version,
            &self.uid,
            &self.os,
            &self.app_name,
            &self.cuid,
        ] {
            parts.push(clean(v));
        }
        let mut body = Map::new();
        body.insert("Kind".into(), Value::from(kind));
        body.insert("Extra".into(), Value::String(parts.join(".")));
        let url = format!(
            "{INSERT_STATS_URL}?Stats=true&owver={}",
            form_encode(&self.label.insert_stats_owver())
        );
        let body = Value::Object(body).to_string().into_bytes();
        HostRequest {
            method: Method::Post,
            url,
            headers: self.common_headers(vec![
                ("content-length", body.len().to_string()),
                ("content-type", "application/json".to_owned()),
            ]),
            body: Some(body),
            timeout: Some(REQUEST_TIMEOUT),
        }
    }

    /// The `cmp-eu-only` request (D.6.2): no client timeout.
    ///
    /// ```
    /// # use tauri_plugin_overwolf::analytics::*;
    /// # let r = Reporter { label: HostLabel::new("tauri", "2.12.1"), app_version: "1.0.0".into(), uid: "u".into(), cuid: "c".into(), os: "darwin".into(), os_version: "25.5.0".into(), app_name: "My App".into(), muid: "m".into(), muid_v2: "m".into(), user_agent: "UA".into(), locale: "en-US".into() };
    /// let req = r.cmp_eu_only();
    /// assert_eq!(req.headers[0], ("cache-control", "no-cache".to_owned()));
    /// assert!(req.timeout.is_none());
    /// ```
    #[must_use]
    pub fn cmp_eu_only(&self) -> HostRequest {
        HostRequest {
            method: Method::Get,
            url: CMP_EU_ONLY_URL.to_owned(),
            headers: self.common_headers(vec![("cache-control", "no-cache".to_owned())]),
            body: None,
            timeout: None,
        }
    }
}

/// The analytics name of a window, from the URL loaded when it is first
/// shown (E.2 #7, matches ow-electron, observed): the last path segment,
/// percent-decoded, without query or fragment, a trailing `.html` / `.htm`
/// (any case) removed, keeping only letters (any script), digits, `-`, `_`
/// and `.`. An empty last segment gives the host name.
///
/// ```
/// use tauri_plugin_overwolf::analytics::window_analytics_name;
/// assert_eq!(window_analytics_name("tauri://localhost/index.html"), "index");
/// assert_eq!(window_analytics_name("https://example.com/some/path/page.php?x=1#frag"), "page.php");
/// assert_eq!(window_analytics_name("https://example.com/"), "example.com");
/// assert_eq!(window_analytics_name("about:blank"), "blank");
/// assert_eq!(window_analytics_name("data:text/html,<title>d</title>"), "title");
/// ```
#[must_use]
pub fn window_analytics_name(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return String::new();
    };
    let path = parsed.path();
    let last = path.rsplit('/').next().unwrap_or_default();
    let decoded = percent_decode(last);
    let strip = |ext: &str| {
        let cut = decoded.len().checked_sub(ext.len())?;
        decoded
            .get(cut..)
            .filter(|tail| tail.eq_ignore_ascii_case(ext))
            .and_then(|_| decoded.get(..cut))
    };
    let stem = strip(".html")
        .or_else(|| strip(".htm"))
        .unwrap_or(&decoded[..]);
    let name: String = stem
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    if last.is_empty() {
        parsed.host_str().unwrap_or_default().to_owned()
    } else {
        name
    }
}

/// The analytics name of a window whose naming webview shows `url`
/// (DESIGN §4.3.3): [`window_analytics_name`] with one rule for Tauri's app
/// origin. Tauri serves `index.html` for an empty path and reports the URL
/// without it (`tauri://localhost/`), where ow-electron loads and reports
/// `file://…/index.html`. So for an app page (`app_page`: the URL's origin
/// is the app's custom-protocol origin, `build.devUrl`'s origin or an
/// `ads.allowedEmbedderOrigins` entry) whose path is empty or ends with `/`,
/// the path is read as `<path>index.html`. Remote pages are unchanged.
///
/// ```
/// use tauri_plugin_overwolf::analytics::app_window_name;
/// assert_eq!(app_window_name("tauri://localhost/", true), "index");
/// assert_eq!(app_window_name("http://tauri.localhost", true), "index");
/// assert_eq!(app_window_name("tauri://localhost/settings.html", true), "settings");
/// assert_eq!(app_window_name("http://localhost:1420/nested/", true), "index");
/// // A history-routed SPA path names the route, as documented.
/// assert_eq!(app_window_name("tauri://localhost/route", true), "route");
/// // Remote pages keep ow-electron's rule (the host name for `/`).
/// assert_eq!(app_window_name("https://example.com/", false), "example.com");
/// ```
#[must_use]
pub fn app_window_name(url: &str, app_page: bool) -> String {
    if app_page && let Ok(mut parsed) = url::Url::parse(url) {
        let path = parsed.path().to_owned();
        if path.is_empty() || path.ends_with('/') {
            parsed.set_path(&format!("{path}index.html"));
            return window_analytics_name(parsed.as_str());
        }
    }
    window_analytics_name(url)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(s.get(i + 1..i + 3).unwrap_or(""), 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The `Extra` of `<label>_sub_info` (E.2 #10): the options' own keys in the
/// order the app passed them, then `providerName: "tebex"` when absent or
/// empty.
///
/// ```
/// use tauri_plugin_overwolf::analytics::sub_info_fields;
/// let opts = serde_json::json!({ "userId": "u1", "paymentId": "p1" });
/// let fields = sub_info_fields(opts.as_object().unwrap());
/// let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
/// assert_eq!(keys, ["userId", "paymentId", "providerName"]);
/// ```
#[must_use]
pub fn sub_info_fields(options: &Map<String, Value>) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = options
        .iter()
        .filter(|(k, v)| !(k.as_str() == "providerName" && v.as_str() == Some("")))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if !out.iter().any(|(k, _)| k == "providerName") {
        out.push(("providerName".into(), Value::String("tebex".into())));
    }
    out
}

#[cfg(test)]
mod tests;
