//! `BrowserWindow` support: creating, loading, closing and evaluating in
//! `bw-*` windows, and turning Tauri window events into `window` host
//! messages (CONTRACT A.2.3, A.2.3.1, A.3).

use std::sync::{Arc, Weak};

use serde_json::{Value, json};
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{LogicalPosition, Manager, Runtime, WebviewBuilder, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;
use url::Url;

use super::main_webview::BOOTSTRAP_JS;
use super::{CloseRequest, EVAL_TIMEOUT_MS, Host, PendingEval};
use crate::error::{Error, ErrorCode};
use crate::ipc::messages::{HostMessage, WindowEventName};
use crate::lifecycle::CLOSE_TIMEOUT_MS;
use crate::platform::webview::MinimizeStage;
use crate::state::log::LogLevel;
use crate::window::options::{
    LoadTarget, NAVIGATION_HOOK_IS_TOP_LEVEL_ONLY, ResolvedLoad, UiNavigation, WindowClassWire,
    WindowCreateRequest, parse_color, resolve_load, ui_navigation,
};
use crate::window::{WindowKind, WindowState, derive_state_events, remote_label, ui_label};

/// JSON string literal for embedding in a script.
fn js_string(s: &str) -> String {
    Value::String(s.to_owned()).to_string()
}

/// The renderer bootstrap: the `process` shim data and the runtime bundle,
/// guarded so it does nothing outside the app origin (A.2.3.1).
pub(crate) fn renderer_init_script(origin: &str, bootstrap: &Value) -> String {
    format!(
        "if (location.origin === {origin}) {{\nwindow.__OW_TAURI_BOOTSTRAP__ = {bootstrap};\n{BOOTSTRAP_JS}\n}}",
        origin = js_string(origin)
    )
}

/// A preload bundle, guarded by origin and run once per document, in the
/// main frame, before page scripts.
pub(crate) fn preload_init_script(origin: &str, preload: &str) -> String {
    format!(
        "if (location.origin === {origin} && window === window.top && !window.__OW_TAURI_PRELOAD_DONE__) {{\nObject.defineProperty(window, '__OW_TAURI_PRELOAD_DONE__', {{ value: true }});\n{preload}\n}}",
        origin = js_string(origin)
    )
}

/// The app origin without a trailing slash, as `location.origin` reports it.
pub(crate) fn origin_string(url: &Url) -> String {
    // `Url::origin` is opaque for custom schemes such as `tauri:`, while
    // `location.origin` in the webview is `tauri://localhost`.
    let mut s = format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default());
    if let Some(port) = url.port() {
        s.push(':');
        s.push_str(&port.to_string());
    }
    s
}

/// Bounds and state data attached to `resize` / `move` events.
fn bounds_data<R: Runtime>(window: &tauri::Window<R>) -> Option<Value> {
    let scale = window.scale_factor().ok()?;
    let pos = window.outer_position().ok()?.to_logical::<f64>(scale);
    let size = window.outer_size().ok()?.to_logical::<f64>(scale);
    Some(
        json!({ "bounds": { "x": pos.x, "y": pos.y, "width": size.width, "height": size.height } }),
    )
}

/// The window's `NSWindow*` address (macOS); `None` elsewhere.
fn native_address<R: Runtime>(window: &tauri::Window<R>) -> Option<usize> {
    #[cfg(target_os = "macos")]
    {
        window.ns_window().ok().map(|p| p as usize)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        None
    }
}

fn read_state<R: Runtime>(window: &tauri::Window<R>) -> WindowState {
    WindowState {
        minimized: window.is_minimized().unwrap_or(false),
        maximized: window.is_maximized().unwrap_or(false),
        fullscreen: window.is_fullscreen().unwrap_or(false),
        visible: window.is_visible().unwrap_or(false),
    }
}

/// Platform crash and load-failure reports of `ow-main` and the `bw-*` /
/// `bwr-*` webviews (A.3, A.6), routed to the host.
pub(super) struct AppReports<R: Runtime>(pub(super) Weak<Host<R>>);

impl<R: Runtime> crate::platform::webview::GuestReports for AppReports<R> {
    fn crashed(&self, label: &str, reason: crate::ads::GoneReason, exit_code: i64) {
        let Some(host) = self.0.upgrade() else { return };
        match crate::window::classify(label) {
            crate::window::WebviewClass::Main => host.main_crashed(),
            crate::window::WebviewClass::Ui(id) | crate::window::WebviewClass::Remote(id) => {
                host.window_render_process_gone(id, reason, exit_code);
            }
            _ => {}
        }
    }

