//! Consent (CONTRACT A.2.2, A.2.7, D.6; DESIGN §4.7): the `cmp-eu-only`
//! request and the hidden startup consent window of every launch,
//! `isCMPRequired()`, the ad privacy settings and default-consent windows,
//! the `cmp_event` command, the consent cookie fallback, and the gate the
//! ad guests' first navigation waits for.
//!
//! Everything starts at `RunEvent::Ready` ([`start`]): `cmp-eu-only` goes
//! out on the request lane right after the launch burst (both wait for the
//! final `<UA>`), bounded by `consent.euOnlyTimeoutMs` (a timeout counts as
//! a failed request: consent is required). The startup window opens
//! whatever the outcome (D.6.1).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::window::Color;
use tauri::{Runtime, Webview, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tokio::sync::{oneshot, watch};
use url::Url;

use super::{Core, LOG_TARGET, lock};
use crate::ads::is_overwolf_url;
use crate::config::CookieFallback;
use crate::consent::{
    CMP_CONFIG_TOKEN, CMP_SCOPE, CmpEventData, CmpEventName, ConsentFacts, DEFAULT_BACKGROUND,
    DEFAULT_CMP_URL, HIDDEN_WINDOW_SIZE, STARTUP_CMP_URL, clear_consent_url, cmp_config,
    consent_cookie, cookie_values, default_consent_url, eu_only_outcome, preloader_url,
    settings_url, startup_url, stored_unified, valid_consent,
};
use crate::error::{Error, Result};
use crate::state::ow_electron::CmpBlock;
use crate::types::CmpWindowOptions;

/// The consent page shim (D.6.6), built from `packages/guest-shims`.
pub(crate) const CMP_JS: &str = include_str!("../../js/cmp.js");

/// The settings window's label (D.6.4).
pub(crate) const CMP_SETTINGS_LABEL: &str = "ow-cmp";

/// The first startup window's label (D.6.1); later rounds append `-<n>`.
pub(crate) const CMP_STARTUP_LABEL: &str = "ow-cmp-startup";

/// The hidden default-consent window's label (D.6.4).
pub(crate) const CMP_DEFAULT_LABEL: &str = "ow-cmp-default";

/// Behaviours that wait for a W3 harness observation of ow-electron
/// (DESIGN §4.2, `last-window-during-consent`). Each default is the
/// behaviour DESIGN-v2 specifies until then.
pub(crate) mod pending_observation {
    /// What happens to an open startup consent window when the last app
    /// window is destroyed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum StartupWindowAtLastWindow {
        /// It closes as soon as its page has saved (or asked to close), or
        /// at its `consent.readyTimeoutMs` bound.
        AfterSave,
        /// It closes at once.
        AtOnce,
    }

    /// The current rule.
    pub(crate) const STARTUP_WINDOW_AT_LAST_WINDOW: StartupWindowAtLastWindow =
        StartupWindowAtLastWindow::AfterSave;
}

/// A function called with the stored consent value after every save
/// (`saveConsent`, `saveUnifiedConsent`): the ads host pushes it to the
/// running guests (D.5).
pub(crate) type ConsentListener = Arc<dyn Fn(&str) + Send + Sync>;

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hidden {
    /// The startup round, `None` for `ow-cmp-default`.
    pub(crate) round: Option<u32>,
    /// The close was started.
    pub(crate) closing: bool,
    /// The page saved consent or asked to close.
    pub(crate) saved: bool,
}

/// The consent state of one launch.
#[derive(Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent once-per-launch markers"
)]
struct ConsentState {
    started: bool,
    mode: CacheMode,
    /// Startup rounds begun.
    rounds: u32,
    /// Rounds whose page loaded, failed or closed.
    resolved: Vec<u32>,
    /// `isCMPRequired()` calls waiting for a round.
    waiters: Vec<(u32, oneshot::Sender<()>)>,
    hidden: BTreeMap<String, Hidden>,
    /// Every consent window label the plugin created (D10).
    created: BTreeSet<String>,
    default_opened: bool,
    params_logged: bool,
    /// The settings window's `cmpURL` origin when it is not an Overwolf
    /// page: the window may load it (D.6.4).
    settings_origin: Option<String>,
    /// The last app window was destroyed (DESIGN §4.2).
    app_gone: bool,
}

