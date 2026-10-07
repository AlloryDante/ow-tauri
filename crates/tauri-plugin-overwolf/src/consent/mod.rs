//! Consent (CMP) windows and storage (CONTRACT A.2.2, A.2.7, D.6).
//!
//! This module holds the pure parts: the consent page URLs and their
//! queries, the `cmp-eu-only` response rule, consent string validation and
//! encoding, the consent cookies and the `cmp_event` wire type. The plugin's
//! host runs the windows with them; the shared `cmp` block itself is read
//! and written by [`crate::state::ow_electron`].

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Write as _;

/// The settings window's default page (D.6.4).
pub const DEFAULT_CMP_URL: &str =
    "https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/cmp.html";

/// The startup and default-consent page (D.6.1, D.6.4).
pub const STARTUP_CMP_URL: &str =
    "https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/ow-cmp-v2.html";

/// Pages under this prefix get the consent globals and `cmp_event` (D.6.4).
pub const CMP_SCOPE: &str = "https://content.overwolf.com/monsdk/electron/";

/// The token `cmp.js` carries in place of its configuration (D.1).
pub const CMP_CONFIG_TOKEN: &str = "/*__OW_TAURI_CMP_CONFIG__*/null";

/// Longest consent string accepted (A.2.7).
pub const MAX_CONSENT_BYTES: usize = 16 * 1024;

/// The consent cookies' lifetime (D.6.3).
pub const COOKIE_DAYS: i64 = 365;

/// The consent window size: 1 x 32 logical pixels (D.6.1).
pub const HIDDEN_WINDOW_SIZE: (f64, f64) = (1.0, 32.0);

/// The settings window's background (D.6.4).
pub const DEFAULT_BACKGROUND: &str = "#0D0D0D";

/// `CMPWindowOptions` with `parent` replaced by `parentId` (A.2.2).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CmpWindowOptions {
    /// `purposes`, `features` or `vendors`.
    pub tab: Option<String>,
    /// Owned by the parent and kept above it.
    pub modal: Option<bool>,
    /// The parent window's id.
    pub parent_id: Option<u32>,
    /// Centre the window.
    pub center: Option<bool>,
    /// Window background colour.
    pub background_color: Option<String>,
    /// Spinner colour of the preloader.
    pub pre_loader_spinner_color: Option<String>,
    /// Width (default 800).
    pub width: Option<f64>,
    /// Height (default 800).
    pub height: Option<f64>,
    /// Left edge.
    pub x: Option<f64>,
    /// Top edge.
    pub y: Option<f64>,
    /// Consent page URL override; any `https:` URL.
    #[serde(rename = "cmpURL")]
    pub cmp_url: Option<String>,
    /// Page language.
    pub language: Option<String>,
}

/// `ExternalPaymentUserIdOptions` (A.2.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalPaymentUserIdOptions {
    /// Payment provider; `tebex` when empty.
    #[serde(default)]
    pub provider_name: String,
    /// The user id at the provider.
    pub user_id: String,
    /// Optional payment id.
    #[serde(default)]
    pub payment_id: Option<String>,
}

/// `encodeURIComponent`.
///
/// ```
/// use tauri_plugin_overwolf::consent::encode_uri_component;
/// assert_eq!(encode_uri_component("cmp=A&ac=2~1.2"), "cmp%3DA%26ac%3D2~1.2");
/// assert_eq!(encode_uri_component("a b!*'()"), "a%20b!*'()");
/// ```
#[must_use]
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// Percent-decodes `s` (invalid escapes are kept as they are).
#[must_use]
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// The facts the consent URLs carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentFacts<'a> {
    /// `<uid>`.
    pub uid: &'a str,
    /// `<PN>`.
    pub app_name: &'a str,
    /// `<muid>`.
    pub muid: &'a str,
    /// `<muidV2>`.
    pub muid_v2: &'a str,
    /// `<owVersion>` or `ads.owVersionOverride`.
    pub ow_version: &'a str,
    /// `<ver>`.
    pub app_version: &'a str,
}