    fn load_failed(&self, label: &str, error_code: i64, description: &str, url: &str) {
        let Some(host) = self.0.upgrade() else { return };
        match crate::window::classify(label) {
            crate::window::WebviewClass::Ui(id) | crate::window::WebviewClass::Remote(id) => {
                host.send_main(HostMessage::window(
                    id,
                    WindowEventName::DidFailLoad,
                    Some(json!({
                        "errorCode": error_code,
                        "errorDescription": description,
                        "validatedURL": url,
                    })),
                ));
            }
            crate::window::WebviewClass::Main => host.log(
                LogLevel::Error,
                &format!("the main webview failed to load its page ({description})"),
            ),
            _ => {}
        }
    }
}

impl<R: Runtime> Host<R> {
    /// Installs the crash and load-failure hooks of an app webview
    /// (`ow-main`, `bw-*`, `bwr-*`).
    pub(crate) fn install_app_hooks(self: &Arc<Self>, webview: &tauri::Webview<R>) {
        let reports = Arc::new(AppReports(Arc::downgrade(self)));
        if let Err(err) = crate::platform::webview::install_app_hooks(webview, reports) {
            self.log(
                LogLevel::Warn,
                &format!("crash hooks for {} failed: {err}", webview.label()),
            );
        }
    }

    /// The render process of window `id`'s webview ended: `render-process-gone`
    /// (A.3) with Electron's `{ reason, exitCode }` details.
    pub(crate) fn window_render_process_gone(
        self: &Arc<Self>,
        id: u32,
        reason: crate::ads::GoneReason,
        exit_code: i64,
    ) {
        self.log(
            LogLevel::Warn,
            &format!("window {id}: render process gone ({})", reason.as_str()),
        );
        self.send_main(HostMessage::window(
            id,
            WindowEventName::RenderProcessGone,
            Some(json!({ "reason": reason.as_str(), "exitCode": exit_code })),
        ));
    }