impl ConsentState {
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

/// The consent service of one app.
pub(crate) struct ConsentCore {
    /// The last `cmp-eu-only` answer said consent is not required (D.6.2).
    not_required: AtomicBool,
    state: Mutex<ConsentState>,
    /// Whether ad guests may make their first navigation (D.6.5).
    gate: watch::Sender<bool>,
    listeners: Mutex<Vec<ConsentListener>>,
}

impl std::fmt::Debug for ConsentCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsentCore")
            .field("not_required", &self.not_required.load(Ordering::SeqCst))
            .field("gate_open", &*self.gate.borrow())
            .finish_non_exhaustive()
    }
}

impl Default for ConsentCore {
    fn default() -> Self {
        ConsentCore {
            not_required: AtomicBool::new(false),
            state: Mutex::new(ConsentState::default()),
            gate: watch::channel(false).0,
            listeners: Mutex::new(Vec::new()),
        }
    }
}

/// The label of startup round `n`: the first is `ow-cmp-startup`.
pub(crate) fn startup_label(round: u32) -> String {
    if round <= 1 {
        CMP_STARTUP_LABEL.to_owned()
    } else {
        format!("{CMP_STARTUP_LABEL}-{round}")
    }
}

/// `#RRGGBB` or `#RRGGBBAA` as a colour.
pub(crate) fn parse_color(s: &str) -> Option<Color> {
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

/// The navigation policy of a consent window (D.6.4): Overwolf pages, in
/// the settings window also `settings_origin` (a custom `cmpURL`'s
/// origin), `data:` (the preloader) and `about:blank`.
pub(crate) fn navigation_allowed(label: &str, url: &Url, settings_origin: Option<&str>) -> bool {
    match url.scheme() {
        "https" => {
            is_overwolf_url(url)
                || (label == CMP_SETTINGS_LABEL
                    && settings_origin == Some(url.origin().ascii_serialization().as_str()))
        }
        "data" => true,
        "about" => url.as_str() == "about:blank",
        _ => false,
    }
}

/// The consent page facts of this launch (D.6.1, D.6.4).
fn with_facts<R: Runtime, T>(core: &Core<R>, f: impl FnOnce(&ConsentFacts<'_>) -> T) -> T {
    let id = &core.identity;
    let label = super::analytics::host_label(&id.config.analytics);
    let ow_version = label.ow_version();
    f(&ConsentFacts {
        uid: &id.app.uid,
        app_name: &id.app.name,
        muid: &id.machine.muid,
        muid_v2: &id.machine.muid_v2,
        ow_version: &ow_version,
        app_version: &id.app.version,
    })
}

impl ConsentCore {
    /// The last known `isCMPRequired()` answer (`true` until the first
    /// round says otherwise).
    pub(crate) fn last_answer(&self) -> bool {
        !self.not_required.load(Ordering::SeqCst)
    }

    /// Whether the ad guests may make their first navigation (D.6.5).
    #[allow(dead_code, reason = "the ads host (W2-A) checks it")]
    pub(crate) fn is_gate_open(&self) -> bool {
        *self.gate.borrow()
    }

    /// A receiver that changes to `true` when the gate opens (D.6.5); the
    /// ads host waits on it for at most 3 s from a mount.
    #[allow(dead_code, reason = "the ads host (W2-A) waits on it")]
    pub(crate) fn subscribe_gate(&self) -> watch::Receiver<bool> {
        self.gate.subscribe()
    }

    /// Adds a function called with the stored value after every consent
    /// save (D.5).
    #[allow(dead_code, reason = "the ads host (W2-A) pushes consent to the guests")]
    pub(crate) fn add_consent_listener(&self, listener: ConsentListener) {
        lock(&self.listeners).push(listener);
    }

    /// Whether the plugin created the consent window `label` (D10).
    pub(crate) fn owns_window(&self, label: &str) -> bool {
        lock(&self.state).created.contains(label)
    }

    /// Opens the gate (D.6.5) once. Lab trace: `consent-gate` with `reason`.
    fn open_gate(&self, reason: &str) {
        if self.gate.send_replace(true) {
            return;
        }
        log::debug!(target: LOG_TARGET, "consent gate open ({reason})");
        crate::lab::record(
            "wc-events.jsonl",
            || serde_json::json!({ "kind": "consent-gate", "reason": reason }),
        );
    }

    /// `isCMPRequired()` (A.2.2, D.6.2): after this launch's request and
    /// startup page load, `false` when the request answered `no-cmp`, else
    /// `true`; with a `{}` body every call runs a round of its own. Before
    /// `RunEvent::Ready` nothing is known yet and the answer is `true` at
    /// once (the consent flow starts at Ready). Never fails.
    pub(crate) async fn is_cmp_required<R: Runtime>(&self, core: &Arc<Core<R>>) -> bool {
        enum Wait {
            Now,
            Round(u32, bool),
        }
        if !core.lifecycle.is_started() {
            return self.last_answer();
        }
        let (tx, rx) = oneshot::channel();
        let wait = {
            let mut s = lock(&self.state);
            let wait = match s.mode {
                CacheMode::Cached if s.resolved.contains(&1) => Wait::Now,
                CacheMode::Uncached => {
                    s.rounds += 1;
                    Wait::Round(s.rounds, true)
                }
                CacheMode::Cached | CacheMode::Pending => Wait::Round(1, false),
            };
            if let Wait::Round(round, _) = wait
                && !s.resolved.contains(&round)
            {
                s.waiters.push((round, tx));
                Some(wait)
            } else {
                None
            }
        };
        if let Some(Wait::Round(round, new)) = wait {
            if new {
                spawn_round(core, round);
            }
            let _ = rx.await;
        }
        self.last_answer()
    }

    /// `openAdPrivacySettingsWindow` / `openCMPWindow` (D.6.4). The
    /// `cmpURL` of a JavaScript caller was checked against
    /// `consent.allowedCmpOrigins` before. `caller_window` is the window of
    /// a JavaScript caller: a `modal` window without `parent` is parented to
    /// it.
    ///
    /// # Errors
    ///
    /// `invalid-argument` for a `cmpURL` that is not `https:`, `not-found`
    /// for an unknown `parent`, `backend` when the window cannot be built.
    pub(crate) fn open_settings_window<R: Runtime>(
        &self,
        core: &Arc<Core<R>>,
        options: &CmpWindowOptions,
        caller_window: Option<&str>,
    ) -> Result<()> {
        let base = core
            .identity
            .config
            .consent
            .cmp_url
            .clone()
            .or_else(|| options.cmp_url.clone())
            .unwrap_or_else(|| DEFAULT_CMP_URL.to_owned());
        let base_url = Url::parse(&base)
            .ok()
            .filter(|u| u.scheme() == "https")
            .ok_or_else(|| Error::invalid_argument("cmpURL must be an https: URL"))?;
        // `parent` is a Tauri window label, found even while that window
        // hosts an ad (SEC-M7, SPA F3).
        let parent_label = options
            .parent
            .clone()
            .or_else(|| (options.modal == Some(true)).then(|| caller_window.map(str::to_owned))?);
        let parent = match parent_label {
            Some(label) => Some(
                crate::compat::window(&core.app, &label)
                    .filter(|_| !crate::config::is_reserved_label(&label))
                    .ok_or_else(|| Error::not_found(format!("no window {label}")))?,
            ),
            None => None,
        };
        // A custom cmpURL off Overwolf loads; only saving is refused there,
        // by the scope of `cmp_event` (D.6.4).
        let custom = (!is_overwolf_url(&base_url)).then(|| base_url.origin().ascii_serialization());
        lock(&self.state).settings_origin = custom;
        if let Some(w) = crate::compat::window(&core.app, CMP_SETTINGS_LABEL) {
            if crate::lab::may_focus() {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            return Ok(());
        }
        self.open_default_consent_once(core);
        let first_launch = core.identity.first_launch;
        let page = with_facts(core, |facts| {
            settings_url(
                &base,
                facts,
                options.tab.map_or("purposes", |t| t.as_str()),
                options.language.as_deref().unwrap_or("en"),
                first_launch,
                self.last_answer(),
            )
        });
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
        let mut builder = cmp_builder(core, CMP_SETTINGS_LABEL, preloader)
            .title("CMP")
            .inner_size(
                options.width.unwrap_or(800.0),
                options.height.unwrap_or(800.0),
            )
            .resizable(false)
            .maximizable(false)
            .minimizable(true);
        if crate::lab::invisible() {
            // The invisible lab never shows a window.
            builder = builder.visible(false).focused(false);
        }
        if let Some(color) = parse_color(&background) {
            builder = builder.background_color(color);
        }
        // As Electron's BrowserWindow: x and y apply only together.
        if options.x.is_some() != options.y.is_some() {
            log::debug!(
                target: LOG_TARGET,
                "openCMPWindow: x and y apply only together; the window is centered"
            );
        }
        builder = match (options.x, options.y) {
            (Some(x), Some(y)) => builder.position(x, y),
            _ if options.center == Some(false) => builder,
            _ => builder.center(),
        };
        if let Some(parent) = &parent {
            builder = with_parent(builder, parent)?;
        }
        lock(&self.state)
            .created
            .insert(CMP_SETTINGS_LABEL.to_owned());
        let window = build_window(builder).map_err(|e| {
            lock(&self.state).created.remove(CMP_SETTINGS_LABEL);
            Error::backend(format!("the ad privacy settings window failed: {e}"))
        })?;
        crate::lab::record(
            "wc-events.jsonl",
            || serde_json::json!({ "kind": "created", "label": CMP_SETTINGS_LABEL, "type": "cmp", "url": page.as_str() }),
        );
        let _ = window.navigate(page);
        Ok(())
    }

    /// The hidden default-consent window of the first settings call of a
    /// launch (D.6.4); none when consent is not required (D.6.2).
    fn open_default_consent_once<R: Runtime>(&self, core: &Arc<Core<R>>) {
        if !self.last_answer() || std::mem::replace(&mut lock(&self.state).default_opened, true) {
            return;
        }
        if let Err(err) = open_hidden_window(core, CMP_DEFAULT_LABEL, &default_consent_url(), None)
        {
            log::warn!(target: LOG_TARGET, "the default-consent window failed: {err}");
        }
    }

    /// A page load of a consent window: the startup page's finished load
    /// resolves `isCMPRequired()` (D.6.1).
    pub(crate) fn page_load<R: Runtime>(
        &self,
        _core: &Arc<Core<R>>,
        webview: &Webview<R>,
        event: PageLoadEvent,
        url: &Url,
    ) {
        if event != PageLoadEvent::Finished || !url.as_str().starts_with(STARTUP_CMP_URL) {
            return;
        }
        let waiters = {
            let mut s = lock(&self.state);
            match s.hidden.get(webview.label()).and_then(|h| h.round) {
                Some(round) => s.resolve(round),
                None => Vec::new(),
            }
        };
        for tx in waiters {
            let _ = tx.send(());
        }
    }

    /// The navigation policy of consent window `label` (D.6.4). A webview
    /// with a reserved label the plugin did not create may not navigate.
    pub(crate) fn navigation<R: Runtime>(
        &self,
        _core: &Arc<Core<R>>,
        label: &str,
        url: &Url,
    ) -> bool {
        let (owned, origin) = {
            let s = lock(&self.state);
            let owned = s.created.contains(label);
            (owned, s.settings_origin.clone())
        };
        let ok = owned && navigation_allowed(label, url, origin.as_deref());
        if !ok {
            log::debug!(
                target: LOG_TARGET,
                "{label}: navigation to a {} URL outside its pages cancelled",
                url.scheme()
            );
        }
        ok
    }

    /// A window event: a consent window that is gone resolves its round and
    /// may open the gate; the destruction of the last app window applies
    /// the last-window rule (DESIGN §4.2): Tauri asks to exit only when no
    /// window at all is left, and the consent windows count.
    pub(crate) fn window_event<R: Runtime>(
        &self,
        core: &Arc<Core<R>>,
        label: &str,
        event: &WindowEvent,
    ) {
        if !matches!(event, WindowEvent::Destroyed) {
            return;
        }
        if label.starts_with(crate::config::CMP_LABEL_PREFIX) {
            self.window_gone(label);
            return;
        }
        if crate::config::is_reserved_label(label) || !core.windows.is_empty() {
            return;
        }
        let startup: Vec<(String, bool)> = {
            let mut s = lock(&self.state);
            s.app_gone = true;
            s.hidden
                .iter()
                .filter(|(_, h)| h.round.is_some())
                .map(|(l, h)| (l.clone(), h.saved))
                .collect()
        };
        for l in [CMP_DEFAULT_LABEL, CMP_SETTINGS_LABEL] {
            if lock(&self.state).hidden.contains_key(l) {
                close_hidden(core, l);
            } else {
                destroy_soon(core, l);
            }
        }
        for (l, saved) in startup {
            use pending_observation::{STARTUP_WINDOW_AT_LAST_WINDOW, StartupWindowAtLastWindow};
            if saved || STARTUP_WINDOW_AT_LAST_WINDOW == StartupWindowAtLastWindow::AtOnce {
                close_hidden(core, &l);
            }
        }
    }

    /// A consent window is gone (closed by the page, the bound, or the
    /// OS): its round resolves, and the first startup window's end opens
    /// the gate.
    fn window_gone(&self, label: &str) {
        let (round, waiters) = {
            let mut s = lock(&self.state);
            s.created.remove(label);
            let round = s.hidden.remove(label).and_then(|h| h.round);
            let waiters = round.map(|r| s.resolve(r)).unwrap_or_default();
            (round, waiters)
        };
        for tx in waiters {
            let _ = tx.send(());
        }
        if round == Some(1) {
            self.open_gate("startup window closed");
        }
    }

    /// `cmp_event` (A.2.7, D.6.6) from the consent window `label`, whose
    /// document is `url`.
    ///
    /// # Errors
    ///
    /// `forbidden` outside the consent page scope, `invalid-argument` for a
    /// bad consent string or flag, `io` when the state could not be saved.
    pub(crate) fn cmp_event<R: Runtime>(
        &self,
        core: &Arc<Core<R>>,
        label: &str,
        url: Option<&Url>,
        name: CmpEventName,
        data: Option<CmpEventData>,
    ) -> Result<()> {
        if !url.is_some_and(in_cmp_scope) {
            log::warn!(target: LOG_TARGET, "{label}: cmp_event from a page outside {CMP_SCOPE} refused");
            return Err(Error::forbidden("this page cannot save consent"));
        }
        let data = data.unwrap_or_default();
        // An empty string clears the stored consent: the clearing startup
        // page saves "" when consent is not required (D.6.2, observed).
        let consent = || {
            data.consent
                .clone()
                .filter(|s| s.is_empty() || valid_consent(s))
                .ok_or_else(|| Error::invalid_argument("invalid consent string"))
        };
        let write = |block: &CmpBlock| {
            core.state
                .ow_electron
                .write_cmp(block)
                .map_err(|e| match e {
                    crate::state::ow_electron::WriteError::Io(io) => {
                        Error::from_io("Updating ow-electron.json", &io)
                    }
                    crate::state::ow_electron::WriteError::InvalidExisting => {
                        Error::backend("ow-electron.json is not valid JSON")
                    }
                })
        };
        let saved = match name {
            CmpEventName::Ready => {
                log::debug!(target: LOG_TARGET, "{label}: consent page ready");
                None
            }
            CmpEventName::SaveConsent => {
                let s = consent()?;
                // ow-electron stores timeStamp 0 with a cleared string
                // (observed).
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
                Some(s)
            }
            CmpEventName::SaveUnifiedConsent => {
                let stored = stored_unified(&consent()?);
                write(&CmpBlock {
                    cmp_string: None,
                    time_stamp: None,
                    unified_consent_string: Some(stored.clone()),
                })?;
                Some(stored)
            }
            CmpEventName::EnableAdOptimization => {
                let enabled = data
                    .enabled
                    .ok_or_else(|| Error::invalid_argument("enabled must be a boolean"))?;
                core.state
                    .ow_tauri
                    .update(|s| s.ad_optimization = Some(enabled))
                    .map_err(|e| Error::from_io("Updating ow-tauri.json", &e))?;
                None
            }
            CmpEventName::Close => {
                self.mark_saved(label);
                if lock(&self.state).hidden.contains_key(label) {
                    close_hidden(core, label);
                } else {
                    destroy_soon(core, label);
                }
                return Ok(());
            }
        };
        if let Some(value) = saved {
            let close_now = self.mark_saved(label);
            let listeners = lock(&self.listeners).clone();
            for listener in listeners {
                listener(&value);
            }
            if close_now {
                close_hidden(core, label);
            }
        }
        Ok(())
    }

    /// Records that the page of hidden window `label` saved; returns
    /// whether it closes now (the last app window is gone, DESIGN §4.2).
    fn mark_saved(&self, label: &str) -> bool {
        let mut s = lock(&self.state);
        let app_gone = s.app_gone;
        s.hidden.get_mut(label).is_some_and(|h| {
            h.saved = true;
            app_gone && h.round.is_some()
        })
    }
}

/// Starts consent at `RunEvent::Ready` (D.6.1): the first `cmp-eu-only`
/// request and startup window of this launch.
pub(crate) fn start<R: Runtime>(core: &Arc<Core<R>>) {
    let round = {
        let mut s = lock(&core.consent.state);
        if std::mem::replace(&mut s.started, true) {
            return;
        }
        s.rounds += 1;
        s.rounds
    };
    spawn_round(core, round);
}

/// Sends the `cmp-eu-only` request of `round` (bounded by
/// `consent.euOnlyTimeoutMs`), then opens its startup window whatever the
/// outcome.
fn spawn_round<R: Runtime>(core: &Arc<Core<R>>, round: u32) {
    let request = core.analytics.cmp_eu_only_request();
    // Queued now, so it follows the launch burst on the request lane.
    let pending = match (crate::lab::cmp_eu_only_stub(), request) {
        (Some(stub), _) => Some(Box::pin(async move { Ok::<_, String>(stub) })
            as crate::analytics::BoxFuture<std::result::Result<Vec<u8>, String>>),
        (None, Some(request)) => {
            let sent = core.analytics.dispatcher.send(request, false);
            Some(Box::pin(async move { sent.await.map(|r| r.body) }) as _)
        }
        // The ow-tauri user switch is off: no request; the window opens as
        // after a failed request.
        (None, None) => None,
    };
    let limit = Duration::from_millis(core.identity.config.consent.eu_only_timeout_ms);
    let weak = Arc::downgrade(core);
    tauri::async_runtime::spawn(async move {
        let (cacheable, required) = match pending {
            Some(pending) => {
                let body = match tokio::time::timeout(limit, pending).await {
                    Ok(Ok(body)) => Some(body),
                    Ok(Err(err)) => {
                        log::debug!(target: LOG_TARGET, "cmp-eu-only failed: {err}");
                        None
                    }
                    Err(_) => {
                        log::debug!(target: LOG_TARGET, "cmp-eu-only timed out; consent counts as required");
                        None
                    }
                };
                let outcome = eu_only_outcome(body.as_deref());
                if let Some(p) = &outcome.params
                    && let Some(core) = weak.upgrade()
                    && !std::mem::replace(&mut lock(&core.consent.state).params_logged, true)
                {
                    log::debug!(target: LOG_TARGET, "cmp-eu-only params: {p}");
                }
                (outcome.cacheable, outcome.cmp_required)
            }
            None => (true, true),
        };
        let Some(core) = weak.upgrade() else { return };
        if core.lifecycle.has_exited() {
            return;
        }
        core.consent.not_required.store(!required, Ordering::SeqCst);
        {
            let mut s = lock(&core.consent.state);
            if s.mode == CacheMode::Pending || round == 1 {
                s.mode = if cacheable {
                    CacheMode::Cached
                } else {
                    CacheMode::Uncached
                };
            }
        }
        open_startup_window(&core, round);
    });
}

/// The hidden consent window of startup round `round` (D.6.1): the consent
/// page, or the clearing page when consent is not required (D.6.2).
fn open_startup_window<R: Runtime>(core: &Arc<Core<R>>, round: u32) {
    let label = startup_label(round);
    let url = if core.consent.last_answer() {
        let stored = core
            .state
            .ow_electron
            .read()
            .state
            .cmp
            .and_then(|c| c.unified_consent_string);
        with_facts(core, |facts| startup_url(facts, stored.as_deref()))
    } else {
        // Consent is not required: ow-electron's guests load at once, while
        // its clearing page is still open (observed, `no-cmp`).
        if round == 1 {
            core.consent.open_gate("consent not required");
        }
        clear_consent_url()
    };
    if let Err(err) = open_hidden_window(core, &label, &url, Some(round)) {
        log::warn!(target: LOG_TARGET, "the startup consent window failed: {err}");
        core.consent.window_gone(&label);
    }
}

/// The `cmp.js` shim with this launch's configuration (D.6.6).
fn cmp_script<R: Runtime>(core: &Core<R>) -> String {
    let ad_optimization =
        crate::consent::ad_optimization(core.state.ow_tauri.get().ad_optimization);
    crate::ads::splice_config(CMP_JS, CMP_CONFIG_TOKEN, &cmp_config(ad_optimization))
        .unwrap_or_else(|| {
            log::error!(target: LOG_TARGET, "cmp.js has no configuration token");
            CMP_JS.to_owned()
        })
}

/// Common settings of the consent windows: the ads environment (Windows),
/// `<UA>`, `cmp.js`, popups opened in the system browser.
fn cmp_builder<'a, R: Runtime>(
    core: &'a Arc<Core<R>>,
    label: &'a str,
    url: Url,
) -> WebviewWindowBuilder<'a, R, tauri::AppHandle<R>> {
    #[cfg_attr(
        not(windows),
        expect(unused_mut, reason = "Windows adds the ads environment")
    )]
    let mut builder = WebviewWindowBuilder::new(&core.app, label, WebviewUrl::External(url))
        .user_agent(&core.analytics.user_agent())
        .initialization_script(cmp_script(core))
        .on_new_window(|url, _| {
            open_in_browser(&url);
            NewWindowResponse::Deny
        });
    #[cfg(windows)]
    {
        builder = builder
            .data_directory(core.identity.ads_data_dir.clone())
            // The consent windows share the ad guests' environment
            // (DESIGN §4.4.2): `WebView2` refuses a second environment on
            // the same data folder with other arguments.
            .additional_browser_args(&crate::ads::ads_browser_args(
                &core.identity.config.ads.browser_args,
            ));
    }
    builder
}