/// The startup consent window's URL (D.6.1). `stored_unified` is the stored
/// `cmp.unifiedConsentString` (already URL-encoded); it is encoded once
/// more.
///
/// ```
/// use tauri_plugin_overwolf::consent::{startup_url, ConsentFacts};
/// let f = ConsentFacts { uid: "u", app_name: "A", muid: "m", muid_v2: "m2", ow_version: "tauri-2.12.1", app_version: "1.0.0" };
/// assert_eq!(
///     startup_url(&f, Some("cmp%3DX")),
///     "https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/ow-cmp-v2.html?unifiedcmp=cmp%253DX&muid=m&uid=u&muidv2=m2&oweVersion=tauri-2.12.1&appVersion=1.0.0"
/// );
/// ```
#[must_use]
pub fn startup_url(f: &ConsentFacts<'_>, stored_unified: Option<&str>) -> String {
    format!(
        "{STARTUP_CMP_URL}?unifiedcmp={}&muid={}&uid={}&muidv2={}&oweVersion={}&appVersion={}",
        encode_uri_component(stored_unified.unwrap_or_default()),
        encode_uri_component(f.muid),
        encode_uri_component(f.uid),
        encode_uri_component(f.muid_v2),
        encode_uri_component(f.ow_version),
        encode_uri_component(f.app_version),
    )
}

/// The default-consent window's URL (D.6.4): always an empty `unifiedcmp`.
///
/// ```
/// assert!(tauri_plugin_overwolf::consent::default_consent_url().ends_with("ow-cmp-v2.html?unifiedcmp=&firstRun=true"));
/// ```
#[must_use]
pub fn default_consent_url() -> String {
    format!("{STARTUP_CMP_URL}?unifiedcmp=&firstRun=true")
}

/// The settings window's URL (D.6.4): `base` plus the query in
/// ow-electron's order; `appName` is inserted without URL encoding.
///
/// ```
/// use tauri_plugin_overwolf::consent::{settings_url, ConsentFacts, DEFAULT_CMP_URL};
/// let f = ConsentFacts { uid: "u", app_name: "My App", muid: "m", muid_v2: "m", ow_version: "tauri-2.12.1", app_version: "1.0.0" };
/// let url = settings_url(DEFAULT_CMP_URL, &f, "purposes", "en", true, true);
/// assert!(url.ends_with("cmp.html?uid=u&appName=My App&tabName=purposes&lang=en&firstRun=true&cmpRequired=true&muid=m&muidv2=m&oweVersion=tauri-2.12.1&appVersion=1.0.0"));
/// ```
#[must_use]
pub fn settings_url(
    base: &str,
    f: &ConsentFacts<'_>,
    tab: &str,
    language: &str,
    first_run: bool,
    cmp_required: bool,
) -> String {
    let sep = if base.contains('?') { '&' } else { '?' };
    format!(
        "{base}{sep}uid={}&appName={}&tabName={}&lang={}&firstRun={first_run}&cmpRequired={cmp_required}&muid={}&muidv2={}&oweVersion={}&appVersion={}",
        encode_uri_component(f.uid),
        f.app_name,
        encode_uri_component(tab),
        encode_uri_component(language),
        encode_uri_component(f.muid),
        encode_uri_component(f.muid_v2),
        encode_uri_component(f.ow_version),
        encode_uri_component(f.app_version),
    )
}

/// What a `cmp-eu-only` response means (D.6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EuOnlyOutcome {
    /// Whether this launch caches the result. A `{}` body (an object
    /// without `params`) is not cached: every `isCMPRequired()` call sends
    /// a new request and opens a new startup window.
    pub cacheable: bool,
    /// `isCMPRequired()`: `false` only when `params` holds the string
    /// `"no-cmp"` (the answer outside the consent region [OBS: Windows lab,
    /// a US runner]); `true` for every other body and a failed request.
    pub cmp_required: bool,
    /// A non-empty `params` body, logged once at debug level.
    pub params: Option<String>,
}

/// The `params` entry that makes consent not required (D.6.2).
pub const NO_CMP_PARAM: &str = "no-cmp";

