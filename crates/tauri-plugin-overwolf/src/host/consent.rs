//! The consent host (CONTRACT A.2.2, A.2.7, D.6): the `cmp-eu-only`
//! request and the hidden startup consent window of every launch,
//! `isCMPRequired()`, the settings and default-consent windows, the
//! `cmp_event` command, and the consent cookie fallback.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::window::Color;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tokio::sync::oneshot;
use url::Url;

use super::Host;
use crate::ads::is_overwolf_url;
use crate::config::CookieFallback;
use crate::consent::{
    CMP_CONFIG_TOKEN, CMP_SCOPE, CmpEventData, CmpEventName, CmpWindowOptions, ConsentFacts,
    DEFAULT_BACKGROUND, DEFAULT_CMP_URL, HIDDEN_WINDOW_SIZE, STARTUP_CMP_URL, clear_consent_url,
    cmp_config, consent_cookie, cookie_values, default_consent_url, eu_only_outcome, preloader_url,
    settings_url, startup_url, stored_unified, valid_consent,
};
use crate::error::Error;
use crate::state::log::LogLevel;
use crate::state::ow_electron::CmpBlock;
use crate::window::{CMP_DEFAULT_LABEL, CMP_STARTUP_LABEL};

/// The consent page shim (D.6.6), built from `packages/ow-tauri/src/guest/cmp.ts`.
pub(crate) const CMP_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/cmp.js"));

/// The settings window's label (D.6.4).
pub(crate) const CMP_SETTINGS_LABEL: &str = "ow-cmp";

/// Whether `isCMPRequired()` results are cached this launch (D.6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CacheMode {
    /// The first round has not finished.
    #[default]
    Pending,
    /// Calls resolve at once.
    Cached,
    /// A `{}` body: every call sends a new request and opens a new window.
    Uncached,
}

/// One hidden consent window (startup or default).
#[derive(Debug, Clone)]
pub(crate) struct Hidden {
    /// The startup round, `None` for `ow-cmp-default`.
    pub(crate) round: Option<u32>,
    /// `consent.readyTimeoutMs` after creation.
    pub(crate) deadline: u64,
    /// The close was started.
    pub(crate) closing: bool,
}

/// The consent service state, inside the host's core lock.
#[derive(Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent once-per-launch markers"
)]
pub(crate) struct ConsentCore {
    /// `start_consent` ran.
    pub(crate) started: bool,
    pub(crate) mode: CacheMode,
    /// Startup rounds begun.
    pub(crate) rounds: u32,
    /// Rounds whose page loaded, failed or closed.
    pub(crate) resolved: Vec<u32>,
    /// `isCMPRequired()` calls waiting for a round.
    pub(crate) waiters: Vec<(u32, oneshot::Sender<()>)>,
    /// Guests may navigate (D.6.5).
    pub(crate) gate_open: bool,
    pub(crate) hidden: BTreeMap<String, Hidden>,
    /// The default-consent window was opened this launch.
    pub(crate) default_opened: bool,
    /// A non-empty `params` body was logged.
    pub(crate) params_logged: bool,
    /// The last `cmp-eu-only` answer said consent is not required
    /// (`isCMPRequired()` is `false`, D.6.2).
    pub(crate) not_required: bool,
    /// The origin of the settings window's `cmpURL` when it is not an
    /// Overwolf page: the window may load it (D.6.4).
    pub(crate) settings_origin: Option<String>,
}

impl ConsentCore {
    /// Marks `round` resolved and takes its waiters.
    fn resolve(&mut self, round: u32) -> Vec<oneshot::Sender<()>> {
        if !self.resolved.contains(&round) {
            self.resolved.push(round);
        }
        let (done, keep): (Vec<_>, Vec<_>) = self.waiters.drain(..).partition(|(r, _)| *r == round);
        self.waiters = keep;
        done.into_iter().map(|(_, tx)| tx).collect()
    }
}

