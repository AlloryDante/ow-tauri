//! # tauri-plugin-overwolf
//!
//! Overwolf ads (`<owadview>`), consent and anonymous analytics for Tauri 2
//! apps, sending Overwolf the same data an ow-electron app sends.
//!
//! The JavaScript half is the npm package `tauri-plugin-overwolf-api`
//! (`getInfo`, consent, email hashes, the analytics switches and the
//! `<owadview>` element). The wire contract is `docs/CONTRACT.md` at the
//! repository root.
//!
#![cfg_attr(
    feature = "plugin",
    doc = r#"
```no_run
# fn example(context: tauri::Context) {
tauri::Builder::default()
    .plugin(tauri_plugin_overwolf::init())
    .run(context)
    .expect("error while running the app");
# }
```
"#
)]
//!
//! ## Features
//!
//! - `plugin` (default): the Tauri plugin.
//! - `ads` (default): ad guests. On Windows and macOS it enables Tauri's
//!   `unstable` feature (child webviews) through a helper crate; on Linux
//!   ads are unsupported and Tauri stays stable.
//! - `updater`: the update client (Windows; `unsupported` elsewhere).
//! - `build`: the app's build step ([`build::run`]); use it as a build
//!   dependency with `default-features = false`.
//! - `test-util`, `lab`: test hooks and the parity lab. Refused in release
//!   builds; never enable them in a shipped app.
//!
//! Modules hidden from the docs are internal building blocks, public only
//! for their tests and not covered by semver.
//!
//! ```
//! assert_eq!(tauri_plugin_overwolf::PLUGIN_NAME, "overwolf");
//! let uid = tauri_plugin_overwolf::identity::computed_uid("Example Studio", "Example App");
//! assert_eq!(uid.len(), 40);
//! ```
#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Release guard (DESIGN §6.3): the test hooks and the parity lab never ship.
#[cfg(all(
    any(feature = "lab", feature = "test-util"),
    any(not(debug_assertions), ow_tauri_release_profile),
    not(ow_tauri_allow_dev_features)
))]
compile_error!(
    "tauri-plugin-overwolf: the `lab` and `test-util` features must not be enabled in release builds (set OW_TAURI_ALLOW_DEV_FEATURES_IN_RELEASE=1 only for lab builds; see CONTRIBUTING.md)"
);

#[cfg(feature = "build")]
pub mod build;
pub mod config;
pub mod error;
pub mod identity;
pub mod paths;
#[cfg(feature = "updater")]
pub mod updater;

// Internal building blocks: public for their doctests and tests only.
#[doc(hidden)]
pub mod ads;
#[doc(hidden)]
pub mod analytics;
#[doc(hidden)]
pub mod consent;
#[doc(hidden)]
pub mod state;

#[cfg(any(feature = "plugin", feature = "build"))]
mod app_identity;
#[cfg(feature = "plugin")]
mod capabilities;
#[cfg(feature = "plugin")]
mod commands;
#[cfg(feature = "plugin")]
mod compat;
#[cfg(feature = "plugin")]
mod ext;
#[cfg(feature = "plugin")]
mod host;
mod lab;
#[cfg(all(
    feature = "plugin",
    any(target_os = "android", target_os = "ios", test)
))]
mod mobile;
mod platform;
#[cfg(feature = "plugin")]
mod plugin;
mod types;

pub use config::{
    AdsConfig, AnalyticsConfig, Config, ConfigError, ConsentConfig, EmailHashesConfig,
    SigningConfig, StateConfig, UpdaterConfig,
};
pub use error::{Error, ErrorCode, Result};
pub use identity::{EmailHashes, HashEncoding};
pub use types::{CmpTab, CmpWindowOptions, HostInfo, Info, MachineIds, PaymentUserIdOptions};

#[cfg(feature = "plugin")]
pub use ext::{Overwolf, OverwolfExt};
#[cfg(all(feature = "plugin", target_os = "macos"))]
pub use platform::terminate::{
    handle_web_content_process_terminate, web_content_process_terminate_hook,
};
#[cfg(feature = "plugin")]
pub use plugin::{Builder, init};

/// Whether lab windows are invisible (feature `lab` and
/// `OW_TAURI_LAB_INVISIBLE=1`). A lab app shell uses it to keep the app out
/// of the Dock as well. Never enable `lab` in a shipped app.
///
/// ```
/// // The doc test process never sets OW_TAURI_LAB_INVISIBLE.
/// assert!(!tauri_plugin_overwolf::lab_invisible());
/// ```
#[cfg(feature = "lab")]
#[must_use]
pub fn lab_invisible() -> bool {
    lab::invisible()
}

/// The plugin name registered with Tauri; permissions are namespaced with
/// it (`overwolf:default`, `overwolf:machine-id`, ...).
///
/// ```
/// use tauri_plugin_overwolf::PLUGIN_NAME;
/// assert_eq!(format!("{PLUGIN_NAME}:default"), "overwolf:default");
/// ```
pub const PLUGIN_NAME: &str = "overwolf";

/// The prefix JavaScript callers put in front of a command name.
///
/// ```
/// use tauri_plugin_overwolf::COMMAND_PREFIX;
/// assert_eq!(format!("{COMMAND_PREFIX}get_info"), "plugin:overwolf|get_info");
/// ```
pub const COMMAND_PREFIX: &str = "plugin:overwolf|";

/// This crate's version.
///
/// ```
/// assert!(!tauri_plugin_overwolf::VERSION.is_empty());
/// ```
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every command the plugin registers, in wire spelling (DESIGN §3.5).
///
/// ```
/// assert_eq!(tauri_plugin_overwolf::COMMANDS.len(), 25);
/// ```
#[cfg(feature = "plugin")]
pub use commands::list::COMMANDS;

#[cfg(test)]
mod tests {
    use super::{COMMAND_PREFIX, PLUGIN_NAME};

    #[test]
    fn command_prefix_is_derived_from_the_plugin_name() {
        assert_eq!(COMMAND_PREFIX, format!("plugin:{PLUGIN_NAME}|"));
    }
}
