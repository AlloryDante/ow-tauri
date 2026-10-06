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
    pub(crate) send_command_logged: bool,
    pub(crate) apply_setting_logged: bool,
}

/// The ads service state, inside the host's core lock.
#[derive(Debug, Default)]
pub(crate) struct AdsCore {
    pub(crate) guests: BTreeMap<String, Guest>,
    pub(crate) next: u32,
    pub(crate) system_info: Option<Value>,
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
    DomReady(String, String),
    Ready(Option<String>),
    Log(&'static str),
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
    pub(crate) fn mount_guest(
        self: &Arc<Self>,
        embedder: &Webview<R>,
        mount: AdviewMount,
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
        let offset = embedder
            .position()
            .map(|p| p.to_logical::<f64>(scale))
            .map_or((0.0, 0.0), |p| (p.x, p.y));
        let (x, y, w, h) = logical_rect(&mount.rect, scale, offset);
        let label = self.with_core(|c| {
            c.ads.next += 1;
            guest_label(&embedder_label, c.ads.next)
        });
        let window_name = self.embedder_window_name(&embedder_label);
        let window_title = window.title().unwrap_or_default();
        let focused = window.is_focused().unwrap_or(false);
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
            system_info,
            attributes: &mount.attributes,
            slot_id: &label,
        };
        let config = guest_config(&facts, mount.visible);
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
            send_command_logged: false,
            apply_setting_logged: false,
        };
        self.with_core(|c| {
            c.ads.guests.insert(label.clone(), guest);
        });
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
                if let Err(err) = tauri_plugin_opener::open_url(url.as_str(), None::<&str>) {
                    self.log(
                        LogLevel::Warn,
                        &format!("opening an ad click failed: {err}"),
                    );
                    return;
                }
                self.guest_eval(
                    label,
                    &deliver_script("ad-clicked", Some(&Value::from(url.as_str()))),
                );
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
        let message = HostMessage::AdviewEvent {
            element_id: element_id.to_owned(),
            name: name.to_owned(),
            data,
            source: "host".to_owned(),
        };
        self.with_core(|c| c.router.push(embedder, message));
    }

    fn guest_eval(&self, label: &str, script: &str) {
        if let Some(w) = self.app.get_webview(label) {
            let _ = w.eval(script);
        }
    }

    /// Delivers a host message to every guest (D.5).
    pub(crate) fn deliver_to_guests(self: &Arc<Self>, kind: &str, data: Option<&Value>) {
        let labels: Vec<String> = self.with_core(|c| c.ads.guests.keys().cloned().collect());
        let script = deliver_script(kind, data);
        for l in labels {
            self.guest_eval(&l, &script);
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
                let offset = embedder
                    .position()
                    .map(|p| p.to_logical::<f64>(scale))
                    .map_or((0.0, 0.0), |p| (p.x, p.y));
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
                let _ = wv.eval(host_call_script(
                    "setVisibility",
                    &Value::from(if visible { "visible" } else { "hidden" }),
                ));
            }
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
            self.guest_eval(label, &deliver_script("customTracking", Some(&t)));
        }
        if let Some(p) = patch.pageurl {
            self.guest_eval(label, &host_call_script("setNextPageUrl", &Value::from(p)));
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
                let url = args
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                self.apply_attribute_patch(
                    &label,
                    AdviewAttributesPatch {
                        pageurl: Some(url),
                        ..AdviewAttributesPatch::default()
                    },
                );
            }
            AdviewCommandName::SendCommand => {
                let first = self.with_core(|c| {
                    c.ads
                        .guests
                        .get_mut(&label)
                        .is_some_and(|g| !std::mem::replace(&mut g.send_command_logged, true))
                });
                if first {
                    self.log(
                        LogLevel::Debug,
                        "owadview sendCommand() has no effect (B.3.3)",
                    );
                }
            }
        }
        Ok(())
    }

    /// Reloads a guest's ad document (a new load, `sessionTS` restarts).
    pub(crate) fn reload_guest(self: &Arc<Self>, label: &str) {
        let now = self.now();
        let started = self.with_core(|c| {
            c.ads.guests.get_mut(label).is_some_and(|g| {
                if !g.navigated {
                    return false;
                }
                g.ready = false;
                g.nav_started_ms = Some(now);
                g.reload_at = None;
                true
            })
        });
        if started && let Some(w) = self.app.get_webview(label) {
            let _ = w.reload();
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
        if !crate::platform::webview::load_shaped(&webview, &url, shaping.as_ref()) {
            let _ = webview.navigate(url);
        }
    }

    /// Timer step of the ads service: first navigations after consent
    /// (D.6.5), scheduled reloads and retries, load-failure detection and
    /// the dropped-message log.
    pub(crate) fn ads_tick(self: &Arc<Self>, now: u64) {
        let gate = self.consent_gate_open();
        let retry_ms = self.info.config.ads.load_error_retry_ms;
        let (navigate, reload, failed, logs) = self.with_core(|c| {
            let mut navigate = Vec::new();
            let mut reload = Vec::new();
            let mut failed = Vec::new();
            let mut logs = Vec::new();
            for (label, g) in &mut c.ads.guests {
                if !g.navigated {
                    if gate || now.saturating_sub(g.mounted_ms) >= CONSENT_WAIT_MS {
                        g.navigated = true;
                        g.nav_started_ms = Some(now);
                        navigate.push(label.clone());
                    }
                    continue;
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
            (navigate, reload, failed, logs)
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
                g.retry_at = Some(now + retry);
                (g.embedder.clone(), g.element_id.clone())
            })
        });
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

    /// A page load of a guest (B.3.5, D.5).
    pub(crate) fn guest_page_load(self: &Arc<Self>, label: &str, event: PageLoadEvent, url: &Url) {
        let known = self.with_core(|c| c.ads.guests.contains_key(label));
        if !known || url.scheme() == "about" {
            return;
        }
        if !is_overwolf_url(url) {
            if event == PageLoadEvent::Started {
                // A top-level navigation off Overwolf (D.7): a click with a
                // gesture opens externally; the guest goes back to the ad page.
                self.guest_open_external(label, url, "navigation");
                self.navigate_guest(label);
            }
            return;
        }
        if event != PageLoadEvent::Finished {
            return;
        }
        let now = self.now();
        let info = self.with_core(|c| {
            c.ads.guests.get_mut(label).map(|g| {
                g.loads += 1;
                g.last_load_ms = now;
                g.retry_at = None;
                (
                    g.embedder.clone(),
                    g.element_id.clone(),
                    g.visible,
                    (g.loads > 1 && g.tracking_changed)
                        .then(|| g.attributes.custom_tracking.clone()),
                )
            })
        });
        let Some((embedder, element_id, visible, tracking)) = info else {
            return;
        };
        if let Some(w) = self.app.get_webview(label) {
            // ow-electron mutes, and signals visibility and focus, on every load.
            let _ = crate::platform::webview::set_muted(&w, true);
            let _ = w.eval(host_call_script(
                "setVisibility",
                &Value::from(if visible { "visible" } else { "hidden" }),
            ));
            let focused = w.window().is_focused().unwrap_or(false);
            let _ = w.eval(host_call_script("setEmbedderFocus", &Value::Bool(focused)));
            if let Some(t) = tracking {
                let _ = w.eval(deliver_script("customTracking", Some(&t)));
            }
        }
        self.host_event(&embedder, &element_id, "did-finish-load", None);
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
                    Next::Nothing
                }
                Some(InternalEvent::DomReady) => {
                    Next::DomReady(g.embedder.clone(), g.element_id.clone())
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
            Next::DomReady(embedder, element_id) => {
                self.host_event(&embedder, &element_id, "dom-ready", None);
            }
            Next::Log(message) => self.log(LogLevel::Debug, message),
            Next::Ready(page_url) => {
                if let Some(p) = page_url {
                    self.guest_eval(label, &host_call_script("setNextPageUrl", &Value::from(p)));
                }
            }
        }
        Ok(())
    }

    /// The embedder window `id` was hidden (D.5 `window-hidden`).
    pub(crate) fn ads_window_hidden(self: &Arc<Self>, id: u32) {
        let labels = self.with_core(|c| {
            let mut l = c.ads.labels_of(&crate::window::ui_label(id));
            l.extend(c.ads.labels_of(&crate::window::remote_label(id)));
            l
        });
        let script = deliver_script("window-hidden", None);
        let hidden = host_call_script("setVisibility", &Value::from("hidden"));
        for l in labels {
            self.guest_eval(&l, &script);
            self.guest_eval(&l, &hidden);
        }
    }

    /// The embedder window `id` gained or lost focus (D.3 `hasWindowFocus`).
    pub(crate) fn ads_window_focus(self: &Arc<Self>, id: u32, focused: bool) {
        let labels = self.with_core(|c| c.ads.labels_of(&crate::window::ui_label(id)));
        let script = host_call_script("setEmbedderFocus", &Value::Bool(focused));
        for l in labels {
            self.guest_eval(&l, &script);
        }
    }

    /// `set_user_email_hashes` (A.2.2): an `eHashes` message to every
    /// existing guest; ignored after `disable_ads_fpd`.
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
