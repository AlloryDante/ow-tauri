//! The guest host (cfg `ow_tauri_ads`: feature `ads` on Windows and
//! macOS). See the parent module for the rules; CONTRACT D for the guest
//! protocol.
//!
//! Locking: the [`AdsState`] lock is held only for pure state changes. Every
//! Tauri call (creating, evaluating in, showing or closing a webview,
//! sending on a channel) runs after it is released. Work started from a
//! window event or a platform callback that may create or close a webview
//! runs on a runtime task.

use std::collections::BTreeMap;
use std::sync::{Arc, Weak};
use std::time::Duration;

use serde_json::{Value, json};
use tauri::ipc::Channel;
use tauri::webview::{NewWindowFeatures, NewWindowResponse, PageLoadEvent};
use tauri::{
    LogicalPosition, LogicalSize, Runtime, Webview, WebviewBuilder, WebviewUrl, Window, WindowEvent,
};
use tokio::sync::oneshot;
use url::Url;

use crate::ads::{
    ADVIEW_CONFIG_TOKEN, ADVIEW_URL, Admission, AdviewAttributes, AdviewAttributesPatch,
    AdviewCommandName, AdviewMount, AdviewRect, AdviewUpdate, AppOpenCap, CLOSE_GRACE_MS,
    CLOSE_HIDE, CONSENT_WAIT_MS, ChannelMessage, CloseHide, EventSource, Generations, GoneReason,
    GuestBuilderSpec, GuestFacts, GuestLimiter, InternalEvent, MAX_DOCUMENT_TITLE,
    MINIMIZE_HIDES_NATIVELY, MINIMIZE_SENDS_WINDOW_HIDDEN, OpenBudget, RecreateLimiter,
    SESSION_RESTORE_MARKER, SNAPSHOT_WAIT_MS, Snapshot, WINDOW_MINIMIZED, cap_event_data,
    deliver_script, fail_load_data, frame_url_allowed, gone_data, guest_builder_spec, guest_config,
    guest_visible, host_call_script, host_message, is_overwolf_url, logical_rect, may_recover,
    native_visibility_on_minimize, next_guest_label, restore_prelude, session_secs,
    snapshot_outcome, snapshot_script, splice_config, valid_event_name, valid_geometry,
    zoom_from_dpr, zoom_from_width,
};
use crate::config::ADVIEW_LABEL_PREFIX;
use crate::error::{Error, Result};
use crate::host::windows::WindowObservation;
use crate::host::{Core, Flags, LOG_TARGET, lock};
use crate::platform::webview::{GuestReports, NativeView, Shaping, ViewAnchor};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

/// The guest shim (D.1), built from `packages/guest-shims`.
pub(crate) const ADVIEW_HOST_JS: &str = include_str!("../../../js/adview-host.js");

/// The one-shot `sessionStorage` restore of a recreated guest (§4.4.6).
pub(crate) const SESSION_RESTORE_JS: &str = include_str!("../../../js/session-restore.js");

/// A guest whose document has not reported `__host:ready` this long after
/// its navigation started is treated as a failed main-frame load (the
/// platform reports none on macOS).
pub(crate) const READY_TIMEOUT_MS: u64 = 20_000;

/// ow-electron reloads a guest about 70 ms after `__overwolf__.reload()`
/// (D.3).
pub(crate) const RELOAD_DELAY_MS: u64 = 70;

/// A reload the page asks for while hidden runs no earlier than this long
/// after the guest was told `hidden`, or as soon as it is visible again
/// (D.5). ow-electron's hidden Chromium document asks 2.6 to 4.8 s after
/// `hidden` (throttled timers, observed); the request is never dropped.
pub(crate) const HIDDEN_RELOAD_HOLD_MS: u64 = 2_500;

/// `did-finish-load` waits this long for the shim's `dom-ready`
/// (ow-electron reports `dom-ready` first); then it goes alone.
pub(crate) const FINISH_WAIT_MS: u64 = 1_000;

/// Whether a reload recreates the native webview (macOS, R9) where
/// `ads.recreateOnReload` and the rate guard allow it.
pub(crate) const RECREATE_PLATFORM: bool = cfg!(target_os = "macos");

/// How long a mount waits for its forced window poll on the main thread.
const POLL_WAIT_MS: u64 = 1_000;

/// One ad guest.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent per-guest flags of D.5 and §4.4"
)]
pub(crate) struct Guest {
    /// The embedder webview's label.
    pub(crate) embedder: String,
    /// The label of the window the guest is a child of.
    pub(crate) window: String,
    /// Mount order (the label's number).
    pub(crate) seq: u32,
    /// The element id in the embedder's page.
    pub(crate) element_id: String,
    /// The element's event channel.
    channel: Channel<ChannelMessage>,
    /// The element's attributes.
    pub(crate) attributes: AdviewAttributes,
    /// The last rectangle (CSS pixels) and page geometry.
    rect: AdviewRect,
    device_pixel_ratio: f64,
    inner_width: f64,
    /// Where the guest is, in the window's logical pixels.
    pub(crate) bounds: (f64, f64, f64, f64),
    /// The element is visible (B.3.4).
    pub(crate) visible: bool,
    /// The first navigation was started (D.6.5).
    pub(crate) navigated: bool,
    nav_started_ms: Option<u64>,
    ready: bool,
    dom_ready: bool,
    finish_pending: Option<u64>,
    /// The random window property of the shim's host API (D.5).
    pub(crate) host_key: String,
    /// The shim with its configuration (kept for a recreate).
    init_script: Arc<str>,
    /// The guest's window is hidden (not minimized).
    pub(crate) window_hidden: bool,
    /// The guest's window is minimized.
    pub(crate) window_minimized: bool,
    /// The guest's window is focused.
    window_focused: bool,
    /// The window is closing (Windows, §4.4.5).
    pub(crate) closing_hidden: bool,
    /// The visibility the shim reports (`visible` when true).
    pub(crate) sent_visible: bool,
    sent_focus: Option<bool>,
    /// The last finished load or crash recovery (session ms).
    last_load_ms: u64,
    /// Finished loads of the ad document.
    pub(crate) loads: u32,
    /// Crash recoveries so far.
    pub(crate) recoveries: u32,
    limiter: GuestLimiter,
    budget: OpenBudget,
    tracking_changed: bool,
    next_page_url: Option<String>,
    reload_at: Option<u64>,
    reload_held: bool,
    hidden_at: Option<u64>,
    retry_at: Option<u64>,
    apply_setting_logged: bool,
    /// Mouse input passes through (a performance guest until its first
    /// `performance_ad_loaded`, B.3.4).
    pub(crate) passthrough: bool,
    /// The native webview instances (§4.4.6.3).
    pub(crate) generations: Generations,
    recreates: RecreateLimiter,
    /// The recreated document still has its `sessionStorage` prelude.
    restore_pending: bool,
    /// macOS: the retained `WKWebView` (close-hide, gesture monitor).
    native: Option<NativeView>,
}

impl Guest {
    /// A new document load starts: readiness and `dom-ready` start over.
    fn begin_load(&mut self, now: u64) {
        self.ready = false;
        self.dom_ready = false;
        self.finish_pending = None;
        self.nav_started_ms = Some(now);
    }

    /// The visibility the shim should report now (§4.4.4).
    fn computed_visible(&self) -> bool {
        guest_visible(
            self.visible,
            self.window_hidden,
            self.window_minimized,
            self.closing_hidden,
        )
    }
}

/// The ads service state.
#[derive(Default)]
pub(crate) struct AdsState {
    pub(crate) guests: BTreeMap<String, Guest>,
    next: u32,
    system_info: Option<Value>,
    /// `guestLimits.externalOpensPerMinuteApp`, created at the first open.
    app_opens: Option<AppOpenCap>,
    /// What a host without OS queries (Tauri's mock runtime) sent to its
    /// guests and did to them, in order; tests read it. Always empty in an
    /// app.
    pub(crate) trace: Vec<Value>,
}

impl AdsState {
    /// The guest of `element_id` in `embedder`.
    pub(crate) fn find(&self, embedder: &str, element_id: &str) -> Option<String> {
        self.guests
            .iter()
            .find(|(_, g)| g.embedder == embedder && g.element_id == element_id)
            .map(|(l, _)| l.clone())
    }

    /// Every guest label.
    pub(crate) fn labels(&self) -> Vec<String> {
        self.guests.keys().cloned().collect()
    }

    fn of_window(&self, window: &str) -> Vec<String> {
        self.guests
            .iter()
            .filter(|(_, g)| g.window == window)
            .map(|(l, _)| l.clone())
            .collect()
    }

    fn of_embedder(&self, embedder: &str) -> Vec<String> {
        self.guests
            .iter()
            .filter(|(_, g)| g.embedder == embedder)
            .map(|(l, _)| l.clone())
            .collect()
    }
}

/// Who closes a guest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Closer {
    /// The app (unmount, a new embedder document, its window or embedder
    /// is gone): the element's runtime knows, no `destroyed` is sent.
    App,
    /// The plugin's own decision (limits, recoveries, a failed mute or
    /// recreate): the element gets `destroyed` (B.3.5).
    Host,
}

fn with_guest<R: Runtime, T>(
    core: &Core<R>,
    label: &str,
    f: impl FnOnce(&mut Guest) -> T,
) -> Option<T> {
    lock(&core.ads.state).guests.get_mut(label).map(f)
}

