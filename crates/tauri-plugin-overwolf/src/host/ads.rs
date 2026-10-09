//! Ad guests (`<owadview>`, DESIGN §4.4): one native child webview per
//! element, in the embedder's window.
//!
//! [`AdsCore`] is the service's state and the hooks
//! [`dispatch`](super::dispatch) calls; the free functions are what the
//! `adview_*` commands call. The guest host proper ([`driver`]) needs
//! Tauri's child webviews (`ads` on Windows and macOS, cfg `ow_tauri_ads`);
//! without them no guest exists, `adview_mount` answers `unsupported` and
//! every other element command `not-found`.
//!
//! Rules every guest follows, in order of the element's life:
//!
//! - **Mount** (§4.4.1): a label `owad-<n>` no webview or window uses, the
//!   builder settings of [`crate::ads::guest_builder_spec`] (never focused,
//!   muted, zoom hotkeys off, the ads environment on Windows), the shim
//!   with the D.2 configuration, the per-OS geometry (W0c ruling 3), a
//!   forced window poll and then 400025 and `did-attach`. The first
//!   navigation waits for the startup consent window, at most
//!   [`CONSENT_WAIT_MS`](crate::ads::CONSENT_WAIT_MS) (D.6.5).
//! - **Visibility** (§4.4.4): the shim reports `visible` iff the element is
//!   visible and its window is neither hidden, minimized nor closing;
//!   `window-hidden` and `window-minimized` follow the window polls (D.5).
//! - **Close-hide** (§4.4.5, W0c ruling 2): macOS hides at
//!   `WindowEvent::Destroyed` through a retained native view; Windows at
//!   `CloseRequested`, shown again after a grace when the close was
//!   prevented.
//! - **Reload** (§4.4.6, R9): macOS recreates the native webview with the
//!   same label, place and state and carries its `sessionStorage`, within a
//!   rate guard; elsewhere (or over the guard) it reloads in place.
//! - **Crash** (§4.4.7): a native crash report recovers the guest up to
//!   `ads.maxRecoveries`, with the crash analytics of E.2 #8.
//! - **Clicks** (§4.9): a popup or an off-Overwolf top-level navigation
//!   opens the system browser only after native user activation, within
//!   the per-guest and per-app caps.
//! - **Frames** (§4.4.9): no frame of a guest may load the app's origins or
//!   any `localhost` host.

use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{Map, Value};
use tauri::ipc::Channel;
use tauri::webview::PageLoadEvent;
use tauri::{Runtime, Webview, WindowEvent};
use url::Url;

use super::{Core, lock};
use crate::ads::{AdviewCommandName, AdviewMount, AdviewUpdate, ChannelMessage};
use crate::error::{Error, Result};

#[cfg(ow_tauri_ads)]
mod driver;

/// A host message every existing guest receives (`eHashes`), with its data.
type Broadcast = Arc<dyn Fn(&str, Option<&Value>) + Send + Sync>;

/// One mounted element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mount {
    /// The guest webview's label (`owad-<n>`).
    pub(crate) guest: String,
}

/// The ads service of one app.
#[derive(Default)]
pub(crate) struct AdsCore {
    /// The guests (the driver owns this lock).
    #[cfg(ow_tauri_ads)]
    state: Mutex<driver::AdsState>,
    /// The value of the last `setUserEmailHashes()` (`None`: cleared).
    email_hashes: Mutex<Option<Value>>,
    /// Delivers a host message to every existing guest; set at the first
    /// mount.
    broadcast: OnceLock<Broadcast>,
    /// The window, consent and broadcast listeners are registered.
    #[cfg_attr(
        not(ow_tauri_ads),
        allow(dead_code, reason = "no guest is ever mounted")
    )]
    wired: OnceLock<()>,
    /// [`AdsCore::supported`], computed once.
    supported: OnceLock<bool>,
}

impl std::fmt::Debug for AdsCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("AdsCore");
        #[cfg(ow_tauri_ads)]
        s.field("guests", &lock(&self.state).labels());
        s.field("supported", &self.supported.get())
            .finish_non_exhaustive()
    }
}

/// The message of `adview_mount` where ads are unsupported.
fn unsupported_message() -> String {
    if cfg!(target_os = "linux") {
        "ads are not available on Linux".to_owned()
    } else if cfg!(not(feature = "ads")) {
        "ads are not available in this build (feature `ads` is off)".to_owned()
    } else {
        let [a, b, c, d] = crate::ads::WEBVIEW2_MINIMUM;
        format!("ads need the WebView2 Runtime {a}.{b}.{c}.{d} or newer")
    }
}

impl AdsCore {
    /// Whether ad guests can be shown in this build on this platform:
    /// feature `ads` on Windows or macOS, and on Windows a `WebView2`
    /// Runtime at least [`WEBVIEW2_MINIMUM`](crate::ads::WEBVIEW2_MINIMUM)
    /// (W0c ruling 5). Read once.
    pub(crate) fn supported(&self) -> bool {
        *self.supported.get_or_init(|| {
            #[cfg(all(ow_tauri_ads, windows))]
            {
                tauri::webview_version().is_ok_and(|v| crate::ads::webview2_supported(&v))
            }
            #[cfg(all(ow_tauri_ads, not(windows)))]
            {
                true
            }
            #[cfg(not(ow_tauri_ads))]
            {
                false
            }
        })
    }