/// Interprets a `cmp-eu-only` response body (or `None` when the request
/// failed, D.6.2).
///
/// ```
/// use tauri_plugin_overwolf::consent::eu_only_outcome;
/// assert!(eu_only_outcome(Some(br#"{"params":[]}"#)).cacheable);
/// assert!(!eu_only_outcome(Some(b"{}")).cacheable);
/// assert!(eu_only_outcome(Some(b"not json")).cacheable);
/// assert!(eu_only_outcome(None).cacheable);
/// assert_eq!(eu_only_outcome(Some(br#"{"params":[true]}"#)).params.as_deref(), Some("[true]"));
/// assert!(eu_only_outcome(Some(br#"{"params":["false"]}"#)).cmp_required);
/// assert!(!eu_only_outcome(Some(br#"{"params":["no-cmp"]}"#)).cmp_required);
/// ```
#[must_use]
pub fn eu_only_outcome(body: Option<&[u8]>) -> EuOnlyOutcome {
    let parsed = body.and_then(|b| serde_json::from_slice::<Value>(b).ok());
    match parsed {
        Some(Value::Object(m)) => match m.get("params") {
            None => EuOnlyOutcome {
                cacheable: false,
                cmp_required: true,
                params: None,
            },
            Some(p) => EuOnlyOutcome {
                cacheable: true,
                cmp_required: !matches!(
                    p,
                    Value::Array(a) if a.iter().any(|v| v.as_str() == Some(NO_CMP_PARAM))
                ),
                params: match p {
                    Value::Array(a) if a.is_empty() => None,
                    other => Some(other.to_string().chars().take(512).collect()),
                },
            },
        },
        _ => EuOnlyOutcome {
            cacheable: true,
            cmp_required: true,
            params: None,
        },
    }
}

/// The startup consent window's URL when consent is not required (D.6.1,
/// D.6.2): the page clears the stored consent itself (`saveConsent("")`,
/// `saveUnifiedConsent("")`) [OBS: Windows lab].
///
/// ```
/// assert!(tauri_plugin_overwolf::consent::clear_consent_url().ends_with("ow-cmp-v2.html?clear=true"));
/// ```
#[must_use]
pub fn clear_consent_url() -> String {
    format!("{STARTUP_CMP_URL}?clear=true")
}

/// Validates a consent string: not empty, printable ASCII (0x21 to 0x7E),
/// at most 16 KiB (A.2.7).
///
/// ```
/// use tauri_plugin_overwolf::consent::valid_consent;
/// assert!(valid_consent("CQTEST.YAAA"));
/// assert!(!valid_consent(""));
/// assert!(!valid_consent("a b"));
/// assert!(!valid_consent("caf\u{e9}"));
/// assert!(!valid_consent(&"x".repeat(16 * 1024 + 1)));
/// ```
#[must_use]
pub fn valid_consent(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_CONSENT_BYTES && s.bytes().all(|b| (0x21..=0x7E).contains(&b))
}

/// The stored form of a unified consent string (F.2): URL-encoded. A value
/// that is not encoded yet (it contains `=` or `&`) is encoded with
/// `encodeURIComponent`; an encoded one is kept.
///
/// ```
/// use tauri_plugin_overwolf::consent::stored_unified;
/// assert_eq!(stored_unified("cmp=A&ac=2~1"), "cmp%3DA%26ac%3D2~1");
/// assert_eq!(stored_unified("cmp%3DA%26ac%3D2~1"), "cmp%3DA%26ac%3D2~1");
/// ```
#[must_use]
pub fn stored_unified(s: &str) -> String {
    if s.contains('=') || s.contains('&') {
        encode_uri_component(s)
    } else {
        s.to_owned()
    }
}

/// The `euconsent-v2` and `acconsent` values from the stored `cmp` block:
/// the TCF string, and the `ac` part of the unified string (D.6.3).
///
/// ```
/// use tauri_plugin_overwolf::consent::cookie_values;
/// assert_eq!(
///     cookie_values(Some("CQ"), Some("cmp%3DCQ%26ac%3D2~1.2")),
///     (Some("CQ".to_owned()), Some("2~1.2".to_owned()))
/// );
/// assert_eq!(cookie_values(None, Some("cmp%3DCQ%26ac%3D")), (Some("CQ".to_owned()), None));
/// ```
#[must_use]
pub fn cookie_values(
    cmp_string: Option<&str>,
    unified: Option<&str>,
) -> (Option<String>, Option<String>) {
    let mut tcf = cmp_string.filter(|s| !s.is_empty()).map(str::to_owned);
    let mut ac = None;
    if let Some(u) = unified {
        for part in percent_decode(u).split('&') {
            match part.split_once('=') {
                Some(("cmp", v)) if tcf.is_none() && !v.is_empty() => tcf = Some(v.to_owned()),
                Some(("ac", v)) if !v.is_empty() => ac = Some(v.to_owned()),
                _ => {}
            }
        }
    }
    (
        tcf.filter(|v| valid_consent(v)),
        ac.filter(|v| valid_consent(v)),
    )
}