/// Opens an `https:` popup of a consent page in the system browser
/// (nothing in the invisible lab).
fn open_in_browser(url: &Url) {
    if url.scheme() != "https"
        || crate::lab::block_os_surface("browser", || serde_json::json!(url.as_str()))
    {
        return;
    }
    if let Err(err) = tauri_plugin_opener::open_url(url.as_str(), None::<&str>) {
        log::debug!(target: LOG_TARGET, "opening a consent link failed: {err}");
    }
}

/// Parents a settings window to `parent` (DESIGN §4.7.4): an owned window
/// on Windows, a child window on macOS, a transient window on Linux.
fn with_parent<'a, R: Runtime>(
    builder: WebviewWindowBuilder<'a, R, tauri::AppHandle<R>>,
    parent: &tauri::Window<R>,
) -> Result<WebviewWindowBuilder<'a, R, tauri::AppHandle<R>>> {
    if !super::windows::native_runtime::<R>() {
        // Tauri's mock runtime has no native windows.
        return Ok(builder);
    }
    #[cfg(windows)]
    {
        let hwnd = parent.hwnd().map_err(|e| Error::backend(e.to_string()))?;
        Ok(builder.owner_raw(hwnd))
    }
    #[cfg(target_os = "macos")]
    {
        let ns_window = parent
            .ns_window()
            .map_err(|e| Error::backend(e.to_string()))?;
        Ok(builder.parent_raw(ns_window))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let gtk = parent
            .gtk_window()
            .map_err(|e| Error::backend(e.to_string()))?;
        Ok(builder.transient_for_raw(&gtk))
    }
}

