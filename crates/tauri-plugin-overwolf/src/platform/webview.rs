//! Platform webview operations the ads and consent services need and Tauri
//! does not offer: muting a guest, the guest's first navigation with
//! `Referer` and `Origin` (D.8.3), and on Windows the request shaping
//! handler, guest crash and load-failure reports (D.7, D.8).
//!
//! Every function hands its work to the webview's thread with
//! `Webview::with_webview` and returns at once.

use std::sync::Arc;

use tauri::{Runtime, Webview};

/// What the request shaping of one guest needs (D.8.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shaping {
    /// `https://www.overwolf.com/<uid>`.
    pub(crate) referer: String,
    /// `https://www.overwolf.com`.
    pub(crate) origin: String,
    /// `<uid>`.
    pub(crate) uid: String,
    /// `phasePercent`.
    pub(crate) phase: String,
    /// The embedder window's analytics name.
    pub(crate) window: String,
}

/// How a request from a guest is shaped (D.8.2).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    not(any(windows, test)),
    expect(
        dead_code,
        reason = "subresource shaping exists on WebView2 only (D.8.3)"
    )
)]
pub(crate) enum RequestShape {
    /// The ad document: `Referer` and `Origin`.
    Document,
    /// The ad library: no `Origin`; `x-ow-uid`, `x-ow-phase`, `x-ow-window`
    /// appended.
    AdLibrary,
    /// Every other request: `Origin` set.
    Other,
}

/// Classifies a guest request (D.8.2). `document` is whether it is the
/// main-frame document request.
#[cfg_attr(
    not(any(windows, test)),
    expect(
        dead_code,
        reason = "subresource shaping exists on WebView2 only (D.8.3)"
    )
)]
pub(crate) fn request_shape(url: &str, document: bool) -> RequestShape {
    if document && url.split(['?', '#']).next() == Some(crate::ads::ADVIEW_URL) {
        RequestShape::Document
    } else if url.starts_with("https://content.overwolf.com/libs/ads/latest/owads.min.js") {
        RequestShape::AdLibrary
    } else {
        RequestShape::Other
    }
}

/// Mutes or unmutes the page's audio. Errors only when the webview is gone.
pub(crate) fn set_muted<R: Runtime>(webview: &Webview<R>, muted: bool) -> tauri::Result<()> {
    webview.with_webview(move |pw| {
        #[cfg(target_os = "macos")]
        {
            macos::set_page_muted(pw.inner(), muted);
        }
        #[cfg(windows)]
        {
            windows_impl::set_muted(&pw.controller(), muted);
        }
        #[cfg(target_os = "linux")]
        {
            use webkit2gtk::WebViewExt as _;
            pw.inner().set_is_muted(muted);
        }
        #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
        {
            let _ = (pw, muted);
        }
    })
}

/// The guest's first navigation to `url` with the document headers of
/// D.8.3: `WKWebView.load(URLRequest)` on macOS and
/// `webkit_web_view_load_request` on Linux; on Windows a plain navigation
/// whose headers the request handler sets. Returns `false` when the
/// platform call could not be posted (the caller navigates plainly).
pub(crate) fn load_shaped<R: Runtime>(
    webview: &Webview<R>,
    url: &url::Url,
    shaping: Option<&Shaping>,
) -> bool {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if let Some(s) = shaping {
        let url = url.to_string();
        let referer = s.referer.clone();
        let origin = s.origin.clone();
        return webview
            .with_webview(move |pw| {
                #[cfg(target_os = "macos")]
                macos::load_request(
                    pw.inner(),
                    &url,
                    &[("Referer", &referer), ("Origin", &origin)],
                );
                #[cfg(target_os = "linux")]
                {
                    use webkit2gtk::{URIRequestExt as _, WebViewExt as _};
                    let request = webkit2gtk::URIRequest::new(&url);
                    if let Some(headers) = request.http_headers() {
                        headers.replace("Referer", &referer);
                        headers.replace("Origin", &origin);
                    }
                    pw.inner().load_request(&request);
                }
            })
            .is_ok();
    }
    let _ = shaping;
    webview.navigate(url.clone()).is_ok()
}

/// Platform reports from a guest that Tauri does not deliver.
pub(crate) trait GuestReports: Send + Sync + 'static {
    /// The guest's render process ended (`reason` is Electron's spelling).
    fn crashed(&self, label: &str, reason: crate::ads::GoneReason, exit_code: i64);
    /// A main-frame load failed.
    #[cfg_attr(
        not(any(windows, target_os = "linux")),
        expect(
            dead_code,
            reason = "WKWebView load failures are not reported to plugins"
        )
    )]
    fn load_failed(&self, label: &str, error_code: i64, description: &str, url: &str);
}