/// Builds a consent cookie with D.6.3's attributes: domain `.overwolf.com`,
/// path `/`, `Secure`, `SameSite=None`, not `HttpOnly`, 365 days.
///
/// `domain()` reports `.overwolf.com` with its leading dot: Tauri passes
/// that value to the platform store, where the dot is what makes the cookie
/// reach every `*.overwolf.com` page (a domain cookie) and not only
/// `overwolf.com` itself.
///
/// ```
/// let c = tauri_plugin_overwolf::consent::consent_cookie("euconsent-v2", "CQ");
/// assert_eq!(c.domain(), Some(".overwolf.com"));
/// ```
#[cfg(feature = "plugin")]
#[must_use]
pub fn consent_cookie(name: &str, value: &str) -> tauri::webview::cookie::Cookie<'static> {
    use tauri::webview::cookie::{Cookie, SameSite, time};
    Cookie::build((name.to_owned(), value.to_owned()))
        .domain(crate::host::cookies::domain_cookie_attr("overwolf.com"))
        .path("/")
        .secure(true)
        .http_only(false)
        .same_site(SameSite::None)
        .expires(time::OffsetDateTime::now_utc() + time::Duration::days(COOKIE_DAYS))
        .build()
}

/// `cmp_event` names (A.2.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CmpEventName {
    /// The shim started.
    Ready,
    /// `window.cmp.saveConsent`.
    SaveConsent,
    /// `window.cmp.saveUnifiedConsent`.
    SaveUnifiedConsent,
    /// `window.privacy.enableAdOptimization`.
    EnableAdOptimization,
    /// `window.close()`.
    Close,
}

/// `cmp_event` data (A.2.7).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CmpEventData {
    /// The consent string.
    #[serde(default)]
    pub consent: Option<String>,
    /// The ad-optimisation toggle.
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// The ad-optimisation answer before a consent page stored one (D.6.6,
/// `app.overwolf.enableAdsOptimization`): ow-electron answers `true` on
/// Windows and `false` on macOS, already at module load, before any
/// request [OBS: Windows lab, macOS lab]. Linux follows macOS (a decision).
pub const AD_OPTIMIZATION_DEFAULT: bool = cfg!(windows);

/// The stored ad-optimisation toggle, or [`AD_OPTIMIZATION_DEFAULT`].
///
/// ```
/// use tauri_plugin_overwolf::consent::{ad_optimization, AD_OPTIMIZATION_DEFAULT};
/// assert!(ad_optimization(Some(true)));
/// assert_eq!(ad_optimization(None), AD_OPTIMIZATION_DEFAULT);
/// ```
#[must_use]
pub fn ad_optimization(stored: Option<bool>) -> bool {
    stored.unwrap_or(AD_OPTIMIZATION_DEFAULT)
}

/// The `cmp.js` configuration (D.6.6).
///
/// ```
/// assert_eq!(tauri_plugin_overwolf::consent::cmp_config(true).to_string(), r#"{"adOptimization":true}"#);
/// ```
#[must_use]
pub fn cmp_config(ad_optimization: bool) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("adOptimization".into(), Value::Bool(ad_optimization));
    Value::Object(m)
}

