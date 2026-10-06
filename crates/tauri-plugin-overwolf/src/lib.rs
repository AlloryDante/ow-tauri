//! # tauri-plugin-overwolf
//!
//! A Tauri 2 plugin that hosts the Overwolf runtime services an app used under
//! ow-electron: ads (`<owadview>`), anonymous analytics, consent, email hashes,
//! the package manager and the update client.
//!
//! This crate is the Rust half of ow-tauri. The JavaScript half, the npm
//! package `ow-tauri`, runs the app's existing main-process code in a hidden
//! webview and talks to this plugin over Tauri IPC: commands from webviews to
//! Rust, and one ordered `tauri::ipc::Channel` per webview from Rust back. The
//! wire contract between the two is specified in `docs/CONTRACT.md` at the
//! repository root, and the design in `docs/ARCHITECTURE.md`.
//!
//! ## Features
//!
//! - `plugin` (default): the Tauri plugin. Without it the crate is only the
//!   manifest parser, the identity functions and the [`build`] helper, which
//!   is what an app's build script needs.
//! - `devtools`: lets `window_devtools` open devtools in release builds.
//! - `test-util`: hooks for testing IPC wiring on Tauri's mock runtime.
//!
//! ## Example
//!
//! ```
//! assert_eq!(tauri_plugin_overwolf::PLUGIN_NAME, "overwolf");
//! assert!(tauri_plugin_overwolf::COMMAND_PREFIX.starts_with("plugin:"));
//! let uid = tauri_plugin_overwolf::identity::computed_uid("Example Studio", "Example App");
//! assert_eq!(uid.len(), 40);
//! ```
#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod accelerator;
pub mod ads;
pub mod analytics;
pub mod build;
pub mod config;
pub mod consent;
pub mod error;
pub mod fs_scope;
pub mod identity;
pub mod ipc;
pub mod lifecycle;
pub mod manifest;
pub mod packages;
pub mod paths;
pub mod screen;
pub mod shell;
pub mod snapshot;
pub mod state;
pub mod updater;
pub mod window;

mod platform;

#[cfg(feature = "plugin")]
mod capabilities;
#[cfg(feature = "plugin")]
mod commands;
#[cfg(feature = "plugin")]
mod ext;
#[cfg(feature = "plugin")]
mod host;
#[cfg(feature = "plugin")]
mod plugin;

pub use error::{Error, ErrorCode, Result};
pub use packages::PackagesBackend;

#[cfg(feature = "plugin")]
pub use ext::{Overwolf, OverwolfExt};
#[cfg(feature = "plugin")]
pub use plugin::{Builder, COMMANDS};

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

/// The ow-tauri version (this crate's version), as reported in
/// `HostSnapshot.versions.owTauri` and the log's session line.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::{COMMAND_PREFIX, PLUGIN_NAME};

    #[test]
    fn command_prefix_is_derived_from_the_plugin_name() {
        assert_eq!(COMMAND_PREFIX, format!("plugin:{PLUGIN_NAME}|"));
    }
}
