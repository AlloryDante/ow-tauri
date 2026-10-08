//! Mobile builds (Android, iOS): the plugin registers its command names so
//! a shared frontend gets a clear answer, and every command answers
//! `unsupported` (DESIGN §3.5).

use tauri::Runtime;
use tauri::ipc::Invoke;

use crate::commands::list::COMMANDS;
use crate::error::Error;

/// The invoke handler: `unsupported` for every command of [`COMMANDS`].
pub(crate) fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    |invoke: Invoke<R>| {
        let command = invoke.message.command().to_owned();
        if !COMMANDS.contains(&command.as_str()) {
            return false;
        }
        invoke.resolver.reject(Error::unsupported(format!(
            "{command} is not available on mobile"
        )));
        true
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_mobile_handler_builds() {
        let handler = super::handler::<tauri::test::MockRuntime>();
        let _ = &handler;
    }
}
