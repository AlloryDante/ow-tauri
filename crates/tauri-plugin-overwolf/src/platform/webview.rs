//! Platform webview operations the ads and consent services need and Tauri
//! does not offer: muting a guest, a transparent guest background on macOS,
//! raising a guest above its sibling webviews, letting clicks pass through a
//! guest, the guest's first navigation with `Referer` and `Origin` (D.8.3), and on
//! Windows the request shaping handler, guest crash and load-failure reports
//! (D.7, D.8).
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

/// Chromium's `net::Error` code and name for a WebView2
/// `COREWEBVIEW2_WEB_ERROR_STATUS` (Electron reports these in
/// `did-fail-load`, D.7). Statuses without a close Chromium equivalent
/// are `ERR_FAILED`.
///
/// The names follow `net/base/net_error_list.h`.
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "WebView2 statuses exist on Windows only")
)]
pub(crate) fn webview2_net_error(status: i32) -> (i64, &'static str) {
    match status {
        1 => (-200, "ERR_CERT_COMMON_NAME_INVALID"),
        2 => (-201, "ERR_CERT_DATE_INVALID"),
        3 => (-117, "ERR_BAD_SSL_CLIENT_AUTH_CERT"),
        4 => (-203, "ERR_CERT_REVOKED"),
        5 => (-207, "ERR_CERT_INVALID"),
        6 => (-109, "ERR_ADDRESS_UNREACHABLE"),
        7 => (-7, "ERR_TIMED_OUT"),
        8 => (-320, "ERR_INVALID_RESPONSE"),
        9 => (-103, "ERR_CONNECTION_ABORTED"),
        10 => (-101, "ERR_CONNECTION_RESET"),
        11 => (-106, "ERR_INTERNET_DISCONNECTED"),
        12 => (-104, "ERR_CONNECTION_FAILED"),
        13 => (-105, "ERR_NAME_NOT_RESOLVED"),
        14 => (-3, "ERR_ABORTED"),
        15 => (-310, "ERR_TOO_MANY_REDIRECTS"),
        17 => (-338, "ERR_INVALID_AUTH_CREDENTIALS"),
        18 => (-127, "ERR_PROXY_AUTH_REQUESTED"),
        _ => (-2, "ERR_FAILED"),
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

/// The rest of a transparent guest background that the webview builder's
/// `transparent` flag does not set (B.3.4): on macOS the `WKWebView`'s own
/// `drawsBackground` (`NO`, the private key-value key Tauri and wry use for
/// transparent webviews, set only when the view answers to it) and the
/// public `underPageBackgroundColor` (clear, macOS 12+), which shows when a
/// page is scrolled past its edge. Nothing on Windows and Linux, where the
/// builder flag makes the whole background transparent.
///
/// `done` runs on the webview's thread with whether the background was
/// cleared (`true` on Windows and Linux); it does not run when the webview
/// is gone. Errors only when the webview is gone.
pub(crate) fn clear_background<R: Runtime>(
    webview: &Webview<R>,
    done: impl FnOnce(bool) + Send + 'static,
) -> tauri::Result<()> {
    webview.with_webview(move |pw| {
        #[cfg(target_os = "macos")]
        let cleared = macos::clear_background(pw.inner());
        #[cfg(not(target_os = "macos"))]
        let cleared = {
            let _ = pw;
            true
        };
        done(cleared);
    })
}

/// Raises the guest to the top of its window's child webviews (B.3.4: the
/// performance guest stays above every other guest). Nothing is detached
/// or re-created, so the page keeps running undisturbed:
///
/// - Windows: `SetWindowPos(HWND_TOP)` of the guest's container window
///   (the controller's `ParentWindow`), without moving, sizing or
///   activating it.
/// - macOS: `-[NSView addSubview:positioned:NSWindowAbove relativeTo:nil]`
///   on the view's superview, which reorders an existing subview (and its
///   layer) without removing it from the window.
/// - Linux: `gdk_window_raise` of the guest widget's own `GdkWindow`, when
///   it has one.
///
/// `done` runs on the webview's thread with whether the guest is now the
/// top child (`None` when the platform cannot tell). Errors only when the
/// webview is gone.
pub(crate) fn raise_to_top<R: Runtime>(
    webview: &Webview<R>,
    done: impl FnOnce(Option<bool>) + Send + 'static,
) -> tauri::Result<()> {
    webview.with_webview(move |pw| {
        #[cfg(target_os = "macos")]
        let top = macos::raise_to_top(pw.inner());
        #[cfg(windows)]
        let top = windows_impl::raise_to_top(&pw.controller());
        #[cfg(target_os = "linux")]
        let top = linux::raise_to_top(&pw.inner());
        #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
        let top = {
            let _ = pw;
            None
        };
        done(top);
    })
}

/// Lets mouse input pass through the guest to the webviews under it (`on`),
/// or makes the guest take its input again (B.3.4: a performance guest
/// until its first `performance_ad_loaded`). The page keeps rendering and
/// running either way:
///
/// - Windows: an empty window region on the guest's container window
///   (`SetWindowRgn`), which hit-testing skips across processes; `NULL`
///   restores it. Clipping the guest's paint changes nothing visible while
///   it loads, because it is transparent.
/// - macOS: the view's `hitTest:` answers `nil` while a per-view flag is
///   set, through a subclass made for the view's class at run time (as
///   key-value observing does), so `AppKit` hit-tests the sibling views.
/// - Linux: an empty input shape on the guest widget's own `GdkWindow`,
///   when it has one.
///
/// `done` runs on the webview's thread with whether the platform applied
/// the change. Errors only when the webview is gone.
pub(crate) fn set_input_passthrough<R: Runtime>(
    webview: &Webview<R>,
    on: bool,
    done: impl FnOnce(bool) + Send + 'static,
) -> tauri::Result<()> {
    webview.with_webview(move |pw| {
        #[cfg(target_os = "macos")]
        let applied = macos::set_input_passthrough(pw.inner(), on);
        #[cfg(windows)]
        let applied = windows_impl::set_input_passthrough(&pw.controller(), on);
        #[cfg(target_os = "linux")]
        let applied = linux::set_input_passthrough(&pw.inner(), on);
        #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
        let applied = {
            let _ = (pw, on);
            false
        };
        done(applied);
    })
}

/// The guest's first navigation to `url` with the document headers of
/// D.8.3: `WKWebView.load(URLRequest)` on macOS,
/// `webkit_web_view_load_request` on Linux and
/// `NavigateWithWebResourceRequest` on Windows, so the page's
/// `document.referrer` is the `Referer` as in ow-electron (on Windows a plain
/// navigation whose request handler set the headers left it empty: Windows
/// lab). Returns `false` when the platform call could not be posted (the
/// caller navigates plainly).
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
    #[cfg(windows)]
    if let Some(s) = shaping {
        let target = url.clone();
        let headers = document_header_fields(Some(s)).join("\r\n");
        let fallback = webview.clone();
        return webview
            .with_webview(move |pw| {
                if windows_impl::navigate_with_headers(&pw.controller(), target.as_str(), &headers)
                    .is_err()
                {
                    let _ = fallback.navigate(target);
                }
            })
            .is_ok();
    }
    let _ = shaping;
    webview.navigate(url.clone()).is_ok()
}

/// The header fields the plugin itself puts on the ad document request
/// (D.8.3), as `Name: value` lines in order; the platform adds the rest.
pub(crate) fn document_header_fields(shaping: Option<&Shaping>) -> Vec<String> {
    shaping.map_or_else(Vec::new, |s| {
        vec![
            format!("Referer: {}", s.referer),
            format!("Origin: {}", s.origin),
        ]
    })
}

/// Platform reports from a webview that Tauri does not deliver.
pub(crate) trait GuestReports: Send + Sync + 'static {
    /// The webview's render process ended (`reason` is Electron's spelling).
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

/// Which kind of webview the platform hooks watch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookTarget {
    /// An ad guest or consent window: an unresponsive render process counts
    /// as gone (it is recovered, D.7), and a load failure reports the ad
    /// document URL.
    Guest,
    /// `ow-main` or a `bw-*` / `bwr-*` webview: only a render or browser
    /// process that exited counts (Electron reports an unresponsive page
    /// separately), and a load failure reports the document URL.
    App,
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
    install_hooks(webview, HookTarget::Guest, shaping, reports)
}

