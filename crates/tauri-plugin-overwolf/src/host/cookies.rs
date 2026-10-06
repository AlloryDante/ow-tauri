//! The ads data store's cookies on host requests (CONTRACT E.1, D.6.3,
//! D.8.1): which stored cookies a request URL gets and in which order, how a
//! `Set-Cookie` response header is written back, and the request hooks the
//! analytics dispatcher calls.
//!
//! ow-electron's host requests go through Chromium's network stack with the
//! default session, so they carry that session's cookies for the request
//! URL and store the cookies the response sets (observed). ow-tauri reads
//! and writes the ads data store instead:
//!
//! - macOS: the default `WKWebsiteDataStore`, read natively (Tauri's
//!   per-URL getter compares domains exactly, so it misses `.overwolf.com`
//!   cookies on `analyticsnew.overwolf.com`) and matched with RFC 6265 here;
//! - Windows: the `EBWebView-ow` environment through any ad guest or consent
//!   window (`ICoreWebView2CookieManager::GetCookies` matches natively);
//!   requests made while none is open go without cookies;
//! - Linux: the default `WebKitGTK` context through any webview
//!   (`webkit_cookie_manager_get_cookies` matches natively).

use std::cmp::Reverse;
use std::sync::{Arc, Weak};
use std::time::Duration;

use tauri::webview::cookie::{Cookie, Expiration};
use tauri::{Manager, Runtime, Webview};
use url::Url;

use super::Host;
use crate::analytics::transport::{BoxFuture, RequestHooks};
use crate::state::log::LogLevel;
use crate::window::{WebviewClass, classify};

/// The longest a host request waits for the store's cookies.
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

/// The `cookie` header for `url` from every cookie of a store; `None` when
/// nothing matches.
#[cfg(test)]
pub(crate) fn cookie_header(cookies: &[StoredCookie], url: &Url, now_secs: f64) -> Option<String> {
    format_header(
        matching_cookies(cookies, url, now_secs)
            .iter()
            .map(|c| (c.name.as_str(), c.value.as_str())),
    )
}