/// Appends `entry` to the lab trace file `file` and, in a host without OS
/// queries (Tauri's mock runtime), to the test trace.
fn record<R: Runtime>(core: &Core<R>, file: &str, entry: impl FnOnce() -> Value) {
    if core.options.os_queries {
        crate::lab::record(file, entry);
        return;
    }
    let entry = entry();
    crate::lab::record(file, || entry.clone());
    lock(&core.ads.state).trace.push(entry);
}

fn webview_of<R: Runtime>(core: &Core<R>, label: &str) -> Option<Webview<R>> {
    crate::compat::webview(&core.app, label)
}

/// Whether a webview or window already uses `label` (SEC-m2).
fn label_taken<R: Runtime>(core: &Core<R>, label: &str) -> bool {
    webview_of(core, label).is_some() || crate::compat::window(&core.app, label).is_some()
}

/// Registers the window, consent and broadcast listeners (once).
fn wire<R: Runtime>(core: &Arc<Core<R>>) {
    if core.ads.wired.set(()).is_err() {
        return;
    }
    let weak = Arc::downgrade(core);
    core.windows.add_poll_listener(Arc::new(
        move |observations: &[WindowObservation], now: u64| {
            if let Some(core) = weak.upgrade() {
                on_poll(&core, observations, now);
            }
        },
    ));
    // D.5: every saved consent value goes to every existing guest (twice
    // per save: the TCF string, then the unified string).
    let weak = Arc::downgrade(core);
    core.consent
        .add_consent_listener(Arc::new(move |value: &str| {
            if let Some(core) = weak.upgrade() {
                deliver_to_all(&core, "consent", Some(&Value::from(value)));
            }
        }));
    let weak = Arc::downgrade(core);
    let _ = core
        .ads
        .broadcast
        .set(Arc::new(move |kind: &str, data: Option<&Value>| {
            if let Some(core) = weak.upgrade() {
                deliver_to_all(&core, kind, data);
            }
        }));
}

/// Reserves the next free guest label (SEC-m2).
fn reserve_label<R: Runtime>(core: &Core<R>) -> (u32, String) {
    loop {
        let (n, label) = {
            let mut s = lock(&core.ads.state);
            let (n, label) = next_guest_label(s.next, |l| s.guests.contains_key(l));
            s.next = n;
            (n, label)
        };
        if !label_taken(core, &label) {
            return (n, label);
        }
    }
}

/// The guest's place in its window's logical pixels (§4.4.3, W0c ruling 3):
/// the rectangle times the page zoom (macOS: the embedder's native width
/// over `innerWidth`; Windows: `devicePixelRatio` over the scale factor),
/// plus where the embedder's page starts in the window.
fn placement<R: Runtime>(
    core: &Core<R>,
    embedder: &Webview<R>,
    window: &Window<R>,
    rect: &AdviewRect,
    device_pixel_ratio: f64,
    inner_width: f64,
) -> (f64, f64, f64, f64) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let zoom = if cfg!(target_os = "macos") {
        let width = embedder
            .size()
            .map(|s| f64::from(s.width) / scale)
            .unwrap_or_default();
        if width > 0.0 {
            zoom_from_width(width, inner_width)
        } else {
            1.0
        }
    } else {
        zoom_from_dpr(device_pixel_ratio, scale)
    };
    let (x, y) = embedder
        .position()
        .map(|p| p.to_logical::<f64>(scale))
        .map_or((0.0, 0.0), |p| (p.x, p.y));
    let inset = if core.options.os_queries {
        crate::platform::webview::content_inset_top(window)
    } else {
        0.0
    };
    logical_rect(
        rect,
        zoom,
        (x, crate::platform::webview::page_origin_y(y, inset)),
    )
}

/// `systemInfo.gpus` (D.2): one entry per adapter with only its driver
/// version (Windows), or one blank entry where the platform lists none.
pub(crate) fn gpu_entries(driver_versions: &[String]) -> Vec<Value> {
    let entry = |v: &str| json!({ "name": "", "model": "", "driverVersion": v, "vendor": "" });
    if driver_versions.is_empty() {
        vec![entry("")]
    } else {
        driver_versions.iter().map(|v| entry(v)).collect()
    }
}

/// `systemInfo` (D.2), read once.
fn system_info<R: Runtime>(core: &Core<R>) -> Value {
    if let Some(v) = lock(&core.ads.state).system_info.clone() {
        return v;
    }
    let os = core.options.os_queries;
    let (monitors, primary) = crate::platform::display::monitors(&core.app, os);
    let displays: Vec<Value> = monitors
        .iter()
        .map(|m| {
            let s = if m.scale_factor > 0.0 {
                m.scale_factor
            } else {
                1.0
            };
            let is_main = primary
                .as_ref()
                .is_some_and(|p| p.x == m.x && p.y == m.y && p.name == m.name);
            json!({
                "name": m.name,
                "isMain": is_main,
                "position": [(f64::from(m.x) / s).round(), (f64::from(m.y) / s).round()],
                "resolution": [(f64::from(m.width) / s).round(), (f64::from(m.height) / s).round()],
                "dpi": (96.0 * s).round(),
            })
        })
        .collect();
    let cpu = if os {
        crate::platform::machine::cpu_brand()
    } else {
        String::new()
    };
    let versions = if os {
        crate::platform::graphics::gpu_driver_versions()
    } else {
        Vec::new()
    };
    let info = json!({ "gpus": gpu_entries(&versions), "cpu": cpu, "displays": displays });
    lock(&core.ads.state).system_info = Some(info.clone());
    info
}

/// `<owVersion>` as guests see it (`ads.owVersionOverride`).
fn guest_ow_version<R: Runtime>(core: &Core<R>) -> String {
    let config = &core.identity.config;
    config
        .ads
        .ow_version_override
        .clone()
        .unwrap_or_else(|| crate::host::analytics::host_label(&config.analytics).ow_version())
}

/// The request shaping of a guest in window `window_name` (D.8), when
/// `ads.requestShaping` is on.
fn shaping<R: Runtime>(core: &Core<R>, window_name: &str) -> Option<Shaping> {
    let id = &core.identity;
    id.config.ads.request_shaping.then(|| Shaping {
        referer: format!("https://www.overwolf.com/{}", id.app.uid),
        origin: "https://www.overwolf.com".to_owned(),
        uid: id.app.uid.clone(),
        phase: id.phase_percent.to_string(),
        window: window_name.to_owned(),
    })
}

/// The `documentReferrer` the shim answers `document.referrer` with
/// (D.8.3): the shaped `Referer`, where the webview does not take it from
/// the header.
pub(crate) fn document_referrer(shaping: Option<&Shaping>) -> Option<String> {
    shaping
        .filter(|_| crate::platform::webview::SHIM_DOCUMENT_REFERRER)
        .map(|s| s.referer.clone())
}

/// The guest's window name (`x-ow-window`, `windowName`).
fn window_name_of<R: Runtime>(core: &Core<R>, label: &str) -> String {
    let window = with_guest(core, label, |g| g.window.clone()).unwrap_or_default();
    core.windows.window_name(core, &window, None)
}

/// Builds the guest webview of `spec` in `window` at `bounds`.
fn build_guest<R: Runtime>(
    core: &Arc<Core<R>>,
    window: &Window<R>,
    spec: &GuestBuilderSpec,
    bounds: (f64, f64, f64, f64),
) -> Result<Webview<R>> {
    let blank = Url::parse(spec.url).map_err(|e| Error::backend(e.to_string()))?;
    let mut builder = WebviewBuilder::new(&spec.label, WebviewUrl::External(blank))
        .user_agent(&spec.user_agent)
        .focused(spec.focused)
        .transparent(spec.transparent)
        .zoom_hotkeys_enabled(spec.zoom_hotkeys)
        .on_new_window(new_window_handler(core, &spec.label));
    for script in &spec.init_scripts {
        builder = builder.initialization_script(script.clone());
    }
    #[cfg(windows)]
    if let Some((dir, args)) = &spec.environment {
        builder = builder
            .data_directory(dir.clone())
            .additional_browser_args(args);
    }
    let (x, y, w, h) = bounds;
    #[cfg(test)]
    let _mock = crate::host::windows::tests::mock_windows();
    // A `<webview>` guest never activates the app in ow-electron.
    crate::platform::webview::without_app_activation(|| {
        window.add_child(builder, LogicalPosition::new(x, y), LogicalSize::new(w, h))
    })
    .map_err(|e| Error::backend(e.to_string()))
}

/// The builder spec of guest `label` (§4.4.1).
fn spec_for<R: Runtime>(
    core: &Core<R>,
    label: &str,
    init_scripts: Vec<String>,
) -> GuestBuilderSpec {
    let ads = &core.identity.config.ads;
    guest_builder_spec(
        label,
        ads.transparent_guests,
        &core.analytics.user_agent(),
        init_scripts,
        &core.identity.ads_data_dir,
        &ads.browser_args,
    )
}

/// Closes a webview, serialised with the test thread under the mock
/// runtime (whose windows live in a `RefCell`).
fn close_webview<R: Runtime>(webview: &Webview<R>) {
    #[cfg(test)]
    let _mock = crate::host::windows::tests::mock_windows();
    let _ = webview.close();
}

