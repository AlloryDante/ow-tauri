//! `BrowserWindow` and `screen` commands (CONTRACT A.2.3, A.2.5).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::ipc::Request;
use tauri::{Manager, Runtime, State, Webview};

use super::{body, host, require_main, require_ui};
use crate::error::{Error, Result};
use crate::ext::Overwolf;
use crate::ipc::messages::present;
use crate::screen::{ElectronDisplay, Point, physical_to_dip};
use crate::window::options::{LoadTarget, WindowClassWire, WindowCreateRequest};
use crate::window::{WindowKind, normalize_window_name, remote_label, ui_label};

/// `window_create` result.
#[derive(Debug, Serialize)]
pub(crate) struct Created {
    id: u32,
    label: String,
}

/// `screen_snapshot` result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScreenSnapshot {
    displays: Vec<ElectronDisplay>,
    primary_display_id: u32,
    cursor: Point,
}

fn no_window(id: u32) -> Error {
    Error::not_found("No window with this id.").with_data(json!({ "id": id }))
}

/// `window_create`. The payload is the [`WindowCreateRequest`] itself.
#[tauri::command]
pub(crate) async fn window_create<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    request: Request<'_>,
) -> Result<Created> {
    require_main(&webview)?;
    let request: WindowCreateRequest = body(&request, "window_create")?;
    if request.window_class == WindowClassWire::Overlay {
        return Err(Error::unsupported(
            "Overlay windows need the overlay package, which is not available.",
        ));
    }
    let (id, label) = host(&state).create_window(&request)?;
    Ok(Created { id, label })
}

#[tauri::command]
pub(crate) async fn window_load<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    id: u32,
    target: LoadTarget,
) -> Result<()> {
    require_main(&webview)?;
    host(&state).load_window(id, &target)
}

#[tauri::command]
pub(crate) async fn window_close_reply<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    id: u32,
    request_id: u64,
    prevent: bool,
) -> Result<()> {
    require_main(&webview)?;
    host(&state).close_reply(id, request_id, prevent)
}

#[tauri::command]
pub(crate) async fn window_destroy<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    id: u32,
) -> Result<()> {
    require_main(&webview)?;
    let host = host(&state);
    if host.with_core(|c| c.windows.get(id).is_none()) {
        return Err(no_window(id));
    }
    host.destroy_window(id);
    Ok(())
}

#[tauri::command]
pub(crate) async fn window_eval<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    id: u32,
    code: String,
    want_result: bool,
) -> Result<Option<Value>> {
    require_main(&webview)?;
    host(&state).eval_in_window(id, &code, want_result).await
}

#[tauri::command]
pub(crate) async fn window_devtools<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    id: u32,
    open: bool,
) -> Result<()> {
    require_main(&webview)?;
    let host = host(&state);
    let kind = host
        .with_core(|c| c.windows.get(id).map(|e| e.kind))
        .ok_or_else(|| no_window(id))?;
    let label = if kind == WindowKind::Remote {
        remote_label(id)
    } else {
        ui_label(id)
    };
    let target = host.app.get_webview(&label).ok_or_else(|| no_window(id))?;
    toggle_devtools(&target, open)
}

#[cfg(any(debug_assertions, feature = "devtools"))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the release variant without devtools returns an error"
)]
fn toggle_devtools<R: Runtime>(webview: &Webview<R>, open: bool) -> Result<()> {
    if open {
        webview.open_devtools();
    } else {
        webview.close_devtools();
    }
    Ok(())
}

#[cfg(not(any(debug_assertions, feature = "devtools")))]
fn toggle_devtools<R: Runtime>(_webview: &Webview<R>, _open: bool) -> Result<()> {
    Err(Error::unsupported(
        "Developer tools need a debug build or the devtools feature.",
    ))
}

#[tauri::command]
pub(crate) async fn window_set_name<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    id: u32,
    name: String,
) -> Result<()> {
    require_main(&webview)?;
    let normalized = normalize_window_name(&name);
    host(&state)
        .with_core(|c| {
            c.windows.get_mut(id).map(|e| {
                e.name = (!normalized.is_empty()).then_some(normalized);
            })
        })
        .ok_or_else(|| no_window(id))
}

#[tauri::command]
pub(crate) async fn screen_snapshot<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<ScreenSnapshot> {
    require_main(&webview)?;
    let host = host(&state);
    let os_queries = host.options.os_queries;
    let (monitors, primary) = crate::host::monitors(&host.app, os_queries);
    let (displays, primary_display_id) = crate::host::displays_of(&monitors, primary.as_ref());
    let cursor = if os_queries {
        host.app
            .cursor_position()
            .map_or(Point::default(), |p| physical_to_dip(&monitors, p.x, p.y))
    } else {
        Point::default()
    };
    if let Ok(value) = serde_json::to_value(&displays) {
        host.with_core(|c| {
            c.patch(vec![
                ("displays".into(), value),
                ("primaryDisplayId".into(), primary_display_id.into()),
            ]);
        });
    }
    Ok(ScreenSnapshot {
        displays,
        primary_display_id,
        cursor,
    })
}

#[derive(Deserialize)]
struct EvalResultArgs {
    id: u64,
    ok: bool,
    #[serde(default, deserialize_with = "present")]
    value: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
}

/// `eval_result`. Reads the raw payload so an absent `value` stays
/// `undefined`.
#[tauri::command]
pub(crate) async fn eval_result<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    request: Request<'_>,
) -> Result<()> {
    let window = require_ui(&webview)?;
    let args: EvalResultArgs = body(&request, "eval_result")?;
    host(&state).eval_result(window, args.id, args.ok, args.value, args.error)
}
