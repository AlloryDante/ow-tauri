//! Native read of an app webview's user agent (DESIGN §4.10).
//!
//! - Windows: `ICoreWebView2Settings2::get_UserAgent`, which equals
//!   `navigator.userAgent` (W0c B2).
//! - macOS: `-[WKWebView valueForKey:@"userAgent"]`, only when the view
//!   answers to the private `_userAgent` getter the key resolves to (so the
//!   call cannot raise `NSUnknownKeyException` on a future `WebKit`), else
//!   `customUserAgent` when the app set one. Both are synchronous and work
//!   before the first page load (W0c).
//! - Linux: nothing (no ads; the template is used).
//!
//! The caller checks the shape of what is read
//! ([`crate::analytics::user_agent::accepts_native`]): an app-set user agent
//! is never used for Overwolf's requests.

use tauri::{Runtime, Webview};

/// Reads `webview`'s user agent natively and calls `done` on the webview's
/// thread with it (`None` when nothing could be read). Returns at once.
///
/// # Errors
///
/// When the webview is gone (`done` is then never called).
pub(crate) fn read_native<R: Runtime>(
    webview: &Webview<R>,
    done: impl FnOnce(Option<String>) + Send + 'static,
) -> tauri::Result<()> {
    webview.with_webview(move |pw| {
        #[cfg(target_os = "macos")]
        let ua = macos::user_agent(pw.inner());
        #[cfg(windows)]
        let ua = windows_impl::user_agent(&pw.controller());
        #[cfg(not(any(target_os = "macos", windows)))]
        let ua = {
            let _ = pw;
            None
        };
        done(ua.filter(|s| !s.is_empty()));
    })
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, sel};
    use objc2_foundation::NSString;

    /// The `WKWebView`'s user agent: the effective one through key-value
    /// coding when the view has `_userAgent`, else `customUserAgent`.
    pub(super) fn user_agent(wk_webview: *mut c_void) -> Option<String> {
        if wk_webview.is_null() {
            return None;
        }
        // SAFETY: Tauri hands a live `WKWebView*` on the main thread.
        let obj: &AnyObject = unsafe { &*wk_webview.cast::<AnyObject>() };
        // SAFETY: `respondsToSelector:` exists on every NSObject.
        let has_getter: bool = unsafe { msg_send![obj, respondsToSelector: sel!(_userAgent)] };
        if has_getter {
            let key = NSString::from_str("userAgent");
            // SAFETY: key-value coding of a key the view answers to
            // (checked above), so it cannot raise.
            let value: Option<Retained<AnyObject>> = unsafe { msg_send![obj, valueForKey: &*key] };
            if let Some(ua) = value
                .and_then(|v| v.downcast::<NSString>().ok())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
            {
                return Some(ua);
            }
        }
        // SAFETY: a public `WKWebView` property (macOS 10.11+).
        let custom: Option<Retained<NSString>> = unsafe { msg_send![obj, customUserAgent] };
        custom.map(|s| s.to_string()).filter(|s| !s.is_empty())
    }
}

#[cfg(windows)]
mod windows_impl {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Controller, ICoreWebView2Settings2,
    };
    use webview2_com::take_pwstr;
    use windows::core::{Interface, PWSTR};

    /// `ICoreWebView2Settings2::get_UserAgent`.
    pub(super) fn user_agent(controller: &ICoreWebView2Controller) -> Option<String> {
        // SAFETY: COM calls on the webview's own thread with a valid out
        // pointer.
        unsafe {
            let settings = controller
                .CoreWebView2()
                .ok()?
                .Settings()
                .ok()?
                .cast::<ICoreWebView2Settings2>()
                .ok()?;
            let mut ua = PWSTR::null();
            settings.UserAgent(&raw mut ua).ok()?;
            Some(take_pwstr(ua))
        }
    }
}
