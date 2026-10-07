//! The ads host (CONTRACT A.2.5, A.2.6, B.3, D): one native child webview
//! per `<owadview>`, its first navigation after consent (D.6.5), the guest
//! messages, clicks, limits, crash recovery and load-error retries (D.4,
//! D.7), and request shaping (D.8).

use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

use serde_json::{Map, Value, json};
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{LogicalPosition, LogicalSize, Manager, Runtime, Webview, WebviewBuilder, WebviewUrl};
use url::Url;

use super::Host;
use crate::ads::{
    ADVIEW_CONFIG_TOKEN, ADVIEW_URL, Admission, AdviewAttributes, AdviewAttributesPatch,
    AdviewCommandName, AdviewEvent, AdviewMount, AdviewRect, AdviewUpdate, CONSENT_WAIT_MS,
    GoneReason, GuestFacts, GuestLimiter, InternalEvent, OpenBudget, cap_event_data,
    deliver_script, fail_load_data, gone_data, guest_config, guest_label, host_call_script,
    is_overwolf_url, logical_rect, may_recover, session_secs, splice_config, valid_element_id,
    valid_event_name, valid_rect,
};
use crate::error::Error;
use crate::ipc::messages::HostMessage;
use crate::platform::webview::{GuestReports, Shaping};
use crate::state::log::LogLevel;

/// The guest shim (D.1), built from `packages/ow-tauri/src/guest/adview-host.ts`.
pub(crate) const ADVIEW_HOST_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/adview-host.js"));

/// A guest whose document has not reported `__host:ready` this long after
/// its navigation started is treated as a failed main-frame load where the
/// platform reports none (macOS).
const READY_TIMEOUT_MS: u64 = 20_000;

/// ow-electron reloads a guest about 70 ms after `__overwolf__.reload()`
/// (D.3).
const RELOAD_DELAY_MS: u64 = 70;

/// A guest's `did-finish-load` waits this long for the shim's `dom-ready`
/// (ow-electron reports `dom-ready` first); after that it goes alone (a
/// document without the shim).
const FINISH_WAIT_MS: u64 = 1_000;

/// One ad guest.
#[derive(Debug)]
#[expect(clippy::struct_excessive_bools, reason = "independent per-guest flags")]
pub(crate) struct Guest {
    pub(crate) embedder: String,
    pub(crate) element_id: String,
    pub(crate) attributes: AdviewAttributes,
    pub(crate) rect: AdviewRect,
    /// The element is visible (B.3.4).
    pub(crate) visible: bool,
    pub(crate) mounted_ms: u64,
    /// The first navigation was started (D.6.5).
    pub(crate) navigated: bool,
    /// When the current document's navigation started.
    pub(crate) nav_started_ms: Option<u64>,
    /// `__host:ready` arrived for the current document.
    pub(crate) ready: bool,
    /// `__host:domReady` arrived for the current document.
    pub(crate) dom_ready: bool,
    /// The platform reported the current document loaded before
    /// `dom-ready` (when): `did-finish-load` waits for it.
    pub(crate) finish_pending: Option<u64>,
    /// The random window property of the shim's host API (D.5).
    pub(crate) host_key: String,
    /// The embedder window is hidden.
    pub(crate) embedder_hidden: bool,
    /// The embedder window is minimized.
    pub(crate) embedder_minimized: bool,
    /// The visibility the shim reports to the page (`visible` when true).
    pub(crate) sent_visible: bool,
    /// Last load or recovery (`sessionTS`, E.2 #8).
    pub(crate) last_load_ms: u64,
    pub(crate) loads: u32,
    pub(crate) recoveries: u32,
    pub(crate) limiter: GuestLimiter,
    pub(crate) budget: OpenBudget,
    /// The live `customTracking` changed after mount: it is resent after
    /// every later reload (D.5).
    pub(crate) tracking_changed: bool,
    /// A `pageurl` for the next load (B.3.3).
    pub(crate) next_page_url: Option<String>,
    pub(crate) reload_at: Option<u64>,
    pub(crate) retry_at: Option<u64>,
    pub(crate) apply_setting_logged: bool,
}

impl Guest {
    /// A new document load starts: readiness and `dom-ready` start over.
    pub(crate) fn begin_load(&mut self, now: u64) {
        self.ready = false;
        self.dom_ready = false;
        self.finish_pending = None;
        self.nav_started_ms = Some(now);
    }
}

/// The ads service state, inside the host's core lock.
#[derive(Debug, Default)]
pub(crate) struct AdsCore {
    pub(crate) guests: BTreeMap<String, Guest>,
    pub(crate) next: u32,
    pub(crate) system_info: Option<Value>,
    /// What a host without OS queries (Tauri's mock runtime) sent to its
    /// guests and did to them natively, in order; tests read it. Always
    /// empty in an app.
    pub(crate) test_trace: Vec<Value>,
}

impl AdsCore {
    /// The guest label of `element_id` in `embedder`.
    pub(crate) fn find(&self, embedder: &str, element_id: &str) -> Option<String> {
        self.guests
            .iter()
            .find(|(_, g)| g.embedder == embedder && g.element_id == element_id)
            .map(|(l, _)| l.clone())
    }

    fn labels_of(&self, embedder: &str) -> Vec<String> {
        self.guests
            .iter()
            .filter(|(_, g)| g.embedder == embedder)
            .map(|(l, _)| l.clone())
            .collect()
    }
}