/// A hidden 1 x 32 consent window (D.6.1, D.6.4), closed at
/// `consent.readyTimeoutMs` at the latest.
fn open_hidden_window<R: Runtime>(
    core: &Arc<Core<R>>,
    label: &str,
    url: &str,
    round: Option<u32>,
) -> Result<()> {
    let url = Url::parse(url).map_err(|e| Error::invalid_argument(e.to_string()))?;
    {
        let mut s = lock(&core.consent.state);
        s.hidden.insert(
            label.to_owned(),
            Hidden {
                round,
                closing: false,
                saved: false,
            },
        );
        s.created.insert(label.to_owned());
    }
    let (w, h) = HIDDEN_WINDOW_SIZE;
    let builder = cmp_builder(core, label, url)
        .title(&core.identity.app.name)
        .inner_size(w, h)
        .center()
        .visible(false)
        .focused(false)
        .focusable(false)
        .decorations(false)
        .skip_taskbar(true);
    // Hidden: building it must not activate the app.
    let window = crate::platform::webview::without_app_activation(|| build_window(builder));
    let window = match window {
        Ok(window) => window,
        Err(err) => {
            let mut s = lock(&core.consent.state);
            s.hidden.remove(label);
            s.created.remove(label);
            return Err(Error::backend(err.to_string()));
        }
    };
    crate::lab::record(
        "wc-events.jsonl",
        || serde_json::json!({ "kind": "created", "label": label, "type": "cmp", "url": window.url().map(|u| u.to_string()).unwrap_or_default() }),
    );
    let reports: Arc<dyn crate::platform::webview::GuestReports> =
        Arc::new(Reports(Arc::downgrade(core)));
    if let Err(err) = crate::platform::webview::install_guest_hooks(window.as_ref(), None, reports)
    {
        log::debug!(target: LOG_TARGET, "consent window hooks failed: {err}");
    }
    let weak = Arc::downgrade(core);
    let label = label.to_owned();
    let bound = Duration::from_millis(core.identity.config.consent.ready_timeout_ms);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(bound).await;
        if let Some(core) = weak.upgrade()
            && lock(&core.consent.state)
                .hidden
                .get(&label)
                .is_some_and(|h| !h.closing)
        {
            log::debug!(target: LOG_TARGET, "{label} timed out; closing it");
            close_hidden(&core, &label);
        }
    });
    Ok(())
}

