//! `dialog` commands (CONTRACT A.2.3), through `tauri-plugin-dialog`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::ipc::Request;
use tauri::{Manager, Runtime, State, Webview, Window};
use tauri_plugin_dialog::{
    Dialog, FileDialogBuilder, FilePath, MessageDialogButtons, MessageDialogKind,
    MessageDialogResult,
};
use tokio::sync::oneshot;

use super::{body, host, require_main};
use crate::error::{Error, Result};
use crate::ext::Overwolf;
use crate::state::log::LogLevel;
use crate::window::ui_label;

/// Electron `FileFilter`.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct FileFilter {
    #[serde(default)]
    name: String,
    #[serde(default)]
    extensions: Vec<String>,
}

/// Electron `OpenDialogOptions`, with `windowId` for the parent.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct OpenDialogOptions {
    window_id: Option<u32>,
    title: Option<String>,
    default_path: Option<String>,
    filters: Vec<FileFilter>,
    properties: Vec<String>,
}

/// Electron `SaveDialogOptions`, with `windowId` for the parent.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SaveDialogOptions {
    window_id: Option<u32>,
    title: Option<String>,
    default_path: Option<String>,
    filters: Vec<FileFilter>,
    properties: Vec<String>,
}

/// Electron `MessageBoxOptions`, with `windowId` for the parent.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct MessageBoxOptions {
    window_id: Option<u32>,
    message: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    buttons: Vec<String>,
    title: Option<String>,
    detail: Option<String>,
    checkbox_checked: bool,
    cancel_id: Option<i64>,
}

/// `dialog_open` result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenResult {
    canceled: bool,
    file_paths: Vec<String>,
}

/// `dialog_save` result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveResult {
    canceled: bool,
    file_path: String,
}

/// `dialog_message` result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageResult {
    response: i64,
    checkbox_checked: bool,
}

/// Most buttons a native message box shows (partial, B.2).
pub(crate) const MAX_BUTTONS: usize = 3;

/// The dialog buttons for Electron `buttons` (first three).
pub(crate) fn dialog_buttons(buttons: &[String]) -> MessageDialogButtons {
    match buttons {
        [] => MessageDialogButtons::Ok,
        [a] => MessageDialogButtons::OkCustom(a.clone()),
        [a, b] => MessageDialogButtons::OkCancelCustom(a.clone(), b.clone()),
        [a, b, c, ..] => MessageDialogButtons::YesNoCancelCustom(a.clone(), b.clone(), c.clone()),
    }
}

/// Electron's default `cancelId`: the first button labelled "cancel" or
/// "no" (any case), else 0.
pub(crate) fn default_cancel_id(buttons: &[String]) -> i64 {
    buttons
        .iter()
        .position(|b| b.eq_ignore_ascii_case("cancel") || b.eq_ignore_ascii_case("no"))
        .and_then(|i| i64::try_from(i).ok())
        .unwrap_or(0)
}

/// Maps the dialog result to Electron's `response` button index.
pub(crate) fn response_index(
    result: &MessageDialogResult,
    buttons: &[String],
    cancel_id: i64,
) -> i64 {
    let used = &buttons[..buttons.len().min(MAX_BUTTONS)];
    let index = match result {
        MessageDialogResult::Ok | MessageDialogResult::Yes => Some(0),
        MessageDialogResult::No if used.len() >= 2 => Some(1),
        MessageDialogResult::Custom(label) => used.iter().position(|b| b == label),
        MessageDialogResult::No | MessageDialogResult::Cancel => None,
    };
    index
        .and_then(|i| i64::try_from(i).ok())
        .unwrap_or(cancel_id)
}

fn dialog<R: Runtime>(webview: &Webview<R>) -> Result<Dialog<R>> {
    webview
        .app_handle()
        .try_state::<Dialog<R>>()
        .map(|d| d.inner().clone())
        .ok_or_else(|| Error::not_ready("The dialog plugin is not registered."))
}