/// What follows the creation of a guest webview, at mount and at a
/// recreate: hidden unless `shown`, muted, transparent, pass-through,
/// platform hooks, and the retained native view with the gesture monitor.
fn prepare_guest<R: Runtime>(
    core: &Arc<Core<R>>,
    webview: &Webview<R>,
    shown: bool,
    passthrough: bool,
    cause: &str,
) {
    if !shown {
        let _ = webview.hide();
    }
    mute_guest(core, webview, true, cause);
    if core.identity.config.ads.transparent_guests {
        clear_guest_background(core, webview);
    }
    if passthrough {
        apply_passthrough(core, webview, true);
    }
    let label = webview.label().to_owned();
    let reports: Arc<dyn GuestReports> = Arc::new(Reports(Arc::downgrade(core)));
    let shaping = shaping(core, &window_name_of(core, &label));
    if let Err(err) = crate::platform::webview::install_guest_hooks(webview, shaping, reports) {
        log::warn!(target: LOG_TARGET, "guest hooks for {label} failed: {err}");
    }
    retain_and_watch(core, webview);
}

/// macOS: keeps the guest's `WKWebView` alive in the guest (released when
/// the guest is closed or recreated) and arms the guest from native input
/// (§4.9).
fn retain_and_watch<R: Runtime>(core: &Arc<Core<R>>, webview: &Webview<R>) {
    let label = webview.label().to_owned();
    let Some(generation) = with_guest(core, &label, |g| g.generations.current()) else {
        return;
    };
    let weak = Arc::downgrade(core);
    let target = webview.clone();
    let _ = crate::platform::webview::retain_native(webview, move |view| {
        let Some(core) = weak.upgrade() else { return };
        let Some(view) = view else { return };
        let address = view.address();
        let mut slot = Some(view);
        {
            let mut s = lock(&core.ads.state);
            if let Some(g) = s.guests.get_mut(&label)
                && g.generations.is_current(generation)
            {
                g.native = slot.take();
            }
        }
        if slot.is_some() {
            return;
        }
        let arm_core = Arc::downgrade(&core);
        let arm_label = label.clone();
        crate::platform::gesture::watch(
            &target,
            address,
            Arc::new(move || {
                if let Some(core) = arm_core.upgrade() {
                    arm(&core, &arm_label);
                }
            }),
        );
    });
}

/// Native user activation of guest `label` (§4.9): opens its activation
/// window.
fn arm<R: Runtime>(core: &Core<R>, label: &str) {
    let now = core.now();
    if with_guest(core, label, |g| g.budget.gesture(now)).is_some() {
        record(
            core,
            "wc-events.jsonl",
            || json!({ "kind": "armed", "label": label, "type": "owadview" }),
        );
    }
}

/// `adview_mount` (A.2.5): creates the guest webview.
#[expect(
    clippy::too_many_lines,
    reason = "the mount sequence of B.3.4, E.2 and D.6.5 in its order"
)]
pub(crate) async fn mount<R: Runtime>(
    core: &Arc<Core<R>>,
    embedder: &Webview<R>,
    request: AdviewMount,
    channel: Channel<ChannelMessage>,
) -> Result<String> {
    if !crate::ads::valid_element_id(&request.element_id)
        || !valid_geometry(
            &request.rect,
            request.device_pixel_ratio,
            request.inner_width,
        )
    {
        return Err(Error::invalid_argument("invalid element id or geometry"));
    }
    wire(core);
    let embedder_label = embedder.label().to_owned();
    let old = lock(&core.ads.state).find(&embedder_label, &request.element_id);
    if let Some(old) = old {
        close_guest(core, &old, Closer::App);
    }
    let window = embedder.window();
    let window_label = window.label().to_owned();
    let bounds = placement(
        core,
        embedder,
        &window,
        &request.rect,
        request.device_pixel_ratio,
        request.inner_width,
    );
    let (seq, label) = reserve_label(core);
    let window_name = core
        .windows
        .window_name(core, &window_label, embedder.url().ok().as_ref());
    let window_title = request
        .document_title
        .as_deref()
        .map(|t| t.chars().take(MAX_DOCUMENT_TITLE).collect::<String>())
        .or_else(|| core.windows.title(&window_label))
        .unwrap_or_else(|| {
            crate::host::windows::window_title(
                None,
                window.title().ok().as_deref(),
                &core.identity.app.name,
            )
        });
    let focused = window.is_focused().unwrap_or(false);
    let hidden = !window.is_visible().unwrap_or(true);
    let minimized = window.is_minimized().unwrap_or(false);
    let shown_now = guest_visible(request.visible, hidden, minimized, false);
    let host_key = format!("_{}", uuid::Uuid::new_v4().simple());
    let id = &core.identity;
    let ow_version = guest_ow_version(core);
    let facts = GuestFacts {
        muid: &id.machine.muid,
        uid: &id.app.uid,
        name: &id.app.name,
        ow_version: &ow_version,
        version: &id.app.version,
        window_name: &window_name,
        window_title: &window_title,
        window_focused: focused,
        test_ad: id.test_ad,
        disable_optimization: Flags::get(&core.flags.ads_optimization_disabled),
        muid_v2: &id.machine.muid_v2,
        phase_percent: id.phase_percent,
        consent: &id.launch_consent,
        system_info: system_info(core),
        attributes: &request.attributes,
        slot_id: &label,
    };
    let mut config = guest_config(&facts, shown_now);
    if let Value::Object(m) = &mut config {
        m.insert("hostKey".into(), Value::from(host_key.as_str()));
        if let Some(r) = document_referrer(shaping(core, &window_name).as_ref()) {
            m.insert("documentReferrer".into(), Value::from(r));
        }
    }
    let script = splice_config(ADVIEW_HOST_JS, ADVIEW_CONFIG_TOKEN, &config)
        .ok_or_else(|| Error::backend("the guest shim has no configuration token"))?;
    let init_script: Arc<str> = Arc::from(script);
    let spec = spec_for(core, &label, vec![init_script.to_string()]);
    let webview = build_guest(core, &window, &spec, bounds)?;
    let now = core.now();
    let ads = &id.config.ads;
    let limits = &ads.guest_limits;
    let performance = request.attributes.performance;
    let guest = Guest {
        embedder: embedder_label.clone(),
        window: window_label.clone(),
        seq,
        element_id: request.element_id.clone(),
        channel,
        attributes: request.attributes,
        rect: request.rect,
        device_pixel_ratio: request.device_pixel_ratio,
        inner_width: request.inner_width,
        bounds,
        visible: request.visible,
        navigated: false,
        nav_started_ms: None,
        ready: false,
        dom_ready: false,
        finish_pending: None,
        host_key,
        init_script,
        window_hidden: hidden,
        window_minimized: minimized,
        window_focused: focused,
        closing_hidden: false,
        sent_visible: shown_now,
        sent_focus: Some(focused),
        last_load_ms: now,
        loads: 0,
        recoveries: 0,
        limiter: GuestLimiter::new(limits, now),
        budget: OpenBudget::new(
            limits.activation_window_ms,
            limits.external_opens_per_minute,
        ),
        tracking_changed: false,
        next_page_url: None,
        reload_at: None,
        reload_held: false,
        hidden_at: (!shown_now).then_some(now),
        retry_at: None,
        apply_setting_logged: false,
        passthrough: performance,
        generations: Generations::default(),
        recreates: RecreateLimiter::new(ads.recreate_min_interval_ms, ads.recreate_max_per_hour),
        restore_pending: false,
        native: None,
    };
    lock(&core.ads.state).guests.insert(label.clone(), guest);
    record(core, "wc-events.jsonl", || {
        json!({
            "kind": "created",
            "label": label,
            "type": "owadview",
            "embedder": embedder_label,
            "elementId": request.element_id,
            "visible": request.visible,
            "bounds": [bounds.0, bounds.1, bounds.2, bounds.3],
            "windowTitle": window_title,
        })
    });
    let native_shown = request.visible && !(MINIMIZE_HIDES_NATIVELY && minimized);
    prepare_guest(core, &webview, native_shown, performance, "mount");
    raise_performance_guest(core, &window_label, &label);
    // A window shown since the last poll counts first: its first-visible
    // heartbeat precedes the 400025 of a guest attached after it (E.2 #5,
    // #6).
    forced_poll(core).await;
    core.analytics.guest_attached();
    host_event(core, &label, "did-attach", None);
    schedule_first_navigation(core, &label);
    Ok(label)
}

/// Polls every window on the main thread now and waits for it (bounded).
async fn forced_poll<R: Runtime>(core: &Arc<Core<R>>) {
    let (tx, rx) = oneshot::channel();
    let weak = Arc::downgrade(core);
    let queued = core.app.run_on_main_thread(move || {
        if let Some(core) = weak.upgrade() {
            core.windows.poll_now(&core);
        }
        let _ = tx.send(());
    });
    if queued.is_ok() {
        let _ = tokio::time::timeout(Duration::from_millis(POLL_WAIT_MS), rx).await;
    }
}

/// Starts the guest's first navigation once the startup consent window is
/// gone, or [`CONSENT_WAIT_MS`] after the mount (D.6.5).
fn schedule_first_navigation<R: Runtime>(core: &Arc<Core<R>>, label: &str) {
    let mut gate = core.consent.subscribe_gate();
    let weak = Arc::downgrade(core);
    let label = label.to_owned();
    tauri::async_runtime::spawn(async move {
        let wait = Duration::from_millis(CONSENT_WAIT_MS);
        let opened = tokio::time::timeout(wait, gate.wait_for(|open| *open))
            .await
            .is_ok_and(|r| r.is_ok());
        let Some(core) = weak.upgrade() else { return };
        let now = core.now();
        let start = with_guest(&core, &label, |g| {
            if g.navigated {
                return false;
            }
            g.navigated = true;
            g.begin_load(now);
            true
        })
        .unwrap_or(false);
        if start {
            record(
                &core,
                "wc-events.jsonl",
                || json!({ "kind": "first-navigation", "label": label, "type": "owadview", "consentGate": opened }),
            );
            navigate_guest(&core, &label);
        }
    });
}