/// The crash and load-failure hooks of `ow-main` and the `bw-*` / `bwr-*`
/// webviews (A.6 crash signals, A.3 `did-fail-load` and
/// `render-process-gone`): Windows `ProcessFailed` and
/// `NavigationCompleted`, Linux `web-process-terminated` and `load-failed`.
/// macOS has no plugin-level hook (A.5).
pub(crate) fn install_app_hooks<R: Runtime>(
    webview: &Webview<R>,
    reports: Arc<dyn GuestReports>,
) -> tauri::Result<()> {
    install_hooks(webview, HookTarget::App, None, reports)
}

fn install_hooks<R: Runtime>(
    webview: &Webview<R>,
    target: HookTarget,
    shaping: Option<Shaping>,
    reports: Arc<dyn GuestReports>,
) -> tauri::Result<()> {
    let label = webview.label().to_owned();
    webview.with_webview(move |pw| {
        #[cfg(windows)]
        {
            windows_impl::install(&pw.controller(), target, shaping, label, reports);
        }
        #[cfg(target_os = "linux")]
        {
            let _ = (shaping, target);
            linux::install(&pw.inner(), label, reports);
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (pw, target, shaping, label, reports);
        }
    })
}

/// Reads every cookie of the default `WKWebsiteDataStore` (the macOS ads
/// data store, D.8.1) and calls `done` with them. Call it on the main
/// thread; `done` runs there later (not at all when `WebKit` is missing).
#[cfg(target_os = "macos")]
pub(crate) fn default_store_cookies(
    done: impl FnOnce(Vec<crate::host::cookies::StoredCookie>) + Send + 'static,
) {
    macos::default_store_cookies(done);
}

/// Lab trace: every cookie of the default `WKWebsiteDataStore` with its
/// attributes, as the ow-electron harness records them. Call it on the
/// main thread; `done` runs there later.
#[cfg(all(target_os = "macos", feature = "lab"))]
pub(crate) fn default_store_cookie_details(
    done: impl FnOnce(Vec<serde_json::Value>) + Send + 'static,
) {
    macos::default_store_cookie_details(done);
}

/// The height of the window's area above the part web content may use, in
/// logical pixels: on macOS, the title bar when the window's content view
/// extends under it (as on macOS 26), where `WKWebView` insets the page by
/// the overlap. 0 elsewhere. An ad guest is placed relative to the
/// embedder's page, which starts below this line (B.3.4).
pub(crate) fn content_inset_top<R: Runtime>(window: &tauri::Window<R>) -> f64 {
    #[cfg(target_os = "macos")]
    {
        let Ok(ptr) = window.ns_window() else {
            return 0.0;
        };
        let address = ptr as usize;
        if objc2::MainThreadMarker::new().is_some() {
            return macos::content_inset_top(address);
        }
        let (tx, rx) = std::sync::mpsc::channel();
        if window
            .run_on_main_thread(move || {
                let _ = tx.send(macos::content_inset_top(address));
            })
            .is_err()
        {
            return 0.0;
        }
        rx.recv_timeout(std::time::Duration::from_millis(500))
            .unwrap_or(0.0)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        0.0
    }
}

/// Shows `window` without making it key and without activating the app
/// (`BrowserWindow.showInactive()`, and every lab window): on macOS
/// `-[NSWindow orderFrontRegardless]` on the main thread, as ow-electron
/// shows an inactive window. `Window::show` would call
/// `makeKeyAndOrderFront:`, which activates the app and takes the keyboard
/// from the app the user is in. On Windows `ShowWindow(SW_SHOWNOACTIVATE)`,
/// as Electron's `showInactive()` does there, issued from a helper thread:
/// the command runs on the main thread inside Tauri's invoke handler, which
/// holds Tauri's plugin lock, and a `ShowWindow` there synchronously sends
/// the window its show and size messages, whose window event re-enters
/// Tauri's event loop callback and waits for that same lock forever
/// (Windows lab: the stack of the hung app). `Window::show` from the main
/// thread is the same call. From the helper thread the show waits until the
/// main thread pumps its messages, after the command returned; a sent
/// message is handled before any posted one, so the window is shown before
/// the app's next call (its page load) reaches the main thread. Elsewhere
/// `Window::show`.
///
/// On macOS, on the main thread the window is ordered front before this
/// returns, so a visibility read right after sees it shown; elsewhere the
/// show is queued to the main thread.
pub(crate) fn show_inactive<R: Runtime>(window: &tauri::Window<R>) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let address = window.ns_window()? as usize;
        now_or_on_main(
            objc2::MainThreadMarker::new().is_some(),
            move || macos::order_front_regardless(address),
            |show| window.run_on_main_thread(show),
        )
    }
    #[cfg(windows)]
    {
        // The mock runtime has no native window: show it the Tauri way.
        match window.hwnd() {
            Ok(hwnd) => windows_impl::show_no_activate_detached(hwnd.0 as isize)
                .map(drop)
                .map_err(tauri::Error::from),
            Err(_) => window.show(),
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        window.show()
    }
}

/// Runs `run` at once when on the main thread (`on_main`), else hands it to
/// `queue` (which runs it on the main thread later).
#[cfg_attr(
    not(any(target_os = "macos", test)),
    expect(dead_code, reason = "macOS only")
)]
fn now_or_on_main<F: FnOnce() + Send + 'static>(
    on_main: bool,
    run: F,
    queue: impl FnOnce(F) -> tauri::Result<()>,
) -> tauri::Result<()> {
    if on_main {
        run();
        Ok(())
    } else {
        queue(run)
    }
}

