//! Directories the plugin derives from the app's identity (DESIGN §4.1).
//!
//! `userData` is `<appData>/<PN>`, as Electron defines it, and the ads data
//! store lives at `<appData>/<PN>/EBWebView-ow`, where ow-electron keeps it,
//! so a user moving from the ow-electron build keeps the ads cookies.
//!
//! ```
//! use std::path::Path;
//! use tauri_plugin_overwolf::paths::{ads_data_dir, user_data_dir};
//! let app_data = Path::new("/home/u/.config");
//! assert_eq!(user_data_dir(app_data, "Example App"), Path::new("/home/u/.config/Example App"));
//! assert!(ads_data_dir(app_data, "Example App").ends_with("Example App/EBWebView-ow"));
//! ```

use std::path::{Path, PathBuf};

/// The operating system a computation is for. Pure functions take it as a
/// parameter so every platform's rules are testable everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOs {
    /// Windows.
    Windows,
    /// macOS.
    Macos,
    /// Linux and other Unix desktops.
    Linux,
}

impl TargetOs {
    /// The OS this binary was built for.
    ///
    /// ```
    /// use tauri_plugin_overwolf::paths::TargetOs;
    /// if cfg!(windows) {
    ///     assert_eq!(TargetOs::current(), TargetOs::Windows);
    /// }
    /// ```
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            TargetOs::Windows
        } else if cfg!(target_os = "macos") {
            TargetOs::Macos
        } else {
            TargetOs::Linux
        }
    }

    /// Node's `process.platform` spelling.
    ///
    /// ```
    /// use tauri_plugin_overwolf::paths::TargetOs;
    /// assert_eq!(TargetOs::Windows.node_platform(), "win32");
    /// assert_eq!(TargetOs::Macos.node_platform(), "darwin");
    /// ```
    #[must_use]
    pub const fn node_platform(self) -> &'static str {
        match self {
            TargetOs::Windows => "win32",
            TargetOs::Macos => "darwin",
            TargetOs::Linux => "linux",
        }
    }
}

/// The `userData` directory: `<appData>/<PN>`.
///
/// ```
/// use std::path::Path;
/// let dir = tauri_plugin_overwolf::paths::user_data_dir(Path::new("/home/u/.config"), "Example App");
/// assert_eq!(dir, Path::new("/home/u/.config/Example App"));
/// ```
#[must_use]
pub fn user_data_dir(app_data: &Path, product_name: &str) -> PathBuf {
    app_data.join(product_name)
}

/// The ads data store (WebView2 user data folder, `WKWebsiteDataStore` name
/// source): `<appData>/<PN>/EBWebView-ow`.
#[must_use]
pub fn ads_data_dir(app_data: &Path, product_name: &str) -> PathBuf {
    user_data_dir(app_data, product_name).join("EBWebView-ow")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        let app_data = Path::new("/data");
        assert_eq!(user_data_dir(app_data, "A"), Path::new("/data/A"));
        assert_eq!(
            ads_data_dir(app_data, "A"),
            Path::new("/data/A/EBWebView-ow")
        );
    }

    #[test]
    fn node_names() {
        assert_eq!(TargetOs::Windows.node_platform(), "win32");
        assert_eq!(TargetOs::Macos.node_platform(), "darwin");
        assert_eq!(TargetOs::Linux.node_platform(), "linux");
    }
}