/// Loads the ad document in guest `label` with the document headers of
/// D.8.3.
fn navigate_guest<R: Runtime>(core: &Arc<Core<R>>, label: &str) {
    let Some(webview) = webview_of(core, label) else {
        return;
    };
    let Ok(url) = Url::parse(ADVIEW_URL) else {
        return;
    };
    let window_name = window_name_of(core, label);
    let shaping = shaping(core, &window_name);
    let shaped = crate::platform::webview::load_shaped(&webview, &url, shaping.as_ref());
    record(core, "shaped-requests.jsonl", || {
        json!({
            "label": label,
            "url": url.as_str(),
            "method": "GET",
            "via": if shaped && shaping.is_some() && !cfg!(windows) { "loadRequest" } else { "navigate" },
            "hostHeaders": crate::platform::webview::document_header_fields(shaping.as_ref()),
            "windowName": window_name,
        })
    });
    if !shaped {
        let _ = webview.navigate(url);
    }
}

/// Mutes or unmutes a guest. A guest that cannot be muted is closed
/// (fail-closed, §4.4.1). Lab trace: `set-muted` with its cause.
fn mute_guest<R: Runtime>(core: &Arc<Core<R>>, webview: &Webview<R>, muted: bool, cause: &str) {
    let label = webview.label().to_owned();
    record(
        core,
        "ipc.jsonl",
        || json!({ "dir": "host->page", "via": "set-muted", "type": "owadview", "label": label, "muted": muted, "cause": cause }),
    );
    let Some(generation) = with_guest(core, &label, |g| g.generations.current()) else {
        return;
    };
    let weak = Arc::downgrade(core);
    let _ = crate::platform::webview::set_muted(webview, muted, move |applied| {
        if applied || !muted {
            return;
        }
        let Some(core) = weak.upgrade() else { return };
        tauri::async_runtime::spawn(async move {
            let current = with_guest(&core, &label, |g| g.generations.is_current(generation));
            if current == Some(true) {
                log::warn!(target: LOG_TARGET, "ad guest {label} could not be muted; closed");
                close_guest(&core, &label, Closer::Host);
            }
        });
    });
}

/// Clears what the builder's `transparent` flag leaves of the guest's
/// background (B.3.4).
fn clear_guest_background<R: Runtime>(core: &Core<R>, webview: &Webview<R>) {
    let label = webview.label().to_owned();
    record(
        core,
        "wc-events.jsonl",
        || json!({ "kind": "transparent", "label": label, "type": "owadview" }),
    );
    let _ = crate::platform::webview::clear_background(webview, move |cleared| {
        crate::lab::record(
            "wc-events.jsonl",
            || json!({ "kind": "transparent-native", "label": label, "type": "owadview", "applied": cleared }),
        );
    });
}

/// Applies an input pass-through state to the guest's native view.
fn apply_passthrough<R: Runtime>(core: &Core<R>, webview: &Webview<R>, on: bool) {
    let label = webview.label().to_owned();
    record(
        core,
        "wc-events.jsonl",
        || json!({ "kind": "passthrough", "label": label, "type": "owadview", "on": on }),
    );
    let _ = crate::platform::webview::set_input_passthrough(webview, on, move |applied| {
        crate::lab::record(
            "wc-events.jsonl",
            || json!({ "kind": "passthrough-native", "label": label, "type": "owadview", "on": on, "applied": applied }),
        );
    });
}

/// Raises the newest performance guest of `window` above every child
/// webview of the window, after guest `mounted` was created there (B.3.4).
fn raise_performance_guest<R: Runtime>(core: &Core<R>, window: &str, mounted: &str) {
    let top = {
        let s = lock(&core.ads.state);
        s.guests
            .iter()
            .filter(|(_, g)| g.window == window && g.attributes.performance)
            .max_by_key(|(_, g)| g.seq)
            .map(|(l, _)| l.clone())
    };
    let Some(top) = top else { return };
    let Some(webview) = webview_of(core, &top) else {
        return;
    };
    record(
        core,
        "wc-events.jsonl",
        || json!({ "kind": "zorder", "label": top, "type": "owadview", "window": window, "after": mounted }),
    );
    let _ = crate::platform::webview::raise_to_top(&webview, move |is_top| {
        crate::lab::record(
            "wc-events.jsonl",
            || json!({ "kind": "zorder-native", "label": top, "type": "owadview", "top": is_top }),
        );
    });
}

/// Sends a host lifecycle event on the element's channel (B.3.5).
fn host_event<R: Runtime>(core: &Core<R>, label: &str, name: &str, data: Option<Value>) {
    let target = with_guest(core, label, |g| (g.channel.clone(), g.element_id.clone()));
    if let Some((channel, element_id)) = target {
        send_host(core, &channel, label, &element_id, name, data);
    }
}

fn send_host<R: Runtime>(
    core: &Core<R>,
    channel: &Channel<ChannelMessage>,
    label: &str,
    element_id: &str,
    name: &str,
    data: Option<Value>,
) {
    record(
        core,
        "ipc.jsonl",
        || json!({ "dir": "host->embedder", "via": "element-event", "label": label, "elementId": element_id, "name": name, "data": data }),
    );
    let _ = channel.send(ChannelMessage::new(name, data, EventSource::Host));
}

/// Calls the shim's host function `function` in guest `label` (D.5).
fn guest_call<R: Runtime>(core: &Core<R>, label: &str, function: &str, arg: &Value) {
    record(
        core,
        "ipc.jsonl",
        || json!({ "dir": "host->page", "via": "guest-call", "type": "owadview", "label": label, "function": function, "args": arg }),
    );
    let key = with_guest(core, label, |g| g.host_key.clone());
    if let (Some(key), Some(w)) = (key, webview_of(core, label)) {
        let _ = w.eval(host_call_script(&key, function, arg));
    }
}

/// Delivers one host message to guest `label` (D.5).
fn guest_deliver<R: Runtime>(core: &Core<R>, label: &str, kind: &str, data: Option<&Value>) {
    record(
        core,
        "ipc.jsonl",
        || json!({ "dir": "host->page", "via": "private-message", "type": "owadview", "label": label, "message": host_message(kind, data) }),
    );
    let key = with_guest(core, label, |g| g.host_key.clone());
    if let (Some(key), Some(w)) = (key, webview_of(core, label)) {
        let _ = w.eval(deliver_script(&key, kind, data));
    }
}

/// Delivers a host message to every existing guest (D.5).
fn deliver_to_all<R: Runtime>(core: &Core<R>, kind: &str, data: Option<&Value>) {
    let labels = lock(&core.ads.state).labels();
    for l in labels {
        guest_deliver(core, &l, kind, data);
    }
}

/// Tells the shim of `label` its visibility (§4.4.4). Sends only a change,
/// unless `force` (a new document).
fn sync_visibility<R: Runtime>(core: &Core<R>, label: &str, force: bool) {
    let now = core.now();
    let send = with_guest(core, label, |g| {
        let visible = g.computed_visible();
        let changed = std::mem::replace(&mut g.sent_visible, visible) != visible;
        if changed {
            g.hidden_at = (!visible).then_some(now);
            if visible && std::mem::take(&mut g.reload_held) {
                // Visible again before the hold ended: the reload the page
                // asked for runs at the next tick (D.5).
                g.reload_at = Some(now);
            }
        }
        (changed || force).then_some(visible)
    })
    .flatten();
    if let Some(visible) = send {
        guest_call(
            core,
            label,
            "setVisibility",
            &Value::from(if visible { "visible" } else { "hidden" }),
        );
    }
}

/// Tells the shim of `label` whether its window is focused (D.3
/// `hasWindowFocus`). Sends only a change, unless `force`.
fn sync_focus<R: Runtime>(core: &Core<R>, label: &str, force: bool) {
    let send = with_guest(core, label, |g| {
        let f = g.window_focused;
        (force || g.sent_focus != Some(f)).then(|| {
            g.sent_focus = Some(f);
            f
        })
    })
    .flatten();
    if let Some(f) = send {
        guest_call(core, label, "setEmbedderFocus", &Value::Bool(f));
    }
}

/// `adview_update`.
pub(crate) fn update<R: Runtime>(
    core: &Arc<Core<R>>,
    embedder: &Webview<R>,
    request: AdviewUpdate,
) -> Result<()> {
    let label = lock(&core.ads.state)
        .find(embedder.label(), &request.element_id)
        .ok_or_else(|| Error::not_found(format!("no mounted element {}", request.element_id)))?;
    if let Some(rect) = request.rect {
        let (dpr, iw) = with_guest(core, &label, |g| {
            (
                request.device_pixel_ratio.unwrap_or(g.device_pixel_ratio),
                request.inner_width.unwrap_or(g.inner_width),
            )
        })
        .unwrap_or((1.0, 1.0));
        if !valid_geometry(&rect, dpr, iw) {
            return Err(Error::invalid_argument("invalid geometry"));
        }
        let window = embedder.window();
        let bounds = placement(core, embedder, &window, &rect, dpr, iw);
        with_guest(core, &label, |g| {
            g.rect = rect;
            g.device_pixel_ratio = dpr;
            g.inner_width = iw;
            g.bounds = bounds;
        });
        if let Some(wv) = webview_of(core, &label) {
            let (x, y, w, h) = bounds;
            let _ = wv.set_position(LogicalPosition::new(x, y));
            let _ = wv.set_size(LogicalSize::new(w, h));
        }
        record(
            core,
            "wc-events.jsonl",
            || json!({ "kind": "bounds", "label": label, "type": "owadview", "bounds": [bounds.0, bounds.1, bounds.2, bounds.3] }),
        );
    }
    if let Some(visible) = request.visible {
        let (changed, minimized) = with_guest(core, &label, |g| {
            let changed = g.visible != visible;
            g.visible = visible;
            (changed, g.window_minimized)
        })
        .unwrap_or_default();
        // While minimized on Windows the webview stays hidden until the
        // restore (MINIMIZE_HIDES_NATIVELY).
        let held = visible && minimized && MINIMIZE_HIDES_NATIVELY;
        if changed
            && !held
            && let Some(wv) = webview_of(core, &label)
        {
            let _ = if visible { wv.show() } else { wv.hide() };
        }
        sync_visibility(core, &label, false);
    }
    if let Some(patch) = request.attributes {
        apply_attribute_patch(core, &label, patch);
    }
    Ok(())
}