/// Platform reports of a consent window: a failed load or a gone process
/// resolves its round and closes it.
struct Reports<R: Runtime>(std::sync::Weak<Core<R>>);

impl<R: Runtime> crate::platform::webview::GuestReports for Reports<R> {
    fn crashed(&self, label: &str, _reason: crate::ads::GoneReason, _exit_code: i64) {
        if let Some(core) = self.0.upgrade() {
            close_hidden(&core, label);
        }
    }

    fn load_failed(&self, label: &str, _error_code: i64, _description: &str, _url: &str) {
        if let Some(core) = self.0.upgrade() {
            close_hidden(&core, label);
        }
    }
}

/// Builds a consent window. Under Tauri's mock runtime, whose windows
/// live in a `RefCell`, the build is serialised with the test thread.
fn build_window<R: Runtime>(
    builder: WebviewWindowBuilder<'_, R, tauri::AppHandle<R>>,
) -> tauri::Result<tauri::WebviewWindow<R>> {
    #[cfg(test)]
    let _mock = crate::host::windows::tests::mock_windows();
    builder.build()
}

/// Destroys a consent window, serialised like [`build_window`]. A failure
/// means the window is already gone.
fn destroy_window<R: Runtime>(window: &tauri::Window<R>) {
    #[cfg(test)]
    let _mock = crate::host::windows::tests::mock_windows();
    let _ = window.destroy();
}

