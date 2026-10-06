//! # tauri-plugin-overwolf
//!
//! A Tauri 2 plugin that hosts the Overwolf runtime services an app used under
//! ow-electron: ads (`<owadview>`), anonymous analytics, consent, email hashes,
//! the package manager and the update client.
//!
//! This crate is the Rust half of ow-tauri. The JavaScript half, the npm
//! package `ow-tauri`, runs the app's existing main-process code in a hidden
//! webview and talks to this plugin over Tauri IPC: commands from webviews to
//! Rust, and one ordered `tauri::ipc::Channel` per webview from Rust back. The wire contract between
//! the two is specified in `docs/CONTRACT.md` at the repository root, and the
//! design in `docs/ARCHITECTURE.md`.
//!
//! ## Status
//!
//! Scaffold only. The modules described in the architecture document land in
//! later commits; until then the crate exposes the constants every other part
//! of the contract refers to.
//!
//! ## Example
//!
//! ```
//! assert_eq!(tauri_plugin_overwolf::PLUGIN_NAME, "overwolf");
//! assert!(tauri_plugin_overwolf::COMMAND_PREFIX.starts_with("plugin:"));
//! ```
#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

/// The plugin name registered with Tauri.
///
/// Permissions are namespaced with it (`overwolf:default`,
/// `overwolf:allow-ipc-invoke`, ...) and so are command names on the wire.
///
/// ```
/// use tauri_plugin_overwolf::PLUGIN_NAME;
/// assert_eq!(format!("{PLUGIN_NAME}:default"), "overwolf:default");
/// ```
pub const PLUGIN_NAME: &str = "overwolf";

/// The prefix that JavaScript callers put in front of a command name when they
/// call `invoke`.
///
/// ```
/// use tauri_plugin_overwolf::COMMAND_PREFIX;
/// assert_eq!(format!("{COMMAND_PREFIX}ipc_invoke"), "plugin:overwolf|ipc_invoke");
/// ```
pub const COMMAND_PREFIX: &str = "plugin:overwolf|";

#[cfg(test)]
mod tests {
    use super::{COMMAND_PREFIX, PLUGIN_NAME};

    #[test]
    fn command_prefix_is_derived_from_the_plugin_name() {
        assert_eq!(COMMAND_PREFIX, format!("plugin:{PLUGIN_NAME}|"));
    }
}
