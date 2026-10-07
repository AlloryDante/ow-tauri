//! Reading the ads data store's cookies (CONTRACT D.6.3, D.8.1): the
//! consent cookie fallback checks whether the consent page wrote its
//! cookies, and the analytics dispatcher's request hooks.
//!
//! Host requests themselves carry no cookies and store none: ow-electron's
//! do not either (observed: Chromium's net log lists every stored cookie as
//! excluded by the request's credentials mode, and no `Set-Cookie` is
//! stored). Reading the store:
//!
//! - macOS: the default `WKWebsiteDataStore`, read natively (Tauri's
//!   per-URL getter compares domains exactly, so it misses `.overwolf.com`
//!   cookies on `www.overwolf.com`) and matched with RFC 6265 here;
//! - Windows: the `EBWebView-ow` environment through any ad guest or consent
//!   window (`ICoreWebView2CookieManager::GetCookies` matches natively);
//! - Linux: the default `WebKitGTK` context through any webview
//!   (`webkit_cookie_manager_get_cookies` matches natively).

use std::cmp::Reverse;
use std::sync::{Arc, Weak};
use std::time::Duration;

use tauri::Runtime;
#[cfg(not(target_os = "macos"))]
use tauri::{Manager, Webview};
use url::Url;

use super::Host;
use crate::analytics::transport::{BoxFuture, RequestHooks};
#[cfg(not(target_os = "macos"))]
use crate::window::{WebviewClass, classify};

/// The longest a read of the store's cookies may take.
const COOKIE_READ_LIMIT: Duration = Duration::from_millis(750);

/// A cookie as a platform store holds it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    not(any(target_os = "macos", test)),
    expect(dead_code, reason = "only macOS matches cookies itself")
)]
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
#[cfg_attr(
    not(any(target_os = "macos", test)),
    expect(dead_code, reason = "only macOS matches cookies itself")
)]
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
#[cfg_attr(
    not(any(target_os = "macos", test)),
    expect(dead_code, reason = "only macOS matches cookies itself")
)]
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

/// The `Domain` value that makes Tauri store a domain cookie for `domain`.
///
/// The platform stores mark a domain cookie with a leading dot
/// (`NSHTTPCookieDomain`, WebView2's `CreateCookie`, `soup_cookie_new`).
/// Tauri hands them `Cookie::domain()`, which removes one leading dot
/// (RFC 6265 5.2.3), so the value carries two: one survives.
pub(crate) fn domain_cookie_attr(domain: &str) -> String {
    format!("..{}", domain.trim_start_matches('.'))
}

/// Now, in Unix seconds (only macOS matches cookies itself).
#[cfg(target_os = "macos")]
fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

impl<R: Runtime> Host<R> {
    /// A webview whose cookie store is the ads data store (D.8.1), if one
    /// is open: on Windows an ad guest or consent window (the `EBWebView-ow`
    /// environment); elsewhere those first, else any webview (one default
    /// store).
    #[cfg(not(target_os = "macos"))]
    fn ads_store_webview(&self) -> Option<Webview<R>> {
        let webviews = self.app.webviews();
        let ads = webviews
            .iter()
            .find(|(label, _)| {
                matches!(
                    classify(label),
                    WebviewClass::AdviewGuest | WebviewClass::Cmp
                )
            })
            .map(|(_, w)| w.clone());
        if cfg!(windows) {
            ads
        } else {
            ads.or_else(|| webviews.into_values().next())
        }
    }

    /// The ads data store's cookies for `url` as `(name, value)` in header
    /// order; `None` when the store is not reachable (no store webview yet,
    /// OS queries off in tests, or no answer within [`COOKIE_READ_LIMIT`]).
    pub(crate) async fn ads_store_cookies(
        self: &Arc<Self>,
        url: &str,
    ) -> Option<Vec<(String, String)>> {
        if !self.options.os_queries {
            return None;
        }
        let url = Url::parse(url).ok()?;
        tokio::time::timeout(COOKIE_READ_LIMIT, self.read_store_cookies(url))
            .await
            .ok()
            .flatten()
    }

    #[cfg(target_os = "macos")]
    async fn read_store_cookies(self: &Arc<Self>, url: Url) -> Option<Vec<(String, String)>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.app
            .run_on_main_thread(move || {
                crate::platform::webview::default_store_cookies(move |cookies| {
                    let _ = tx.send(cookies);
                });
            })
            .ok()?;
        let cookies = rx.await.ok()?;
        Some(
            matching_cookies(&cookies, &url, now_secs())
                .into_iter()
                .map(|c| (c.name.clone(), c.value.clone()))
                .collect(),
        )
    }

    #[cfg(not(target_os = "macos"))]
    async fn read_store_cookies(self: &Arc<Self>, url: Url) -> Option<Vec<(String, String)>> {
        let webview = self.ads_store_webview()?;
        // The platform call blocks until the main thread answers.
        let cookies = tauri::async_runtime::spawn_blocking(move || webview.cookies_for_url(url))
            .await
            .ok()?
            .ok()?;
        Some(
            cookies
                .iter()
                .map(|c| (c.name().to_owned(), c.value().to_owned()))
                .collect(),
        )
    }
}

/// The analytics dispatcher's hooks into the host (E.1).
pub(crate) struct HostRequestHooks<R: Runtime>(pub(crate) Weak<Host<R>>);

impl<R: Runtime> RequestHooks for HostRequestHooks<R> {
    fn wait_user_agent(&self) -> BoxFuture<()> {
        let host = self.0.upgrade();
        Box::pin(async move {
            if let Some(host) = host {
                host.wait_user_agent().await;
            }
        })
    }

    fn user_agent(&self) -> Option<String> {
        self.0.upgrade().map(|h| h.user_agent())
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
    fn two_dots_reach_the_store_as_one() {
        let c = tauri::webview::cookie::Cookie::build(("a", "b"))
            .domain(domain_cookie_attr(".overwolf.com"))
            .build();
        assert_eq!(c.domain(), Some(".overwolf.com"));
    }
}