/// Destroys the consent window `label` from a runtime task: a hook or
/// event handler runs with Tauri's plugin store locked, and a window
/// destroyed inline on the main thread would report its `Destroyed` back
/// into that lock.
fn destroy_soon<R: Runtime>(core: &Arc<Core<R>>, label: &str) {
    let weak = Arc::downgrade(core);
    let label = label.to_owned();
    tauri::async_runtime::spawn(async move {
        let Some(core) = weak.upgrade() else { return };
        if let Some(w) = crate::compat::window(&core.app, &label) {
            destroy_window(&w);
            core.consent.window_gone(&label);
        }
    });
}

/// macOS: the web content process of consent window `label` ended (the
/// app's `on_web_content_process_terminate` hook, W0c ruling 1). A hidden
/// window closes as after any crash ([`close_hidden`], as the platform
/// crash report of [`open_hidden_window`] does) and `true` is returned;
/// `false` for any other window (the visible settings window), which the
/// caller reloads in place.
#[cfg(target_os = "macos")]
pub(crate) fn web_content_terminated<R: Runtime>(core: &Arc<Core<R>>, label: &str) -> bool {
    if !lock(&core.consent.state).hidden.contains_key(label) {
        return false;
    }
    close_hidden(core, label);
    true
}

/// Closes a hidden consent window. After a startup window, the cookie
/// fallback runs first (D.6.3).
pub(crate) fn close_hidden<R: Runtime>(core: &Arc<Core<R>>, label: &str) {
    let target = {
        let mut s = lock(&core.consent.state);
        s.hidden.get_mut(label).and_then(|h| {
            if h.closing {
                None
            } else {
                h.closing = true;
                Some(h.round)
            }
        })
    };
    let Some(round) = target else { return };
    let weak = Arc::downgrade(core);
    let label = label.to_owned();
    tauri::async_runtime::spawn(async move {
        let Some(core) = weak.upgrade() else { return };
        if round.is_some()
            && core.identity.config.consent.host_cookie_fallback == CookieFallback::Auto
        {
            cookie_fallback(&core, &label).await;
        }
        if let Some(w) = crate::compat::window(&core.app, &label) {
            destroy_window(&w);
        }
        // At once rather than at `Destroyed`, which follows within
        // milliseconds (and never arrives for a window that failed).
        core.consent.window_gone(&label);
    });
}