fn apply_attribute_patch<R: Runtime>(core: &Core<R>, label: &str, patch: AdviewAttributesPatch) {
    let tracking = patch
        .custom_tracking
        .map(|v| if v.is_object() { v } else { Value::Null });
    with_guest(core, label, |g| {
        if let Some(t) = &tracking {
            g.attributes.custom_tracking = t.clone();
            g.tracking_changed = true;
        }
        if let Some(p) = &patch.pageurl {
            g.next_page_url = Some(p.clone());
            g.attributes.pageurl.clone_from(p);
        }
    });
    if let Some(t) = tracking {
        guest_deliver(core, label, "customTracking", Some(&t));
    }
    if let Some(p) = patch.pageurl {
        guest_call(core, label, "setNextPageUrl", &Value::from(p));
    }
}

/// `adview_unmount` (idempotent).
pub(crate) fn unmount<R: Runtime>(core: &Arc<Core<R>>, embedder: &str, element_id: &str) {
    let label = lock(&core.ads.state).find(embedder, element_id);
    if let Some(label) = label {
        close_guest(core, &label, Closer::App);
    }
}

/// Forgets guest `label` and closes its webview (on a runtime task); the
/// element gets `destroyed` when the plugin decided it ([`Closer::Host`]).
fn close_guest<R: Runtime>(core: &Arc<Core<R>>, label: &str, closer: Closer) {
    let removed = lock(&core.ads.state).guests.remove(label);
    let Some(guest) = removed else { return };
    crate::platform::gesture::unwatch(label);
    record(
        core,
        "wc-events.jsonl",
        || json!({ "kind": "closed", "label": label, "type": "owadview", "byHost": closer == Closer::Host }),
    );
    if closer == Closer::Host {
        send_host(
            core,
            &guest.channel,
            label,
            &guest.element_id,
            "destroyed",
            None,
        );
    }
    // The retained view is given back on the main thread.
    drop(guest);
    let weak = Arc::downgrade(core);
    let label = label.to_owned();
    tauri::async_runtime::spawn(async move {
        let Some(core) = weak.upgrade() else { return };
        // A recreate may have put a new guest under the label meanwhile.
        if lock(&core.ads.state).guests.contains_key(&label) {
            return;
        }
        if let Some(w) = webview_of(&core, &label) {
            close_webview(&w);
        }
    });
}

/// `adview_command` (B.3.3).
pub(crate) fn command<R: Runtime>(
    core: &Arc<Core<R>>,
    embedder: &str,
    element_id: &str,
    command: AdviewCommandName,
    args: &[Value],
) -> Result<()> {
    let label = lock(&core.ads.state)
        .find(embedder, element_id)
        .ok_or_else(|| Error::not_found(format!("no mounted element {element_id}")))?;
    match command {
        AdviewCommandName::SetAudioMuted => {
            let muted = args.first().and_then(Value::as_bool).unwrap_or(true);
            if let Some(w) = webview_of(core, &label) {
                mute_guest(core, &w, muted, "setAudioMuted");
            }
        }
        AdviewCommandName::Reload => reload_guest(core, &label),
        AdviewCommandName::SetPageUrl => {
            // The `pageurl` of the next load, as the attribute sets it, then
            // the private message ow-electron sends (observed).
            let url = args.first().cloned().unwrap_or(Value::Null);
            apply_attribute_patch(
                core,
                &label,
                AdviewAttributesPatch {
                    pageurl: Some(url.as_str().unwrap_or_default().to_owned()),
                    ..AdviewAttributesPatch::default()
                },
            );
            guest_deliver(core, &label, "setPageUrl", Some(&Value::Array(vec![url])));
        }
        AdviewCommandName::SendCommand => {
            guest_deliver(
                core,
                &label,
                "sendCommand",
                Some(&Value::Array(args.to_vec())),
            );
        }
    }
    Ok(())
}

/// Reloads a guest's ad document (a new load; `sessionTS` restarts): a
/// recreate on macOS within the rate guard (§4.4.6), else in place (a
/// native reload on Windows, a new shaped load elsewhere, which repeats the
/// document headers, D.8.3).
pub(crate) fn reload_guest<R: Runtime>(core: &Arc<Core<R>>, label: &str) {
    let now = core.now();
    let recreate_on_reload = core.identity.config.ads.recreate_on_reload;
    let plan = with_guest(core, label, |g| {
        if !g.navigated {
            return None;
        }
        g.begin_load(now);
        g.reload_at = None;
        g.reload_held = false;
        g.retry_at = None;
        Some(RECREATE_PLATFORM && recreate_on_reload && g.recreates.try_recreate(now))
    })
    .flatten();
    let Some(recreate) = plan else { return };
    record(
        core,
        "wc-events.jsonl",
        || json!({ "kind": "reload", "label": label, "type": "owadview", "recreate": recreate }),
    );
    if recreate {
        let core = Arc::clone(core);
        let label = label.to_owned();
        tauri::async_runtime::spawn(async move {
            recreate_guest(&core, &label).await;
        });
    } else if cfg!(windows) {
        if let Some(w) = webview_of(core, label) {
            let _ = w.reload();
        }
    } else {
        navigate_guest(core, label);
    }
}

/// Runs a page-requested reload once it is due.
fn run_due_reload<R: Runtime>(core: &Arc<Core<R>>, label: &str) {
    let now = core.now();
    let due = with_guest(core, label, |g| {
        g.reload_at.is_some_and(|t| now >= t) && g.reload_at.take().is_some()
    })
    .unwrap_or(false);
    if due {
        reload_guest(core, label);
    }
}

/// Waits for a oneshot answer at most [`SNAPSHOT_WAIT_MS`].
async fn bounded<T>(rx: oneshot::Receiver<T>) -> Option<T> {
    tokio::time::timeout(Duration::from_millis(SNAPSHOT_WAIT_MS), rx)
        .await
        .ok()
        .and_then(Result::ok)
}

/// The recreate of §4.4.6 (macOS): a new native webview under the same
/// label, configuration, place, stacking, mute, transparency and
/// pass-through, carrying the top frame's `sessionStorage`. No
/// `did-attach`, no 400025. Events of the old instance are dropped from
/// the retire on.
#[expect(
    clippy::too_many_lines,
    reason = "the recreate sequence of DESIGN §4.4.6 in its order"
)]
async fn recreate_guest<R: Runtime>(core: &Arc<Core<R>>, label: &str) {
    let Some(old) = webview_of(core, label) else {
        return;
    };
    let Some(generation) = with_guest(core, label, |g| g.generations.retire()) else {
        return;
    };
    let (tx, rx) = oneshot::channel();
    let posted = crate::platform::webview::eval_for_string(&old, snapshot_script(), move |a| {
        let _ = tx.send(a);
    })
    .is_ok();
    let answer = if posted {
        bounded(rx).await.flatten()
    } else {
        None
    };
    let prelude = match snapshot_outcome(answer.as_deref()) {
        Snapshot::TooBig => {
            log::debug!(target: LOG_TARGET, "{label}: sessionStorage too large to carry; reloading in place");
            with_guest(core, label, |g| g.generations.go_live(generation));
            navigate_guest(core, label);
            return;
        }
        Snapshot::Carry(json) => restore_prelude(SESSION_RESTORE_JS, &json),
        Snapshot::Nothing => None,
    };
    let anchor = ViewAnchor::default();
    let (tx, rx) = oneshot::channel();
    if crate::platform::webview::read_anchor(&old, &anchor, move || {
        let _ = tx.send(());
    })
    .is_ok()
    {
        let _ = bounded(rx).await;
    }
    let state = with_guest(core, label, |g| {
        if !g.generations.is_current(generation) {
            return None;
        }
        // Released on the main thread.
        g.native = None;
        Some((
            g.window.clone(),
            g.bounds,
            g.visible && !(MINIMIZE_HIDES_NATIVELY && g.window_minimized),
            g.passthrough,
            Arc::clone(&g.init_script),
        ))
    })
    .flatten();
    let Some((window_label, bounds, shown, passthrough, init_script)) = state else {
        return;
    };
    crate::platform::gesture::unwatch(label);
    let Some(window) = crate::compat::window(&core.app, &window_label) else {
        return;
    };
    close_webview(&old);
    let mut scripts = vec![init_script.to_string()];
    if let Some(p) = &prelude {
        scripts.push(p.clone());
    }
    let spec = spec_for(core, label, scripts);
    match build_guest(core, &window, &spec, bounds) {
        Ok(webview) => {
            let traced = label.to_owned();
            let _ = crate::platform::webview::place_below_anchor(
                &webview,
                &anchor,
                move |had_above, placed| {
                    crate::lab::record(
                        "wc-events.jsonl",
                        || json!({ "kind": "recreated-zorder", "label": traced, "type": "owadview", "hadAbove": had_above, "placed": placed }),
                    );
                },
            );
            with_guest(core, label, |g| {
                g.generations.go_live(generation);
                g.restore_pending = prelude.is_some();
            });
            prepare_guest(core, &webview, shown, passthrough, "recreate");
            record(
                core,
                "wc-events.jsonl",
                || json!({ "kind": "recreated", "label": label, "type": "owadview", "generation": generation, "carried": prelude.is_some() }),
            );
            navigate_guest(core, label);
        }
        Err(err) => {
            log::warn!(target: LOG_TARGET, "ad guest {label} could not be recreated: {err}");
            let removed = lock(&core.ads.state).guests.remove(label);
            if let Some(g) = removed {
                record(
                    core,
                    "wc-events.jsonl",
                    || json!({ "kind": "closed", "label": label, "type": "owadview", "byHost": true }),
                );
                send_host(
                    core,
                    &g.channel,
                    label,
                    &g.element_id,
                    "did-fail-load",
                    Some(fail_load_data(-2, "ERR_FAILED", ADVIEW_URL, true)),
                );
                send_host(core, &g.channel, label, &g.element_id, "destroyed", None);
            }
        }
    }
}