    /// `window_create`.
    #[expect(
        clippy::too_many_lines,
        reason = "maps every BrowserWindow option in one place"
    )]
    pub(crate) fn create_window(
        self: &Arc<Self>,
        req: &WindowCreateRequest,
    ) -> Result<(u32, String), Error> {
        req.validate()?;
        let ignored = req.options.ignored_keys();
        let preload = match &req.preload {
            Some(p) => Some(self.read_asset_text(p)?),
            None => None,
        };
        // The native window, not its webview: after a switch to a remote
        // page the window `bw-<id>` holds the webview `bwr-<id>`.
        let parent = match req.options.parent_id {
            Some(pid) => Some(
                self.app
                    .get_window(&ui_label(pid))
                    .ok_or_else(|| Error::not_found("The parent window does not exist."))?,
            ),
            None => None,
        };
        let id = self.with_core(|c| c.windows.allocate());
        let label = ui_label(id);
        let o = &req.options;
        let origin = origin_string(&self.info.app_origin);
        let bootstrap = self.with_core(|c| {
            json!({
                "versions": c.state.get("versions").cloned().unwrap_or_default(),
                "switches": c.state.get("switches").cloned().unwrap_or_default(),
                "platform": self.info.os.node_platform(),
                "arch": crate::paths::node_arch(),
            })
        });
        let blank =
            Url::parse("about:blank").map_err(|_| Error::backend("about:blank does not parse"))?;
        let (w, h) = o.size();
        let visible = o.show.unwrap_or(true);
        // `show: false` creates the window without focus, as Electron does:
        // a later `showInactive()` must find it unfocused (Windows lab: the
        // webview took focus at creation, so ad guests read
        // `windowFocused: true`). `show()` still focuses it.
        let mut b = WebviewWindowBuilder::new(&self.app, &label, WebviewUrl::External(blank))
            .on_new_window(self.new_window_handler(id))
            .initialization_script(renderer_init_script(&origin, &bootstrap))
            .inner_size(w, h)
            .visible(visible)
            .focused(visible)
            .title(
                o.title
                    .clone()
                    .unwrap_or_else(|| self.info.manifest.product_name.clone()),
            );
        if let Some(p) = &preload {
            b = b.initialization_script(preload_init_script(&origin, p));
        }
        if let (Some(x), Some(y)) = (o.x, o.y) {
            b = b.position(x, y);
        } else if o.center.unwrap_or(false) || (o.x.is_none() && o.y.is_none()) {
            b = b.center();
        }
        if o.min_width.is_some() || o.min_height.is_some() {
            b = b.min_inner_size(o.min_width.unwrap_or(0.0), o.min_height.unwrap_or(0.0));
        }
        if o.max_width.is_some() || o.max_height.is_some() {
            b = b.max_inner_size(
                o.max_width.unwrap_or(f64::MAX),
                o.max_height.unwrap_or(f64::MAX),
            );
        }
        if let Some(v) = o.resizable {
            b = b.resizable(v);
        }
        if let Some(v) = o.minimizable {
            b = b.minimizable(v);
        }
        if let Some(v) = o.maximizable {
            b = b.maximizable(v);
        }
        if let Some(v) = o.closable {
            b = b.closable(v);
        }
        if let Some(v) = o.focusable {
            b = b.focusable(v);
        }
        if let Some(v) = o.always_on_top {
            b = b.always_on_top(v);
        }
        if let Some(v) = o.fullscreen {
            b = b.fullscreen(v);
        }
        if let Some(v) = o.skip_taskbar {
            b = b.skip_taskbar(v);
        }
        if let Some(v) = o.transparent {
            b = b.transparent(v);
        }
        if let Some(c) = o.background_color.as_deref().and_then(parse_color) {
            b = b.background_color(tauri::window::Color(c.0, c.1, c.2, c.3));
        }
        if o.frame == Some(false) {
            #[cfg(target_os = "macos")]
            {
                b = b
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .hidden_title(true);
            }
            #[cfg(not(target_os = "macos"))]
            {
                b = b.decorations(false);
            }
        }
        if let Some(dev) = o.web_preferences.as_ref().and_then(|w| w.dev_tools) {
            b = b.devtools(dev);
        }
        if let Some(parent) = &parent {
            b = with_parent(b, parent)?;
        }
        #[cfg(windows)]
        {
            b = b.additional_browser_args(&self.info.browser_args);
        }
        // Lab windows are built hidden and shown invisible (feature `lab`).
        b = crate::lab::window_builder(b, visible);
        // A hidden window (and every lab window) is built without activating
        // the app, as ow-electron builds `show: false` windows; a shown one
        // activates it, as ow-electron's `show()` does.
        let window = if visible && !crate::lab::invisible() {
            b.build()
        } else {
            crate::platform::webview::without_app_activation(|| b.build())
        }
        .map_err(Error::from)?;
        crate::lab::after_build(&window, visible);
        self.install_app_hooks(window.as_ref());
        if let Some(z) = o.web_preferences.as_ref().and_then(|w| w.zoom_factor) {
            let _ = window.set_zoom(z);
        }
        let kind = match req.window_class {
            WindowClassWire::Ui => WindowKind::Ui,
            WindowClassWire::Overlay => WindowKind::Overlay,
        };
        let name = o.name.as_deref().map(crate::window::normalize_window_name);
        let title = o
            .title
            .clone()
            .unwrap_or_else(|| self.info.manifest.product_name.clone());
        self.with_core(|c| {
            c.windows.insert(
                id,
                kind,
                WindowState {
                    visible,
                    ..WindowState::default()
                },
            );
            if let Some(entry) = c.windows.get_mut(id) {
                entry.name = name;
                entry.title = title;
            }
        });
        if !ignored.is_empty() {
            self.log(
                LogLevel::Warn,
                &format!(
                    "BrowserWindow {id}: options without effect: {}",
                    ignored.join(", ")
                ),
            );
        }
        Ok((id, label))
    }

    fn window_kind(self: &Arc<Self>, id: u32) -> Result<WindowKind, Error> {
        self.with_core(|c| c.windows.get(id).map(|e| e.kind))
            .ok_or_else(|| {
                Error::not_found("No window with this id.").with_data(json!({ "id": id }))
            })
    }

    /// `window_load`.
    pub(crate) fn load_window(self: &Arc<Self>, id: u32, target: &LoadTarget) -> Result<(), Error> {
        let kind = self.window_kind(id)?;
        let resolved = resolve_load(target, &self.info.app_origin)?;
        match (kind, resolved) {
            (WindowKind::Remote, ResolvedLoad::App(_)) => Err(Error::invalid_argument(
                "The window shows a remote page; it cannot load app assets again.",
            )),
            (WindowKind::Remote, ResolvedLoad::Remote(url)) => {
                let webview = self
                    .app
                    .get_webview(&remote_label(id))
                    .ok_or_else(|| Error::not_found("No window with this id."))?;
                webview.navigate(url).map_err(Error::from)
            }
            (_, ResolvedLoad::App(url)) => {
                let webview = self
                    .app
                    .get_webview(&ui_label(id))
                    .ok_or_else(|| Error::not_found("No window with this id."))?;
                webview.navigate(url).map_err(Error::from)
            }
            (_, ResolvedLoad::Remote(url)) => self.switch_to_remote(id, url),
        }
    }

    /// Replaces the app webview of window `id` with a fresh remote webview
    /// `bwr-<id>` filling the window (A.2.3.1).
    fn switch_to_remote(self: &Arc<Self>, id: u32, url: Url) -> Result<(), Error> {
        let window = self
            .app
            .get_window(&ui_label(id))
            .ok_or_else(|| Error::not_found("No window with this id."))?;
        let scale = window.scale_factor().map_err(Error::from)?;
        let size = window
            .inner_size()
            .map_err(Error::from)?
            .to_logical::<f64>(scale);
        #[cfg_attr(
            not(windows),
            expect(unused_mut, reason = "browser arguments are added on Windows only")
        )]
        let mut builder = WebviewBuilder::new(remote_label(id), WebviewUrl::External(url))
            .auto_resize()
            .on_new_window(self.new_window_handler(id));
        #[cfg(windows)]
        {
            builder = builder.additional_browser_args(&self.info.browser_args);
        }
        // Part of the window, so it activates the app only when the window is
        // shown (see `create_window`).
        let shown = window.is_visible().unwrap_or(false) && !crate::lab::invisible();
        let remote = if shown {
            window.add_child(builder, LogicalPosition::new(0.0, 0.0), size)
        } else {
            crate::platform::webview::without_app_activation(|| {
                window.add_child(builder, LogicalPosition::new(0.0, 0.0), size)
            })
        }
        .map_err(Error::from)?;
        self.install_app_hooks(&remote);
        if let Some(old) = self.app.get_webview(&ui_label(id))
            && let Err(err) = old.close()
        {
            self.log(
                LogLevel::Warn,
                &format!("closing the app webview of window {id} failed: {err}"),
            );
        }
        let evals = self.with_core(|c| {
            if let Some(entry) = c.windows.get_mut(id) {
                entry.kind = WindowKind::Remote;
            }
            c.router.remove_peer(&ui_label(id), None);
            c.sinks.remove(&ui_label(id));
            c.urls.remove(&ui_label(id));
            c.in_page_urls.remove(&ui_label(id));
            take_evals(c, id)
        });
        reject_evals(evals, "The window switched to a remote page.");
        Ok(())
    }

    /// Destroys window `id` without a `close` event.
    pub(crate) fn destroy_window(self: &Arc<Self>, id: u32) {
        if let Some(window) = self.app.get_window(&ui_label(id)) {
            self.ads_window_closing(id);
            if let Err(err) = window.destroy() {
                self.log(
                    LogLevel::Warn,
                    &format!("destroying window {id} failed: {err}"),
                );
            }
        } else {
            // Already gone: make sure the registry agrees.
            self.window_destroyed(id);
        }
    }

    /// `window_close_reply`.
    pub(crate) fn close_reply(
        self: &Arc<Self>,
        id: u32,
        request_id: u64,
        prevent: bool,
    ) -> Result<(), Error> {
        enum Outcome {
            Quit(Vec<crate::lifecycle::QuitAction>),
            Close(bool),
            Unknown,
        }
        let now = self.now();
        let outcome = self.with_core(|c| {
            if c.quit.owns_close(request_id) {
                let mut ids = c.request_ids;
                let a = c.quit.answer_close(request_id, prevent, now, &mut ids);
                c.request_ids = ids;
                Outcome::Quit(a)
            } else if c
                .close_requests
                .get(&request_id)
                .is_some_and(|r| r.window == id)
            {
                c.close_requests.remove(&request_id);
                Outcome::Close(!prevent)
            } else {
                Outcome::Unknown
            }
        });
        match outcome {
            Outcome::Quit(actions) => {
                self.run_quit_actions(actions);
                Ok(())
            }
            Outcome::Close(true) => {
                self.destroy_window(id);
                Ok(())
            }
            Outcome::Close(false) => Ok(()),
            Outcome::Unknown => Err(Error::not_found("Unknown or expired close request.")),
        }
    }

    /// `window_eval`.
    pub(crate) async fn eval_in_window(
        self: &Arc<Self>,
        id: u32,
        code: &str,
        want_result: bool,
    ) -> Result<Option<Value>, Error> {
        let kind = self.window_kind(id)?;
        let label = if kind == WindowKind::Remote {
            remote_label(id)
        } else {
            ui_label(id)
        };
        let webview = self
            .app
            .get_webview(&label)
            .ok_or_else(|| Error::not_found("No window with this id."))?;
        if kind == WindowKind::Remote || !want_result {
            webview.eval(code).map_err(Error::from)?;
            return Ok(None);
        }
        let (tx, rx) = oneshot::channel();
        let deadline = self.now() + EVAL_TIMEOUT_MS;
        let n = self.with_core(|c| {
            c.next_eval += 1;
            let n = c.next_eval;
            c.evals.insert(
                n,
                PendingEval {
                    window: id,
                    tx,
                    deadline,
                },
            );
            n
        });
        let begin = format!("__OW_TAURI_RUNTIME__.evalBegin({n}, () => ({code}\n))");
        let fallback = format!("__OW_TAURI_RUNTIME__.evalFallback({n}, () => {{ {code}\n}})");
        if let Err(err) = webview.eval(begin).and_then(|()| webview.eval(fallback)) {
            self.with_core(|c| c.evals.remove(&n));
            return Err(Error::from(err));
        }
        rx.await.unwrap_or_else(|_| {
            Err(Error::not_ready(
                "The window went away before reporting a result.",
            ))
        })
    }

    /// `eval_result` from window `id`.
    pub(crate) fn eval_result(
        self: &Arc<Self>,
        id: u32,
        eval_id: u64,
        ok: bool,
        value: Option<Value>,
        error: Option<Value>,
    ) -> Result<(), Error> {
        let pending = self.with_core(|c| {
            if c.evals.get(&eval_id).is_some_and(|e| e.window == id) {
                c.evals.remove(&eval_id)
            } else {
                None
            }
        });
        let pending = pending.ok_or_else(|| Error::not_found("Unknown executeJavaScript id."))?;
        let result = if ok {
            Ok(value)
        } else {
            let message = error
                .as_ref()
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("executeJavaScript failed.")
                .to_owned();
            Err(Error::new(ErrorCode::IpcRemoteError, message, error))
        };
        let _ = pending.tx.send(result);
        Ok(())
    }

    /// `app_focus`: focuses the most recent visible UI window.
    pub(crate) fn focus_app(self: &Arc<Self>) {
        let candidates = self.with_core(|c| c.windows.focus_candidates());
        for id in candidates {
            if let Some(w) = self.app.get_window(&ui_label(id))
                && w.is_visible().unwrap_or(false)
            {
                if crate::lab::may_focus() {
                    let _ = w.set_focus();
                }
                return;
            }
        }
    }

    /// Sends `window` events for state changes of every window.
    pub(crate) fn poll_windows(self: &Arc<Self>) {
        let ids = self.with_core(|c| c.windows.ids());
        for id in ids {
            if let Some(w) = self.app.get_window(&ui_label(id)) {
                self.apply_window_state(id, read_state(&w));
            }
        }
    }

    pub(crate) fn apply_window_state(self: &Arc<Self>, id: u32, now: WindowState) {
        let minimized = self.with_core(|c| {
            let entry = c.windows.get_mut(id)?;
            if now.minimized {
                entry.minimizing = false;
            }
            let events = derive_state_events(entry.state, now);
            let changed = entry.state.minimized != now.minimized;
            entry.state = now;
            for e in events {
                c.queue_main(HostMessage::window(id, e, None));
            }
            changed.then_some(now.minimized)
        });
        if let Some(minimized) = minimized {
            self.ads_window_minimized(id, minimized);
        }
    }

    /// The OS reported a stage of the minimize of the window whose
    /// `NSWindow*` is `ns_window` (macOS); windows of other hosts are
    /// ignored.
    pub(crate) fn os_window_minimize(self: &Arc<Self>, ns_window: usize, stage: MinimizeStage) {
        let ids = self.with_core(|c| c.windows.ids());
        let id = ids.into_iter().find(|&id| {
            self.app
                .get_window(&ui_label(id))
                .is_some_and(|w| native_address(&w) == Some(ns_window))
        });
        if let Some(id) = id {
            self.window_minimize_stage(id, stage);
        }
    }

    /// A stage of the minimize of window `id`. While it animates into the
    /// Dock, macOS reports the window neither visible nor minimized: from
    /// [`MinimizeStage::Will`] the visibility poll treats it as minimized,
    /// not hidden. At [`MinimizeStage::Did`] the minimize is applied at
    /// once (the `minimize` event, and the guests' `window-minimized` and
    /// `window-hidden`, as ow-electron sends them when the minimize ends
    /// (observed)).
    pub(crate) fn window_minimize_stage(self: &Arc<Self>, id: u32, stage: MinimizeStage) {
        crate::lab::record(
            "wc-events.jsonl",
            || serde_json::json!({ "kind": "minimize-stage", "windowId": id, "stage": format!("{stage:?}") }),
        );
        match stage {
            MinimizeStage::Will => self.with_core(|c| {
                if let Some(e) = c.windows.get_mut(id) {
                    e.minimizing = true;
                }
            }),
            MinimizeStage::Did => {
                let known = self.with_core(|c| c.windows.get(id).map(|e| e.state));
                let mut now = self
                    .app
                    .get_window(&ui_label(id))
                    .map(|w| read_state(&w))
                    .or(known)
                    .unwrap_or_default();
                now.minimized = true;
                self.apply_window_state(id, now);
            }
        }
    }

    /// Tauri window event for `bw-<id>`.
    pub(crate) fn window_event(self: &Arc<Self>, id: u32, event: &tauri::WindowEvent) {
        let known = self.with_core(|c| c.windows.get(id).is_some());
        if !known {
            return;
        }
        let window = self.app.get_window(&ui_label(id));
        match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let now = self.now();
                self.with_core(|c| {
                    if c.close_requests.values().any(|r| r.window == id) {
                        return;
                    }
                    let request_id = c.next_request_id();
                    c.close_requests.insert(
                        request_id,
                        CloseRequest {
                            window: id,
                            deadline: now + CLOSE_TIMEOUT_MS,
                        },
                    );
                    c.queue_main(HostMessage::Window {
                        id,
                        event: WindowEventName::Close,
                        request_id: Some(request_id),
                        data: None,
                    });
                });
            }
            tauri::WindowEvent::Destroyed => self.window_destroyed(id),
            tauri::WindowEvent::Focused(focused) => self.with_core(|c| {
                if *focused {
                    c.windows.focused(id);
                }
                let e = if *focused {
                    WindowEventName::Focus
                } else {
                    WindowEventName::Blur
                };
                c.queue_main(HostMessage::window(id, e, None));
            }),
            tauri::WindowEvent::Resized(_) => {
                let data = window.as_ref().and_then(bounds_data);
                self.send_main(HostMessage::window(id, WindowEventName::Resize, data));
                if let Some(w) = &window {
                    self.apply_window_state(id, read_state(w));
                }
            }
            tauri::WindowEvent::Moved(_) => {
                let data = window.as_ref().and_then(bounds_data);
                self.send_main(HostMessage::window(id, WindowEventName::Move, data));
            }
            _ => {}
        }
    }

    /// Window `id` is gone: tell `ow-main`, drop its IPC state (A.3).
    pub(crate) fn window_destroyed(self: &Arc<Self>, id: u32) {
        self.analytics_window_gone(id);
        self.close_guests_of(&ui_label(id));
        self.close_guests_of(&remote_label(id));
        let now = self.now();
        let (actions, evals) = self.with_core(|c| {
            if c.windows.remove(id).is_none() {
                return (Vec::new(), Vec::new());
            }
            c.router.remove_peer(&ui_label(id), Some(id));
            c.router.remove_peer(&remote_label(id), None);
            c.sinks.remove(&ui_label(id));
            c.urls.remove(&ui_label(id));
            c.urls.remove(&remote_label(id));
            c.in_page_urls.remove(&ui_label(id));
            c.close_requests.retain(|_, r| r.window != id);
            c.queue_main(HostMessage::window(id, WindowEventName::Closed, None));
            c.restart_stale_windows.remove(&id);
            let mut ids = c.request_ids;
            let actions = c.quit.window_gone(id, now, &mut ids);
            c.request_ids = ids;
            (actions, take_evals(c, id))
        });
        reject_evals(evals, "The window was destroyed.");
        self.run_quit_actions(actions);
    }

    /// Page load of `bw-<id>` or `bwr-<id>`.
    pub(crate) fn window_page_load(
        self: &Arc<Self>,
        id: u32,
        app_webview: bool,
        label: &str,
        event: PageLoadEvent,
        url: &Url,
    ) {
        let href = url.to_string();
        match event {
            PageLoadEvent::Started => {
                let evals = self.with_core(|c| {
                    c.urls.insert(label.to_owned(), href);
                    c.in_page_urls.remove(label);
                    if !app_webview {
                        return Vec::new();
                    }
                    if c.router.is_subscribed(label) {
                        c.router.document_unloaded(label);
                        c.sinks.remove(label);
                    }
                    take_evals(c, id)
                });
                reject_evals(evals, "The window navigated.");
                if app_webview {
                    // B.3.4: the document unloads, so its guests close.
                    self.close_guests_of(label);
                }
            }
            PageLoadEvent::Finished => {
                self.analytics_page_finished(id, &href);
                self.with_core(|c| {
                    // An in-page navigation during the load wins (B.2).
                    let href = c.in_page_urls.remove(label).unwrap_or(href);
                    c.urls.insert(label.to_owned(), href.clone());
                    let Some(entry) = c.windows.get_mut(id) else {
                        return;
                    };
                    let first = !entry.shown_ready;
                    entry.shown_ready = true;
                    c.queue_main(HostMessage::window(id, WindowEventName::DomReady, None));
                    c.queue_main(HostMessage::window(
                        id,
                        WindowEventName::DidFinishLoad,
                        Some(json!({ "url": href })),
                    ));
                    if first {
                        c.queue_main(HostMessage::window(id, WindowEventName::ReadyToShow, None));
                    }
                });
            }
        }
    }

    /// The navigation policy of the `bw-<id>` webview (A.2.3.1, see
    /// [`ui_navigation`]). A cancelled top-level `http(s)` navigation opens
    /// in the system browser and reaches the app as `will-navigate`.
    pub(crate) fn ui_navigation(self: &Arc<Self>, id: u32, url: &Url) -> bool {
        match ui_navigation(
            url,
            &self.info.app_origin,
            NAVIGATION_HOOK_IS_TOP_LEVEL_ONLY,
        ) {
            UiNavigation::Allow => true,
            UiNavigation::OpenExternal => {
                self.send_main(HostMessage::window(
                    id,
                    WindowEventName::WillNavigate,
                    Some(json!({ "url": url.as_str() })),
                ));
                match crate::shell::validate_external_url(url.as_str()) {
                    Ok(u) => {
                        let _ = self.open_in_browser(&u);
                    }
                    Err(_) => self.log(LogLevel::Warn, "navigation to an invalid URL cancelled"),
                }
                false
            }
            UiNavigation::Cancel => {
                self.log(
                    LogLevel::Warn,
                    &format!("navigation to a {} URL cancelled", url.scheme()),
                );
                false
            }
        }
    }

    /// `navigation_external` (A.2.5): a top-level navigation of window `id`
    /// that the renderer bootstrap cancelled on macOS and Linux (A.2.3.1).
    /// The URL passes the `shell_open_external` checks and is not on the app
    /// origin; it opens in the system browser and `ow-main` gets the same
    /// `will-navigate` the Windows navigation hook sends.
    pub(crate) fn navigation_external(self: &Arc<Self>, id: u32, url: &str) -> Result<(), Error> {
        let target = crate::shell::validate_external_url(url)?;
        if origin_string(&target) == origin_string(&self.info.app_origin) {
            return Err(Error::invalid_argument(
                "The URL is on the app origin; it loads in the window.",
            ));
        }
        self.send_main(HostMessage::window(
            id,
            WindowEventName::WillNavigate,
            Some(json!({ "url": target.as_str() })),
        ));
        self.open_in_browser(&target)
    }

    /// `navigation_in_page` (A.2.5): the top document of window `id`, in
    /// the webview `label`, changed its URL without a new load. The URL must
    /// keep the loaded document's origin. It becomes the window's URL
    /// (`webContents.getURL()`, the next `did-finish-load` while the document
    /// is still loading) and `ow-main` gets `did-navigate-in-page`, as
    /// Electron emits it. Reporting the current URL again does nothing.
    pub(crate) fn navigation_in_page(
        self: &Arc<Self>,
        id: u32,
        label: &str,
        url: &str,
    ) -> Result<(), Error> {
        let target =
            Url::parse(url).map_err(|_| Error::invalid_argument("The URL is not valid."))?;
        self.with_core(|c| {
            let current = c.urls.get(label).and_then(|u| Url::parse(u).ok());
            let Some(current) = current else {
                return Err(Error::invalid_argument(
                    "The window has no loaded document.",
                ));
            };
            if origin_string(&current) != origin_string(&target) {
                return Err(Error::invalid_argument(
                    "An in-page navigation keeps the document's origin.",
                ));
            }
            if current == target {
                return Ok(());
            }
            c.urls.insert(label.to_owned(), target.to_string());
            // Read by the `did-finish-load` of a load still running; the
            // next load start drops it.
            c.in_page_urls.insert(label.to_owned(), target.to_string());
            c.queue_main(HostMessage::window(
                id,
                WindowEventName::DidNavigateInPage,
                Some(json!({ "url": target.as_str(), "isMainFrame": true })),
            ));
            Ok(())
        })
    }

    /// The `window.open` handler of window `id`'s webviews: the request is
    /// always denied natively and reported as a `new-window` event, so the
    /// main runtime runs the app's `setWindowOpenHandler` (B.2).
    fn new_window_handler(
        self: &Arc<Self>,
        id: u32,
    ) -> impl Fn(Url, tauri::webview::NewWindowFeatures) -> NewWindowResponse<R> + Send + 'static
    {
        let weak = Arc::downgrade(self);
        move |url, _features| {
            if let Some(host) = weak.upgrade() {
                host.send_main(HostMessage::window(
                    id,
                    WindowEventName::NewWindow,
                    Some(json!({ "url": url.as_str() })),
                ));
            }
            NewWindowResponse::Deny
        }
    }
}