/// Runs `create` (which builds a webview) without letting the webview
/// library activate the app. On macOS, wry calls `-[NSApplication activate]`
/// for every webview it creates ("make sure the window is always on top"),
/// which takes the keyboard from the app the user is in (a game) each time
/// an ad guest, a hidden window or a consent page is created. ow-electron
/// creates `<webview>` guests and hidden windows without activating the app.
/// While `create` runs, `activate` and `activateIgnoringOtherApps:` of
/// `NSApplication` do nothing (lab trace: `activation-suppressed` in
/// `wc-events.jsonl`); other platforms run `create` as is.
pub(crate) fn without_app_activation<T>(create: impl FnOnce() -> T) -> T {
    #[cfg(target_os = "macos")]
    {
        macos::install_activation_guard();
        let _guard = macos::ActivationGuard::new();
        create()
    }
    #[cfg(not(target_os = "macos"))]
    {
        create()
    }
}

/// A stage of a window's minimize, as the OS reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(any(target_os = "macos", feature = "test-util")),
    expect(
        dead_code,
        reason = "only the macOS observer and the test hooks report a stage"
    )
)]
pub(crate) enum MinimizeStage {
    /// The minimize started: on macOS the window is reported neither
    /// visible nor minimized while it animates into the Dock.
    Will,
    /// The window is minimized.
    Did,
}

/// Calls `on` with the window's `NSWindow*` address on each stage of a
/// window minimize in this process (macOS
/// `NSWindowWillMiniaturizeNotification` and
/// `NSWindowDidMiniaturizeNotification`, on the main thread). Elsewhere
/// does nothing: Windows and Linux report a minimized window at once.
pub(crate) fn observe_minimize(on: impl Fn(usize, MinimizeStage) + Send + Sync + 'static) {
    #[cfg(target_os = "macos")]
    macos::observe_minimize(Box::new(on));
    #[cfg(not(target_os = "macos"))]
    drop(on);
}