/// A platform-reported main-frame load failure (D.7): retried every
/// `ads.loadErrorRetryMs`, without analytics.
pub(crate) fn guest_load_failed<R: Runtime>(
    core: &Arc<Core<R>>,
    label: &str,
    error_code: i64,
    description: &str,
    url: &str,
) {
    let now = core.now();
    let retry = core.identity.config.ads.load_error_retry_ms;
    let known = with_guest(core, label, |g| {
        if !g.generations.admits() {
            return false;
        }
        g.ready = false;
        g.nav_started_ms = None;
        g.finish_pending = None;
        g.retry_at = Some(now + retry);
        true
    })
    .unwrap_or(false);
    if !known {
        return;
    }
    record(
        core,
        "wc-events.jsonl",
        || json!({ "kind": "did-fail-load", "label": label, "type": "owadview", "errorCode": error_code, "description": description, "url": url }),
    );
    host_event(
        core,
        label,
        "did-fail-load",
        Some(fail_load_data(error_code, description, url, true)),
    );
}

/// A guest crashed (D.7, E.2 #8): `render-process-gone`, the crash Counter,
/// the reload (a recreate on macOS), then Kind 400024; the guest is closed
/// after `ads.maxRecoveries` recoveries. Crashes within 10 s of the last
/// load or recovery send no analytics.
pub(crate) fn guest_crashed<R: Runtime>(
    core: &Arc<Core<R>>,
    label: &str,
    reason: GoneReason,
    exit_code: i64,
) {
    let now = core.now();
    let max = core.identity.config.ads.max_recoveries;
    let decision = with_guest(core, label, |g| {
        let secs = session_secs(now, g.last_load_ms);
        let recover = may_recover(max, g.recoveries);
        if recover {
            g.recoveries += 1;
            g.last_load_ms = now;
        }
        (secs, recover)
    });
    let Some((secs, recover)) = decision else {
        return;
    };
    record(
        core,
        "wc-events.jsonl",
        || json!({ "kind": "render-process-gone", "label": label, "type": "owadview", "reason": reason.as_str(), "exitCode": exit_code, "sessionTS": secs, "recover": recover }),
    );
    host_event(
        core,
        label,
        "render-process-gone",
        Some(gone_data(reason, exit_code)),
    );
    if !recover {
        log::warn!(target: LOG_TARGET, "ad guest {label} closed after its recoveries (ads.maxRecoveries)");
        close_guest(core, label, Closer::Host);
        return;
    }
    core.analytics.guest_crash_counter(secs, reason.as_str());
    reload_guest(core, label);
    core.analytics.guest_crash_stats(secs, reason.as_str());
}

/// The origins a guest frame may never load besides `localhost` hosts
/// (§4.4.9): the app's own origins.
fn blocked_origins<R: Runtime>(core: &Core<R>) -> Vec<String> {
    let id = &core.identity;
    let mut origins = vec![crate::commands::origin_of(&id.app_origin)];
    origins.extend(
        id.dev_origin
            .iter()
            .map(|o| o.trim_end_matches('/').to_owned()),
    );
    origins.extend(
        id.config
            .ads
            .allowed_embedder_origins
            .iter()
            .map(|o| o.trim_end_matches('/').to_owned()),
    );
    origins
}

/// The navigation policy of guest `label` (§4.4.9, D.7).
pub(crate) fn guest_navigation<R: Runtime>(core: &Arc<Core<R>>, label: &str, url: &Url) -> bool {
    let known = lock(&core.ads.state).guests.contains_key(label);
    let blocked = blocked_origins(core);
    let refs: Vec<&str> = blocked.iter().map(String::as_str).collect();
    let mut ok = known && frame_url_allowed(url, &refs);
    // Windows: the hook sees top-level navigations only. One off Overwolf
    // is cancelled here; the platform hook reports it with its activation
    // flag (§4.9).
    if ok && cfg!(windows) && matches!(url.scheme(), "http" | "https") && !is_overwolf_url(url) {
        ok = false;
    }
    if !ok {
        log::debug!(target: LOG_TARGET, "{label}: navigation to a {} URL refused", url.scheme());
        record(
            core,
            "wc-events.jsonl",
            || json!({ "kind": "navigation-refused", "label": label, "type": "owadview", "scheme": url.scheme(), "known": known }),
        );
    }
    ok
}

/// A page load of any webview (dispatch: all labels).
pub(crate) fn page_load<R: Runtime>(
    core: &Arc<Core<R>>,
    webview: &Webview<R>,
    event: PageLoadEvent,
    url: &Url,
) {
    let label = webview.label();
    if label.starts_with(ADVIEW_LABEL_PREFIX) {
        guest_page_load(core, label, event, url);
        return;
    }
    if crate::config::is_reserved_label(label) || event != PageLoadEvent::Started {
        return;
    }
    // A new document in an embedder: its elements are gone (§4.4.8).
    let labels = lock(&core.ads.state).of_embedder(label);
    for l in labels {
        close_guest(core, &l, Closer::App);
    }
}

/// A page load of guest `label` (B.3.5, D.5). `did-finish-load` follows
/// the shim's `dom-ready`, as in ow-electron.
fn guest_page_load<R: Runtime>(core: &Arc<Core<R>>, label: &str, event: PageLoadEvent, url: &Url) {
    let live = with_guest(core, label, |g| g.generations.admits()).unwrap_or(false);
    if !live || url.scheme() == "about" {
        return;
    }
    let now = core.now();
    if !is_overwolf_url(url) {
        if event == PageLoadEvent::Started && matches!(url.scheme(), "http" | "https") {
            // A top-level navigation off Overwolf (D.7, macOS; Windows
            // cancels it before it starts): opens externally after native
            // activation, and the guest goes back to the ad page.
            open_external(core, label, url, None, "navigation");
            with_guest(core, label, |g| g.begin_load(now));
            navigate_guest(core, label);
        }
        return;
    }
    if event == PageLoadEvent::Started {
        with_guest(core, label, |g| {
            g.dom_ready = false;
            g.finish_pending = None;
        });
        return;
    }
    let info = with_guest(core, label, |g| {
        g.loads += 1;
        g.last_load_ms = now;
        g.retry_at = None;
        let finish_now = g.dom_ready;
        if !finish_now {
            g.finish_pending = Some(now);
        }
        (
            finish_now,
            (g.loads > 1 && g.tracking_changed).then(|| g.attributes.custom_tracking.clone()),
            std::mem::take(&mut g.restore_pending),
        )
    });
    let Some((finish_now, tracking, restore)) = info else {
        return;
    };
    if let Some(w) = webview_of(core, label) {
        // ow-electron mutes, and signals visibility and focus, on every load.
        mute_guest(core, &w, true, "load");
        sync_visibility(core, label, true);
        let focused = w.window().is_focused().unwrap_or(false);
        with_guest(core, label, |g| g.window_focused = focused);
        sync_focus(core, label, true);
        if let Some(t) = tracking {
            guest_deliver(core, label, "customTracking", Some(&t));
        }
        if restore {
            // One-shot (W0c ruling 8): later loads start without it.
            let traced = label.to_owned();
            let _ = crate::platform::webview::remove_user_scripts_marked(
                &w,
                SESSION_RESTORE_MARKER,
                move |removed| {
                    crate::lab::record(
                        "wc-events.jsonl",
                        || json!({ "kind": "restore-removed", "label": traced, "type": "owadview", "removed": removed }),
                    );
                },
            );
        }
    }
    if finish_now {
        host_event(core, label, "did-finish-load", None);
    }
}