/// What a guest event leads to, decided under the lock.
enum Next {
    Nothing,
    Forward(String, String, Option<Value>),
    Reload,
    Close,
    Crash,
    Mute(bool),
    /// `dom-ready`, then `did-finish-load` when the load already finished.
    DomReady(String, String, bool),
    Ready(Option<String>),
    Log(&'static str),
    ScheduleReload,
}

/// Platform reports for guests and consent windows, routed to the host.
pub(super) struct Reports<R: Runtime>(pub(super) Weak<Host<R>>);

impl<R: Runtime> GuestReports for Reports<R> {
    fn crashed(&self, label: &str, reason: GoneReason, exit_code: i64) {
        if let Some(host) = self.0.upgrade() {
            if crate::window::classify(label) == crate::window::WebviewClass::Cmp {
                host.consent_load_failed(label);
            } else {
                host.guest_crashed(label, reason, exit_code);
            }
        }
    }

    fn load_failed(&self, label: &str, error_code: i64, description: &str, url: &str) {
        if let Some(host) = self.0.upgrade() {
            if crate::window::classify(label) == crate::window::WebviewClass::Cmp {
                host.consent_load_failed(label);
            } else {
                host.guest_load_failed(label, error_code, description, url);
            }
        }
    }
}

/// Where the embedder's page starts in its window (logical): the embedder
/// webview's position, moved below the window's content inset when the
/// platform insets the page there (macOS 26 title bar). Guest rectangles
/// are page coordinates, so this is their offset.
/// Without `os_queries` (mock runtime tests) the native window is not
/// asked: a mock window has no real handle.
fn page_offset<R: Runtime>(
    embedder: &Webview<R>,
    window: &tauri::Window<R>,
    scale: f64,
    os_queries: bool,
) -> (f64, f64) {
    let (x, y) = embedder
        .position()
        .map(|p| p.to_logical::<f64>(scale))
        .map_or((0.0, 0.0), |p| (p.x, p.y));
    let inset = if os_queries {
        crate::platform::webview::content_inset_top(window)
    } else {
        0.0
    };
    (x, crate::platform::webview::page_origin_y(y, inset))
}

impl<R: Runtime> Host<R> {
    /// macOS: a web content process of an ad guest or consent window ended
    /// (the app forwards `on_web_content_process_terminate`, A.5).
    pub(crate) fn web_content_terminated(self: &Arc<Self>, label: &str) {
        Reports(Arc::downgrade(self)).crashed(label, GoneReason::Crashed, 0);
    }