    /// The mount of element `element_id` of webview `embedder`; `None`
    /// when that webview did not mount it (another webview's element is
    /// never found, DESIGN §4.5).
    #[cfg_attr(
        not(ow_tauri_ads),
        expect(clippy::unused_self, reason = "no guest is ever mounted")
    )]
    pub(crate) fn mount_of(&self, embedder: &str, element_id: &str) -> Option<Mount> {
        #[cfg(ow_tauri_ads)]
        {
            lock(&self.state)
                .find(embedder, element_id)
                .map(|guest| Mount { guest })
        }
        #[cfg(not(ow_tauri_ads))]
        {
            let _ = (embedder, element_id);
            None
        }
    }

    /// `setUserEmailHashes(value)` towards the guests (W4 ruling L1,
    /// observed on ow-electron 42.11.4): every existing guest gets one
    /// `eHashes` private message whose data is the value as given, or `{}`
    /// for `undefined` (`None`) and every other falsy value (`null`, `""`,
    /// `false`, `0`), as ow-electron sends `value || {}`. Guests mounted or
    /// reloaded later get nothing.
    pub(crate) fn set_email_hashes(&self, value: Option<Value>) {
        let data = value
            .as_ref()
            .filter(|v| is_truthy(v))
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
        *lock(&self.email_hashes) = value;
        if let Some(broadcast) = self.broadcast.get() {
            broadcast("eHashes", Some(&data));
        }
    }

    /// The value of the last `setUserEmailHashes()`.
    #[allow(dead_code, reason = "read by tests and the Rust API's future getter")]
    pub(crate) fn email_hashes(&self) -> Option<Value> {
        lock(&self.email_hashes).clone()
    }

    /// `on_webview_ready`. Guests get their platform hooks when they are
    /// created, so nothing happens here.
    #[expect(
        clippy::unused_self,
        reason = "the hook signature of host::dispatch (frozen)"
    )]
    pub(crate) fn webview_ready<R: Runtime>(&self, core: &Arc<Core<R>>, webview: &Webview<R>) {
        let _ = (core, webview);
    }

    /// A top-level page load of any webview: a guest's load (B.3.5, D.5),
    /// or an app webview's new document, which closes the guests it
    /// embedded (§4.4.8).
    #[expect(
        clippy::unused_self,
        reason = "the hook signature of host::dispatch (frozen)"
    )]
    pub(crate) fn page_load<R: Runtime>(
        &self,
        core: &Arc<Core<R>>,
        webview: &Webview<R>,
        event: PageLoadEvent,
        url: &Url,
    ) {
        #[cfg(ow_tauri_ads)]
        driver::page_load(core, webview, event, url);
        #[cfg(not(ow_tauri_ads))]
        let _ = (core, webview, event, url);
    }

    /// The navigation policy of guest `label` (§4.4.9, D.7): only a guest
    /// the plugin created navigates, never to the app's origins or a
    /// `localhost` host.
    #[expect(
        clippy::unused_self,
        reason = "the hook signature of host::dispatch (frozen)"
    )]
    pub(crate) fn guest_navigation<R: Runtime>(
        &self,
        core: &Arc<Core<R>>,
        label: &str,
        url: &Url,
    ) -> bool {
        #[cfg(ow_tauri_ads)]
        {
            driver::guest_navigation(core, label, url)
        }
        #[cfg(not(ow_tauri_ads))]
        {
            let _ = (core, label, url);
            false
        }
    }

    /// A window event: focus, the close-hide (§4.4.5) and the guests of a
    /// destroyed window.
    #[expect(
        clippy::unused_self,
        reason = "the hook signature of host::dispatch (frozen)"
    )]
    pub(crate) fn window_event<R: Runtime>(
        &self,
        core: &Arc<Core<R>>,
        label: &str,
        event: &WindowEvent,
    ) {
        #[cfg(ow_tauri_ads)]
        driver::window_event(core, label, event);
        #[cfg(not(ow_tauri_ads))]
        let _ = (core, label, event);
    }
}

/// JavaScript truthiness of a JSON value (`value || {}` in ow-electron).
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `adview_mount`: creates the guest of `request` in `embedder`'s window
/// and returns its label. Events of the element go to `channel`.
///
/// # Errors
///
/// `unsupported` without ads, `invalid-argument` for a bad request,
/// `backend` when the webview could not be created.
#[cfg_attr(
    not(ow_tauri_ads),
    allow(clippy::unused_async, reason = "the command awaits it")
)]
pub(crate) async fn mount<R: Runtime>(
    core: &Arc<Core<R>>,
    embedder: &Webview<R>,
    request: AdviewMount,
    channel: Channel<ChannelMessage>,
) -> Result<String> {
    if !core.ads.supported() {
        return Err(Error::unsupported(unsupported_message()));
    }
    #[cfg(ow_tauri_ads)]
    {
        driver::mount(core, embedder, request, channel).await
    }
    #[cfg(not(ow_tauri_ads))]
    {
        let _ = (embedder, request, channel);
        Err(Error::unsupported(unsupported_message()))
    }
}

