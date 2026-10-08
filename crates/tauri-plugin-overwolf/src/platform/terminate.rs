//! macOS: the web content process terminate hook (DESIGN §3.3, D13a).
//!
//! `WebKit` runs each webview's content in a process of its own; when that
//! process dies the webview turns blank and stays blank unless someone
//! reloads it. Tauri exposes the event only through the app-wide
//! `tauri::Builder::on_web_content_process_terminate`, which a plugin
//! cannot install and cannot query. So the app wires it:
//!
//! - [`web_content_process_terminate_hook`] returns a ready hook and
//!   records that it was built (before plugin setup runs);
//! - an app that composes its own hook calls
//!   [`handle_web_content_process_terminate`] from it and declares
//!   `Builder::forwards_web_content_process_terminate()`.
//!
//! When neither happened, plugin setup warns once ([`warn_if_unwired`]).

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{Runtime, Webview};

use crate::host::LOG_TARGET;

/// Set when [`web_content_process_terminate_hook`] was built.
static HOOK_BUILT: AtomicBool = AtomicBool::new(false);

/// The warning [`warn_if_unwired`] prints.
pub(crate) const UNWIRED_WARNING: &str = "tauri-plugin-overwolf: ad guests cannot be recovered after a crash on macOS; wire tauri_plugin_overwolf::web_content_process_terminate_hook() (docs/TROUBLESHOOTING.md#blank-ad-macos)";

/// Whether the app wired the hook: `forwards` (the builder switch) or
/// [`web_content_process_terminate_hook`] was built.
pub(crate) fn is_wired(forwards: bool) -> bool {
    forwards || HOOK_BUILT.load(Ordering::SeqCst)
}

/// Plugin setup: one warning (log and stderr) when the hook is not wired.
#[expect(
    clippy::print_stderr,
    reason = "DESIGN D13a: the missing hook is reported on stderr even without a logger"
)]
pub(crate) fn warn_if_unwired(forwards: bool) {
    if !is_wired(forwards) {
        log::warn!(target: LOG_TARGET, "{UNWIRED_WARNING}");
        eprintln!("{UNWIRED_WARNING}");
    }
}

/// Handles the end of `webview`'s web content process: an app webview is
/// reloaded; the plugin's own webviews are recovered by their owners (an
/// ad guest as after any crash, a hidden consent window is closed, the
/// visible consent settings window is reloaded).
///
/// Call it from the app's own `on_web_content_process_terminate` hook, and
/// declare `Builder::forwards_web_content_process_terminate()`.
///
/// ```no_run
/// # fn example(context: tauri::Context) {
/// tauri::Builder::default()
///     .plugin(
///         tauri_plugin_overwolf::Builder::new()
///             .forwards_web_content_process_terminate()
///             .build(),
///     )
///     .on_web_content_process_terminate(|webview| {
///         tauri_plugin_overwolf::handle_web_content_process_terminate(webview);
///     })
///     .run(context)
///     .expect("error while running the app");
/// # }
/// ```
pub fn handle_web_content_process_terminate<R: Runtime>(webview: &Webview<R>) {
    let label = webview.label();
    if crate::config::is_reserved_label(label) {
        log::debug!(target: LOG_TARGET, "web content process of {label} ended");
        if recovered_by_the_plugin(webview, label) {
            return;
        }
    }
    if let Err(err) = webview.reload() {
        log::warn!(target: LOG_TARGET, "could not reload {label} after its web content process ended: {err}");
    }
}

/// W0c ruling 1: an ad guest (`owad-*`) is recovered by the ads host as
/// after any crash (§4.4.7); a hidden consent window (`ow-cmp*`) closes as
/// after any crash, which resolves its round. Returns `false` for the
/// visible consent settings window, which reloads in place as an app
/// webview does.
fn recovered_by_the_plugin<R: Runtime>(webview: &Webview<R>, label: &str) -> bool {
    let Some(core) = crate::host::core_of(webview) else {
        // No plugin state: no guest or consent window of the plugin exists.
        return true;
    };
    if label.starts_with(crate::config::ADVIEW_LABEL_PREFIX) {
        crate::host::ads::web_content_terminated(&core, label);
        return true;
    }
    crate::host::consent::web_content_terminated(&core, label)
}

/// A ready `on_web_content_process_terminate` hook that calls
/// [`handle_web_content_process_terminate`]. Building it tells the plugin
/// the hook is wired.
///
/// ```no_run
/// # fn example(context: tauri::Context) {
/// tauri::Builder::default()
///     .plugin(tauri_plugin_overwolf::init())
///     .on_web_content_process_terminate(tauri_plugin_overwolf::web_content_process_terminate_hook())
///     .run(context)
///     .expect("error while running the app");
/// # }
/// ```
pub fn web_content_process_terminate_hook<R: Runtime>()
-> impl Fn(&Webview<R>) + Send + Sync + 'static {
    HOOK_BUILT.store(true, Ordering::SeqCst);
    |webview: &Webview<R>| handle_web_content_process_terminate(webview)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn building_the_hook_wires_it() {
        assert!(is_wired(true));
        let _hook = web_content_process_terminate_hook::<tauri::test::MockRuntime>();
        assert!(is_wired(false));
    }
}