/// The native window `bw-<id>`, whatever webview it holds (after a switch to
/// a remote page it holds `bwr-<id>`).
fn parent<R: Runtime>(webview: &Webview<R>, window_id: Option<u32>) -> Option<Window<R>> {
    window_id.and_then(|id| webview.app_handle().get_window(&ui_label(id)))
}

fn file_builder<R: Runtime>(
    dialog: &Dialog<R>,
    parent: Option<&Window<R>>,
    title: Option<&String>,
    default_path: Option<&String>,
    filters: &[FileFilter],
    properties: &[String],
) -> FileDialogBuilder<R> {
    let mut b = dialog.file();
    if let Some(p) = parent {
        b = b.set_parent(p);
    }
    if let Some(t) = title {
        b = b.set_title(t);
    }
    if let Some(path) = default_path.map(PathBuf::from) {
        if path.is_dir() {
            b = b.set_directory(&path);
        } else {
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                b = b.set_directory(dir);
            }
            if let Some(name) = path.file_name() {
                b = b.set_file_name(name.to_string_lossy());
            }
        }
    }
    for f in filters {
        let ext: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
        b = b.add_filter(&f.name, &ext);
    }
    if properties.iter().any(|p| p == "createDirectory") {
        b = b.set_can_create_directories(true);
    }
    b
}

fn path_string(p: FilePath) -> String {
    match p.into_path() {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(_) => String::new(),
    }
}

#[tauri::command]
pub(crate) async fn dialog_open<R: Runtime>(
    webview: Webview<R>,
    _state: State<'_, Overwolf<R>>,
    request: Request<'_>,
) -> Result<OpenResult> {
    require_main(&webview)?;
    let o: OpenDialogOptions = body(&request, "dialog_open")?;
    if crate::lab::block_os_surface(
        "dialog_open",
        || serde_json::json!({ "title": o.title, "properties": o.properties }),
    ) {
        return Ok(OpenResult {
            canceled: true,
            file_paths: Vec::new(),
        });
    }
    let d = dialog(&webview)?;
    let parent = parent(&webview, o.window_id);
    let b = file_builder(
        &d,
        parent.as_ref(),
        o.title.as_ref(),
        o.default_path.as_ref(),
        &o.filters,
        &o.properties,
    );
    let has = |p: &str| o.properties.iter().any(|x| x == p);
    let (folders, multi) = (has("openDirectory"), has("multiSelections"));
    let (tx, rx) = oneshot::channel::<Option<Vec<FilePath>>>();
    match (folders, multi) {
        (true, true) => b.pick_folders(move |r| {
            let _ = tx.send(r);
        }),
        (true, false) => b.pick_folder(move |r| {
            let _ = tx.send(r.map(|p| vec![p]));
        }),
        (false, true) => b.pick_files(move |r| {
            let _ = tx.send(r);
        }),
        (false, false) => b.pick_file(move |r| {
            let _ = tx.send(r.map(|p| vec![p]));
        }),
    }
    let picked = rx.await.ok().flatten().unwrap_or_default();
    let file_paths: Vec<String> = picked
        .into_iter()
        .map(path_string)
        .filter(|s| !s.is_empty())
        .collect();
    Ok(OpenResult {
        canceled: file_paths.is_empty(),
        file_paths,
    })
}