/// `name=value; name=value`, or `None` for no cookies.
pub(crate) fn format_header<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> Option<String> {
    let parts: Vec<String> = pairs.map(|(n, v)| format!("{n}={v}")).collect();
    (!parts.is_empty()).then(|| parts.join("; "))
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

/// RFC 6265 5.1.4 default path of `url`.
fn default_path(url: &Url) -> String {
    let path = url.path();
    match path.rfind('/') {
        Some(i) if i > 0 && path.starts_with('/') => path[..i].to_owned(),
        _ => "/".to_owned(),
    }
}

/// A `Set-Cookie` header received from `url`, as a cookie Tauri's
/// `set_cookie` stores with the scope a browser gives it: a domain cookie
/// for a `Domain` attribute that covers the host (RFC 6265 5.3 step 6), a
/// host-only cookie otherwise, the default path when `Path` is absent or
/// not absolute. A `Domain` outside the host, or without a dot (a
/// top-level domain), rejects the cookie.
pub(crate) fn from_set_cookie(header: &str, url: &Url) -> Option<Cookie<'static>> {
    let parsed = Cookie::parse(header.to_owned()).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let mut cookie = Cookie::new(parsed.name().to_owned(), parsed.value().to_owned());
    match parsed.domain().map(str::to_ascii_lowercase) {
        Some(d) if !d.is_empty() => {
            if !d.contains('.') || !domain_matches(&format!(".{d}"), &host) {
                return None;
            }
            cookie.set_domain(domain_cookie_attr(&d));
        }
        _ => cookie.set_domain(host),
    }
    let path = parsed
        .path()
        .filter(|p| p.starts_with('/'))
        .map_or_else(|| default_path(url), str::to_owned);
    cookie.set_path(path);
    if let Some(secure) = parsed.secure() {
        cookie.set_secure(secure);
    }
    if let Some(http_only) = parsed.http_only() {
        cookie.set_http_only(http_only);
    }
    if let Some(same_site) = parsed.same_site() {
        cookie.set_same_site(same_site);
    }
    if let Some(max_age) = parsed.max_age() {
        cookie.set_max_age(max_age);
    } else if let Some(Expiration::DateTime(at)) = parsed.expires() {
        cookie.set_expires(at);
    }
    Some(cookie)
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

    /// The ads data store's `cookie` header for `url`, `None` when it has
    /// none, is not reachable yet, or does not answer within
    /// [`COOKIE_READ_LIMIT`].
    pub(crate) async fn ads_store_cookie_header(self: &Arc<Self>, url: &str) -> Option<String> {
        let cookies = self.ads_store_cookies(url).await?;
        format_header(cookies.iter().map(|(n, v)| (n.as_str(), v.as_str())))
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

    /// Writes the `Set-Cookie` values of a host response from `url` to the
    /// ads data store (E.1). Cookies it cannot scope are dropped.
    pub(crate) async fn ads_store_set_cookies(
        self: &Arc<Self>,
        url: &str,
        set_cookies: Vec<String>,
    ) {
        if !self.options.os_queries {
            return;
        }
        let Ok(url) = Url::parse(url) else { return };
        let cookies: Vec<Cookie<'static>> = set_cookies
            .iter()
            .filter_map(|h| from_set_cookie(h, &url))
            .collect();
        if cookies.is_empty() {
            return;
        }
        let Some(webview) = self.ads_store_webview() else {
            self.log(
                LogLevel::Debug,
                "response cookies dropped: the ads data store is not open yet",
            );
            return;
        };
        let written = tauri::async_runtime::spawn_blocking(move || {
            cookies
                .into_iter()
                .filter(|c| webview.set_cookie(c.clone()).is_ok())
                .count()
        })
        .await
        .unwrap_or(0);
        self.log(
            LogLevel::Debug,
            &format!("{written} response cookies written to the ads data store"),
        );
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

    fn cookie_header(&self, url: &str) -> BoxFuture<Option<String>> {
        let host = self.0.upgrade();
        let url = url.to_owned();
        Box::pin(async move {
            match host {
                Some(h) => h.ads_store_cookie_header(&url).await,
                None => None,
            }
        })
    }

    fn store_cookies(&self, url: &str, set_cookies: Vec<String>) -> BoxFuture<()> {
        let host = self.0.upgrade();
        let url = url.to_owned();
        Box::pin(async move {
            if let Some(h) = host {
                h.ads_store_set_cookies(&url, set_cookies).await;
            }
        })
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
        let url = Url::parse("https://analyticsnew.overwolf.com/analytics/Counter?Name=x").unwrap();
        assert_eq!(
            cookie_header(&jar, &url, 1_000.0).as_deref(),
            Some("deep=v-deep; euconsent-v2=v-euconsent-v2; live=v-live")
        );
        // Secure cookies stay off plain http.
        let http = Url::parse("http://analyticsnew.overwolf.com/").unwrap();
        assert_eq!(
            cookie_header(&jar, &http, 1_000.0).as_deref(),
            Some("live=v-live")
        );
        let none = Url::parse("https://example.org/").unwrap();
        assert_eq!(cookie_header(&jar, &none, 1_000.0), None);
    }

    #[test]
    fn two_dots_reach_the_store_as_one() {
        let c = Cookie::build(("a", "b"))
            .domain(domain_cookie_attr(".overwolf.com"))
            .build();
        assert_eq!(c.domain(), Some(".overwolf.com"));
    }

    #[test]
    fn set_cookie_is_scoped_like_a_browser() {
        let url =
            Url::parse("https://tracking.overwolf.com/tracking/InsertStats?Stats=true").unwrap();
        let c = from_set_cookie(
            "sid=1; Domain=.Overwolf.com; Path=/; Secure; HttpOnly; Max-Age=60; SameSite=None",
            &url,
        )
        .unwrap();
        assert_eq!((c.name(), c.value()), ("sid", "1"));
        assert_eq!(c.domain(), Some(".overwolf.com"));
        assert_eq!(c.path(), Some("/"));
        assert_eq!(c.secure(), Some(true));
        assert_eq!(c.http_only(), Some(true));
        assert_eq!(
            c.max_age()
                .map(tauri::webview::cookie::time::Duration::whole_seconds),
            Some(60)
        );
        // Host-only, default path.
        let c = from_set_cookie("h=2", &url).unwrap();
        assert_eq!(c.domain(), Some("tracking.overwolf.com"));
        assert_eq!(c.path(), Some("/tracking"));
        // Foreign or top-level domains are rejected.
        assert!(from_set_cookie("x=1; Domain=example.com", &url).is_none());
        assert!(from_set_cookie("x=1; Domain=com", &url).is_none());
        // A relative path falls back to the default path.
        let c = from_set_cookie("p=1; Path=rel", &url).unwrap();
        assert_eq!(c.path(), Some("/tracking"));
    }

    #[test]
    fn header_formatting() {
        assert_eq!(format_header(std::iter::empty()), None);
        assert_eq!(
            format_header([("a", "1"), ("b", "2")].into_iter()).as_deref(),
            Some("a=1; b=2")
        );
    }
}
