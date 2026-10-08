//! Matching the ads data store's cookies (CONTRACT D.6.3, D.8.1) for the
//! consent cookie fallback.
//!
//! Host requests themselves carry no cookies and store none: ow-electron's
//! do not either (observed). On macOS the default `WKWebsiteDataStore` is
//! read natively (Tauri's per-URL getter compares domains exactly, so it
//! misses `.overwolf.com` cookies on `www.overwolf.com`) and matched with
//! RFC 6265 here; on Windows `ICoreWebView2CookieManager::GetCookies`
//! matches natively through an `owad-*` or `ow-cmp*` webview, never an app
//! webview (CRIT §2.5). Only the consent cookies' names leave a read
//! (SEC-m8): no value and no other cookie is kept.
#![cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "only macOS matches the store's cookies here; WebView2 matches natively"
    )
)]

use std::cmp::Reverse;
use std::sync::Arc;
use std::time::Duration;

use tauri::Runtime;
use url::Url;

use super::Core;

/// The consent cookies (D.6.3).
pub(crate) const CONSENT_COOKIES: [&str; 2] = ["euconsent-v2", "acconsent"];

/// The page whose cookies the consent fallback checks (D.6.3).
pub(crate) const CONSENT_COOKIE_URL: &str = "https://www.overwolf.com/";

/// How long a store read may take.
const STORE_READ_LIMIT: Duration = Duration::from_secs(3);

/// A cookie as a platform store holds it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredCookie {
    pub(crate) name: String,
    pub(crate) value: String,
    /// `.example.com` for a domain cookie, `www.example.com` for a
    /// host-only one (the stores' own convention).
    pub(crate) domain: String,
    pub(crate) path: String,
    pub(crate) secure: bool,
    /// Unix seconds; `None` for a session cookie.
    pub(crate) expires: Option<f64>,
}

/// RFC 6265 5.1.3 domain matching with the store's convention: a leading
/// dot marks a domain cookie.
///
/// Domain cookies never match an IP address.
pub(crate) fn domain_matches(cookie_domain: &str, host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let domain = cookie_domain.to_ascii_lowercase();
    match domain.strip_prefix('.') {
        Some(suffix) => {
            host.parse::<std::net::IpAddr>().is_err()
                && (host == suffix
                    || host
                        .strip_suffix(suffix)
                        .is_some_and(|rest| rest.ends_with('.')))
        }
        None => host == domain,
    }
}

/// RFC 6265 5.1.4 path matching.
pub(crate) fn path_matches(cookie_path: &str, request_path: &str) -> bool {
    let cookie_path = if cookie_path.is_empty() {
        "/"
    } else {
        cookie_path
    };
    request_path == cookie_path
        || request_path
            .strip_prefix(cookie_path)
            .is_some_and(|rest| cookie_path.ends_with('/') || rest.starts_with('/'))
}

/// The cookies of a store that a request to `url` carries (RFC 6265 5.4):
/// matching domain, path, scheme and expiry, longer paths first, the
/// store's order otherwise.
pub(crate) fn matching_cookies<'a>(
    cookies: &'a [StoredCookie],
    url: &Url,
    now_secs: f64,
) -> Vec<&'a StoredCookie> {
    let Some(host) = url.host_str() else {
        return Vec::new();
    };
    let secure = matches!(url.scheme(), "https" | "wss");
    let mut matched: Vec<&StoredCookie> = cookies
        .iter()
        .filter(|c| {
            domain_matches(&c.domain, host)
                && path_matches(&c.path, url.path())
                && (secure || !c.secure)
                && c.expires.is_none_or(|e| e > now_secs)
        })
        .collect();
    matched.sort_by_key(|c| Reverse(c.path.len()));
    matched
}

/// The names of the consent cookies among `cookies` that a request to
/// `url` carries (SEC-m8: names only, consent cookies only).
pub(crate) fn consent_cookie_names(
    cookies: &[StoredCookie],
    url: &Url,
    now_secs: f64,
) -> Vec<String> {
    matching_cookies(cookies, url, now_secs)
        .into_iter()
        .filter(|c| CONSENT_COOKIES.contains(&c.name.as_str()))
        .map(|c| c.name.clone())
        .collect()
}

