//! App lifecycle and Overwolf API commands (CONTRACT A.2.1, A.2.2).

use serde_json::Value;
use tauri::{Runtime, State, Webview};

use super::{host, require_main};
use crate::error::{Error, Result};
use crate::ext::Overwolf;
use crate::state::log::LogLevel;

/// Longest log message kept; longer ones are cut (F.4).
const MAX_LOG_MESSAGE: usize = 16 * 1024;

/// Cuts `message` to at most `max` bytes on a character boundary.
fn truncate(message: &str, max: usize) -> &str {
    if message.len() <= max {
        return message;
    }
    let mut end = max;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    &message[..end]
}

#[tauri::command]
pub(crate) async fn app_quit_reply<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    request_id: u64,
    prevent: bool,
) -> Result<()> {
    require_main(&webview)?;
    let host = host(&state);
    let now = host.now();
    let actions = host.with_core(|c| {
        if !c.quit.owns_lifecycle(request_id) {
            return None;
        }
        let windows = c.windows.ids();
        let mut ids = c.request_ids;
        let a = c
            .quit
            .answer_lifecycle(request_id, prevent, now, &windows, &mut ids);
        c.request_ids = ids;
        Some(a)
    });
    let actions = actions.ok_or_else(|| Error::not_found("Unknown or expired quit request."))?;
    host.run_quit_actions(actions);
    Ok(())
}

#[tauri::command]
pub(crate) async fn app_relaunch<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    args: Option<Vec<String>>,
    exec_path: Option<Value>,
) -> Result<()> {
    require_main(&webview)?;
    if exec_path.is_some_and(|v| !v.is_null()) {
        return Err(Error::unsupported(
            "app.relaunch({ execPath }) is not supported.",
        ));
    }
    let host = host(&state);
    let args = args.unwrap_or_else(|| host.info.argv.iter().skip(1).cloned().collect());
    host.with_core(|c| c.relaunch_args = Some(args));
    Ok(())
}

#[tauri::command]
pub(crate) async fn app_quit<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<()> {
    require_main(&webview)?;
    host(&state).begin_quit(0);
    Ok(())
}

#[tauri::command]
pub(crate) async fn app_exit<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    code: Option<i32>,
) -> Result<()> {
    require_main(&webview)?;
    host(&state).finish_exit(code.unwrap_or(0));
    Ok(())
}

#[tauri::command]
pub(crate) async fn app_focus<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    steal: Option<bool>,
) -> Result<()> {
    require_main(&webview)?;
    // `steal` only matters on macOS, where focusing a window of this app
    // activates it either way.
    let _ = steal;
    host(&state).focus_app();
    Ok(())
}

#[tauri::command]
pub(crate) async fn log<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    level: LogLevel,
    message: String,
) -> Result<()> {
    require_main(&webview)?;
    host(&state).log(level, truncate(&message, MAX_LOG_MESSAGE));
    Ok(())
}

#[tauri::command]
pub(crate) async fn disable_anonymous_analytics<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<()> {
    require_main(&webview)?;
    let ow = state.inner();
    if ow.0.with_core(|c| c.main_ready) {
        ow.log(
            LogLevel::Warn,
            "disableAnonymousAnalytics() after app ready applies to later events only",
        );
    }
    ow.disable_anonymous_analytics();
    Ok(())
}

#[tauri::command]
pub(crate) async fn disable_ads_optimization<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<()> {
    require_main(&webview)?;
    state.inner().disable_ads_optimization();
    Ok(())
}

#[tauri::command]
pub(crate) async fn disable_ads_fpd<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
) -> Result<()> {
    require_main(&webview)?;
    state.inner().disable_ads_fpd();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncate_keeps_char_boundaries() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abc", 2), "ab");
        assert_eq!(truncate("a\u{e9}b", 2), "a");
    }
}
