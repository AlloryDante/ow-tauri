//! `globalShortcut` commands (CONTRACT A.2.3), through
//! `tauri-plugin-global-shortcut`.

use std::sync::{Arc, Weak};

use tauri::{Manager, Runtime, State, Webview};
use tauri_plugin_global_shortcut::{GlobalShortcut, ShortcutState};

use super::{host, require_main};
use crate::error::Result;
use crate::ext::Overwolf;
use crate::host::Host;
use crate::ipc::messages::HostMessage;
use crate::state::log::LogLevel;

impl<R: Runtime> Host<R> {
    fn shortcuts(&self) -> Option<State<'_, GlobalShortcut<R>>> {
        self.app.try_state::<GlobalShortcut<R>>()
    }

    /// Registers an Electron accelerator; `false` when it is invalid or
    /// taken (Electron semantics).
    pub(crate) fn register_shortcut(self: &Arc<Self>, accelerator: &str, id: u64) -> bool {
        let Some(normalized) = crate::accelerator::normalize(accelerator) else {
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
        if self.with_core(|c| c.shortcuts.contains_key(accelerator))
            || plugin.is_registered(normalized.as_str())
        {
            return false;
        }
        let weak: Weak<Self> = Arc::downgrade(self);
        let reported = accelerator.to_owned();
        let registered = plugin.on_shortcut(normalized.as_str(), move |_app, _shortcut, event| {
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
                self.with_core(|c| c.shortcuts.insert(accelerator.to_owned(), id));
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

    /// Unregisters one accelerator this plugin registered.
    pub(crate) fn unregister_shortcut(self: &Arc<Self>, accelerator: &str) {
        if self
            .with_core(|c| c.shortcuts.remove(accelerator))
            .is_none()
        {
            return;
        }
        if let (Some(plugin), Some(normalized)) =
            (self.shortcuts(), crate::accelerator::normalize(accelerator))
            && let Err(err) = plugin.unregister(normalized.as_str())
        {
            self.log(
                LogLevel::Warn,
                &format!("globalShortcut: unregistering {accelerator:?} failed: {err}"),
            );
        }
    }

    /// Unregisters every accelerator this plugin registered (shortcuts the
    /// app registered itself stay).
    pub(crate) fn unregister_all_shortcuts(self: &Arc<Self>) {
        let all: Vec<String> = self.with_core(|c| c.shortcuts.keys().cloned().collect());
        for accelerator in all {
            self.unregister_shortcut(&accelerator);
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