/// Unix seconds now.
fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// The consent cookies of the ads data store that a request to
/// [`CONSENT_COOKIE_URL`] carries, by name; `None` when the store does not
/// answer (or on Linux, which has no ads store). `via_label` is the consent
/// window that reads it on Windows.
pub(crate) async fn consent_cookies_in_store<R: Runtime>(
    core: &Arc<Core<R>>,
    via_label: &str,
) -> Option<Vec<String>> {
    let url = Url::parse(CONSENT_COOKIE_URL).ok()?;
    #[cfg(target_os = "macos")]
    {
        let _ = via_label;
        if !super::windows::native_runtime::<R>() {
            return None;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        core.app
            .run_on_main_thread(move || {
                crate::platform::webview::default_store_cookies(move |cookies| {
                    let _ = tx.send(consent_cookie_names(&cookies, &url, now_secs()));
                });
            })
            .ok()?;
        tokio::time::timeout(STORE_READ_LIMIT, rx).await.ok()?.ok()
    }
    #[cfg(windows)]
    {
        let webview = crate::compat::webview(&core.app, via_label)?;
        let read = tauri::async_runtime::spawn_blocking(move || webview.cookies_for_url(url));
        let cookies = tokio::time::timeout(STORE_READ_LIMIT, read)
            .await
            .ok()?
            .ok()?
            .ok()?;
        Some(
            cookies
                .iter()
                .map(|c| c.name().to_owned())
                .filter(|n| CONSENT_COOKIES.contains(&n.as_str()))
                .collect(),
        )
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (core, via_label, url, STORE_READ_LIMIT);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(name: &str, domain: &str, path: &str, secure: bool) -> StoredCookie {
        StoredCookie {
            name: name.into(),
            value: format!("v-{name}"),
            domain: domain.into(),
            path: path.into(),
            secure,
            expires: None,
        }
    }

    #[test]
    fn domain_and_path_matching() {
        assert!(domain_matches(".overwolf.com", "analyticsnew.overwolf.com"));
        assert!(domain_matches(".overwolf.com", "overwolf.com"));
        assert!(!domain_matches(".overwolf.com", "notoverwolf.com"));
        assert!(domain_matches("www.overwolf.com", "WWW.overwolf.com"));
        assert!(!domain_matches("overwolf.com", "www.overwolf.com"));
        assert!(!domain_matches(".0.0.1", "127.0.0.1"));
        assert!(path_matches("/", "/analytics/Counter"));
        assert!(path_matches("/analytics", "/analytics/Counter"));
        assert!(path_matches("/analytics/", "/analytics/Counter"));
        assert!(!path_matches("/analytics", "/analyticsx"));
        assert!(path_matches("", "/x"));
    }

    #[test]
    fn header_order_scope_and_expiry() {
        let mut expired = stored("old", ".overwolf.com", "/", false);
        expired.expires = Some(10.0);
        let mut live = stored("live", ".overwolf.com", "/", false);
        live.expires = Some(1e12);
        let jar = vec![
            stored("euconsent-v2", ".overwolf.com", "/", true),
            stored("deep", ".overwolf.com", "/analytics", false),
            stored("host", "www.overwolf.com", "/", false),
            stored("other", ".example.com", "/", false),
            expired,
            live,
        ];
        let names = |url: &str| {
            let url = Url::parse(url).unwrap();
            matching_cookies(&jar, &url, 1_000.0)
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names("https://analyticsnew.overwolf.com/analytics/Counter?Name=x"),
            ["deep", "euconsent-v2", "live"]
        );
        // Secure cookies stay off plain http.
        assert_eq!(names("http://analyticsnew.overwolf.com/"), ["live"]);
        assert!(names("https://example.org/").is_empty());
    }

    #[test]
    fn only_consent_cookie_names_leave_a_read() {
        let mut tcf = stored("euconsent-v2", ".overwolf.com", "/", true);
        tcf.value = "CQ-secret".into();
        let jar = vec![
            tcf,
            stored("acconsent", ".overwolf.com", "/", true),
            stored("_session", ".overwolf.com", "/", true),
            stored("euconsent-v2", ".example.com", "/", true),
        ];
        let url = Url::parse(CONSENT_COOKIE_URL).unwrap();
        assert_eq!(
            consent_cookie_names(&jar, &url, 1_000.0),
            ["euconsent-v2", "acconsent"]
        );
        assert!(consent_cookie_names(&jar[2..], &url, 1_000.0).is_empty());
    }

    #[test]
    fn two_dots_reach_the_store_as_one() {
        let c = tauri::webview::cookie::Cookie::build(("a", "b"))
            .domain(crate::consent::domain_cookie_attr(".overwolf.com"))
            .build();
        assert_eq!(c.domain(), Some(".overwolf.com"));
    }
}
