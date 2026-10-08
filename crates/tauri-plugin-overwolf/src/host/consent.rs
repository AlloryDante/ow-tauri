//! Consent (DESIGN §4.7): the startup `cmp-eu-only` request and consent
//! round, `isCMPRequired()`, and the ad privacy settings window.
//!
//! W1 holds the frozen shape and the checks that do not need a window: the
//! `cmpURL` allowlist of JavaScript callers. The startup round, the windows
//! and the cookie fallback arrive in W2; until then consent counts as
//! required and the windows answer `unsupported`.
#![allow(
    dead_code,
    clippy::unused_self,
    reason = "the consent flow (W2) reads the remaining state"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::webview::PageLoadEvent;
use tauri::{Runtime, Webview, WindowEvent};
use url::Url;

use super::Core;
use crate::error::{Error, Result};
use crate::types::CmpWindowOptions;

/// The consent service of one app.
#[derive(Debug, Default)]
pub(crate) struct ConsentCore {
    /// The last `cmp-eu-only` answer said consent is not required
    /// (D.6.2).
    not_required: AtomicBool,
}

impl ConsentCore {
    /// `isCMPRequired()` (D.6.2): never fails.
    pub(crate) fn is_cmp_required(&self) -> bool {
        !self.not_required.load(Ordering::SeqCst)
    }

    /// `openAdPrivacySettingsWindow` / `openCMPWindow` (W2).
    ///
    /// # Errors
    ///
    /// `unsupported` until the consent windows arrive.
    pub(crate) fn open_settings_window<R: Runtime>(
        &self,
        _core: &Arc<Core<R>>,
        _options: &CmpWindowOptions,
        _caller_window: Option<&str>,
    ) -> Result<()> {
        Err(Error::unsupported(
            "the ad privacy settings window is not available in this build yet",
        ))
    }

    /// A page load of a consent window (W2).
    pub(crate) fn page_load<R: Runtime>(
        &self,
        _core: &Arc<Core<R>>,
        _webview: &Webview<R>,
        _event: PageLoadEvent,
        _url: &Url,
    ) {
    }

    /// The navigation policy of consent window `label` (W2). No consent
    /// window exists yet: a webview with a reserved label the plugin did
    /// not create may not navigate anywhere.
    pub(crate) fn navigation<R: Runtime>(
        &self,
        _core: &Arc<Core<R>>,
        _label: &str,
        _url: &Url,
    ) -> bool {
        false
    }

    /// A window event (W2: consent windows closing, the last-window rule).
    pub(crate) fn window_event<R: Runtime>(
        &self,
        _core: &Arc<Core<R>>,
        _label: &str,
        _event: &WindowEvent,
    ) {
    }
}

/// Starts consent at Ready: `cmp-eu-only` with the burst, then the startup
/// round (W2).
pub(crate) fn start<R: Runtime>(_core: &Arc<Core<R>>) {}

/// Checks a `cmpURL` from JavaScript against `consent.allowedCmpOrigins`
/// (Rust callers are trusted and may use any `https:` URL).
///
/// # Errors
///
/// `invalid-argument` when the URL is not `https:` or its origin is not
/// listed.
pub(crate) fn check_js_cmp_url(url: &str, allowed_origins: &[String]) -> Result<()> {
    let parsed = Url::parse(url).map_err(|_| Error::invalid_argument("cmpURL is not a URL"))?;
    if parsed.scheme() != "https" {
        return Err(Error::invalid_argument("cmpURL must be an https: URL"));
    }
    let origin = parsed.origin().ascii_serialization();
    if allowed_origins
        .iter()
        .any(|o| o.trim_end_matches('/') == origin)
    {
        Ok(())
    } else {
        Err(Error::invalid_argument(
            "cmpURL's origin is not in plugins.overwolf.consent.allowedCmpOrigins",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_cmp_urls_must_be_allowed() {
        let allowed = vec!["https://content.overwolf.com".to_owned()];
        check_js_cmp_url("https://content.overwolf.com/monsdk/x.html", &allowed).unwrap();
        for bad in [
            "http://content.overwolf.com/x",
            "https://evil.example/x",
            "https://content.overwolf.com.evil.example/x",
            "not a url",
        ] {
            let err = check_js_cmp_url(bad, &allowed).unwrap_err();
            assert_eq!(err.code(), crate::ErrorCode::InvalidArgument, "{bad}");
        }
        assert!(ConsentCore::default().is_cmp_required());
    }
}