/// Makes `parent` (a `bw-*` window, whatever webview it holds) the parent
/// of the window `b` builds, as `WebviewWindowBuilder::parent` does for a
/// `WebviewWindow`: owner on Windows, child window on macOS, transient on
/// Linux.
fn with_parent<'a, R: Runtime, M: Manager<R>>(
    b: WebviewWindowBuilder<'a, R, M>,
    parent: &tauri::Window<R>,
) -> Result<WebviewWindowBuilder<'a, R, M>, Error> {
    #[cfg(windows)]
    {
        Ok(b.owner_raw(parent.hwnd().map_err(Error::from)?))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(b.parent_raw(parent.ns_window().map_err(Error::from)?))
    }
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    {
        Ok(b.transient_for_raw(&parent.gtk_window().map_err(Error::from)?))
    }
    #[cfg(not(any(
        windows,
        target_os = "macos",
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )))]
    {
        let _ = parent;
        Ok(b)
    }
}

fn take_evals(c: &mut super::Core, id: u32) -> Vec<PendingEval> {
    let ids: Vec<u64> = c
        .evals
        .iter()
        .filter(|(_, e)| e.window == id)
        .map(|(n, _)| *n)
        .collect();
    ids.into_iter().filter_map(|n| c.evals.remove(&n)).collect()
}

fn reject_evals(evals: Vec<PendingEval>, why: &str) {
    for e in evals {
        let _ = e.tx.send(Err(Error::not_ready(why)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_scripts() {
        let s = renderer_init_script("tauri://localhost", &json!({"platform": "darwin"}));
        assert!(s.starts_with("if (location.origin === \"tauri://localhost\") {"));
        assert!(s.contains("window.__OW_TAURI_BOOTSTRAP__ = {\"platform\":\"darwin\"};"));
        let p = preload_init_script("http://tauri.localhost", "console.log(1)");
        assert!(p.contains("window === window.top"));
        assert!(p.contains("__OW_TAURI_PRELOAD_DONE__"));
        assert!(p.ends_with("console.log(1)\n}"));
        assert_eq!(
            origin_string(&Url::parse("tauri://localhost/").unwrap()),
            "tauri://localhost"
        );
        assert_eq!(
            origin_string(&Url::parse("http://localhost:1420/x").unwrap()),
            "http://localhost:1420"
        );
    }
}