/// `adview_update`.
///
/// # Errors
///
/// `not-found` for an element `embedder` did not mount, `invalid-argument`
/// for a bad rectangle.
#[cfg_attr(
    not(ow_tauri_ads),
    allow(
        clippy::needless_pass_by_value,
        reason = "the ads build takes the request"
    )
)]
pub(crate) fn update<R: Runtime>(
    core: &Arc<Core<R>>,
    embedder: &Webview<R>,
    request: AdviewUpdate,
) -> Result<()> {
    #[cfg(ow_tauri_ads)]
    {
        driver::update(core, embedder, request)
    }
    #[cfg(not(ow_tauri_ads))]
    {
        let _ = (core, embedder);
        Err(Error::not_found(format!(
            "no mounted element {}",
            request.element_id
        )))
    }
}

/// `adview_unmount` (idempotent): closes the guest; the element's own
/// runtime dispatches `destroyed` (B.3.5).
pub(crate) fn unmount<R: Runtime>(core: &Arc<Core<R>>, embedder: &Webview<R>, element_id: &str) {
    #[cfg(ow_tauri_ads)]
    driver::unmount(core, embedder.label(), element_id);
    #[cfg(not(ow_tauri_ads))]
    let _ = (core, embedder, element_id);
}

/// `adview_command` (B.3.3).
///
/// # Errors
///
/// `not-found` for an element `embedder` did not mount.
pub(crate) fn command<R: Runtime>(
    core: &Arc<Core<R>>,
    embedder: &Webview<R>,
    element_id: &str,
    command: AdviewCommandName,
    args: &[Value],
) -> Result<()> {
    #[cfg(ow_tauri_ads)]
    {
        driver::command(core, embedder.label(), element_id, command, args)
    }
    #[cfg(not(ow_tauri_ads))]
    {
        let _ = (core, embedder, command, args);
        Err(Error::not_found(format!("no mounted element {element_id}")))
    }
}

/// `adview_event` of guest `label` (A.2.6, D.4).
///
/// # Errors
///
/// `invalid-argument` for a bad event name, `not-found` for a webview that
/// is not a live guest.
pub(crate) fn guest_event<R: Runtime>(
    core: &Arc<Core<R>>,
    label: &str,
    slot_id: Option<&str>,
    name: &str,
    data: Option<Value>,
) -> Result<()> {
    #[cfg(ow_tauri_ads)]
    {
        driver::guest_event(core, label, slot_id, name, data)
    }
    #[cfg(not(ow_tauri_ads))]
    {
        let _ = (core, slot_id, name, data);
        Err(Error::not_found(format!("no ad guest {label}")))
    }
}

/// macOS: the web content process of guest `label` ended (the app's
/// `on_web_content_process_terminate` hook, W0c ruling 1): the guest is
/// recovered as after any crash (§4.4.7).
#[cfg(target_os = "macos")]
pub(crate) fn web_content_terminated<R: Runtime>(core: &Arc<Core<R>>, label: &str) {
    #[cfg(ow_tauri_ads)]
    driver::guest_crashed(core, label, crate::ads::GoneReason::Crashed, 0);
    #[cfg(not(ow_tauri_ads))]
    let _ = (core, label);
}

/// Whether `url` may call `adview_event`: the ad page's scope (the guest
/// capability's remote URL, checked again here, §7.3).
pub(crate) fn guest_url_allowed(url: &Url) -> bool {
    crate::ads::is_overwolf_url(url) && url.as_str().starts_with(crate::ads::ADVIEW_SCOPE)
}

#[cfg(test)]
mod facade_tests {
    use super::*;

    #[test]
    fn only_the_ad_page_scope_may_send_guest_events() {
        let ok = |u: &str| guest_url_allowed(&u.parse().unwrap());
        assert!(ok(
            "https://www.overwolf.com/monsdk/electron/latest/adview.html"
        ));
        assert!(!ok("https://www.overwolf.com/other.html"));
        assert!(!ok(
            "http://www.overwolf.com/monsdk/electron/latest/adview.html"
        ));
        assert!(!ok("tauri://localhost/index.html"));
        assert!(!ok("https://evil.example/monsdk/electron/"));
    }

    #[test]
    fn falsy_values_are_javascript_falsy() {
        use serde_json::json;
        for v in [json!(null), json!(false), json!(0), json!(0.0), json!("")] {
            assert!(!is_truthy(&v), "{v}");
        }
        for v in [json!(true), json!(1), json!("a"), json!([]), json!({})] {
            assert!(is_truthy(&v), "{v}");
        }
    }

    #[test]
    fn an_unmounted_element_belongs_to_nobody() {
        let ads = AdsCore::default();
        assert!(ads.mount_of("main", "e1").is_none());
        if !cfg!(windows) {
            assert_eq!(ads.supported(), cfg!(ow_tauri_ads));
        }
    }
}
