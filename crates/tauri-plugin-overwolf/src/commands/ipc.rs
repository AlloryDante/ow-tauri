//! IPC routing commands (CONTRACT A.2.1, A.2.5, C).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::ipc::{Channel, Request};
use tauri::{Runtime, State, Webview};

use super::{body, host, require_main, require_main_or_ui, require_ui};
use crate::config::filter_pending_browser_args;
use crate::error::{Error, Result};
use crate::ext::Overwolf;
use crate::ipc::messages::{HostMessage, present};
use crate::ipc::router::SenderInfo;
use crate::state::log::LogLevel;

/// `ipc_subscribe` result.
#[derive(Debug, Serialize)]
pub(crate) struct Subscribed {
    epoch: String,
}

/// `ipc_invoke` result.
#[derive(Debug, Serialize)]
pub(crate) struct Accepted {
    id: u64,
}

fn encoded_size(channel: &str, args: &[Value]) -> usize {
    channel.len() + serde_json::to_vec(args).map_or(usize::MAX, |v| v.len())
}

/// The sender of an `ipc_invoke` / `ipc_send`, with the document URL the
/// plugin recorded from the webview's page loads (no round trip to the
/// event loop per message).
fn sender(core: &crate::host::Core, label: &str, window_id: u32) -> SenderInfo {
    SenderInfo {
        label: label.to_owned(),
        window_id,
        url: core.urls.get(label).cloned().unwrap_or_default(),
    }
}

#[tauri::command]
pub(crate) async fn ipc_subscribe<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    on_message: Channel<Vec<HostMessage>>,
    user_agent: Option<String>,
) -> Result<Subscribed> {
    let host = host(&state);
    require_main_or_ui(&webview, host)?;
    if let Some(ua) = user_agent.as_deref()
        && require_main(&webview).is_ok()
    {
        // E.1: the platform webview's default UA, reported once.
        host.report_user_agent(ua);
    }
    let label = webview.label().to_owned();
    let epoch = uuid::Uuid::new_v4().simple().to_string();
    let now = host.now();
    host.with_core(|c| {
        c.sinks.insert(label.clone(), on_message);
        c.router.subscribe(&label, &epoch, now);
    });
    Ok(Subscribed { epoch })
}

#[tauri::command]
pub(crate) async fn bootstrap<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<Value> {
    require_main(&webview)?;
    Ok(host(&state).snapshot())
}

#[tauri::command]
pub(crate) async fn main_ready<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    pending_browser_args: Option<Vec<String>>,
) -> Result<()> {
    require_main(&webview)?;
    let host = host(&state);
    let first = host.with_core(|c| !std::mem::replace(&mut c.main_ready, true));
    if !first {
        return Ok(());
    }
    // A.1.1: the switches recorded before main_ready replace the stored set.
    let args = filter_pending_browser_args(&pending_browser_args.unwrap_or_default());
    let ignored = !args.is_empty() && host.info.os != crate::paths::TargetOs::Windows;
    if let Err(err) = host.ow_tauri.update(|s| s.pending_browser_args = args) {
        host.log(
            LogLevel::Warn,
            &format!("could not record browser switches: {}", err.kind()),
        );
    }
    if ignored {
        host.log(
            LogLevel::Warn,
            "browser switches are recorded but only apply on Windows",
        );
    }
    host.log(LogLevel::Info, "main_ready");
    host.start_analytics();
    Ok(())
}

#[tauri::command]
pub(crate) async fn ipc_main_ready<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<()> {
    require_main(&webview)?;
    let host = host(&state);
    let now = host.now();
    host.with_core(|c| c.router.main_ready(now));
    Ok(())
}

#[tauri::command]
pub(crate) async fn ipc_invoke<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    channel: String,
    args: Vec<Value>,
    epoch: String,
    seq: u64,
) -> Result<Accepted> {
    let host = host(&state);
    let window_id = require_ui(&webview, host)?;
    let size = encoded_size(&channel, &args);
    let label = webview.label();
    let now = host.now();
    let id = host.with_core(|c| {
        let sender = sender(c, label, window_id);
        c.router
            .invoke(&sender, &epoch, seq, &channel, args, size, now)
    })?;
    Ok(Accepted { id })
}

#[tauri::command]
pub(crate) async fn ipc_send<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    channel: String,
    args: Vec<Value>,
    epoch: String,
    seq: u64,
) -> Result<()> {
    let host = host(&state);
    let window_id = require_ui(&webview, host)?;
    let size = encoded_size(&channel, &args);
    let label = webview.label();
    let now = host.now();
    host.with_core(|c| {
        let sender = sender(c, label, window_id);
        c.router
            .send(&sender, &epoch, seq, &channel, args, size, now)
    })
}

#[tauri::command]
pub(crate) async fn ipc_skip<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    epoch: String,
    seq: u64,
) -> Result<()> {
    let host = host(&state);
    require_ui(&webview, host)?;
    let label = webview.label().to_owned();
    let now = host.now();
    host.with_core(|c| c.router.skip(&label, &epoch, seq, now));
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplyArgs {
    id: u64,
    ok: bool,
    #[serde(default, deserialize_with = "present")]
    value: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
    seq: u64,
}

/// `ipc_reply`. Reads the raw payload so an absent `value` (`undefined`)
/// stays distinct from `null`.
#[tauri::command]
pub(crate) async fn ipc_reply<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    request: Request<'_>,
) -> Result<()> {
    require_main(&webview)?;
    let args: ReplyArgs = body(&request, "ipc_reply")?;
    let error = if args.ok {
        None
    } else {
        args.error.or_else(|| {
            serde_json::to_value(Error::new(
                crate::ErrorCode::IpcRemoteError,
                "The handler failed.",
                None,
            ))
            .ok()
        })
    };
    let host = host(&state);
    let now = host.now();
    host.with_core(|c| {
        c.router
            .reply(args.id, args.ok, args.value, error, args.seq, now);
    });
    Ok(())
}

#[tauri::command]
pub(crate) async fn ipc_emit<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    target: u32,
    channel: String,
    args: Vec<Value>,
    seq: u64,
) -> Result<()> {
    require_main(&webview)?;
    let host = host(&state);
    let size = encoded_size(&channel, &args);
    let now = host.now();
    host.with_core(|c| {
        let label = c.windows.ipc_target(target);
        c.router
            .emit(target, label.as_deref(), seq, &channel, args, size, now)
    })
}

/// `ipc_emit_skip`: outbound `seq` to window `target` will never be sent
/// (C.5). The main runtime reports it when an `ipc_emit` or `ipc_reply` it
/// numbered could not reach the plugin.
#[tauri::command]
pub(crate) async fn ipc_emit_skip<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    target: u32,
    seq: u64,
) -> Result<()> {
    require_main(&webview)?;
    let host = host(&state);
    let now = host.now();
    host.with_core(|c| {
        let local = c.windows.ipc_target(target).is_some();
        c.router.emit_skip(target, local, seq, now);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_args_keep_null_distinct() {
        let a: ReplyArgs =
            serde_json::from_value(serde_json::json!({"id":1,"ok":true,"seq":1})).unwrap();
        assert!(a.value.is_none());
        let b: ReplyArgs =
            serde_json::from_value(serde_json::json!({"id":1,"ok":true,"value":null,"seq":1}))
                .unwrap();
        assert_eq!(b.value, Some(Value::Null));
        assert_eq!(encoded_size("ab", &[serde_json::json!(1)]), 5);
    }
}
