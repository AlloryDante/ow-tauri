//! The fan-out of Tauri's hooks to the modules that own each concern
//! (DESIGN §4.1, frozen in W1). `plugin.rs` forwards every hook here and
//! nothing else; later waves fill the modules, never this file.
//!
//! | Hook | Modules |
//! |---|---|
//! | `on_window_ready` | windows |
//! | `on_webview_ready` | windows, `platform::input` (macOS), ads |
//! | `on_page_load` | windows (app webviews), ads (all), consent (`ow-cmp*`) |
//! | `on_navigation` | ads (`owad-*`), consent (`ow-cmp*`) |
//! | `on_event` | lifecycle, windows, ads, consent |

use std::sync::Arc;

use tauri::webview::PageLoadPayload;
use tauri::{RunEvent, Runtime, Webview, Window};
use url::Url;

use super::{Core, lifecycle};
use crate::config::{ADVIEW_LABEL_PREFIX, CMP_LABEL_PREFIX};

/// `on_window_ready`.
pub(crate) fn on_window_ready<R: Runtime>(core: &Arc<Core<R>>, window: &Window<R>) {
    core.windows.window_ready(core, window);
}

/// `on_webview_ready`.
pub(crate) fn on_webview_ready<R: Runtime>(core: &Arc<Core<R>>, webview: &Webview<R>) {
    core.windows.webview_ready(core, webview);
    #[cfg(target_os = "macos")]
    crate::platform::input::on_webview_ready(core, webview);
    core.ads.webview_ready(core, webview);
}

/// `on_page_load` (top-level documents only).
pub(crate) fn on_page_load<R: Runtime>(
    core: &Arc<Core<R>>,
    webview: &Webview<R>,
    payload: &PageLoadPayload<'_>,
) {
    let label = webview.label();
    let (event, url) = (payload.event(), payload.url());
    if label.starts_with(CMP_LABEL_PREFIX) {
        core.consent.page_load(core, webview, event, url);
    } else if !label.starts_with(ADVIEW_LABEL_PREFIX) {
        core.windows.page_load(core, webview, event, url);
    }
    core.ads.page_load(core, webview, event, url);
}

/// `on_navigation`: the plugin decides only for its own webviews; app
/// webviews navigate as the app allows.
pub(crate) fn on_navigation<R: Runtime>(
    core: &Arc<Core<R>>,
    webview: &Webview<R>,
    url: &Url,
) -> bool {
    let label = webview.label();
    if label.starts_with(ADVIEW_LABEL_PREFIX) {
        core.ads.guest_navigation(core, label, url)
    } else if label.starts_with(CMP_LABEL_PREFIX) {
        core.consent.navigation(core, label, url)
    } else {
        true
    }
}

/// `on_event`. `RunEvent::ExitRequested` is deliberately left alone: the
/// plugin never prevents an exit and never exits the app (D2).
pub(crate) fn on_event<R: Runtime>(core: &Arc<Core<R>>, event: &RunEvent) {
    match event {
        RunEvent::Ready => lifecycle::on_ready(core),
        RunEvent::Exit => lifecycle::on_exit(core),
        RunEvent::WindowEvent { label, event, .. } => {
            core.windows.window_event(core, label, event);
            core.ads.window_event(core, label, event);
            core.consent.window_event(core, label, event);
        }
        _ => {}
    }
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use std::path::PathBuf;

    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
    use tauri::{App, WebviewUrl, WebviewWindowBuilder};

    use crate::platform::input::{KeyFix, tests::seen};

    /// A mock app with the plugin; `key_fix` is `Builder::macos_key_fix`.
    fn app(name: &str, key_fix: bool) -> (App<MockRuntime>, PathBuf) {
        let dir = crate::state::test_dir(name);
        let mut context = mock_context(noop_assets());
        context.config_mut().plugins.0.insert(
            "overwolf".into(),
            serde_json::json!({
                "author": "Example Studio",
                "name": "Example App",
                "state": { "appDataDir": dir },
                "analytics": { "muidStrategy": "per-install" }
            }),
        );
        let mut builder = crate::Builder::new().macos_key_fix(key_fix);
        let options = builder.options_mut();
        options.os_queries = false;
        options.runtime_capabilities = false;
        options.argv = Some(vec!["app".into()]);
        let app = mock_builder()
            .plugin(builder.build())
            .build(context)
            .unwrap();
        (app, dir)
    }

    #[test]
    fn webview_ready_reaches_the_macos_key_fix() {
        let (app, dir) = app("dispatch-key-fix-on", true);
        WebviewWindowBuilder::new(&app, "key-fix-on", WebviewUrl::default())
            .build()
            .unwrap();
        std::fs::remove_dir_all(dir).ok();
        assert_eq!(seen("key-fix-on"), [KeyFix::SpliceAndFocus]);
    }

    #[test]
    fn the_escape_hatch_reaches_the_macos_key_fix() {
        let (app, dir) = app("dispatch-key-fix-off", false);
        WebviewWindowBuilder::new(&app, "key-fix-off", WebviewUrl::default())
            .build()
            .unwrap();
        std::fs::remove_dir_all(dir).ok();
        assert_eq!(seen("key-fix-off"), [KeyFix::Off]);
    }
}