/// The settings window's preloader page (D.6.4): a spinner in
/// `spinner_color` on `background`, as a `data:` URL. Colours that are not
/// plain CSS colour tokens fall back to the defaults.
///
/// ```
/// let url = tauri_plugin_overwolf::consent::preloader_url("#0D0D0D", "red; x");
/// assert!(url.starts_with("data:text/html,"));
/// assert!(!url.contains("red; x"));
/// ```
#[must_use]
pub fn preloader_url(background: &str, spinner_color: &str) -> String {
    let ok = |c: &str| {
        !c.is_empty()
            && c.len() <= 32
            && c.bytes().all(|b| {
                b.is_ascii_alphanumeric() || matches!(b, b'#' | b'(' | b')' | b',' | b'.' | b'%')
            })
    };
    let bg = if ok(background) {
        background
    } else {
        DEFAULT_BACKGROUND
    };
    let fg = if ok(spinner_color) {
        spinner_color
    } else {
        "#FFFFFF"
    };
    let html = format!(
        "<!doctype html><html><head><meta charset=utf-8><title>CMP</title><style>html,body{{margin:0;height:100%;background:{bg}}}div{{position:absolute;top:50%;left:50%;width:40px;height:40px;margin:-20px;border:4px solid transparent;border-top-color:{fg};border-radius:50%;animation:s 1s linear infinite}}@keyframes s{{to{{transform:rotate(360deg)}}}}</style></head><body><div></div></body></html>"
    );
    format!("data:text/html,{}", encode_uri_component(&html))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (Windows lab, a US runner): `cmp-eu-only` answered
    /// `{"params":["no-cmp"]}` and ow-electron's `isCMPRequired()` was
    /// `false`; ow-tauri said `true` and ran the full consent page.
    #[test]
    fn no_cmp_is_the_only_answer_that_makes_consent_not_required() {
        let required = |body: Option<&[u8]>| eu_only_outcome(body).cmp_required;
        assert!(!required(Some(br#"{"params":["no-cmp"]}"#)));
        assert!(!required(Some(br#"{"params":["x","no-cmp"]}"#)));
        let no_cmp = eu_only_outcome(Some(br#"{"params":["no-cmp"]}"#));
        assert!(no_cmp.cacheable);
        assert_eq!(no_cmp.params.as_deref(), Some(r#"["no-cmp"]"#));
        for body in [
            &br#"{"params":[]}"#[..],
            br#"{"params":[false]}"#,
            br#"{"params":["NO-CMP"]}"#,
            br#"{"params":"no-cmp"}"#,
            br#"{"params":{"no-cmp":true}}"#,
            b"{}",
            b"not json",
        ] {
            assert!(required(Some(body)), "{}", String::from_utf8_lossy(body));
        }
        assert!(required(None));
    }

    #[test]
    fn decode_roundtrip() {
        let raw = "cmp=CQ.A-_~&ac=2~1.35";
        assert_eq!(percent_decode(&encode_uri_component(raw)), raw);
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
    }

    #[test]
    fn cmp_event_wire() {
        let e: CmpEventName = serde_json::from_str("\"saveUnifiedConsent\"").unwrap();
        assert_eq!(e, CmpEventName::SaveUnifiedConsent);
        let d: CmpEventData = serde_json::from_str(r#"{"consent":"X"}"#).unwrap();
        assert_eq!(d.consent.as_deref(), Some("X"));
        assert!(serde_json::from_str::<CmpEventName>("\"other\"").is_err());
    }

    #[test]
    fn settings_url_with_query() {
        let f = ConsentFacts {
            uid: "u",
            app_name: "A",
            muid: "m",
            muid_v2: "m",
            ow_version: "v",
            app_version: "1",
        };
        assert!(
            settings_url("https://x.example/p?a=1", &f, "vendors", "de", false, true).starts_with(
                "https://x.example/p?a=1&uid=u&appName=A&tabName=vendors&lang=de&firstRun=false"
            )
        );
    }

    #[cfg(feature = "plugin")]
    #[test]
    fn cookie_attributes() {
        let c = consent_cookie("euconsent-v2", "CQ");
        assert_eq!(c.domain(), Some(".overwolf.com"));
        assert_eq!(c.path(), Some("/"));
        assert_eq!(c.secure(), Some(true));
        assert_eq!(c.same_site(), Some(tauri::webview::cookie::SameSite::None));
        assert_eq!(c.http_only(), Some(false));
        assert!(c.expires_datetime().is_some());
    }
}