/// The displays as the OS names them (`NSScreen.localizedName` with each
/// screen's frame, top-left origin, on macOS); empty elsewhere, where
/// Tauri's monitor names are already the OS names.
pub(crate) fn screen_names<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Vec<crate::screen::OsScreenName> {
    #[cfg(target_os = "macos")]
    {
        if objc2::MainThreadMarker::new().is_some() {
            return macos::screen_names();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        if app
            .run_on_main_thread(move || {
                let _ = tx.send(macos::screen_names());
            })
            .is_err()
        {
            return Vec::new();
        }
        rx.recv_timeout(std::time::Duration::from_millis(500))
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Vec::new()
    }
}

/// Where the page of a webview at `webview_y` (logical, in its window)
/// starts: the platform insets a page whose webview reaches above the
/// window's content inset ([`content_inset_top`]) down to that line.
///
pub(crate) fn page_origin_y(webview_y: f64, inset_top: f64) -> f64 {
    webview_y.max(inset_top)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, Once, OnceLock};

    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
    use objc2::{class, msg_send, sel};
    use objc2_foundation::{NSArray, NSHTTPCookie, NSMutableURLRequest, NSPoint, NSString, NSURL};

    use crate::host::cookies::StoredCookie;

    /// How many webview creations run under [`super::without_app_activation`].
    static SUPPRESS_ACTIVATION: AtomicUsize = AtomicUsize::new(0);

    /// The `NSApplication` implementations [`install_activation_guard`]
    /// replaced (`IMP` addresses), set once.
    static ACTIVATE_ORIGINAL: OnceLock<usize> = OnceLock::new();
    static ACTIVATE_IGNORING_ORIGINAL: OnceLock<usize> = OnceLock::new();

    /// `-[NSApplication activate]` (macOS 14+).
    type Activate = unsafe extern "C-unwind" fn(&AnyObject, Sel);
    /// `-[NSApplication activateIgnoringOtherApps:]`.
    type ActivateIgnoring = unsafe extern "C-unwind" fn(&AnyObject, Sel, Bool);

    /// Holds activation off while it lives.
    pub(super) struct ActivationGuard;

    impl ActivationGuard {
        pub(super) fn new() -> Self {
            SUPPRESS_ACTIVATION.fetch_add(1, Ordering::SeqCst);
            Self
        }
    }

    impl Drop for ActivationGuard {
        fn drop(&mut self) {
            SUPPRESS_ACTIVATION.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn suppressed(selector: &str) -> bool {
        if SUPPRESS_ACTIVATION.load(Ordering::SeqCst) == 0 {
            return false;
        }
        crate::lab::record(
            "wc-events.jsonl",
            || serde_json::json!({ "kind": "activation-suppressed", "selector": selector }),
        );
        true
    }

    extern "C-unwind" fn guarded_activate(this: &AnyObject, cmd: Sel) {
        if suppressed("activate") {
            return;
        }
        if let Some(imp) = ACTIVATE_ORIGINAL.get() {
            // SAFETY: the original `activate` of NSApplication, stored by
            // `install_activation_guard`, called with its own arguments.
            unsafe { std::mem::transmute::<usize, Activate>(*imp)(this, cmd) };
        }
    }

    extern "C-unwind" fn guarded_activate_ignoring(this: &AnyObject, cmd: Sel, flag: Bool) {
        if suppressed("activateIgnoringOtherApps:") {
            return;
        }
        if let Some(imp) = ACTIVATE_IGNORING_ORIGINAL.get() {
            // SAFETY: as in `guarded_activate`.
            unsafe { std::mem::transmute::<usize, ActivateIgnoring>(*imp)(this, cmd, flag) };
        }
    }

    /// Replaces `activate` and `activateIgnoringOtherApps:` of
    /// `NSApplication`, once, with versions that do nothing while an
    /// [`ActivationGuard`] lives and otherwise call the original. Every
    /// other caller (the app itself, tao at launch) is unaffected.
    pub(super) fn install_activation_guard() {
        static INSTALL: Once = Once::new();
        INSTALL.call_once(|| {
            let class = class!(NSApplication);
            let replace = |selector: Sel, slot: &OnceLock<usize>, imp: objc2::runtime::Imp| {
                let Some(method) = class.instance_method(selector) else {
                    return;
                };
                // The original is stored before the replacement can run.
                let _ = slot.set(method.implementation() as usize);
                // SAFETY: a method of a registered class; its encoding
                // outlives the call.
                let types = unsafe { objc2::ffi::method_getTypeEncoding(method) };
                // SAFETY: `imp` has the selector's signature, as `types`
                // says; the runtime keeps it for the class's lifetime.
                unsafe {
                    objc2::ffi::class_replaceMethod(
                        std::ptr::from_ref(class).cast_mut(),
                        selector,
                        imp,
                        types,
                    );
                }
            };
            let activate: Activate = guarded_activate;
            let ignoring: ActivateIgnoring = guarded_activate_ignoring;
            // SAFETY: function pointers cast to the runtime's `Imp` type.
            let (activate, ignoring) = unsafe {
                (
                    std::mem::transmute::<Activate, objc2::runtime::Imp>(activate),
                    std::mem::transmute::<ActivateIgnoring, objc2::runtime::Imp>(ignoring),
                )
            };
            replace(sel!(activate), &ACTIVATE_ORIGINAL, activate);
            replace(
                sel!(activateIgnoringOtherApps:),
                &ACTIVATE_IGNORING_ORIGINAL,
                ignoring,
            );
        });
    }

    /// Every [`super::observe_minimize`] callback (one per host).
    type MinimizeCallback = Box<dyn Fn(usize, super::MinimizeStage) + Send + Sync>;
    static MINIMIZE_CALLBACKS: Mutex<Vec<MinimizeCallback>> = Mutex::new(Vec::new());

    /// Adds `on` and, once per process, observes both minimize
    /// notifications of every window.
    pub(super) fn observe_minimize(on: MinimizeCallback) {
        static INSTALL: Once = Once::new();
        MINIMIZE_CALLBACKS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(on);
        INSTALL.call_once(|| {
            observe(
                "NSWindowWillMiniaturizeNotification",
                super::MinimizeStage::Will,
            );
            observe(
                "NSWindowDidMiniaturizeNotification",
                super::MinimizeStage::Did,
            );
        });
    }

    /// `-[NSNotificationCenter addObserverForName:object:queue:usingBlock:]`
    /// for `name` from any object, delivered on the posting (main) thread;
    /// the observer lives as long as the process.
    fn observe(name: &str, stage: super::MinimizeStage) {
        let name = NSString::from_str(name);
        let block = block2::RcBlock::new(move |note: NonNull<AnyObject>| {
            // SAFETY: AppKit passes a valid NSNotification for the call.
            let note = unsafe { note.as_ref() };
            // SAFETY: `-[NSNotification object]`, the NSWindow posting it.
            let window: *const AnyObject = unsafe { msg_send![note, object] };
            let callbacks = MINIMIZE_CALLBACKS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for on in callbacks.iter() {
                on(window as usize, stage);
            }
        });
        // SAFETY: a class method of Foundation's public API.
        let center: Option<Retained<AnyObject>> =
            unsafe { msg_send![class!(NSNotificationCenter), defaultCenter] };
        let Some(center) = center else { return };
        // SAFETY: the documented signature; a nil object and queue observe
        // every sender on the posting thread, and the center copies the
        // block.
        let token: Option<Retained<AnyObject>> = unsafe {
            msg_send![
                &*center,
                addObserverForName: &*name,
                object: std::ptr::null::<AnyObject>(),
                queue: std::ptr::null::<AnyObject>(),
                usingBlock: &*block
            ]
        };
        // The observer stays registered for the process's lifetime.
        std::mem::forget(token);
    }

    /// `-[NSWindow orderFrontRegardless]`: on screen, front of its level,
    /// neither key nor activating the app.
    pub(super) fn order_front_regardless(address: usize) {
        if address == 0 {
            return;
        }
        // SAFETY: `address` is the live `NSWindow*` Tauri handed out for this
        // window, used on the main thread.
        let window: &AnyObject = unsafe { &*(address as *const AnyObject) };
        // SAFETY: a public NSWindow method without arguments.
        let () = unsafe { msg_send![window, orderFrontRegardless] };
    }

    /// `NSWindow.contentView.frame` top minus `contentLayoutRect` top, in
    /// window coordinates (points).
    pub(super) fn content_inset_top(address: usize) -> f64 {
        use objc2_foundation::NSRect;
        if address == 0 {
            return 0.0;
        }
        // SAFETY: `address` is the live `NSWindow*` Tauri handed out for this
        // window, used on the main thread.
        let window: &AnyObject = unsafe { &*(address as *const AnyObject) };
        // SAFETY: public NSWindow properties (macOS 10.10+).
        let layout: NSRect = unsafe { msg_send![window, contentLayoutRect] };
        // SAFETY: as above.
        let view: Option<Retained<AnyObject>> = unsafe { msg_send![window, contentView] };
        let Some(view) = view else { return 0.0 };
        // SAFETY: `frame` of an NSView, in its superview's (the window's)
        // coordinates.
        let frame: NSRect = unsafe { msg_send![&*view, frame] };
        let top = frame.origin.y + frame.size.height;
        let layout_top = layout.origin.y + layout.size.height;
        (top - layout_top).max(0.0)
    }

    /// `NSScreen.screens` with `localizedName` (macOS 10.15+) and `frame`
    /// flipped to a top-left origin against the primary (first) screen.
    pub(super) fn screen_names() -> Vec<crate::screen::OsScreenName> {
        use objc2_foundation::NSRect;
        // SAFETY: a public AppKit class method, on the main thread (the
        // caller's contract).
        let screens: Option<Retained<NSArray<AnyObject>>> =
            unsafe { msg_send![class!(NSScreen), screens] };
        let Some(screens) = screens else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut primary_height = None;
        for screen in &screens {
            // SAFETY: public NSScreen properties.
            let frame: NSRect = unsafe { msg_send![&*screen, frame] };
            let responds: bool =
                // SAFETY: NSObject's respondsToSelector:.
                unsafe { msg_send![&*screen, respondsToSelector: sel!(localizedName)] };
            let name: Option<Retained<NSString>> = if responds {
                // SAFETY: checked above (macOS 10.15+).
                unsafe { msg_send![&*screen, localizedName] }
            } else {
                None
            };
            let top = *primary_height.get_or_insert(frame.size.height);
            out.push(crate::screen::OsScreenName {
                x: frame.origin.x,
                y: top - (frame.origin.y + frame.size.height),
                width: frame.size.width,
                height: frame.size.height,
                name: name.map(|n| n.to_string()).unwrap_or_default(),
            });
        }
        out
    }

    /// `-[WKHTTPCookieStore getAllCookies:]` on the default data store.
    pub(super) fn default_store_cookies(done: impl FnOnce(Vec<StoredCookie>) + Send + 'static) {
        // SAFETY: class methods and properties of WebKit's public API, on
        // the main thread (the caller's contract); `wry` links WebKit.
        let store: Option<Retained<AnyObject>> =
            unsafe { msg_send![class!(WKWebsiteDataStore), defaultDataStore] };
        let Some(store) = store else { return };
        // SAFETY: as above; `httpCookieStore` is non-null on macOS 10.13+.
        let cookie_store: Option<Retained<AnyObject>> =
            unsafe { msg_send![&*store, httpCookieStore] };
        let Some(cookie_store) = cookie_store else {
            return;
        };
        let done = Mutex::new(Some(done));
        let block = block2::RcBlock::new(move |cookies: NonNull<NSArray<NSHTTPCookie>>| {
            // SAFETY: WebKit passes a valid array for the duration of the call.
            let cookies = unsafe { cookies.as_ref() };
            let list = cookies.iter().map(|c| stored(&c)).collect();
            if let Some(done) = done
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                done(list);
            }
        });
        // SAFETY: `getAllCookies:` takes a block `void (^)(NSArray<NSHTTPCookie *> *)`,
        // which WebKit copies.
        let () = unsafe { msg_send![&*cookie_store, getAllCookies: &*block] };
    }

    /// Every cookie of the default store with its attributes (lab trace).
    #[cfg(feature = "lab")]
    pub(super) fn default_store_cookie_details(
        done: impl FnOnce(Vec<serde_json::Value>) + Send + 'static,
    ) {
        // SAFETY: as in `default_store_cookies`.
        let store: Option<Retained<AnyObject>> =
            unsafe { msg_send![class!(WKWebsiteDataStore), defaultDataStore] };
        let Some(store) = store else { return };
        // SAFETY: as in `default_store_cookies`.
        let cookie_store: Option<Retained<AnyObject>> =
            unsafe { msg_send![&*store, httpCookieStore] };
        let Some(cookie_store) = cookie_store else {
            return;
        };
        let done = Mutex::new(Some(done));
        let block = block2::RcBlock::new(move |cookies: NonNull<NSArray<NSHTTPCookie>>| {
            // SAFETY: WebKit passes a valid array for the duration of the call.
            let cookies = unsafe { cookies.as_ref() };
            let list = cookies
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "name": c.name().to_string(),
                        "value": c.value().to_string(),
                        "domain": c.domain().to_string(),
                        "path": c.path().to_string(),
                        "secure": c.isSecure(),
                        "httpOnly": c.isHTTPOnly(),
                        "session": c.isSessionOnly(),
                        "sameSite": c.sameSitePolicy().map(|p| p.to_string()),
                        "expirationDate": c
                            .expiresDate()
                            .filter(|_| !c.isSessionOnly())
                            .map(|d| d.timeIntervalSince1970()),
                    })
                })
                .collect();
            if let Some(done) = done
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                done(list);
            }
        });
        // SAFETY: as in `default_store_cookies`.
        let () = unsafe { msg_send![&*cookie_store, getAllCookies: &*block] };
    }

    fn stored(cookie: &NSHTTPCookie) -> StoredCookie {
        StoredCookie {
            name: cookie.name().to_string(),
            value: cookie.value().to_string(),
            domain: cookie.domain().to_string(),
            path: cookie.path().to_string(),
            secure: cookie.isSecure(),
            expires: cookie
                .expiresDate()
                .filter(|_| !cookie.isSessionOnly())
                .map(|d| d.timeIntervalSince1970()),
        }
    }

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

    /// `drawsBackground = NO` (key-value coding, when the view has the
    /// private `_setDrawsBackground:` setter the key resolves to, so the
    /// call cannot raise) and a clear `underPageBackgroundColor` (macOS
    /// 12+). Returns whether `drawsBackground` was set.
    pub(super) fn clear_background(wk_webview: *mut c_void) -> bool {
        if wk_webview.is_null() {
            return false;
        }
        // SAFETY: Tauri hands a live `WKWebView*` on the main thread.
        let obj: &AnyObject = unsafe { &*wk_webview.cast::<AnyObject>() };
        // SAFETY: `respondsToSelector:` exists on every NSObject.
        let has_setter: bool =
            unsafe { msg_send![obj, respondsToSelector: sel!(_setDrawsBackground:)] };
        if has_setter {
            // SAFETY: a public NSNumber class method.
            let no: Option<Retained<AnyObject>> =
                unsafe { msg_send![class!(NSNumber), numberWithBool: false] };
            if let Some(no) = no {
                let key = NSString::from_str("drawsBackground");
                // SAFETY: key-value coding of a key the view answers to
                // (checked above), with an NSNumber value.
                let () = unsafe { msg_send![obj, setValue: &*no, forKey: &*key] };
            }
        }
        // SAFETY: `respondsToSelector:` exists on every NSObject.
        let has_under_page: bool =
            unsafe { msg_send![obj, respondsToSelector: sel!(setUnderPageBackgroundColor:)] };
        if has_under_page {
            // SAFETY: a public NSColor class method (AppKit, linked by wry).
            let clear: Option<Retained<AnyObject>> =
                unsafe { msg_send![class!(NSColor), clearColor] };
            if let Some(clear) = clear {
                // SAFETY: public WKWebView property (macOS 12+, checked
                // above) taking an NSColor.
                let () = unsafe { msg_send![obj, setUnderPageBackgroundColor: &*clear] };
            }
        }
        has_setter
    }

    /// `NSWindowAbove` (`NSWindowOrderingMode`).
    const NS_WINDOW_ABOVE: isize = 1;

    /// Moves the view to the end of its superview's subviews (the top),
    /// unless it is there already. Returns whether it is the top subview.
    pub(super) fn raise_to_top(view: *mut c_void) -> Option<bool> {
        if view.is_null() {
            return None;
        }
        // SAFETY: Tauri hands a live `WKWebView*` on the main thread.
        let obj: &AnyObject = unsafe { &*view.cast::<AnyObject>() };
        // SAFETY: public NSView property.
        let superview: Option<Retained<AnyObject>> = unsafe { msg_send![obj, superview] };
        let superview = superview?;
        let is_top = || {
            // SAFETY: public NSView property; `lastObject` of an NSArray.
            let subviews: Option<Retained<NSArray<AnyObject>>> =
                unsafe { msg_send![&*superview, subviews] };
            subviews
                .and_then(|s| s.lastObject())
                .is_some_and(|last| std::ptr::eq(Retained::as_ptr(&last), obj))
        };
        if !is_top() {
            let nil: *const AnyObject = std::ptr::null();
            // SAFETY: public NSView method; `obj` is already a subview, so
            // AppKit only reorders it (no `viewWillMoveToWindow:`).
            unsafe {
                let () = msg_send![&*superview, addSubview: obj, positioned: NS_WINDOW_ABOVE, relativeTo: nil];
            }
        }
        Some(is_top())
    }

    /// The pass-through flag's associated-object key (its address).
    static PASSTHROUGH_KEY: u8 = 0;

    /// The classes whose `hitTest:` is [`passthrough_hit_test`], each with
    /// the implementation it had before (class and `IMP` addresses).
    static PASSTHROUGH_ORIGINALS: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());

    /// The signature of `-[NSView hitTest:]`.
    type HitTest = unsafe extern "C-unwind" fn(&AnyObject, Sel, NSPoint) -> *mut AnyObject;

    /// The view's own class: its class without the subclasses key-value
    /// observing puts in (`NSKVONotifying_*`), so every web view of one
    /// class shares one installation however it is observed.
    fn own_class(class: &'static AnyClass) -> &'static AnyClass {
        let mut class = class;
        while class.name().to_bytes().starts_with(b"NSKVONotifying_") {
            match class.superclass() {
                Some(superclass) => class = superclass,
                None => break,
            }
        }
        class
    }

    /// The `hitTest:` that [`install_passthrough`] replaced for `class` or
    /// its nearest ancestor that has one.
    fn original_hit_test(class: &'static AnyClass) -> Option<HitTest> {
        let originals = PASSTHROUGH_ORIGINALS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut class = Some(class);
        while let Some(c) = class {
            let key = std::ptr::from_ref(c) as usize;
            if let Some(&(_, imp)) = originals.iter().find(|(k, _)| *k == key) {
                // SAFETY: stored by `install_passthrough` from the
                // `hitTest:` method of that class: a valid `HitTest`.
                return Some(unsafe { std::mem::transmute::<usize, HitTest>(imp) });
            }
            class = c.superclass();
        }
        None
    }

    /// `hitTest:` of a web view class with pass-through support: `nil`
    /// while the view's flag is set (the event goes to the views under
    /// it), otherwise the class's original `hitTest:`.
    extern "C-unwind" fn passthrough_hit_test(
        this: &AnyObject,
        cmd: Sel,
        point: NSPoint,
    ) -> *mut AnyObject {
        // SAFETY: reads an associated object of a live object.
        let flag = unsafe {
            objc2::ffi::objc_getAssociatedObject(this, (&raw const PASSTHROUGH_KEY).cast())
        };
        if !flag.is_null() {
            return std::ptr::null_mut();
        }
        match original_hit_test(this.class()) {
            // SAFETY: the original implementation, with its own arguments.
            Some(original) => unsafe { original(this, cmd, point) },
            None => std::ptr::null_mut(),
        }
    }

    /// Gives `class` [`passthrough_hit_test`], once, keeping the `hitTest:`
    /// it had (its own or inherited). The view's class and object stay as
    /// they are: changing a web view's class at run time breaks `AppKit`'s
    /// key-value observing and dynamic properties of the view when it
    /// leaves its window. Views without a set flag behave as before.
    fn install_passthrough(class: &'static AnyClass) -> bool {
        let mut originals = PASSTHROUGH_ORIGINALS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = std::ptr::from_ref(class) as usize;
        if originals.iter().any(|(k, _)| *k == key) {
            return true;
        }
        let Some(method) = class.instance_method(sel!(hitTest:)) else {
            return false;
        };
        let original = method.implementation();
        // SAFETY: a method of a registered class; its encoding outlives
        // the call.
        let types = unsafe { objc2::ffi::method_getTypeEncoding(method) };
        let replacement: HitTest = passthrough_hit_test;
        // SAFETY: `HitTest` is `hitTest:`'s signature, as `types` says;
        // the runtime stores the implementation for the class's lifetime.
        unsafe {
            objc2::ffi::class_replaceMethod(
                std::ptr::from_ref(class).cast_mut(),
                sel!(hitTest:),
                std::mem::transmute::<HitTest, objc2::runtime::Imp>(replacement),
                types,
            );
        }
        originals.push((key, original as usize));
        true
    }

    /// Sets or clears the view's pass-through flag, giving its class
    /// pass-through support first. Returns whether the flag is now as
    /// asked.
    pub(super) fn set_input_passthrough(view: *mut c_void, on: bool) -> bool {
        if view.is_null() {
            return false;
        }
        // SAFETY: Tauri hands a live `WKWebView*` on the main thread.
        let obj: &AnyObject = unsafe { &*view.cast::<AnyObject>() };
        if on && !install_passthrough(own_class(obj.class())) {
            return false;
        }
        let value: *mut AnyObject = if on {
            view.cast()
        } else {
            std::ptr::null_mut()
        };
        // SAFETY: an unretained (assign) associated value on a live object:
        // the object itself, so it never dangles.
        unsafe {
            objc2::ffi::objc_setAssociatedObject(
                view.cast(),
                (&raw const PASSTHROUGH_KEY).cast(),
                value,
                objc2::ffi::OBJC_ASSOCIATION_ASSIGN,
            );
        }
        true
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

    #[cfg(test)]
    mod tests {
        use objc2::{class, sel};

        use super::*;

        #[test]
        fn activation_is_suppressed_only_while_a_guard_lives() {
            install_activation_guard();
            install_activation_guard(); // once
            let class = class!(NSApplication);
            let imp = |selector| {
                class
                    .instance_method(selector)
                    .map(|m| m.implementation() as usize)
            };
            let activate: Activate = guarded_activate;
            let ignoring: ActivateIgnoring = guarded_activate_ignoring;
            assert_eq!(
                imp(sel!(activateIgnoringOtherApps:)),
                Some(ignoring as usize)
            );
            assert_ne!(ACTIVATE_IGNORING_ORIGINAL.get(), Some(&(ignoring as usize)));
            if let Some(original) = ACTIVATE_ORIGINAL.get() {
                assert_eq!(imp(sel!(activate)), Some(activate as usize));
                assert_ne!(*original, activate as usize);
            }
            assert!(!suppressed("activate"));
            {
                let _outer = ActivationGuard::new();
                assert!(suppressed("activate"));
                {
                    let _inner = ActivationGuard::new();
                    assert!(suppressed("activate"));
                }
                assert!(suppressed("activateIgnoringOtherApps:"));
            }
            assert!(!suppressed("activate"));
            assert!(super::super::without_app_activation(|| suppressed(
                "activate"
            )));
            assert!(!suppressed("activate"));
        }
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
                let (code, name) = net_error(error);
                reports.load_failed(&label, code, name, uri);
            }
            false
        });
    }

    /// `gdk_window_raise` of the widget's own `GdkWindow`. A widget that
    /// draws into its parent's window has no stacking of its own; it is
    /// left alone (re-adding it to its container would move it in the
    /// container's layout). Returns `Some(true)` when raised.
    pub(super) fn raise_to_top(webview: &webkit2gtk::WebView) -> Option<bool> {
        use gtk::prelude::WidgetExt as _;
        if !webview.has_window() {
            return None;
        }
        let window = webview.window()?;
        window.raise();
        Some(true)
    }

    /// An empty input shape on the widget's own `GdkWindow` (`on`), or
    /// none. A widget without its own window is left alone. Returns whether
    /// the shape was set.
    pub(super) fn set_input_passthrough(webview: &webkit2gtk::WebView, on: bool) -> bool {
        use gtk::prelude::WidgetExt as _;
        if !webview.has_window() {
            return false;
        }
        if on {
            webview.input_shape_combine_region(Some(&gtk::cairo::Region::create()));
        } else {
            webview.input_shape_combine_region(None);
        }
        true
    }

    /// Chromium's `net::Error` code and name closest to a `WebKitGTK` load
    /// error (D.7); `ERR_FAILED` when none is close.
    fn net_error(error: &webkit2gtk::glib::Error) -> (i64, &'static str) {
        use webkit2gtk::gio::{IOErrorEnum, ResolverError, TlsError};
        if let Some(kind) = error.kind::<IOErrorEnum>() {
            return match kind {
                IOErrorEnum::TimedOut => (-7, "ERR_TIMED_OUT"),
                IOErrorEnum::HostNotFound => (-105, "ERR_NAME_NOT_RESOLVED"),
                IOErrorEnum::ConnectionRefused => (-102, "ERR_CONNECTION_REFUSED"),
                IOErrorEnum::HostUnreachable => (-109, "ERR_ADDRESS_UNREACHABLE"),
                IOErrorEnum::NetworkUnreachable => (-106, "ERR_INTERNET_DISCONNECTED"),
                IOErrorEnum::BrokenPipe | IOErrorEnum::NotConnected => {
                    (-101, "ERR_CONNECTION_RESET")
                }
                IOErrorEnum::ProxyFailed => (-130, "ERR_PROXY_CONNECTION_FAILED"),
                IOErrorEnum::ProxyAuthFailed | IOErrorEnum::ProxyNeedAuth => {
                    (-127, "ERR_PROXY_AUTH_REQUESTED")
                }
                _ => (-2, "ERR_FAILED"),
            };
        }
        if error.is::<ResolverError>() {
            (-105, "ERR_NAME_NOT_RESOLVED")
        } else if error.is::<TlsError>() {
            (-107, "ERR_SSL_PROTOCOL_ERROR")
        } else if error.matches(webkit2gtk::NetworkError::UnknownProtocol) {
            (-301, "ERR_UNKNOWN_URL_SCHEME")
        } else if error.matches(webkit2gtk::NetworkError::FileDoesNotExist) {
            (-6, "ERR_FILE_NOT_FOUND")
        } else {
            (-2, "ERR_FAILED")
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::sync::Arc;

    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
        COREWEBVIEW2_PROCESS_FAILED_REASON, COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED,
        COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED,
        COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY,
        COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED, COREWEBVIEW2_WEB_ERROR_STATUS,
        COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED, COREWEBVIEW2_WEB_RESOURCE_CONTEXT,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
        COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL, ICoreWebView2_2, ICoreWebView2_8,
        ICoreWebView2_22, ICoreWebView2Controller, ICoreWebView2Environment2,
        ICoreWebView2ProcessFailedEventArgs2,
    };
    use webview2_com::{
        NavigationCompletedEventHandler, ProcessFailedEventHandler,
        WebResourceRequestedEventHandler, take_pwstr,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject, SetWindowRgn};
    use windows::Win32::UI::WindowsAndMessaging::{
        GW_HWNDPREV, GetWindow, HWND_TOP, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SetWindowPos, ShowWindow,
    };
    use windows::core::{BOOL, HSTRING, Interface, PWSTR};

    use super::{
        GuestReports, HookTarget, RequestShape, Shaping, request_shape, webview2_net_error,
    };
    use crate::ads::GoneReason;

    /// Navigates the controller's webview to `url` with `headers` (CRLF
    /// separated `Name: value` lines) on the request, so the document's
    /// `Referer` also becomes its `document.referrer`.
    pub(super) fn navigate_with_headers(
        controller: &ICoreWebView2Controller,
        url: &str,
        headers: &str,
    ) -> windows::core::Result<()> {
        // SAFETY: COM calls on the webview's own thread.
        unsafe {
            let core = controller.CoreWebView2()?.cast::<ICoreWebView2_2>()?;
            let environment = core.Environment()?.cast::<ICoreWebView2Environment2>()?;
            let request = environment.CreateWebResourceRequest(
                &HSTRING::from(url),
                &HSTRING::from("GET"),
                None::<&windows::Win32::System::Com::IStream>,
                &HSTRING::from(headers),
            )?;
            core.NavigateWithWebResourceRequest(&request)
        }
    }

    /// `ShowWindow(SW_SHOWNOACTIVATE)`: shown in place, neither activated
    /// nor focused. `IsWindowVisible` (Tauri's `is_visible`) reads it at once.
    pub(super) fn show_no_activate(hwnd: isize) {
        // SAFETY: a window handle of this process; a stale one makes the
        // call fail, which is ignored.
        let _ = unsafe { ShowWindow(HWND(hwnd as *mut std::ffi::c_void), SW_SHOWNOACTIVATE) };
    }

    /// [`show_no_activate`] from a new helper thread, so the calling thread
    /// never runs the window's show messages itself: they reach the window's
    /// own thread when it next pumps (see `show_inactive`). The handle ends
    /// once the window was shown.
    pub(super) fn show_no_activate_detached(
        hwnd: isize,
    ) -> std::io::Result<std::thread::JoinHandle<()>> {
        std::thread::Builder::new()
            .name("ow-show-inactive".into())
            .spawn(move || show_no_activate(hwnd))
    }

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

    /// The guest's container window (the controller's parent window, one
    /// per webview).
    fn container(controller: &ICoreWebView2Controller) -> Option<HWND> {
        let mut hwnd = HWND::default();
        // SAFETY: a COM call on the webview's own thread with a valid out
        // pointer.
        unsafe { controller.ParentWindow(&raw mut hwnd) }.ok()?;
        (!hwnd.is_invalid()).then_some(hwnd)
    }

    /// `SetWindowPos(HWND_TOP)` of the container; whether it is now the
    /// top sibling (no previous window in z-order).
    pub(super) fn raise_to_top(controller: &ICoreWebView2Controller) -> Option<bool> {
        let hwnd = container(controller)?;
        // SAFETY: Win32 calls on a live window of this thread.
        unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .ok()?;
            Some(GetWindow(hwnd, GW_HWNDPREV).map_or(true, |prev| prev.is_invalid()))
        }
    }

    /// An empty window region on the container (`on`), or none. Returns
    /// whether the region was set.
    pub(super) fn set_input_passthrough(controller: &ICoreWebView2Controller, on: bool) -> bool {
        let Some(hwnd) = container(controller) else {
            return false;
        };
        // SAFETY: Win32 calls on a live window of this thread. On success
        // the system owns the region; on failure it is deleted here.
        unsafe {
            if !on {
                return SetWindowRgn(hwnd, None, true) != 0;
            }
            let region = CreateRectRgn(0, 0, 0, 0);
            if region.is_invalid() {
                return false;
            }
            if SetWindowRgn(hwnd, Some(region), true) == 0 {
                let _ = DeleteObject(region.into());
                return false;
            }
        }
        true
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
            request.Uri(&raw mut uri)?;
            let uri = take_pwstr(uri);
            let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
            args.ResourceContext(&raw mut context)?;
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
        target: HookTarget,
        kind: COREWEBVIEW2_PROCESS_FAILED_KIND,
        reason: COREWEBVIEW2_PROCESS_FAILED_REASON,
    ) -> Option<GoneReason> {
        let counts = match target {
            HookTarget::Guest => {
                kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                    || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
            }
            HookTarget::App => {
                kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                    || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED
            }
        };
        if !counts {
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
        target: HookTarget,
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
                        &raw mut token,
                    )?;
                }
                let crash_label = label.clone();
                let crash_reports = Arc::clone(&reports);
                let mut token = 0_i64;
                core.add_ProcessFailed(
                    &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                        args.ProcessFailedKind(&raw mut kind)?;
                        let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
                        let mut exit_code = 0_i32;
                        if let Ok(a2) = args.cast::<ICoreWebView2ProcessFailedEventArgs2>() {
                            let _ = a2.Reason(&raw mut reason);
                            let _ = a2.ExitCode(&raw mut exit_code);
                        }
                        if let Some(r) = gone_reason(target, kind, reason) {
                            crash_reports.crashed(&crash_label, r, i64::from(exit_code));
                        }
                        Ok(())
                    })),
                    &raw mut token,
                )?;
                let mut token = 0_i64;
                core.add_NavigationCompleted(
                    &NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
                        let Some(args) = args else { return Ok(()) };
                        let mut ok = BOOL::default();
                        args.IsSuccess(&raw mut ok)?;
                        if !ok.as_bool() {
                            let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                            let _ = args.WebErrorStatus(&raw mut status);
                            // Operation canceled: a new navigation replaced it.
                            if status != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED {
                                let (error_code, error_name) = webview2_net_error(status.0);
                                let url = match target {
                                    HookTarget::Guest => crate::ads::ADVIEW_URL.to_owned(),
                                    HookTarget::App => sender
                                        .as_ref()
                                        .and_then(|page| {
                                            let mut source = PWSTR::null();
                                            page.Source(&raw mut source).ok()?;
                                            Some(take_pwstr(source))
                                        })
                                        .unwrap_or_default(),
                                };
                                reports.load_failed(&label, error_code, error_name, &url);
                            }
                        }
                        Ok(())
                    })),
                    &raw mut token,
                )?;
                Ok(())
            })()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (Windows lab): `showInactive()` showed the window on the
    /// main thread inside Tauri's invoke handler; the window's show message
    /// re-entered Tauri's event loop callback, which waited for the plugin
    /// lock the invoke handler held, and the app hung. The show now comes
    /// from a helper thread: the calling thread runs none of the window's
    /// messages until it pumps, and the window ends up shown without being
    /// activated.
    #[cfg(windows)]
    #[test]
    fn show_inactive_never_runs_the_window_messages_on_the_calling_thread() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::{Duration, Instant};

        use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow,
            IsWindowVisible, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, WINDOW_EX_STYLE,
            WM_SHOWWINDOW, WNDCLASSW, WS_OVERLAPPEDWINDOW,
        };
        use windows::core::w;

        static SHOW_MESSAGE: AtomicBool = AtomicBool::new(false);
        extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            if msg == WM_SHOWWINDOW {
                SHOW_MESSAGE.store(true, Ordering::SeqCst);
            }
            // SAFETY: the default handling of this window's own message.
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            lpszClassName: w!("ow-show-inactive-test"),
            ..Default::default()
        };
        // SAFETY: a plain top-level window of this thread, off screen and
        // destroyed below.
        let hwnd = unsafe {
            RegisterClassW(&raw const class);
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("ow-show-inactive-test"),
                w!("show-inactive test"),
                WS_OVERLAPPEDWINDOW,
                -32000,
                -32000,
                100,
                100,
                None,
                None,
                None,
                None,
            )
        }
        .expect("a test window");
        let shown = windows_impl::show_no_activate_detached(hwnd.0 as isize).unwrap();
        // Without pumping, nothing reaches the window: the calling thread
        // (Tauri's main thread inside its invoke handler) is never re-entered.
        std::thread::sleep(Duration::from_millis(100));
        assert!(!SHOW_MESSAGE.load(Ordering::SeqCst));
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut msg = MSG::default();
        while !shown.is_finished() {
            assert!(Instant::now() < deadline, "the show never completed");
            // SAFETY: this thread's own message queue.
            unsafe {
                while PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    DispatchMessageW(&raw const msg);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        shown.join().unwrap();
        assert!(SHOW_MESSAGE.load(Ordering::SeqCst));
        // SAFETY: as above.
        unsafe {
            assert!(IsWindowVisible(hwnd).as_bool());
            assert_ne!(GetForegroundWindow(), hwnd);
            let _ = DestroyWindow(hwnd);
        }
    }

    /// Regression (parity lab, macOS 26): a window's content view reaches
    /// under the title bar and `WKWebView` insets the page below it; guests
    /// placed at the webview's own origin overlapped the title bar and their
    /// page lost the overlap (a 300 x 250 slot reported 300 x 226).
    /// Regression (lab diff, `reward-optin`): `showInactive()` queued the
    /// show to the main thread, and the page loaded and mounted its ad
    /// guests first, so their 400025 preceded the first-visible-window
    /// heartbeat. On the main thread the show now happens before returning.
    #[test]
    fn a_show_on_the_main_thread_happens_before_returning() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let shown = Arc::new(AtomicBool::new(false));
        let s = Arc::clone(&shown);
        now_or_on_main(
            true,
            move || s.store(true, Ordering::SeqCst),
            |_| panic!("not queued on the main thread"),
        )
        .unwrap();
        assert!(shown.load(Ordering::SeqCst));
        let queued = std::cell::Cell::new(false);
        let s = Arc::clone(&shown);
        shown.store(false, Ordering::SeqCst);
        now_or_on_main(
            false,
            move || s.store(true, Ordering::SeqCst),
            |_| {
                queued.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert!(queued.get() && !shown.load(Ordering::SeqCst));
    }

    #[test]
    fn guests_are_placed_below_the_content_inset() {
        // An app window's webview fills the window from its top edge.
        assert!((page_origin_y(0.0, 32.0) - 32.0).abs() < f64::EPSILON);
        // No inset (title bar outside the content view, other platforms).
        assert!(page_origin_y(0.0, 0.0).abs() < f64::EPSILON);
        // A webview already below the line is not moved.
        assert!((page_origin_y(100.0, 32.0) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn webview2_statuses_map_to_chromium_errors() {
        assert_eq!(webview2_net_error(7), (-7, "ERR_TIMED_OUT"));
        assert_eq!(webview2_net_error(13), (-105, "ERR_NAME_NOT_RESOLVED"));
        assert_eq!(webview2_net_error(12), (-104, "ERR_CONNECTION_FAILED"));
        assert_eq!(webview2_net_error(0), (-2, "ERR_FAILED"));
        assert_eq!(webview2_net_error(99), (-2, "ERR_FAILED"));
    }

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