/// What a guest event leads to, decided under the lock.
enum Next {
    Nothing,
    /// Forward to the element; `true` when the guest's pass-through ends
    /// first (its first `performance_ad_loaded`).
    Forward(Option<Value>, bool),
    Reload,
    Close,
    Crash,
    Mute(bool),
    /// `dom-ready`, then `did-finish-load` when the load already finished.
    DomReady(bool),
    Ready(Option<String>),
    Log(&'static str),
    /// A page reload, due this many milliseconds from now.
    ScheduleReload(u64),
}

/// `adview_event` from guest `label` (A.2.6, D.4). The caller label is
/// authoritative; a claimed slot id is only logged. Events of a retired
/// native instance are dropped (§4.4.6.3).
pub(crate) fn guest_event<R: Runtime>(
    core: &Arc<Core<R>>,
    label: &str,
    slot_id: Option<&str>,
    name: &str,
    data: Option<Value>,
) -> Result<()> {
    guest_event_at(core, label, slot_id, name, data, core.now())
}

/// [`guest_event`] at session time `now`.
fn guest_event_at<R: Runtime>(
    core: &Arc<Core<R>>,
    label: &str,
    slot_id: Option<&str>,
    name: &str,
    data: Option<Value>,
    now: u64,
) -> Result<()> {
    if !valid_event_name(name) {
        return Err(Error::invalid_argument("invalid event name"));
    }
    record(
        core,
        "ipc.jsonl",
        || json!({ "dir": "page->host", "via": "adview_event", "type": "owadview", "label": label, "channel": name, "data": data }),
    );
    let (data, bytes) = match data {
        Some(d) => {
            let (v, n) = cap_event_data(d);
            (Some(v), n)
        }
        None => (None, 0),
    };
    if slot_id.is_some_and(|claimed| claimed != label) {
        log::debug!(target: LOG_TARGET, "ad guest {label} sent an event for another slot id");
    }
    let internal = InternalEvent::parse(name);
    let next = {
        let mut s = lock(&core.ads.state);
        let Some(g) = s.guests.get_mut(label) else {
            return Err(Error::not_found(format!("no ad guest {label}")));
        };
        if g.generations.admits() {
            match g.limiter.admit(now, bytes) {
                Admission::Admit => decide(g, internal, name, data, now),
                Admission::Drop => Next::Nothing,
                Admission::Reload => Next::Reload,
                Admission::Close => Next::Close,
            }
        } else {
            Next::Nothing
        }
    };
    run_next(core, label, name, next);
    Ok(())
}

/// The decision for one admitted guest event.
fn decide(
    g: &mut Guest,
    internal: Option<InternalEvent>,
    name: &str,
    data: Option<Value>,
    now: u64,
) -> Next {
    match internal {
        None if InternalEvent::is_reserved(name) => Next::Nothing,
        None => {
            // The interstitial turns modal at its first
            // `performance_ad_loaded` (B.3.4, observed).
            let modal = g.passthrough && name == crate::ads::MODAL_EVENT;
            if modal {
                g.passthrough = false;
            }
            Next::Forward(data, modal)
        }
        Some(InternalEvent::Ready) => {
            g.ready = true;
            g.nav_started_ms = None;
            let page_url = data
                .as_ref()
                .and_then(|d| d.get("pageUrl"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            Next::Ready(
                g.next_page_url
                    .clone()
                    .filter(|next| page_url.as_deref() != Some(next.as_str())),
            )
        }
        // Never an authority (§4.9): native input arms a guest.
        Some(InternalEvent::Gesture | InternalEvent::Focus) => Next::Nothing,
        Some(InternalEvent::SetMute) => Next::Mute(
            data.as_ref()
                .and_then(|d| d.get("muted"))
                .and_then(Value::as_bool)
                .unwrap_or(true),
        ),
        Some(InternalEvent::ApplySetting) => {
            if std::mem::replace(&mut g.apply_setting_logged, true) {
                Next::Nothing
            } else {
                Next::Log("an ad guest setting was requested; ow-tauri applies none (D.4)")
            }
        }
        Some(InternalEvent::Crash) => Next::Crash,
        Some(InternalEvent::Reload) => {
            let held = g
                .hidden_at
                .filter(|_| !g.sent_visible)
                .map(|h| h + HIDDEN_RELOAD_HOLD_MS)
                .filter(|&t| t > now + RELOAD_DELAY_MS);
            let at = held.unwrap_or(now + RELOAD_DELAY_MS);
            g.reload_at = Some(at);
            g.reload_held = held.is_some();
            Next::ScheduleReload(at - now)
        }
        Some(InternalEvent::DomReady) => {
            g.dom_ready = true;
            Next::DomReady(g.finish_pending.take().is_some())
        }
    }
}

fn run_next<R: Runtime>(core: &Arc<Core<R>>, label: &str, name: &str, next: Next) {
    match next {
        Next::Nothing => {}
        Next::Forward(data, modal) => {
            if modal && let Some(w) = webview_of(core, label) {
                // Natively first, so the guest takes clicks by the time the
                // element turns modal in the page.
                apply_passthrough(core, &w, false);
            }
            let channel = with_guest(core, label, |g| g.channel.clone());
            if let Some(channel) = channel {
                let _ = channel.send(ChannelMessage::new(name, data, EventSource::Guest));
            }
        }
        Next::Reload => {
            log::warn!(target: LOG_TARGET, "ad guest {label} over its message limit for 10 s; reloading");
            reload_guest(core, label);
        }
        Next::Close => {
            log::warn!(target: LOG_TARGET, "ad guest {label} over its message limit again; closing");
            close_guest(core, label, Closer::Host);
        }
        Next::Crash => guest_crashed(core, label, GoneReason::Killed, 0),
        Next::Mute(muted) => {
            if let Some(w) = webview_of(core, label) {
                mute_guest(core, &w, muted, "page");
            }
        }
        Next::DomReady(finish) => {
            host_event(core, label, "dom-ready", None);
            if finish {
                host_event(core, label, "did-finish-load", None);
            }
        }
        Next::Log(message) => log::debug!(target: LOG_TARGET, "{message}"),
        Next::Ready(page_url) => {
            if let Some(p) = page_url {
                guest_call(core, label, "setNextPageUrl", &Value::from(p));
            }
        }
        Next::ScheduleReload(delay_ms) => {
            // A one-shot timer: the reload follows ~70 ms later (or when a
            // hidden page's hold ends), not at the next 250 ms tick.
            let weak = Arc::downgrade(core);
            let label = label.to_owned();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                if let Some(core) = weak.upgrade() {
                    run_due_reload(&core, &label);
                }
            });
        }
    }
}

/// A popup (`on_new_window`) of guest `label`: always denied. On macOS
/// the system browser opens after native activation; on Windows the
/// platform hook reports the popup with WebView2's activation flag.
fn new_window_handler<R: Runtime>(
    core: &Arc<Core<R>>,
    label: &str,
) -> impl Fn(Url, NewWindowFeatures) -> NewWindowResponse<R> + Send + 'static {
    let weak: Weak<Core<R>> = Arc::downgrade(core);
    let label = label.to_owned();
    move |url, _features| {
        if !cfg!(windows)
            && let Some(core) = weak.upgrade()
        {
            open_external(&core, &label, &url, None, "popup");
        }
        NewWindowResponse::Deny
    }
}

/// Opens `url` from guest `label` in the system browser when the guest's
/// activation and the per-guest and per-app caps allow it (§4.9, D.7);
/// `user_initiated` is the platform's own flag (`None`: the native arming
/// of the gesture monitor). Each open sends `ad-clicked` to the guest and
/// to the element.
fn open_external<R: Runtime>(
    core: &Arc<Core<R>>,
    label: &str,
    url: &Url,
    user_initiated: Option<bool>,
    what: &str,
) {
    let now = core.now();
    let app_cap = core
        .identity
        .config
        .ads
        .guest_limits
        .external_opens_per_minute_app;
    let decision = {
        let mut s = lock(&core.ads.state);
        let state = &mut *s;
        let app = state
            .app_opens
            .get_or_insert_with(|| AppOpenCap::new(app_cap));
        state.guests.get_mut(label).map(|g| {
            if g.generations.admits() {
                g.budget.try_open_capped(app, now, url, user_initiated)
            } else {
                Err(crate::ads::OpenRefusal::NoGesture)
            }
        })
    };
    match decision {
        None => return,
        Some(Err(reason)) => {
            log::debug!(target: LOG_TARGET, "ad guest {what} to a {} URL refused ({reason:?})", url.scheme());
            record(
                core,
                "wc-events.jsonl",
                || json!({ "kind": "open-refused", "label": label, "type": "owadview", "what": what, "reason": format!("{reason:?}") }),
            );
            return;
        }
        Some(Ok(())) => {}
    }
    record(
        core,
        "wc-events.jsonl",
        || json!({ "kind": "open-external", "label": label, "type": "owadview", "what": what, "url": url.as_str() }),
    );
    if !open_in_browser(core, url) {
        return;
    }
    guest_deliver(core, label, "ad-clicked", Some(&Value::from(url.as_str())));
    host_event(
        core,
        label,
        "ad-clicked",
        Some(json!({ "url": url.as_str() })),
    );
}

/// Opens `url` in the system browser; `true` when it was opened or held
/// back on purpose (the invisible lab, a host without OS queries).
fn open_in_browser<R: Runtime>(core: &Core<R>, url: &Url) -> bool {
    if !core.options.os_queries || crate::lab::block_os_surface("browser", || json!(url.as_str())) {
        return true;
    }
    match tauri_plugin_opener::open_url(url.as_str(), None::<&str>) {
        Ok(()) => true,
        Err(err) => {
            log::debug!(target: LOG_TARGET, "opening an ad link failed: {err}");
            false
        }
    }
}

/// Platform reports of a guest.
struct Reports<R: Runtime>(Weak<Core<R>>);