/// The label of startup round `n`: the first is `ow-cmp-startup`.
fn startup_label(round: u32) -> String {
    if round <= 1 {
        CMP_STARTUP_LABEL.to_owned()
    } else {
        format!("{CMP_STARTUP_LABEL}-{round}")
    }
}

/// `#RRGGBB` or `#RRGGBBAA` as a colour.
fn parse_color(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 => Some(Color(byte(0)?, byte(2)?, byte(4)?, 255)),
        8 => Some(Color(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
        _ => None,
    }
}

/// Whether `url` is a consent page under the `cmp_event` scope (D.6.4).
pub(crate) fn in_cmp_scope(url: &Url) -> bool {
    url.scheme() == "https" && url.as_str().starts_with(CMP_SCOPE)
}

impl<R: Runtime> Host<R> {
    fn consent_facts_owned(&self) -> (String, String) {
        (self.guest_ow_version(), self.info.manifest.version.clone())
    }

    /// Whether ad guests may start their first navigation (D.6.5).
    pub(crate) fn consent_gate_open(self: &Arc<Self>) -> bool {
        self.with_core(|c| c.consent.gate_open)
    }

    /// `RunEvent::Ready`: the first `cmp-eu-only` request and startup
    /// window of this launch (D.6.1, D.6.2).
    pub(crate) fn start_consent(self: &Arc<Self>) {
        let first = self.with_core(|c| !std::mem::replace(&mut c.consent.started, true));
        if first {
            // Before any consent page can save `cmp` (F.2 key order).
            self.record_first_launch();
            let round = self.with_core(|c| {
                c.consent.rounds += 1;
                c.consent.rounds
            });
            self.spawn_round(round);
        }
    }

    /// Sends the `cmp-eu-only` request of `round`, then opens its startup
    /// window whatever the outcome.
    fn spawn_round(self: &Arc<Self>, round: u32) {
        let host = self.clone();
        tauri::async_runtime::spawn(async move {
            let (cacheable, required) = if host.analytics.user_enabled() {
                let request = host.analytics.reporter().cmp_eu_only();
                let body = match host.analytics.dispatcher.send(request, false).await {
                    Ok(r) => Some(r.body),
                    Err(err) => {
                        host.log(LogLevel::Debug, &format!("cmp-eu-only failed: {err}"));
                        None
                    }
                };
                let outcome = eu_only_outcome(body.as_deref());
                if let Some(p) = outcome.params {
                    let first =
                        host.with_core(|c| !std::mem::replace(&mut c.consent.params_logged, true));
                    if first {
                        host.log(LogLevel::Debug, &format!("cmp-eu-only params: {p}"));
                    }
                }
                (outcome.cacheable, outcome.cmp_required)
            } else {
                // The ow-tauri user switch is off: no request; the window
                // opens as after a failed request.
                (true, true)
            };
            host.with_core(|c| {
                c.consent.not_required = !required;
                if c.consent.mode == CacheMode::Pending || round == 1 {
                    c.consent.mode = if cacheable {
                        CacheMode::Cached
                    } else {
                        CacheMode::Uncached
                    };
                }
            });
            host.open_startup_window(round);
        });
    }

    /// The hidden consent window of startup round `round` (D.6.1): the
    /// consent page, or the clearing page when consent is not required
    /// (D.6.2).
    fn open_startup_window(self: &Arc<Self>, round: u32) {
        let label = startup_label(round);
        if self.with_core(|c| c.consent.not_required) {
            self.open_startup_url(&label, &clear_consent_url(), round);
            return;
        }
        let stored = self
            .ow_electron
            .read()
            .state
            .cmp
            .and_then(|c| c.unified_consent_string);
        let (ow_version, app_version) = self.consent_facts_owned();
        let facts = ConsentFacts {
            uid: &self.info.identity.uid,
            app_name: &self.info.manifest.product_name,
            muid: &self.info.muid,
            muid_v2: &self.info.muid_v2,
            ow_version: &ow_version,
            app_version: &app_version,
        };
        self.open_startup_url(&label, &startup_url(&facts, stored.as_deref()), round);
    }

    fn open_startup_url(self: &Arc<Self>, label: &str, url: &str, round: u32) {
        if let Err(err) = self.open_hidden_window(label, url, Some(round)) {
            self.log(
                LogLevel::Warn,
                &format!("the startup consent window failed: {err}"),
            );
            self.with_core(|c| c.consent.hidden.remove(label));
            self.consent_window_failed(label, Some(round));
        }
    }

    /// The `cmp.js` shim with this launch's configuration (D.6.6).
    fn cmp_script(&self) -> String {
        let ad_optimization = self.ow_tauri.get().ad_optimization.unwrap_or(false);
        crate::ads::splice_config(CMP_JS, CMP_CONFIG_TOKEN, &cmp_config(ad_optimization))
            .unwrap_or_else(|| {
                self.log(LogLevel::Error, "cmp.js has no configuration token");
                CMP_JS.to_owned()
            })
    }

    /// Common settings of the three consent windows: ads data store, `<UA>`,
    /// `cmp.js`, popups opened in the system browser.
    fn cmp_builder<'a>(
        self: &'a Arc<Self>,
        label: &'a str,
        url: Url,
    ) -> WebviewWindowBuilder<'a, R, AppHandle<R>> {
        let weak = Arc::downgrade(self);
        #[cfg_attr(
            not(windows),
            expect(unused_mut, reason = "Windows adds the ads environment")
        )]
        let mut builder = WebviewWindowBuilder::new(&self.app, label, WebviewUrl::External(url))
            .user_agent(&self.user_agent())
            .initialization_script(self.cmp_script())
            .on_new_window(move |url, _| {
                if let Some(host) = weak.upgrade()
                    && url.scheme() == "https"
                {
                    let _ = host.open_in_browser(&url);
                }
                NewWindowResponse::Deny
            });
        #[cfg(windows)]
        {
            builder = builder
                .data_directory(self.info.ads_data_dir.clone())
                .additional_browser_args(&self.ads_browser_args());
        }
        builder
    }

    /// A hidden 1 x 32 consent window (D.6.1, D.6.4).
    fn open_hidden_window(
        self: &Arc<Self>,
        label: &str,
        url: &str,
        round: Option<u32>,
    ) -> Result<(), Error> {
        let url = Url::parse(url).map_err(|e| Error::invalid_argument(e.to_string()))?;
        let now = self.now();
        let deadline = now + self.info.config.consent.ready_timeout_ms;
        self.with_core(|c| {
            c.consent.hidden.insert(
                label.to_owned(),
                Hidden {
                    round,
                    deadline,
                    closing: false,
                },
            );
        });
        let (w, h) = HIDDEN_WINDOW_SIZE;
        let window = self
            .cmp_builder(label, url)
            .title(&self.info.manifest.product_name)
            .inner_size(w, h)
            .center()
            .visible(false)
            .focused(false)
            .focusable(false)
            .decorations(false)
            .skip_taskbar(true);
        // Hidden: building it must not activate the app.
        let window = crate::platform::webview::without_app_activation(|| window.build())
            .map_err(Error::from)?;
        crate::lab::after_build(&window, false);
        crate::lab::record(
            "wc-events.jsonl",
            || serde_json::json!({ "kind": "created", "label": label, "type": "cmp", "url": window.url().map(|u| u.to_string()).unwrap_or_default() }),
        );
        self.install_consent_hooks(&window);
        Ok(())
    }

    fn install_consent_hooks(self: &Arc<Self>, window: &WebviewWindow<R>) {
        let reports: Arc<dyn crate::platform::webview::GuestReports> =
            Arc::new(super::ads::Reports(Arc::downgrade(self)));
        if let Err(err) =
            crate::platform::webview::install_guest_hooks(window.as_ref(), None, reports)
        {
            self.log(
                LogLevel::Debug,
                &format!("consent window hooks failed: {err}"),
            );
        }
    }

    /// A page load of a consent window: the startup page's
    /// `did-finish-load` resolves `isCMPRequired()` (D.6.1).
    pub(crate) fn consent_page_load(
        self: &Arc<Self>,
        label: &str,
        event: PageLoadEvent,
        url: &Url,
    ) {
        if event != PageLoadEvent::Finished || !url.as_str().starts_with(STARTUP_CMP_URL) {
            return;
        }
        let waiters = self.with_core(|c| {
            let round = c.consent.hidden.get(label).and_then(|h| h.round)?;
            Some(c.consent.resolve(round))
        });
        for tx in waiters.into_iter().flatten() {
            let _ = tx.send(());
        }
    }

    /// A consent window's main-frame load failed or its process ended:
    /// `isCMPRequired()` resolves, guests may navigate, the window closes.
    pub(crate) fn consent_load_failed(self: &Arc<Self>, label: &str) {
        let round = self.with_core(|c| c.consent.hidden.get(label).and_then(|h| h.round));
        self.consent_window_failed(label, round);
        self.close_hidden(label);
    }

    fn consent_window_failed(self: &Arc<Self>, label: &str, round: Option<u32>) {
        let waiters = self.with_core(|c| {
            if round == Some(1) || label == CMP_STARTUP_LABEL {
                c.consent.gate_open = true;
            }
            round.map(|r| c.consent.resolve(r)).unwrap_or_default()
        });
        for tx in waiters {
            let _ = tx.send(());
        }
    }

    /// Closes a hidden consent window. After a startup window, the cookie
    /// fallback runs first (D.6.3).
    pub(crate) fn close_hidden(self: &Arc<Self>, label: &str) {
        let target = self.with_core(|c| {
            c.consent.hidden.get_mut(label).and_then(|h| {
                if h.closing {
                    None
                } else {
                    h.closing = true;
                    Some(h.round)
                }
            })
        });
        let Some(round) = target else { return };
        let host = self.clone();
        let label = label.to_owned();
        tauri::async_runtime::spawn(async move {
            if round.is_some()
                && host.info.config.consent.host_cookie_fallback == CookieFallback::Auto
            {
                host.cookie_fallback(&label).await;
            }
            if let Some(w) = host.app.get_webview_window(&label) {
                let _ = w.destroy();
            }
            host.consent_window_gone(&label);
        });
    }

    /// D.6.3: when both consent cookies are missing from the ads data
    /// store, writes them from the stored `cmp` values. Runs off the main
    /// thread (WebView2 cookie access must not block it).
    async fn cookie_fallback(self: &Arc<Self>, label: &str) {
        let Some(window) = self.app.get_webview_window(label) else {
            return;
        };
        let Some(cookies) = self.ads_store_cookies("https://www.overwolf.com/").await else {
            self.log(
                LogLevel::Debug,
                "consent cookie check skipped: the ads data store did not answer",
            );
            return;
        };
        if cookies
            .iter()
            .any(|(name, _)| name == "euconsent-v2" || name == "acconsent")
        {
            return;
        }
        let stored = self.ow_electron.read().state.cmp.unwrap_or_default();
        let (tcf, ac) = cookie_values(
            stored.cmp_string.as_deref(),
            stored.unified_consent_string.as_deref(),
        );
        let wanted: Vec<_> = [("euconsent-v2", tcf), ("acconsent", ac)]
            .into_iter()
            .filter_map(|(name, value)| value.map(|v| consent_cookie(name, &v)))
            .collect();
        if wanted.is_empty() {
            return;
        }
        let results = tauri::async_runtime::spawn_blocking(move || {
            wanted
                .into_iter()
                .map(|c| (c.name().to_owned(), window.set_cookie(c)))
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        let mut wrote = false;
        for (name, result) in results {
            match result {
                Ok(()) => wrote = true,
                Err(err) => self.log(LogLevel::Warn, &format!("writing {name} failed: {err}")),
            }
        }
        if wrote {
            self.log(
                LogLevel::Info,
                "consent cookies written by the host (hostCookieFallback)",
            );
        }
    }

    /// A consent window is gone (closed by the page, the timeout, or the
    /// OS).
    pub(crate) fn consent_window_gone(self: &Arc<Self>, label: &str) {
        let removed = self.with_core(|c| c.consent.hidden.remove(label));
        if let Some(h) = removed {
            self.consent_window_failed(label, h.round);
        }
    }

    /// Timer step: hidden windows still open after
    /// `consent.readyTimeoutMs` are closed (D.6.1).
    pub(crate) fn consent_tick(self: &Arc<Self>, now: u64) {
        let expired: Vec<String> = self.with_core(|c| {
            c.consent
                .hidden
                .iter()
                .filter(|(_, h)| !h.closing && now >= h.deadline)
                .map(|(l, _)| l.clone())
                .collect()
        });
        for l in expired {
            self.log(LogLevel::Debug, &format!("{l} timed out; closing it"));
            self.close_hidden(&l);
        }
    }

    /// `is_cmp_required` (A.2.2, D.6.2): after this launch's request and
    /// startup page load, `false` when the request answered `no-cmp`, else
    /// `true`; with a `{}` body every call runs its own round.
    pub(crate) async fn is_cmp_required(self: &Arc<Self>) -> bool {
        enum Wait {
            Now,
            Round(u32, bool),
        }
        let wait = self.with_core(|c| match c.consent.mode {
            CacheMode::Cached if c.consent.resolved.contains(&1) => Wait::Now,
            CacheMode::Uncached => {
                c.consent.rounds += 1;
                Wait::Round(c.consent.rounds, true)
            }
            CacheMode::Cached | CacheMode::Pending => Wait::Round(1, false),
        });
        let required = |host: &Arc<Self>| host.with_core(|c| !c.consent.not_required);
        let Wait::Round(round, new) = wait else {
            return required(self);
        };
        let (tx, rx) = oneshot::channel();
        let already = self.with_core(|c| {
            if c.consent.resolved.contains(&round) {
                true
            } else {
                c.consent.waiters.push((round, tx));
                false
            }
        });
        if new {
            self.spawn_round(round);
        } else {
            // A call before `RunEvent::Ready` starts the first round.
            self.start_consent();
        }
        if !already {
            let _ = rx.await;
        }
        required(self)
    }

    /// The hidden default-consent window of the first settings call of a
    /// launch (D.6.4); none when consent is not required (D.6.2).
    fn open_default_consent_once(self: &Arc<Self>) {
        if self.with_core(|c| c.consent.not_required) {
            return;
        }
        if !self.with_core(|c| std::mem::replace(&mut c.consent.default_opened, true))
            && let Err(err) =
                self.open_hidden_window(CMP_DEFAULT_LABEL, &default_consent_url(), None)
        {
            self.log(
                LogLevel::Warn,
                &format!("the default-consent window failed: {err}"),
            );
            self.with_core(|c| c.consent.hidden.remove(CMP_DEFAULT_LABEL));
        }
    }

    /// `open_ad_privacy_settings_window` / `open_cmp_window` (D.6.4).
    pub(crate) fn open_cmp_window(
        self: &Arc<Self>,
        options: &CmpWindowOptions,
    ) -> Result<(), Error> {
        let base = self
            .info
            .config
            .consent
            .cmp_url
            .clone()
            .or_else(|| options.cmp_url.clone())
            .unwrap_or_else(|| DEFAULT_CMP_URL.to_owned());
        let base_url = Url::parse(&base)
            .ok()
            .filter(|u| u.scheme() == "https")
            .ok_or_else(|| Error::invalid_argument("cmpURL must be an https: URL."))?;
        // A custom cmpURL off Overwolf loads; only saving is refused there,
        // by the capability scope of `cmp_event` (D.6.4).
        let custom = (!is_overwolf_url(&base_url)).then(|| base_url.origin().ascii_serialization());
        self.with_core(|c| c.consent.settings_origin = custom);
        if let Some(w) = self.app.get_webview_window(CMP_SETTINGS_LABEL) {
            if crate::lab::may_focus() {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            return Ok(());
        }
        self.open_default_consent_once();
        let (ow_version, app_version) = self.consent_facts_owned();
        let facts = ConsentFacts {
            uid: &self.info.identity.uid,
            app_name: &self.info.manifest.product_name,
            muid: &self.info.muid,
            muid_v2: &self.info.muid_v2,
            ow_version: &ow_version,
            app_version: &app_version,
        };
        let page = settings_url(
            &base,
            &facts,
            options.tab.as_deref().unwrap_or("purposes"),
            options.language.as_deref().unwrap_or("en"),
            self.analytics.first_launch(),
            self.with_core(|c| !c.consent.not_required),
        );
        let page = Url::parse(&page).map_err(|e| Error::invalid_argument(e.to_string()))?;
        let background = options
            .background_color
            .clone()
            .unwrap_or_else(|| DEFAULT_BACKGROUND.to_owned());
        let preloader = preloader_url(
            &background,
            options
                .pre_loader_spinner_color
                .as_deref()
                .unwrap_or("#FFFFFF"),
        );
        let preloader = Url::parse(&preloader).map_err(|e| Error::backend(e.to_string()))?;
        let mut builder = self
            .cmp_builder(CMP_SETTINGS_LABEL, preloader)
            .title("CMP")
            .inner_size(
                options.width.unwrap_or(800.0),
                options.height.unwrap_or(800.0),
            )
            .resizable(false)
            .maximizable(false)
            .minimizable(true);
        // Lab windows are built hidden and shown invisible (feature `lab`).
        builder = crate::lab::window_builder(builder, true);
        if let Some(color) = parse_color(&background) {
            builder = builder.background_color(color);
        }
        // As Electron's BrowserWindow: x and y apply only together.
        if options.x.is_some() != options.y.is_some() {
            self.log(
                LogLevel::Debug,
                "openCMPWindow: x and y apply only together; the window is centered",
            );
        }
        builder = match (options.x, options.y) {
            (Some(x), Some(y)) => builder.position(x, y),
            _ if options.center == Some(false) => builder,
            _ => builder.center(),
        };
        if options.modal == Some(true)
            && let Some(parent) = options
                .parent_id
                .and_then(|id| self.app.get_webview_window(&crate::window::ui_label(id)))
        {
            builder = builder.parent(&parent).map_err(Error::from)?;
        }
        let window = builder.build().map_err(Error::from)?;
        crate::lab::after_build(&window, true);
        crate::lab::record(
            "wc-events.jsonl",
            || serde_json::json!({ "kind": "created", "label": CMP_SETTINGS_LABEL, "type": "cmp", "url": page.as_str() }),
        );
        let _ = window.navigate(page);
        Ok(())
    }

    /// The navigation policy of a consent window (D.6.4): Overwolf pages,
    /// in the settings window also the origin of a custom `cmpURL`, `data:`
    /// (the preloader) and `about:blank`.
    pub(crate) fn cmp_navigation(self: &Arc<Self>, label: &str, url: &Url) -> bool {
        let ok = match url.scheme() {
            "https" => {
                is_overwolf_url(url)
                    || (label == CMP_SETTINGS_LABEL
                        && self.with_core(|c| c.consent.settings_origin.clone())
                            == Some(url.origin().ascii_serialization()))
            }
            "data" => true,
            "about" => url.as_str() == "about:blank",
            _ => false,
        };
        if !ok {
            self.log(
                LogLevel::Debug,
                &format!(
                    "{label}: navigation to a {} URL outside its pages cancelled",
                    url.scheme()
                ),
            );
        }
        ok
    }

    /// `cmp_event` (A.2.7, D.6.6) from the consent window `label`, whose
    /// document is `url`.
    pub(crate) fn cmp_event(
        self: &Arc<Self>,
        label: &str,
        url: Option<&Url>,
        name: CmpEventName,
        data: Option<CmpEventData>,
    ) -> Result<(), Error> {
        if !url.is_some_and(in_cmp_scope) {
            self.log(
                LogLevel::Warn,
                &format!("{label}: cmp_event from a page outside {CMP_SCOPE} refused"),
            );
            return Err(Error::forbidden("This page cannot save consent."));
        }
        let data = data.unwrap_or_default();
        // An empty string clears the stored consent: the clearing startup
        // page saves "" when consent is not required (D.6.2) [OBS].
        let consent = || {
            data.consent
                .clone()
                .filter(|s| s.is_empty() || valid_consent(s))
                .ok_or_else(|| Error::invalid_argument("Invalid consent string."))
        };
        let write = |block: &CmpBlock| {
            self.ow_electron
                .write_cmp(block)
                .map_err(|e| Error::io(e.to_string()))
        };
        match name {
            CmpEventName::Ready => {
                self.log(LogLevel::Debug, &format!("{label}: consent page ready"));
            }
            CmpEventName::SaveConsent => {
                let s = consent()?;
                // ow-electron stores timeStamp 0 with a cleared string [OBS].
                let secs = if s.is_empty() {
                    0
                } else {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs())
                };
                write(&CmpBlock {
                    cmp_string: Some(s.clone()),
                    time_stamp: Some(secs),
                    unified_consent_string: None,
                })?;
                self.deliver_to_guests("consent", Some(&Value::String(s)));
            }
            CmpEventName::SaveUnifiedConsent => {
                let stored = stored_unified(&consent()?);
                write(&CmpBlock {
                    cmp_string: None,
                    time_stamp: None,
                    unified_consent_string: Some(stored.clone()),
                })?;
                self.deliver_to_guests("consent", Some(&Value::String(stored)));
            }
            CmpEventName::EnableAdOptimization => {
                let enabled = data
                    .enabled
                    .ok_or_else(|| Error::invalid_argument("enabled must be a boolean."))?;
                self.ow_tauri
                    .update(|s| s.ad_optimization = Some(enabled))
                    .map_err(|e| Error::from_io("ow-tauri.json", &e))?;
            }
            CmpEventName::Close => {
                if self.with_core(|c| c.consent.hidden.contains_key(label)) {
                    self.close_hidden(label);
                } else if let Some(w) = self.app.get_webview_window(label) {
                    let _ = w.destroy();
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_colors() {
        assert_eq!(startup_label(1), "ow-cmp-startup");
        assert_eq!(startup_label(3), "ow-cmp-startup-3");
        assert_eq!(parse_color("#0D0D0D"), Some(Color(13, 13, 13, 255)));
        assert_eq!(parse_color("#0D0D0D80"), Some(Color(13, 13, 13, 128)));
        assert_eq!(parse_color("red"), None);
        let scope =
            Url::parse("https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/cmp.html")
                .unwrap();
        assert!(in_cmp_scope(&scope));
        assert!(!in_cmp_scope(
            &Url::parse("https://evil.example/monsdk/electron/").unwrap()
        ));
    }

    #[test]
    fn the_shim_carries_its_configuration_token() {
        let script =
            crate::ads::splice_config(CMP_JS, CMP_CONFIG_TOKEN, &cmp_config(true)).unwrap();
        assert!(script.contains(r#"{"adOptimization":true}"#));
    }

    #[test]
    fn resolve_takes_round_waiters() {
        let mut c = ConsentCore::default();
        let (a, mut ra) = oneshot::channel();
        let (b, mut rb) = oneshot::channel();
        c.waiters.push((1, a));
        c.waiters.push((2, b));
        for tx in c.resolve(1) {
            tx.send(()).unwrap();
        }
        assert!(ra.try_recv().is_ok());
        assert!(rb.try_recv().is_err());
        assert_eq!(c.waiters.len(), 1);
        assert_eq!(c.resolved, vec![1]);
    }
}