/// Installs the per-guest platform hooks: on Windows the request shaping
/// handler (D.8.3), `ProcessFailed` and `NavigationCompleted`; on Linux
/// `web-process-terminated` and `load-failed`. macOS reports crashes through
/// the app's `on_web_content_process_terminate` hook instead (A.5).
pub(crate) fn install_guest_hooks<R: Runtime>(
    webview: &Webview<R>,
    shaping: Option<Shaping>,
    reports: Arc<dyn GuestReports>,
) -> tauri::Result<()> {
    let label = webview.label().to_owned();
    webview.with_webview(move |pw| {
        #[cfg(windows)]
        {
            windows_impl::install(&pw.controller(), shaping, label, reports);
        }
        #[cfg(target_os = "linux")]
        {
            let _ = shaping;
            linux::install(&pw.inner(), label, reports);
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (pw, shaping, label, reports);
        }
    })
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;

    use objc2::runtime::AnyObject;
    use objc2::{msg_send, sel};
    use objc2_foundation::{NSMutableURLRequest, NSString, NSURL};

    /// Mutes the page with the `WebKit` selector `_setPageMuted:` when the selector
    /// exists (the only way WKWebView mutes every frame; a known
    /// deviation: private API).
    pub(super) fn set_page_muted(wk_webview: *mut c_void, muted: bool) {
        if wk_webview.is_null() {
            return;
        }
        // SAFETY: Tauri hands a live `WKWebView*` on the main thread.
        let obj: &AnyObject = unsafe { &*wk_webview.cast::<AnyObject>() };
        // SAFETY: `respondsToSelector:` exists on every NSObject.
        let responds: bool = unsafe { msg_send![obj, respondsToSelector: sel!(_setPageMuted:)] };
        if !responds {
            return;
        }
        // `_WKMediaMutedState`: bit 0 mutes audio.
        let state: usize = usize::from(muted);
        // SAFETY: the selector exists (checked above) and takes an
        // NSUInteger.
        let () = unsafe { msg_send![obj, _setPageMuted: state] };
    }

    /// `-[WKWebView loadRequest:]` with extra header fields.
    pub(super) fn load_request(wk_webview: *mut c_void, url: &str, headers: &[(&str, &str)]) {
        if wk_webview.is_null() {
            return;
        }
        let Some(nsurl) = NSURL::URLWithString(&NSString::from_str(url)) else {
            return;
        };
        let request = NSMutableURLRequest::requestWithURL(&nsurl);
        for (name, value) in headers {
            request.setValue_forHTTPHeaderField(
                Some(&NSString::from_str(value)),
                &NSString::from_str(name),
            );
        }
        // SAFETY: Tauri hands a live `WKWebView*` on the main thread.
        let obj: &AnyObject = unsafe { &*wk_webview.cast::<AnyObject>() };
        // SAFETY: `loadRequest:` takes an NSURLRequest and returns a
        // (nullable, autoreleased) WKNavigation.
        let _: *mut AnyObject = unsafe { msg_send![obj, loadRequest: &*request] };
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::Arc;

    use webkit2gtk::WebViewExt as _;

    use super::GuestReports;
    use crate::ads::GoneReason;

    pub(super) fn install(
        webview: &webkit2gtk::WebView,
        label: String,
        reports: Arc<dyn GuestReports>,
    ) {
        let crash_label = label.clone();
        let crash_reports = Arc::clone(&reports);
        let _ = webview.connect_web_process_terminated(move |_, reason| {
            let reason = match reason {
                webkit2gtk::WebProcessTerminationReason::ExceededMemoryLimit => GoneReason::Oom,
                webkit2gtk::WebProcessTerminationReason::TerminatedByApi => GoneReason::Killed,
                _ => GoneReason::Crashed,
            };
            crash_reports.crashed(&crash_label, reason, 0);
        });
        let _ = webview.connect_load_failed(move |_, _event, uri, error| {
            // A cancelled load (a new navigation replaced it) is not a failure.
            if !error.matches(webkit2gtk::NetworkError::Cancelled) {
                reports.load_failed(&label, -2, &error.to_string(), uri);
            }
            false
        });
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::sync::Arc;

    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
        COREWEBVIEW2_PROCESS_FAILED_REASON, COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED,
        COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED,
        COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY,
        COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED, COREWEBVIEW2_WEB_ERROR_STATUS,
        COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED, COREWEBVIEW2_WEB_RESOURCE_CONTEXT,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
        COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL, ICoreWebView2_8, ICoreWebView2_22,
        ICoreWebView2Controller, ICoreWebView2ProcessFailedEventArgs2,
    };
    use webview2_com::{
        NavigationCompletedEventHandler, ProcessFailedEventHandler,
        WebResourceRequestedEventHandler, take_pwstr,
    };
    use windows::core::{BOOL, HSTRING, Interface, PWSTR};

    use super::{GuestReports, RequestShape, Shaping, request_shape};
    use crate::ads::GoneReason;

    pub(super) fn set_muted(controller: &ICoreWebView2Controller, muted: bool) {
        // SAFETY: COM calls on the webview's own thread.
        unsafe {
            if let Ok(core) = controller.CoreWebView2()
                && let Ok(w8) = core.cast::<ICoreWebView2_8>()
            {
                let _ = w8.SetIsMuted(muted);
            }
        }
    }

    fn shape(
        args: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2WebResourceRequestedEventArgs,
        shaping: &Shaping,
    ) -> windows::core::Result<()> {
        // SAFETY: COM calls on the webview's own thread with valid out
        // pointers.
        unsafe {
            let request = args.Request()?;
            let mut uri = PWSTR::null();
            request.Uri(&mut uri)?;
            let uri = take_pwstr(uri);
            let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
            args.ResourceContext(&mut context)?;
            let headers = request.Headers()?;
            let document = context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT;
            match request_shape(&uri, document) {
                RequestShape::Document => {
                    headers
                        .SetHeader(&HSTRING::from("Referer"), &HSTRING::from(&shaping.referer))?;
                    headers.SetHeader(&HSTRING::from("Origin"), &HSTRING::from(&shaping.origin))?;
                }
                RequestShape::AdLibrary => {
                    let _ = headers.RemoveHeader(&HSTRING::from("Origin"));
                    headers.SetHeader(&HSTRING::from("x-ow-uid"), &HSTRING::from(&shaping.uid))?;
                    headers
                        .SetHeader(&HSTRING::from("x-ow-phase"), &HSTRING::from(&shaping.phase))?;
                    headers.SetHeader(
                        &HSTRING::from("x-ow-window"),
                        &HSTRING::from(&shaping.window),
                    )?;
                }
                RequestShape::Other => {
                    headers.SetHeader(&HSTRING::from("Origin"), &HSTRING::from(&shaping.origin))?;
                }
            }
        }
        Ok(())
    }

    fn gone_reason(
        kind: COREWEBVIEW2_PROCESS_FAILED_KIND,
        reason: COREWEBVIEW2_PROCESS_FAILED_REASON,
    ) -> Option<GoneReason> {
        if kind != COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
            && kind != COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
        {
            return None;
        }
        Some(if reason == COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED {
            GoneReason::Killed
        } else if reason == COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED {
            GoneReason::Crashed
        } else if reason == COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY {
            GoneReason::Oom
        } else if reason == COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED {
            GoneReason::LaunchFailed
        } else {
            GoneReason::AbnormalExit
        })
    }

    pub(super) fn install(
        controller: &ICoreWebView2Controller,
        shaping: Option<Shaping>,
        label: String,
        reports: Arc<dyn GuestReports>,
    ) {
        // SAFETY: COM calls on the webview's own thread with valid out
        // pointers; the handlers live as long as the webview.
        let _ = unsafe {
            (|| -> windows::core::Result<()> {
                let core = controller.CoreWebView2()?;
                if let Some(shaping) = shaping {
                    let filter = HSTRING::from("*");
                    if let Ok(w22) = core.cast::<ICoreWebView2_22>() {
                        w22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                            &filter,
                            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                            COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                        )?;
                    } else {
                        core.AddWebResourceRequestedFilter(
                            &filter,
                            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                        )?;
                    }
                    let mut token = 0_i64;
                    core.add_WebResourceRequested(
                        &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                            if let Some(args) = args {
                                let _ = shape(&args, &shaping);
                            }
                            Ok(())
                        })),
                        &mut token,
                    )?;
                }
                let crash_label = label.clone();
                let crash_reports = Arc::clone(&reports);
                let mut token = 0_i64;
                core.add_ProcessFailed(
                    &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                        args.ProcessFailedKind(&mut kind)?;
                        let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
                        let mut exit_code = 0_i32;
                        if let Ok(a2) = args.cast::<ICoreWebView2ProcessFailedEventArgs2>() {
                            let _ = a2.Reason(&mut reason);
                            let _ = a2.ExitCode(&mut exit_code);
                        }
                        if let Some(r) = gone_reason(kind, reason) {
                            crash_reports.crashed(&crash_label, r, i64::from(exit_code));
                        }
                        Ok(())
                    })),
                    &mut token,
                )?;
                let mut token = 0_i64;
                core.add_NavigationCompleted(
                    &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let mut ok = BOOL::default();
                        args.IsSuccess(&mut ok)?;
                        if !ok.as_bool() {
                            let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                            let _ = args.WebErrorStatus(&mut status);
                            // Operation canceled: a new navigation replaced it.
                            if status != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED {
                                reports.load_failed(
                                    &label,
                                    -i64::from(status.0),
                                    &format!("WebView2 web error status {}", status.0),
                                    crate::ads::ADVIEW_URL,
                                );
                            }
                        }
                        Ok(())
                    })),
                    &mut token,
                )?;
                Ok(())
            })()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        assert_eq!(
            request_shape(crate::ads::ADVIEW_URL, true),
            RequestShape::Document
        );
        assert_eq!(
            request_shape(crate::ads::ADVIEW_URL, false),
            RequestShape::Other
        );
        assert_eq!(
            request_shape(
                "https://content.overwolf.com/libs/ads/latest/owads.min.js?uid=u&phase=1&window=index",
                false
            ),
            RequestShape::AdLibrary
        );
        assert_eq!(
            request_shape("https://cdn.example/x.js", false),
            RequestShape::Other
        );
    }
}
