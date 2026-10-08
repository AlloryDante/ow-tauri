//! Matching the ads data store's cookies (CONTRACT D.6.3, D.8.1) for the
//! consent cookie fallback.
//!
//! Host requests themselves carry no cookies and store none: ow-electron's
//! do not either (observed). On macOS the default `WKWebsiteDataStore` is
//! read natively (Tauri's per-URL getter compares domains exactly, so it
//! misses `.overwolf.com` cookies on `www.overwolf.com`) and matched with
//! RFC 6265 here; on Windows `ICoreWebView2CookieManager::GetCookies`
//! matches natively through an `owad-*` or `ow-cmp*` webview. The store
//! reads come back with the consent host (W2); the matching rules are
//! pinned here.
#![allow(
    dead_code,
    reason = "the consent host (W2) reads the store through these"
)]

use std::cmp::Reverse;

use url::Url;

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
    fn two_dots_reach_the_store_as_one() {
        let c = tauri::webview::cookie::Cookie::build(("a", "b"))
            .domain(crate::consent::domain_cookie_attr(".overwolf.com"))
            .build();
        assert_eq!(c.domain(), Some(".overwolf.com"));
    }
}