#[tauri::command]
pub(crate) async fn dialog_save<R: Runtime>(
    webview: Webview<R>,
    _state: State<'_, Overwolf<R>>,
    request: Request<'_>,
) -> Result<SaveResult> {
    require_main(&webview)?;
    let o: SaveDialogOptions = body(&request, "dialog_save")?;
    if crate::lab::block_os_surface(
        "dialog_save",
        || serde_json::json!({ "title": o.title, "properties": o.properties }),
    ) {
        return Ok(SaveResult {
            canceled: true,
            file_path: String::new(),
        });
    }
    let d = dialog(&webview)?;
    let parent = parent(&webview, o.window_id);
    let b = file_builder(
        &d,
        parent.as_ref(),
        o.title.as_ref(),
        o.default_path.as_ref(),
        &o.filters,
        &o.properties,
    );
    let (tx, rx) = oneshot::channel::<Option<FilePath>>();
    b.save_file(move |r| {
        let _ = tx.send(r);
    });
    let file_path = rx.await.ok().flatten().map(path_string).unwrap_or_default();
    Ok(SaveResult {
        canceled: file_path.is_empty(),
        file_path,
    })
}

#[tauri::command]
pub(crate) async fn dialog_message<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    request: Request<'_>,
) -> Result<MessageResult> {
    require_main(&webview)?;
    let options: MessageBoxOptions = body(&request, "dialog_message")?;
    let cancel_id = options
        .cancel_id
        .unwrap_or_else(|| default_cancel_id(&options.buttons));
    if crate::lab::block_os_surface(
        "dialog_message",
        || serde_json::json!({ "message": options.message, "buttons": options.buttons }),
    ) {
        return Ok(MessageResult {
            response: cancel_id,
            checkbox_checked: options.checkbox_checked,
        });
    }
    let d = dialog(&webview)?;
    if options.buttons.len() > MAX_BUTTONS {
        host(&state).log(
            LogLevel::Warn,
            &format!(
                "dialog.showMessageBox shows the first {MAX_BUTTONS} of {} buttons",
                options.buttons.len()
            ),
        );
    }
    let text = match options.detail.as_deref().filter(|d| !d.is_empty()) {
        Some(detail) => format!("{}\n\n{detail}", options.message),
        None => options.message.clone(),
    };
    let kind = match options.kind.as_deref() {
        Some("error") => MessageDialogKind::Error,
        Some("warning") => MessageDialogKind::Warning,
        _ => MessageDialogKind::Info,
    };
    let mut b = d
        .message(text)
        .kind(kind)
        .buttons(dialog_buttons(&options.buttons));
    if let Some(t) = options.title.as_ref() {
        b = b.title(t);
    }
    if let Some(p) = parent(&webview, options.window_id) {
        b = b.parent(&p);
    }
    let (tx, rx) = oneshot::channel();
    b.show_with_result(move |r| {
        let _ = tx.send(r);
    });
    let result = rx.await.unwrap_or(MessageDialogResult::Cancel);
    Ok(MessageResult {
        response: response_index(&result, &options.buttons, cancel_id),
        checkbox_checked: options.checkbox_checked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn buttons_map_to_native_sets() {
        assert!(matches!(dialog_buttons(&[]), MessageDialogButtons::Ok));
        assert!(
            matches!(dialog_buttons(&b(&["Go"])), MessageDialogButtons::OkCustom(a) if a == "Go")
        );
        assert!(matches!(
            dialog_buttons(&b(&["A", "B", "C", "D"])),
            MessageDialogButtons::YesNoCancelCustom(a, b, c) if a == "A" && b == "B" && c == "C"
        ));
    }

    #[test]
    fn responses_map_to_indices() {
        let three = b(&["Save", "Don't save", "Cancel"]);
        let cancel = default_cancel_id(&three);
        assert_eq!(cancel, 2);
        assert_eq!(
            response_index(
                &MessageDialogResult::Custom("Don't save".into()),
                &three,
                cancel
            ),
            1
        );
        assert_eq!(response_index(&MessageDialogResult::Yes, &three, cancel), 0);
        assert_eq!(
            response_index(&MessageDialogResult::Cancel, &three, cancel),
            2
        );
        assert_eq!(response_index(&MessageDialogResult::Ok, &[], 0), 0);
        assert_eq!(default_cancel_id(&b(&["Yes", "No"])), 1);
        assert_eq!(default_cancel_id(&b(&["OK"])), 0);
    }
}
