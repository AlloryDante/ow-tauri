//! `globalShortcut` commands (CONTRACT A.2.3), through
//! `tauri-plugin-global-shortcut`.

use std::sync::{Arc, Weak};

use tauri::{Manager, Runtime, State, Webview};
use tauri_plugin_global_shortcut::{GlobalShortcut, Shortcut, ShortcutState};

use super::{host, require_main};
use crate::error::Result;
use crate::ext::Overwolf;
use crate::host::Host;
use crate::ipc::messages::HostMessage;
use crate::state::log::LogLevel;

/// The accelerator in plugin syntax and the shortcut it means; `None` when
/// it does not parse.
fn parse(accelerator: &str) -> Option<(String, Shortcut)> {
    let normalized = crate::accelerator::normalize(accelerator)?;
    let shortcut = normalized.parse::<Shortcut>().ok()?;
    Some((normalized, shortcut))
}

impl<R: Runtime> Host<R> {
    fn shortcuts(&self) -> Option<State<'_, GlobalShortcut<R>>> {
        self.app.try_state::<GlobalShortcut<R>>()
    }

    /// Registers an Electron accelerator; `false` when it is invalid or
    /// taken (Electron semantics).
    pub(crate) fn register_shortcut(self: &Arc<Self>, accelerator: &str, id: u64) -> bool {
        let Some((normalized, shortcut)) = parse(accelerator) else {
            self.log(
                LogLevel::Warn,
                &format!("globalShortcut: unsupported accelerator {accelerator:?}"),
            );
            return false;
        };
        let Some(plugin) = self.shortcuts() else {
            self.log(
                LogLevel::Warn,
                "globalShortcut: the global-shortcut plugin is not registered",
            );
            return false;
        };
        if self.with_core(|c| c.shortcuts.contains_key(&shortcut.id()))
            || plugin.is_registered(shortcut)
        {
            return false;
        }
        let weak: Weak<Self> = Arc::downgrade(self);
        let reported = accelerator.to_owned();
        let registered = plugin.on_shortcut(shortcut, move |_app, _shortcut, event| {
            let Some(host) = weak.upgrade() else { return };
            let state = match event.state {
                ShortcutState::Pressed => "pressed",
                ShortcutState::Released => "released",
            };
            host.send_main(HostMessage::GlobalShortcut {
                id,
                accelerator: reported.clone(),
                state,
            });
        });
        match registered {
            Ok(()) => {
                self.with_core(|c| c.shortcuts.insert(shortcut.id(), normalized));
                true
            }
            Err(err) => {
                self.log(
                    LogLevel::Warn,
                    &format!("globalShortcut: {accelerator:?} not registered: {err}"),
                );
                false
            }
        }
    }

    /// Unregisters one accelerator this plugin registered, matched by
    /// meaning: `Ctrl+K` and `CommandOrControl+K` are the same shortcut where
    /// they name the same keys.
    pub(crate) fn unregister_shortcut(self: &Arc<Self>, accelerator: &str) {
        let Some((_, shortcut)) = parse(accelerator) else {
            return;
        };
        self.unregister_id(shortcut.id());
    }

    fn unregister_id(self: &Arc<Self>, key: u32) {
        let Some(normalized) = self.with_core(|c| c.shortcuts.remove(&key)) else {
            return;
        };
        if let Some(plugin) = self.shortcuts()
            && let Err(err) = plugin.unregister(normalized.as_str())
        {
            self.log(
                LogLevel::Warn,
                &format!("globalShortcut: unregistering {normalized:?} failed: {err}"),
            );
        }
    }

    /// Unregisters every accelerator this plugin registered (shortcuts the
    /// app registered itself stay).
    pub(crate) fn unregister_all_shortcuts(self: &Arc<Self>) {
        let all: Vec<u32> = self.with_core(|c| c.shortcuts.keys().copied().collect());
        for key in all {
            self.unregister_id(key);
        }
    }
}

#[tauri::command]
pub(crate) async fn global_shortcut_register<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    accelerator: String,
    id: u64,
) -> Result<bool> {
    require_main(&webview)?;
    Ok(host(&state).register_shortcut(&accelerator, id))
}

#[tauri::command]
pub(crate) async fn global_shortcut_unregister<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    accelerator: Option<String>,
) -> Result<()> {
    require_main(&webview)?;
    let host = host(&state);
    match accelerator {
        Some(a) => host.unregister_shortcut(&a),
        None => host.unregister_all_shortcuts(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn accelerators_match_by_meaning() {
        let (_, a) = parse("Ctrl+Shift+K").unwrap();
        let (_, b) = parse("Control+Shift+K").unwrap();
        assert_eq!(a.id(), b.id());
        let (_, c) = parse("CommandOrControl+Shift+K").unwrap();
        let (_, d) = parse(if cfg!(target_os = "macos") {
            "Cmd+Shift+K"
        } else {
            "Ctrl+Shift+K"
        })
        .unwrap();
        assert_eq!(c.id(), d.id());
        assert!(parse("Ctrl+").is_none());
    }
}