/// D.6.3: when both consent cookies are missing from the ads data store,
/// writes them from the stored `cmp` values. Reads only the consent cookie
/// names (SEC-m8). Runs off the main thread.
async fn cookie_fallback<R: Runtime>(core: &Arc<Core<R>>, label: &str) {
    let Some(present) = super::cookies::consent_cookies_in_store(core, label).await else {
        log::debug!(target: LOG_TARGET, "consent cookie check skipped: the ads data store did not answer");
        return;
    };
    if !present.is_empty() {
        return;
    }
    let stored = core.state.ow_electron.read().state.cmp.unwrap_or_default();
    let (tcf, ac) = cookie_values(
        stored.cmp_string.as_deref(),
        stored.unified_consent_string.as_deref(),
    );
    let wanted: Vec<_> = [("euconsent-v2", tcf), ("acconsent", ac)]
        .into_iter()
        .filter_map(|(name, value)| value.map(|v| consent_cookie(name, &v)))
        .collect();
    let Some(webview) = crate::compat::webview(&core.app, label) else {
        return;
    };
    if wanted.is_empty() {
        return;
    }
    let results = tauri::async_runtime::spawn_blocking(move || {
        wanted
            .into_iter()
            .map(|c| (c.name().to_owned(), webview.set_cookie(c)))
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    let mut wrote = false;
    for (name, result) in results {
        match result {
            Ok(()) => wrote = true,
            Err(err) => log::warn!(target: LOG_TARGET, "writing {name} failed: {err}"),
        }
    }
    if wrote {
        log::info!(target: LOG_TARGET, "consent cookies written by the host (hostCookieFallback)");
    }
}

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
mod tests;