    /// `systemInfo` (D.2), read once.
    fn system_info(self: &Arc<Self>) -> Value {
        if let Some(v) = self.with_core(|c| c.ads.system_info.clone()) {
            return v;
        }
        let (monitors, primary) = super::monitors(&self.app, self.options.os_queries);
        let displays: Vec<Value> = monitors
            .iter()
            .map(|m| {
                let s = if m.scale_factor > 0.0 { m.scale_factor } else { 1.0 };
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
        let cpu = if self.options.os_queries {
            crate::platform::machine::cpu_brand()
        } else {
            String::new()
        };
        let info = json!({
            "gpus": [{ "name": "", "model": "", "driverVersion": "", "vendor": "" }],
            "cpu": cpu,
            "displays": displays,
        });
        self.with_core(|c| c.ads.system_info = Some(info.clone()));
        info
    }

    /// `<owVersion>` as guests and consent pages see it (`ads.owVersionOverride`).
    pub(crate) fn guest_ow_version(&self) -> String {
        self.info
            .config
            .ads
            .ow_version_override
            .clone()
            .unwrap_or_else(|| {
                super::analytics::host_label(&self.info.config.analytics).ow_version()
            })
    }

    fn shaping(&self, window_name: &str) -> Option<Shaping> {
        self.info.config.ads.request_shaping.then(|| Shaping {
            referer: format!("https://www.overwolf.com/{}", self.info.identity.uid),
            origin: "https://www.overwolf.com".to_owned(),
            uid: self.info.identity.uid.clone(),
            phase: self.info.phase_percent.to_string(),
            window: window_name.to_owned(),
        })
    }

    /// The analytics name of the embedder webview's window (D.2 `windowName`).
    fn embedder_window_name(self: &Arc<Self>, embedder: &str) -> String {
        self.with_core(|c| {
            let id = match crate::window::classify(embedder) {
                crate::window::WebviewClass::Ui(id) | crate::window::WebviewClass::Remote(id) => {
                    Some(id)
                }
                _ => None,
            };
            id.and_then(|id| c.windows.get(id))
                .and_then(|e| e.analytics_name.clone())
                .or_else(|| {
                    c.urls
                        .get(embedder)
                        .map(|u| crate::analytics::window_analytics_name(u))
                })
                .unwrap_or_default()
        })
    }

    /// `adview_mount`: creates the guest webview (A.2.5).
    #[expect(
        clippy::too_many_lines,
        reason = "one linear setup sequence (D.1, D.2, D.8.1)"
    )]
    /// `document_title` is the embedder document's `document.title`
    /// (`windowTitle`, D.2); the native window title stands in without it.
    pub(crate) fn mount_guest(
        self: &Arc<Self>,
        embedder: &Webview<R>,
        mount: AdviewMount,
        document_title: Option<String>,
    ) -> Result<String, Error> {
        if !valid_element_id(&mount.element_id) || !valid_rect(&mount.rect) {
            return Err(Error::invalid_argument("Invalid element id or rectangle."));
        }
        let embedder_label = embedder.label().to_owned();
        if let Some(old) = self.with_core(|c| c.ads.find(&embedder_label, &mount.element_id)) {
            self.close_guest(&old);
        }
        let window = embedder.window();
        let scale = window.scale_factor().unwrap_or(1.0);
        let offset = page_offset(embedder, &window, scale, self.options.os_queries);
        let (x, y, w, h) = logical_rect(&mount.rect, scale, offset);
        let label = self.with_core(|c| {
            c.ads.next += 1;
            guest_label(&embedder_label, c.ads.next)
        });
        let window_name = self.embedder_window_name(&embedder_label);
        let window_title = document_title.unwrap_or_else(|| window.title().unwrap_or_default());
        let focused = window.is_focused().unwrap_or(false);
        let embedder_hidden = !window.is_visible().unwrap_or(true);
        let embedder_minimized = window.is_minimized().unwrap_or(false);
        let sent_visible = mount.visible && !embedder_hidden && !embedder_minimized;
        let host_key = format!("_{}", uuid::Uuid::new_v4().simple());
        let flags = self.with_core(|c| c.flags);
        let system_info = self.system_info();
        let facts = GuestFacts {
            muid: &self.info.muid,
            uid: &self.info.identity.uid,
            name: &self.info.manifest.product_name,
            ow_version: &self.guest_ow_version(),
            version: &self.info.manifest.version,
            window_name: &window_name,
            window_title: &window_title,
            window_focused: focused,
            test_ad: self.info.config.ads.test_ad || self.info.switches.test_ad,
            disable_optimization: flags.ads_optimization_disabled
                || self.info.manifest.build_overwolf.disable_ad_optimization,
            muid_v2: &self.info.muid_v2,
            phase_percent: self.info.phase_percent,
            consent: &self.info.launch_consent,
            system_info,
            attributes: &mount.attributes,
            slot_id: &label,
        };
        let mut config = guest_config(&facts, sent_visible);
        if let Value::Object(m) = &mut config {
            m.insert("hostKey".into(), Value::from(host_key.as_str()));
        }
        let script =
            splice_config(ADVIEW_HOST_JS, ADVIEW_CONFIG_TOKEN, &config).unwrap_or_else(|| {
                self.log(
                    LogLevel::Error,
                    "adview-host.js has no configuration token; the guest runs without its shim",
                );
                ADVIEW_HOST_JS.to_owned()
            });
        let blank = Url::parse("about:blank").map_err(|_| Error::backend("about:blank"))?;
        let ua = self.user_agent();
        #[cfg_attr(
            not(windows),
            expect(unused_mut, reason = "Windows adds the ads environment")
        )]
        let mut builder = WebviewBuilder::new(&label, WebviewUrl::External(blank))
            .user_agent(&ua)
            .initialization_script(script)
            .focused(false)
            .on_new_window(self.guest_new_window_handler(label.clone()));
        #[cfg(windows)]
        {
            builder = builder
                .data_directory(self.info.ads_data_dir.clone())
                .additional_browser_args(&self.ads_browser_args());
        }
        let webview = window
            .add_child(builder, LogicalPosition::new(x, y), LogicalSize::new(w, h))
            .map_err(Error::from)?;
        if !mount.visible {
            let _ = webview.hide();
        }
        let _ = crate::platform::webview::set_muted(&webview, true);
        let reports: Arc<dyn GuestReports> = Arc::new(Reports(Arc::downgrade(self)));
        if let Err(err) = crate::platform::webview::install_guest_hooks(
            &webview,
            self.shaping(&window_name),
            reports,
        ) {
            self.log(
                LogLevel::Warn,
                &format!("guest hooks for {label} failed: {err}"),
            );
        }
        let now = self.now();
        let limits = &self.info.config.ads.guest_limits;
        let guest = Guest {
            embedder: embedder_label.clone(),
            element_id: mount.element_id.clone(),
            attributes: mount.attributes,
            rect: mount.rect,
            visible: mount.visible,
            mounted_ms: now,
            navigated: false,
            nav_started_ms: None,
            ready: false,
            dom_ready: false,
            finish_pending: None,
            host_key,
            embedder_hidden,
            embedder_minimized,
            sent_visible,
            last_load_ms: now,
            loads: 0,
            recoveries: 0,
            limiter: GuestLimiter::new(limits, now),
            budget: OpenBudget::new(
                self.info.config.ads.gesture_window_ms,
                limits.external_opens_per_minute,
            ),
            tracking_changed: false,
            next_page_url: None,
            reload_at: None,
            retry_at: None,
            apply_setting_logged: false,
        };
        self.with_core(|c| {
            c.ads.guests.insert(label.clone(), guest);
        });
        crate::lab::record("wc-events.jsonl", || {
            json!({
                "kind": "created",
                "label": label,
                "type": "owadview",
                "embedder": embedder_label,
                "elementId": mount.element_id,
                "visible": mount.visible,
                "bounds": [x, y, w, h],
            })
        });
        // A window shown since the last poll counts first: ow-electron sees
        // `show` at once, so its first-visible-window heartbeat precedes the
        // 400025 of a guest that attaches after (E.2 #5, #6; observed).
        self.poll_visibility();
        self.analytics_guest_attached();
        self.host_event(&embedder_label, &mount.element_id, "did-attach", None);
        // The first navigation may already be allowed (D.6.5).
        self.ads_tick(now);
        Ok(label)
    }

    /// The browser arguments of the ads environment (A.1.1).
    #[cfg(windows)]
    pub(crate) fn ads_browser_args(&self) -> String {
        format!(
            "{} --disable-web-security --allow-running-insecure-content",
            self.info.browser_args
        )
        .trim()
        .to_owned()
    }

    fn guest_new_window_handler(
        self: &Arc<Self>,
        label: String,
    ) -> impl Fn(Url, tauri::webview::NewWindowFeatures) -> NewWindowResponse<R> + Send + 'static
    {
        let weak = Arc::downgrade(self);
        move |url, _features| {
            if let Some(host) = weak.upgrade() {
                host.guest_open_external(&label, &url, "popup");
            }
            NewWindowResponse::Deny
        }
    }

    /// A popup or gesture navigation from a guest (D.7): opens `url` in the
    /// system browser when the guest's budget allows it.
    fn guest_open_external(self: &Arc<Self>, label: &str, url: &Url, what: &str) {
        let now = self.now();
        let decision = self.with_core(|c| {
            c.ads.guests.get_mut(label).map(|g| {
                (
                    g.budget.try_open(now, url, None),
                    g.embedder.clone(),
                    g.element_id.clone(),
                )
            })
        });
        let Some((decision, embedder, element_id)) = decision else {
            return;
        };
        match decision {
            Ok(()) => {
                if self.open_in_browser(url).is_err() {
                    return;
                }
                self.guest_deliver(label, "ad-clicked", Some(&Value::from(url.as_str())));
                self.host_event(
                    &embedder,
                    &element_id,
                    "ad-clicked",
                    Some(json!({ "url": url.as_str() })),
                );
            }
            Err(reason) => self.log(
                LogLevel::Debug,
                &format!(
                    "ad guest {what} to a {} URL dropped ({reason:?})",
                    url.scheme()
                ),
            ),
        }
    }

    /// Queues a host lifecycle event for an element (B.3.5).
    pub(crate) fn host_event(
        self: &Arc<Self>,
        embedder: &str,
        element_id: &str,
        name: &str,
        data: Option<Value>,
    ) {
        crate::lab::record(
            "ipc.jsonl",
            || json!({ "dir": "host->embedder", "via": "element-event", "embedder": embedder, "elementId": element_id, "name": name, "data": data }),
        );
        let message = HostMessage::AdviewEvent {
            element_id: element_id.to_owned(),
            name: name.to_owned(),
            data,
            source: "host".to_owned(),
        };
        self.with_core(|c| c.router.push(embedder, message));
    }

    /// Calls the shim's host function `function` in the guest `label`.
    fn guest_call(self: &Arc<Self>, label: &str, function: &str, arg: &Value) {
        crate::lab::record(
            "ipc.jsonl",
            || json!({ "dir": "host->page", "via": "guest-call", "type": "owadview", "label": label, "function": function, "args": arg }),
        );
        let key = self.with_core(|c| c.ads.guests.get(label).map(|g| g.host_key.clone()));
        if let (Some(key), Some(w)) = (key, self.app.get_webview(label)) {
            let _ = w.eval(host_call_script(&key, function, arg));
        }
    }

    /// Delivers one host message to the guest `label` (D.5).
    fn guest_deliver(self: &Arc<Self>, label: &str, kind: &str, data: Option<&Value>) {
        self.guest_record("ipc.jsonl", || {
            // As delivered (no `data` key when there is none).
            json!({ "dir": "host->page", "via": "private-message", "type": "owadview", "label": label, "message": crate::ads::host_message(kind, data) })
        });
        let key = self.with_core(|c| c.ads.guests.get(label).map(|g| g.host_key.clone()));
        if let (Some(key), Some(w)) = (key, self.app.get_webview(label)) {
            let _ = w.eval(deliver_script(&key, kind, data));
        }
    }

    /// Appends `entry` to the lab trace file `file` and, in a host without
    /// OS queries (Tauri's mock runtime), to the guests' test trace.
    fn guest_record(self: &Arc<Self>, file: &str, entry: impl FnOnce() -> Value) {
        if self.options.os_queries {
            crate::lab::record(file, entry);
            return;
        }
        let entry = entry();
        crate::lab::record(file, || entry.clone());
        self.with_core(|c| c.ads.test_trace.push(entry));
    }

    /// Delivers a host message to every guest (D.5).
    pub(crate) fn deliver_to_guests(self: &Arc<Self>, kind: &str, data: Option<&Value>) {
        let labels: Vec<String> = self.with_core(|c| c.ads.guests.keys().cloned().collect());
        for l in labels {
            self.guest_deliver(&l, kind, data);
        }
    }

    /// Tells the shim of `label` its visibility (`document.visibilityState`,
    /// D.5): visible when the element is visible and its window is shown
    /// and not minimized (Chromium hides the documents of a minimized
    /// window).
    /// Sends only a change, unless `force` (a new document).
    fn sync_visibility(self: &Arc<Self>, label: &str, force: bool) {
        let send = self.with_core(|c| {
            c.ads.guests.get_mut(label).and_then(|g| {
                let visible = g.visible && !g.embedder_hidden && !g.embedder_minimized;
                let changed = std::mem::replace(&mut g.sent_visible, visible) != visible;
                (changed || force).then_some(visible)
            })
        });
        if let Some(visible) = send {
            self.guest_call(
                label,
                "setVisibility",
                &Value::from(if visible { "visible" } else { "hidden" }),
            );
        }
    }

    /// `adview_update`.
    pub(crate) fn update_guest(
        self: &Arc<Self>,
        embedder: &Webview<R>,
        update: AdviewUpdate,
    ) -> Result<(), Error> {
        let embedder_label = embedder.label().to_owned();
        let label = self
            .with_core(|c| c.ads.find(&embedder_label, &update.element_id))
            .ok_or_else(|| Error::not_found("No ad guest for this element."))?;
        let webview = self.app.get_webview(&label);
        if let Some(rect) = update.rect {
            if !valid_rect(&rect) {
                return Err(Error::invalid_argument("Invalid rectangle."));
            }
            if let Some(wv) = &webview {
                let window = embedder.window();
                let scale = window.scale_factor().unwrap_or(1.0);
                let offset = page_offset(embedder, &window, scale, self.options.os_queries);
                let (x, y, w, h) = logical_rect(&rect, scale, offset);
                let _ = wv.set_position(LogicalPosition::new(x, y));
                let _ = wv.set_size(LogicalSize::new(w, h));
            }
            self.with_core(|c| {
                if let Some(g) = c.ads.guests.get_mut(&label) {
                    g.rect = rect;
                }
            });
        }
        if let Some(visible) = update.visible {
            let changed = self.with_core(|c| {
                c.ads.guests.get_mut(&label).is_some_and(|g| {
                    let changed = g.visible != visible;
                    g.visible = visible;
                    changed
                })
            });
            if changed && let Some(wv) = &webview {
                let _ = if visible { wv.show() } else { wv.hide() };
            }
            self.sync_visibility(&label, false);
        }
        if let Some(patch) = update.attributes {
            self.apply_attribute_patch(&label, patch);
        }
        Ok(())
    }

    fn apply_attribute_patch(self: &Arc<Self>, label: &str, patch: AdviewAttributesPatch) {
        let tracking = patch
            .custom_tracking
            .map(|v| if v.is_object() { v } else { Value::Null });
        self.with_core(|c| {
            if let Some(g) = c.ads.guests.get_mut(label) {
                if let Some(t) = &tracking {
                    g.attributes.custom_tracking = t.clone();
                    g.tracking_changed = true;
                }
                if let Some(p) = &patch.pageurl {
                    g.next_page_url = Some(p.clone());
                }
            }
        });
        if let Some(t) = tracking {
            self.guest_deliver(label, "customTracking", Some(&t));
        }
        if let Some(p) = patch.pageurl {
            self.guest_call(label, "setNextPageUrl", &Value::from(p));
        }
    }

    /// `adview_unmount` (idempotent).
    pub(crate) fn unmount_guest(self: &Arc<Self>, embedder: &str, element_id: &str) {
        if let Some(label) = self.with_core(|c| c.ads.find(embedder, element_id)) {
            self.close_guest(&label);
        }
    }

    /// Closes one guest webview and forgets it.
    pub(crate) fn close_guest(self: &Arc<Self>, label: &str) {
        let removed = self.with_core(|c| c.ads.guests.remove(label).is_some());
        crate::lab::record(
            "wc-events.jsonl",
            || json!({ "kind": "closed", "label": label, "type": "owadview", "known": removed }),
        );
        if removed && let Some(w) = self.app.get_webview(label) {
            let _ = w.close();
        }
    }

    /// Closes every guest embedded in the webview `embedder` (A.3, B.3.4).
    pub(crate) fn close_guests_of(self: &Arc<Self>, embedder: &str) {
        let labels = self.with_core(|c| c.ads.labels_of(embedder));
        for l in labels {
            self.close_guest(&l);
        }
    }

    /// `adview_command` (B.3.3).
    pub(crate) fn guest_command(
        self: &Arc<Self>,
        embedder: &str,
        element_id: &str,
        command: AdviewCommandName,
        args: &[Value],
    ) -> Result<(), Error> {
        let label = self
            .with_core(|c| c.ads.find(embedder, element_id))
            .ok_or_else(|| Error::not_found("No ad guest for this element."))?;
        match command {
            AdviewCommandName::SetAudioMuted => {
                let muted = args.first().and_then(Value::as_bool).unwrap_or(true);
                if let Some(w) = self.app.get_webview(&label) {
                    let _ = crate::platform::webview::set_muted(&w, muted);
                }
            }
            AdviewCommandName::Reload => self.reload_guest(&label),
            AdviewCommandName::SetPageUrl => {
                // The `pageurl` of the next load, as the attribute sets it,
                // then the private message ow-electron sends (observed).
                let url = args.first().cloned().unwrap_or(Value::Null);
                self.apply_attribute_patch(
                    &label,
                    AdviewAttributesPatch {
                        pageurl: Some(url.as_str().unwrap_or_default().to_owned()),
                        ..AdviewAttributesPatch::default()
                    },
                );
                self.guest_deliver(&label, "setPageUrl", Some(&Value::Array(vec![url])));
            }
            AdviewCommandName::SendCommand => {
                // Forwarded verbatim as a private message (observed); the
                // ad page decides what, if anything, it does with it.
                self.guest_deliver(&label, "sendCommand", Some(&Value::Array(args.to_vec())));
            }
        }
        Ok(())
    }

    /// Reloads a guest's ad document (a new load, `sessionTS` restarts).
    /// On Windows a native reload, which the request handler shapes again;
    /// on macOS and Linux a new shaped load of the ad document, because a
    /// native reload may not repeat the `Referer` and `Origin` that only
    /// the shaped load request carries (D.8.3).
    pub(crate) fn reload_guest(self: &Arc<Self>, label: &str) {
        let now = self.now();
        let started = self.with_core(|c| {
            c.ads.guests.get_mut(label).is_some_and(|g| {
                if !g.navigated {
                    return false;
                }
                g.begin_load(now);
                g.reload_at = None;
                true
            })
        });
        if !started {
            return;
        }
        crate::lab::record(
            "wc-events.jsonl",
            || json!({ "kind": "reload", "label": label, "type": "owadview" }),
        );
        if cfg!(windows) {
            if let Some(w) = self.app.get_webview(label) {
                let _ = w.reload();
            }
        } else {
            self.navigate_guest(label);
        }
    }

    /// Runs a reload `__overwolf__.reload()` scheduled, once it is due.
    fn run_due_reload(self: &Arc<Self>, label: &str) {
        let now = self.now();
        let due = self.with_core(|c| {
            c.ads.guests.get_mut(label).is_some_and(|g| {
                g.reload_at.is_some_and(|t| now >= t) && g.reload_at.take().is_some()
            })
        });
        if due {
            self.reload_guest(label);
        }
    }

    fn navigate_guest(self: &Arc<Self>, label: &str) {
        let Some(webview) = self.app.get_webview(label) else {
            return;
        };
        let Ok(url) = Url::parse(ADVIEW_URL) else {
            return;
        };
        let embedder = self.with_core(|c| c.ads.guests.get(label).map(|g| g.embedder.clone()));
        let window_name = embedder
            .map(|e| self.embedder_window_name(&e))
            .unwrap_or_default();
        let shaping = self.shaping(&window_name);
        let shaped = crate::platform::webview::load_shaped(&webview, &url, shaping.as_ref());
        crate::lab::record("shaped-requests.jsonl", || {
            json!({
                "label": label,
                "url": url.as_str(),
                "method": "GET",
                "via": if shaped && !cfg!(windows) { "loadRequest" } else { "navigate" },
                "hostHeaders": crate::platform::webview::document_header_fields(shaping.as_ref()),
                "windowName": window_name,
            })
        });
        if !shaped {
            let _ = webview.navigate(url);
        }
    }

    /// Timer step of the ads service: first navigations after consent
    /// (D.6.5), scheduled reloads and retries, load-failure detection and
    /// the dropped-message log.
    pub(crate) fn ads_tick(self: &Arc<Self>, now: u64) {
        let gate = self.consent_gate_open();
        let retry_ms = self.info.config.ads.load_error_retry_ms;
        let (navigate, reload, failed, finished, logs) = self.with_core(|c| {
            let mut navigate = Vec::new();
            let mut reload = Vec::new();
            let mut failed = Vec::new();
            let mut finished = Vec::new();
            let mut logs = Vec::new();
            for (label, g) in &mut c.ads.guests {
                if !g.navigated {
                    if gate || now.saturating_sub(g.mounted_ms) >= CONSENT_WAIT_MS {
                        g.navigated = true;
                        g.begin_load(now);
                        navigate.push(label.clone());
                    }
                    continue;
                }
                if g.finish_pending
                    .is_some_and(|t| now.saturating_sub(t) >= FINISH_WAIT_MS)
                {
                    g.finish_pending = None;
                    finished.push((g.embedder.clone(), g.element_id.clone()));
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
                    failed.push((g.embedder.clone(), g.element_id.clone()));
                }
                if let Some(n) = g.limiter.take_drop_log(now) {
                    logs.push((label.clone(), n));
                }
            }
            (navigate, reload, failed, finished, logs)
        });
        for l in navigate {
            self.navigate_guest(&l);
        }
        for l in reload {
            self.reload_guest(&l);
        }
        for (embedder, element_id) in failed {
            self.host_event(
                &embedder,
                &element_id,
                "did-fail-load",
                Some(fail_load_data(-7, "ERR_TIMED_OUT", ADVIEW_URL, true)),
            );
        }
        for (embedder, element_id) in finished {
            self.host_event(&embedder, &element_id, "did-finish-load", None);
        }
        for (l, n) in logs {
            self.log(
                LogLevel::Warn,
                &format!("ad guest {l}: {n} messages over the limit dropped"),
            );
        }
    }

    /// A platform-reported main-frame load failure (D.7): retried every
    /// `ads.loadErrorRetryMs`, without analytics.
    #[cfg_attr(
        not(any(windows, target_os = "linux")),
        expect(
            dead_code,
            reason = "WKWebView load failures are not reported to plugins"
        )
    )]
    pub(crate) fn guest_load_failed(
        self: &Arc<Self>,
        label: &str,
        error_code: i64,
        description: &str,
        url: &str,
    ) {
        let now = self.now();
        let retry = self.info.config.ads.load_error_retry_ms;
        let target = self.with_core(|c| {
            c.ads.guests.get_mut(label).map(|g| {
                g.ready = false;
                g.nav_started_ms = None;
                g.finish_pending = None;
                g.retry_at = Some(now + retry);
                (g.embedder.clone(), g.element_id.clone())
            })
        });
        crate::lab::record(
            "wc-events.jsonl",
            || json!({ "kind": "did-fail-load", "label": label, "type": "owadview", "errorCode": error_code, "description": description, "url": url }),
        );
        if let Some((embedder, element_id)) = target {
            self.host_event(
                &embedder,
                &element_id,
                "did-fail-load",
                Some(fail_load_data(error_code, description, url, true)),
            );
        }
    }

    /// A guest crashed (D.7, E.2 #8): `render-process-gone`, the crash
    /// Counter, the reload, then Kind 400024; or the guest is closed after
    /// `ads.maxRecoveries` recoveries.
    pub(crate) fn guest_crashed(self: &Arc<Self>, label: &str, reason: GoneReason, exit_code: i64) {
        let now = self.now();
        let max = self.info.config.ads.max_recoveries;
        let decision = self.with_core(|c| {
            c.ads.guests.get_mut(label).map(|g| {
                let secs = session_secs(now, g.last_load_ms);
                let recover = may_recover(max, g.recoveries);
                if recover {
                    g.recoveries += 1;
                    g.last_load_ms = now;
                }
                (g.embedder.clone(), g.element_id.clone(), secs, recover)
            })
        });
        let Some((embedder, element_id, secs, recover)) = decision else {
            return;
        };
        crate::lab::record(
            "wc-events.jsonl",
            || json!({ "kind": "render-process-gone", "label": label, "type": "owadview", "reason": reason.as_str(), "exitCode": exit_code, "sessionTS": secs, "recover": recover }),
        );
        self.host_event(
            &embedder,
            &element_id,
            "render-process-gone",
            Some(gone_data(reason, exit_code)),
        );
        if !recover {
            self.log(
                LogLevel::Warn,
                &format!("ad guest {label} closed after its recoveries (ads.maxRecoveries)"),
            );
            self.close_guest(label);
            return;
        }
        self.analytics_guest_crash_counter(secs, reason.as_str());
        self.reload_guest(label);
        self.analytics_guest_crash_stats(secs, reason.as_str());
    }

    /// The navigation policy of a guest (D.7). Non-web schemes are always
    /// cancelled. Where the hook sees only top-level navigations (Windows),
    /// a navigation off Overwolf is cancelled and, with a gesture, opened in
    /// the system browser; elsewhere frames pass and top-level escapes are
    /// caught at page-load start.
    pub(crate) fn guest_navigation(self: &Arc<Self>, label: &str, url: &Url) -> bool {
        match url.scheme() {
            "https" | "http" => {}
            "about" | "data" | "blob" => return true,
            _ => {
                self.log(
                    LogLevel::Debug,
                    &format!("ad guest navigation to a {} URL cancelled", url.scheme()),
                );
                return false;
            }
        }
        if crate::window::options::NAVIGATION_HOOK_IS_TOP_LEVEL_ONLY && !is_overwolf_url(url) {
            self.guest_open_external(label, url, "navigation");
            return false;
        }
        true
    }

    /// A page load of a guest (B.3.5, D.5). `did-finish-load` follows the
    /// shim's `dom-ready`, as in ow-electron.
    pub(crate) fn guest_page_load(self: &Arc<Self>, label: &str, event: PageLoadEvent, url: &Url) {
        let known = self.with_core(|c| c.ads.guests.contains_key(label));
        if !known || url.scheme() == "about" {
            return;
        }
        let now = self.now();
        if !is_overwolf_url(url) {
            if event == PageLoadEvent::Started {
                // A top-level navigation off Overwolf (D.7): a click with a
                // gesture opens externally; the guest goes back to the ad
                // page, a new load the readiness timeout watches.
                self.guest_open_external(label, url, "navigation");
                self.with_core(|c| {
                    if let Some(g) = c.ads.guests.get_mut(label) {
                        g.begin_load(now);
                    }
                });
                self.navigate_guest(label);
            }
            return;
        }
        if event == PageLoadEvent::Started {
            self.with_core(|c| {
                if let Some(g) = c.ads.guests.get_mut(label) {
                    g.dom_ready = false;
                    g.finish_pending = None;
                }
            });
            return;
        }
        let info = self.with_core(|c| {
            c.ads.guests.get_mut(label).map(|g| {
                g.loads += 1;
                g.last_load_ms = now;
                g.retry_at = None;
                let finish_now = g.dom_ready;
                if !finish_now {
                    g.finish_pending = Some(now);
                }
                (
                    g.embedder.clone(),
                    g.element_id.clone(),
                    finish_now,
                    (g.loads > 1 && g.tracking_changed)
                        .then(|| g.attributes.custom_tracking.clone()),
                )
            })
        });
        let Some((embedder, element_id, finish_now, tracking)) = info else {
            return;
        };
        if let Some(w) = self.app.get_webview(label) {
            // ow-electron mutes, and signals visibility and focus, on every load.
            let _ = crate::platform::webview::set_muted(&w, true);
            self.sync_visibility(label, true);
            let focused = w.window().is_focused().unwrap_or(false);
            self.guest_call(label, "setEmbedderFocus", &Value::Bool(focused));
            if let Some(t) = tracking {
                self.guest_deliver(label, "customTracking", Some(&t));
            }
        }
        if finish_now {
            self.host_event(&embedder, &element_id, "did-finish-load", None);
        }
    }

    /// `adview_event` from the guest `label` (A.2.6, D.4).
    #[expect(
        clippy::too_many_lines,
        reason = "one match over the guest message names (D.4)"
    )]
    pub(crate) fn guest_event(
        self: &Arc<Self>,
        label: &str,
        event: AdviewEvent,
    ) -> Result<(), Error> {
        if !valid_event_name(&event.name) {
            return Err(Error::invalid_argument("Invalid event name."));
        }
        crate::lab::record(
            "ipc.jsonl",
            || json!({ "dir": "page->host", "via": "adview_event", "type": "owadview", "label": label, "channel": event.name, "data": event.data }),
        );
        let (data, bytes) = match event.data {
            Some(d) => {
                let (v, n) = cap_event_data(d);
                (Some(v), n)
            }
            None => (None, 0),
        };
        let now = self.now();
        if let Some(claimed) = &event.slot_id
            && claimed != label
        {
            // The caller label is authoritative; the claim is only logged.
            self.log(
                LogLevel::Debug,
                &format!("ad guest {label} sent an event for another slot id"),
            );
        }
        let internal = InternalEvent::parse(&event.name);
        let next = self.with_core(|c| {
            let Some(g) = c.ads.guests.get_mut(label) else {
                return Next::Nothing;
            };
            match g.limiter.admit(now, bytes) {
                Admission::Admit => {}
                Admission::Drop => return Next::Nothing,
                Admission::Reload => return Next::Reload,
                Admission::Close => return Next::Close,
            }
            match internal {
                None if InternalEvent::is_reserved(&event.name) => Next::Nothing,
                None => Next::Forward(g.embedder.clone(), g.element_id.clone(), data.clone()),
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
                Some(InternalEvent::Gesture) => {
                    g.budget.gesture(now);
                    Next::Nothing
                }
                Some(InternalEvent::Focus) => Next::Nothing,
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
                    g.reload_at = Some(now + RELOAD_DELAY_MS);
                    Next::ScheduleReload
                }
                Some(InternalEvent::DomReady) => {
                    g.dom_ready = true;
                    let finish = g.finish_pending.take().is_some();
                    Next::DomReady(g.embedder.clone(), g.element_id.clone(), finish)
                }
            }
        });
        match next {
            Next::Nothing => {}
            Next::Forward(embedder, element_id, data) => {
                let message = HostMessage::AdviewEvent {
                    element_id,
                    name: event.name,
                    data,
                    source: "guest".into(),
                };
                self.with_core(|c| c.router.push(&embedder, message));
            }
            Next::Reload => {
                self.log(
                    LogLevel::Warn,
                    &format!("ad guest {label} over its message limit for 10 s; reloading"),
                );
                self.reload_guest(label);
            }
            Next::Close => {
                self.log(
                    LogLevel::Warn,
                    &format!("ad guest {label} over its message limit again; closing"),
                );
                self.close_guest(label);
            }
            Next::Crash => self.guest_crashed(label, GoneReason::Killed, 0),
            Next::Mute(muted) => {
                if let Some(w) = self.app.get_webview(label) {
                    let _ = crate::platform::webview::set_muted(&w, muted);
                }
            }
            Next::DomReady(embedder, element_id, finish) => {
                self.host_event(&embedder, &element_id, "dom-ready", None);
                if finish {
                    self.host_event(&embedder, &element_id, "did-finish-load", None);
                }
            }
            Next::Log(message) => self.log(LogLevel::Debug, message),
            Next::Ready(page_url) => {
                if let Some(p) = page_url {
                    self.guest_call(label, "setNextPageUrl", &Value::from(p));
                }
            }
            Next::ScheduleReload => {
                // A one-shot timer: the reload follows ~70 ms later, not at
                // the next 250 ms host tick.
                let host = Arc::clone(self);
                let label = label.to_owned();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(RELOAD_DELAY_MS)).await;
                    host.run_due_reload(&label);
                });
            }
        }
        Ok(())
    }

    /// The guests embedded in window `id`, in its app or remote webview.
    fn guests_of_window(self: &Arc<Self>, id: u32) -> Vec<String> {
        self.with_core(|c| {
            let mut l = c.ads.labels_of(&crate::window::ui_label(id));
            l.extend(c.ads.labels_of(&crate::window::remote_label(id)));
            l
        })
    }

    /// The embedder window `id` was hidden (D.5 `window-hidden`): its
    /// guests' documents become hidden.
    pub(crate) fn ads_window_hidden(self: &Arc<Self>, id: u32) {
        self.set_embedder_hidden(id, true);
    }

    /// The embedder window `id` was shown again: its guests' documents
    /// become visible again where the element is visible.
    pub(crate) fn ads_window_shown(self: &Arc<Self>, id: u32) {
        self.set_embedder_hidden(id, false);
    }

    /// The embedder window `id` is about to be destroyed: its guests'
    /// documents become hidden first, as in ow-electron (observed), without
    /// a `window-hidden` message.
    pub(crate) fn ads_window_closing(self: &Arc<Self>, id: u32) {
        for l in self.guests_of_window(id) {
            self.with_core(|c| {
                if let Some(g) = c.ads.guests.get_mut(&l) {
                    g.embedder_hidden = true;
                }
            });
            self.sync_visibility(&l, false);
        }
    }

    fn set_embedder_hidden(self: &Arc<Self>, id: u32, hidden: bool) {
        for l in self.guests_of_window(id) {
            let changed = self.with_core(|c| {
                c.ads
                    .guests
                    .get_mut(&l)
                    .is_some_and(|g| std::mem::replace(&mut g.embedder_hidden, hidden) != hidden)
            });
            if changed && hidden {
                self.guest_deliver(&l, "window-hidden", None);
            }
            self.sync_visibility(&l, false);
        }
    }

    /// The embedder window `id` was minimized or restored: its guests'
    /// documents are hidden while it is minimized. Nothing is sent to the
    /// page (D.5: no message for minimize or restore).
    pub(crate) fn ads_window_minimized(self: &Arc<Self>, id: u32, minimized: bool) {
        for l in self.guests_of_window(id) {
            self.with_core(|c| {
                if let Some(g) = c.ads.guests.get_mut(&l) {
                    g.embedder_minimized = minimized;
                }
            });
            self.sync_visibility(&l, false);
        }
    }

    /// The embedder window `id` gained or lost focus (D.3 `hasWindowFocus`).
    pub(crate) fn ads_window_focus(self: &Arc<Self>, id: u32, focused: bool) {
        for l in self.guests_of_window(id) {
            self.guest_call(&l, "setEmbedderFocus", &Value::Bool(focused));
        }
    }

    /// `set_user_email_hashes` (A.2.2): an `eHashes` message to every
    /// existing guest, and the hashes stored in `ow-electron.json` as
    /// ow-electron does (observed); ignored after `disable_ads_fpd`.
    pub(crate) fn send_email_hashes(self: &Arc<Self>, hashes: Option<&Map<String, Value>>) {
        let Some(h) = hashes else { return };
        let get = |k: &str| {
            h.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let (sha1, md5, sha256) = (get("sha1"), get("md5"), get("sha256"));
        if sha1.is_empty() && md5.is_empty() && sha256.is_empty() {
            return;
        }
        if self.with_core(|c| c.flags.ads_fpd_disabled) {
            self.log(
                LogLevel::Warn,
                "setUserEmailHashes() after disableAdsFPD() is ignored",
            );
            return;
        }
        if let Err(err) = self.ow_electron.write_e_hashes(&sha1, &md5, &sha256) {
            self.log(LogLevel::Warn, &format!("eHashes not stored: {err}"));
        }
        let mut m = Map::new();
        m.insert("sha1".into(), sha1.into());
        m.insert("md5".into(), md5.into());
        m.insert("sha256".into(), sha256.into());
        self.deliver_to_guests("eHashes", Some(&Value::Object(m)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shim_carries_its_configuration_token() {
        let config = json!({ "slotId": "owad-bw-1-1", "windowTitle": "</script>" });
        let script = splice_config(ADVIEW_HOST_JS, ADVIEW_CONFIG_TOKEN, &config).unwrap();
        assert!(script.contains(r#""slotId":"owad-bw-1-1""#));
        assert!(!script.contains("</script>"));
        assert!(!script.contains(ADVIEW_CONFIG_TOKEN));
    }
}
