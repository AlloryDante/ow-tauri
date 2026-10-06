//! The plugin's commands (CONTRACT A.2).
//!
//! Every command first checks the class of the calling webview from its
//! label and returns `forbidden` when it does not match, so a mis-scoped
//! capability cannot widen access (ARCHITECTURE 5.2, defence in depth).

mod app;
mod dialog;
mod fs;
mod ipc;
pub(crate) mod list;
mod shell;
mod shortcut;
mod window;

use std::sync::Arc;

use tauri::{Runtime, State, Webview};

use crate::error::Error;
use crate::ext::Overwolf;
use crate::host::Host;
use crate::window::{WebviewClass, classify};

/// The host behind the managed state.
pub(crate) fn host<'a, R: Runtime>(state: &'a State<'_, Overwolf<R>>) -> &'a Arc<Host<R>> {
    &state.inner().0
}

fn forbidden(label: &str) -> Error {
    Error::forbidden("This window may not call this command.")
        .with_data(serde_json::json!({ "label": label }))
}

/// The caller must be `ow-main`.
pub(crate) fn require_main<R: Runtime>(webview: &Webview<R>) -> Result<(), Error> {
    match classify(webview.label()) {
        WebviewClass::Main => Ok(()),
        _ => Err(forbidden(webview.label())),
    }
}

/// The caller must be a `bw-*` UI or overlay webview; returns its window id.
pub(crate) fn require_ui<R: Runtime>(webview: &Webview<R>) -> Result<u32, Error> {
    match classify(webview.label()) {
        WebviewClass::Ui(id) if webview.window().label() == webview.label() => Ok(id),
        _ => Err(forbidden(webview.label())),
    }
}

/// The caller must be `ow-main` or a `bw-*` webview.
pub(crate) fn require_main_or_ui<R: Runtime>(webview: &Webview<R>) -> Result<(), Error> {
    if require_main(webview).is_ok() || require_ui(webview).is_ok() {
        Ok(())
    } else {
        Err(forbidden(webview.label()))
    }
}

/// Deserializes the whole JSON payload of `request` (commands whose wire
/// arguments are one flat options object, or that must tell an absent field
/// from `null`).
pub(crate) fn body<T: serde::de::DeserializeOwned>(
    request: &tauri::ipc::Request<'_>,
    command: &str,
) -> Result<T, Error> {
    let tauri::ipc::InvokeBody::Json(value) = request.body() else {
        return Err(Error::invalid_argument("Expected a JSON payload."));
    };
    serde_json::from_value(value.clone()).map_err(|e| {
        Error::invalid_argument(format!("Malformed {command} arguments."))
            .with_data(serde_json::json!({ "raw": e.to_string() }))
    })
}

/// The invoke handler with every command of [`list::COMMANDS`].
pub(crate) fn handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static
{
    tauri::generate_handler![
        ipc::ipc_subscribe,
        ipc::bootstrap,
        ipc::main_ready,
        ipc::ipc_main_ready,
        app::app_quit_reply,
        app::app_relaunch,
        app::app_quit,
        app::app_exit,
        app::app_focus,
        app::log,
        ipc::ipc_reply,
        ipc::ipc_emit,
        app::disable_anonymous_analytics,
        app::disable_ads_optimization,
        app::disable_ads_fpd,
        window::window_create,
        window::window_load,
        window::window_close_reply,
        window::window_destroy,
        window::window_eval,
        window::window_devtools,
        window::window_set_name,
        window::screen_snapshot,
        shell::shell_open_external,
        shell::shell_open_path,
        shell::shell_show_item_in_folder,
        dialog::dialog_open,
        dialog::dialog_save,
        dialog::dialog_message,
        shortcut::global_shortcut_register,
        shortcut::global_shortcut_unregister,
        fs::fs_read_text,
        fs::fs_write_text,
        fs::fs_exists,
        fs::fs_mkdir,
        ipc::ipc_invoke,
        ipc::ipc_send,
        ipc::ipc_skip,
        window::eval_result,
    ]
}