impl<R: Runtime> GuestReports for Reports<R> {
    fn crashed(&self, label: &str, reason: GoneReason, exit_code: i64) {
        if let Some(core) = self.0.upgrade() {
            guest_crashed(&core, label, reason, exit_code);
        }
    }

    fn load_failed(&self, label: &str, error_code: i64, description: &str, url: &str) {
        if let Some(core) = self.0.upgrade() {
            guest_load_failed(&core, label, error_code, description, url);
        }
    }

    fn popup(&self, label: &str, url: &str, user_initiated: bool) {
        if let (Some(core), Ok(url)) = (self.0.upgrade(), Url::parse(url)) {
            open_external(&core, label, &url, Some(user_initiated), "popup");
        }
    }

    fn top_navigation(
        &self,
        label: &str,
        url: &str,
        user_initiated: bool,
        input_age_ms: Option<u64>,
    ) {
        let (Some(core), Ok(url)) = (self.0.upgrade(), Url::parse(url)) else {
            return;
        };
        let window = core.identity.config.ads.guest_limits.activation_window_ms;
        let active = user_initiated || input_age_ms.is_some_and(|age| age <= window);
        open_external(&core, label, &url, Some(active), "navigation");
    }

    fn frame_blocked(&self, label: &str, url: &str) {
        if let Some(core) = self.0.upgrade() {
            let scheme = url.split(':').next().unwrap_or_default().to_owned();
            log::debug!(target: LOG_TARGET, "{label}: a {scheme} frame was refused");
            record(
                &core,
                "wc-events.jsonl",
                || json!({ "kind": "navigation-refused", "label": label, "type": "owadview", "scheme": scheme, "known": true }),
            );
        }
    }
}

/// A window event (dispatch: every window).
pub(crate) fn window_event<R: Runtime>(core: &Arc<Core<R>>, label: &str, event: &WindowEvent) {
    match event {
        WindowEvent::Focused(focused) => {
            let labels = {
                let mut s = lock(&core.ads.state);
                let labels = s.of_window(label);
                for l in &labels {
                    if let Some(g) = s.guests.get_mut(l) {
                        g.window_focused = *focused;
                    }
                }
                labels
            };
            if labels.is_empty() {
                return;
            }
            let weak = Arc::downgrade(core);
            tauri::async_runtime::spawn(async move {
                if let Some(core) = weak.upgrade() {
                    for l in labels {
                        sync_focus(&core, &l, false);
                    }
                }
            });
        }
        WindowEvent::CloseRequested { .. } => close_requested(core, label),
        WindowEvent::Destroyed => window_destroyed(core, label),
        _ => {}
    }
}

/// `CloseRequested` of window `label` (Windows, W0c ruling 2): its guests'
/// documents turn hidden (no `window-hidden`); after [`CLOSE_GRACE_MS`] a
/// window that is still there (a prevented close) gets its guests'
/// real visibility back.
pub(crate) fn close_requested<R: Runtime>(core: &Arc<Core<R>>, label: &str) {
    if CLOSE_HIDE != CloseHide::AtCloseRequested {
        return;
    }
    let labels = {
        let mut s = lock(&core.ads.state);
        let labels = s.of_window(label);
        for l in &labels {
            if let Some(g) = s.guests.get_mut(l) {
                g.closing_hidden = true;
            }
        }
        labels
    };
    if labels.is_empty() {
        return;
    }
    let weak = Arc::downgrade(core);
    let window = label.to_owned();
    tauri::async_runtime::spawn(async move {
        if let Some(core) = weak.upgrade() {
            for l in &labels {
                sync_visibility(&core, l, false);
            }
        }
        tokio::time::sleep(Duration::from_millis(CLOSE_GRACE_MS)).await;
        let Some(core) = weak.upgrade() else { return };
        if crate::compat::window(&core.app, &window).is_none() {
            return;
        }
        for l in &labels {
            with_guest(&core, l, |g| g.closing_hidden = false);
            sync_visibility(&core, l, false);
        }
    });
}

/// `Destroyed` of window `label`: on macOS its visible guests' documents
/// turn hidden first through the retained native views (W0c ruling 2);
/// then every guest of the window is closed.
fn window_destroyed<R: Runtime>(core: &Arc<Core<R>>, label: &str) {
    let labels = lock(&core.ads.state).of_window(label);
    if CLOSE_HIDE == CloseHide::AtDestroyed {
        for l in &labels {
            let hide = with_guest(core, l, |g| {
                if !std::mem::replace(&mut g.sent_visible, false) {
                    return None;
                }
                Some((
                    g.native.take(),
                    host_call_script(&g.host_key, "setVisibility", &Value::from("hidden")),
                ))
            })
            .flatten();
            if let Some((native, script)) = hide {
                record(
                    core,
                    "ipc.jsonl",
                    || json!({ "dir": "host->page", "via": "guest-call", "type": "owadview", "label": l, "function": "setVisibility", "args": "hidden", "at": "destroyed" }),
                );
                if let Some(native) = native {
                    native.eval_then_release(script);
                }
            }
        }
    }
    for l in labels {
        close_guest(core, &l, Closer::App);
    }
}

/// The window poll listener (main thread): window state edges, then the
/// guests' timers.
pub(crate) fn on_poll<R: Runtime>(
    core: &Arc<Core<R>>,
    observations: &[WindowObservation],
    now: u64,
) {
    for o in observations {
        let labels = lock(&core.ads.state).of_window(&o.label);
        let hidden = !o.visible && !o.minimized;
        for l in labels {
            apply_window_state(core, &l, hidden, o.minimized);
        }
    }
    tick(core, now);
}

/// A window poll's view of guest `label`'s window (D.5): on hide,
/// `window-hidden` then the visibility; on minimize, the visibility, then
/// `window-minimized` and (not on Windows) `window-hidden`, with the
/// webview hidden natively on Windows until the restore.
fn apply_window_state<R: Runtime>(core: &Arc<Core<R>>, label: &str, hidden: bool, minimized: bool) {
    let change = with_guest(core, label, |g| {
        let was_hidden = std::mem::replace(&mut g.window_hidden, hidden);
        let was_minimized = std::mem::replace(&mut g.window_minimized, minimized);
        (was_hidden, was_minimized, g.visible)
    });
    let Some((was_hidden, was_minimized, visible)) = change else {
        return;
    };
    let minimize_changed = was_minimized != minimized;
    if minimize_changed
        && MINIMIZE_HIDES_NATIVELY
        && let Some(show) = native_visibility_on_minimize(minimized, visible)
        && let Some(webview) = webview_of(core, label)
    {
        record(
            core,
            "wc-events.jsonl",
            || json!({ "kind": "native-visibility", "label": label, "type": "owadview", "visible": show }),
        );
        tauri::async_runtime::spawn(async move {
            let _ = if show { webview.show() } else { webview.hide() };
        });
    }
    if hidden && !was_hidden {
        guest_deliver(core, label, "window-hidden", None);
    }
    sync_visibility(core, label, false);
    if minimize_changed && minimized {
        guest_deliver(core, label, WINDOW_MINIMIZED, None);
        if MINIMIZE_SENDS_WINDOW_HIDDEN && !was_hidden {
            guest_deliver(core, label, "window-hidden", None);
        }
    }
}

/// Timer step (every window poll): due reloads and retries, the readiness
/// timeout, `did-finish-load` without `dom-ready`, dropped-message logs,
/// and guests whose embedder webview is gone.
pub(crate) fn tick<R: Runtime>(core: &Arc<Core<R>>, now: u64) {
    let retry_ms = core.identity.config.ads.load_error_retry_ms;
    let mut reload = Vec::new();
    let mut failed = Vec::new();
    let mut finished = Vec::new();
    let mut logs = Vec::new();
    let embedders: Vec<(String, String)> = {
        let mut s = lock(&core.ads.state);
        for (label, g) in &mut s.guests {
            if !g.navigated || !g.generations.admits() {
                continue;
            }
            if g.finish_pending
                .is_some_and(|t| now.saturating_sub(t) >= FINISH_WAIT_MS)
            {
                g.finish_pending = None;
                finished.push(label.clone());
            }
            if g.reload_at.is_some_and(|t| now >= t) || g.retry_at.is_some_and(|t| now >= t) {
                g.reload_at = None;
                g.retry_at = None;
                reload.push(label.clone());
            } else if !g.ready
                && g.retry_at.is_none()
                && g.nav_started_ms
                    .is_some_and(|t| now.saturating_sub(t) >= READY_TIMEOUT_MS)
            {
                g.nav_started_ms = None;
                g.retry_at = Some(now + retry_ms);
                failed.push(label.clone());
            }
            if let Some(n) = g.limiter.take_drop_log(now) {
                logs.push((label.clone(), n));
            }
        }
        s.guests
            .iter()
            .map(|(l, g)| (l.clone(), g.embedder.clone()))
            .collect()
    };
    for (label, embedder) in embedders {
        if webview_of(core, &embedder).is_none() {
            close_guest(core, &label, Closer::App);
        }
    }
    for l in reload {
        reload_guest(core, &l);
    }
    for l in failed {
        host_event(
            core,
            &l,
            "did-fail-load",
            Some(fail_load_data(-7, "ERR_TIMED_OUT", ADVIEW_URL, true)),
        );
    }
    for l in finished {
        host_event(core, &l, "did-finish-load", None);
    }
    for (l, n) in logs {
        log::warn!(target: LOG_TARGET, "ad guest {l}: {n} messages over the limit dropped");
    }
}
