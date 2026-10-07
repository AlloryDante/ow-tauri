//! `app.getPath()` names (CONTRACT B.2.1) and `HostSnapshot.paths`.
//!
//! The values follow Electron's definitions, so app code that keeps its
//! preferences under `app.getPath('userData')` finds the same directory after
//! the port: `userData` is `<appData>/<productName>`.
//!
//! ```
//! use std::path::{Path, PathBuf};
//! use tauri_plugin_overwolf::paths::{electron_paths, BaseDirs, TargetOs};
//! let base = BaseDirs {
//!     app_data: PathBuf::from("/home/u/.config"),
//!     home: PathBuf::from("/home/u"),
//!     temp: PathBuf::from("/tmp"),
//!     exe: PathBuf::from("/opt/app/app"),
//!     resources: PathBuf::from("/opt/app"),
//!     ..BaseDirs::default()
//! };
//! let paths = electron_paths(&base, "Example App", TargetOs::Linux);
//! assert_eq!(Path::new(&paths["userData"]), Path::new("/home/u/.config/Example App"));
//! assert_eq!(Path::new(&paths["logs"]), Path::new("/home/u/.config/Example App/logs"));
//! ```

use std::collections::BTreeMap;
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

/// Node's `process.arch` spelling for the running binary.
///
/// ```
/// let arch = tauri_plugin_overwolf::paths::node_arch();
/// if cfg!(target_arch = "x86_64") {
///     assert_eq!(arch, "x64");
/// }
/// ```
#[must_use]
pub fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "x86" => "ia32",
        "aarch64" => "arm64",
        "arm" => "arm",
        other => other,
    }
}

/// The OS directories `getPath` is computed from. Unknown ones stay empty and
/// are omitted from the result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BaseDirs {
    /// The OS configuration directory (`%APPDATA%`,
    /// `~/Library/Application Support`, `$XDG_CONFIG_HOME` or `~/.config`).
    pub app_data: PathBuf,
    /// Home directory.
    pub home: PathBuf,
    /// Temporary directory.
    pub temp: PathBuf,
    /// Desktop.
    pub desktop: PathBuf,
    /// Documents.
    pub documents: PathBuf,
    /// Downloads.
    pub downloads: PathBuf,
    /// Music.
    pub music: PathBuf,
    /// Pictures.
    pub pictures: PathBuf,
    /// Videos.
    pub videos: PathBuf,
    /// The running executable.
    pub exe: PathBuf,
    /// The app's resource directory; the virtual app root lives below it.
    pub resources: PathBuf,
}

/// The `userData` directory: `<appData>/<productName>`.
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

/// The virtual app root behind `app.getAppPath()`: `<resources>/app`.
/// `<appPath>/package.json` is served from the embedded manifest.
///
/// ```
/// use std::path::Path;
/// let root = tauri_plugin_overwolf::paths::app_path(Path::new("/opt/example"));
/// assert_eq!(root.join("package.json"), Path::new("/opt/example/app/package.json"));
/// ```
#[must_use]
pub fn app_path(resources: &Path) -> PathBuf {
    resources.join("app")
}

fn put(map: &mut BTreeMap<String, String>, key: &str, path: &Path) {
    if !path.as_os_str().is_empty() {
        map.insert(key.to_owned(), path.to_string_lossy().into_owned());
    }
}

/// Every `getPath` name plus `appPath`, as strings. Names whose base
/// directory is unknown are omitted.
///
/// ```
/// use std::path::{Path, PathBuf};
/// use tauri_plugin_overwolf::paths::{electron_paths, BaseDirs, TargetOs};
/// let base = BaseDirs {
///     app_data: PathBuf::from("/Users/u/Library/Application Support"),
///     home: PathBuf::from("/Users/u"),
///     ..BaseDirs::default()
/// };
/// let paths = electron_paths(&base, "Example App", TargetOs::Macos);
/// assert_eq!(Path::new(&paths["logs"]), Path::new("/Users/u/Library/Logs/Example App"));
/// assert!(!paths.contains_key("downloads"));
/// ```
#[must_use]
pub fn electron_paths(
    base: &BaseDirs,
    product_name: &str,
    os: TargetOs,
) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let user_data = if base.app_data.as_os_str().is_empty() {
        PathBuf::new()
    } else {
        user_data_dir(&base.app_data, product_name)
    };
    let logs = match os {
        TargetOs::Macos if !base.home.as_os_str().is_empty() => {
            base.home.join("Library").join("Logs").join(product_name)
        }
        _ if !user_data.as_os_str().is_empty() => user_data.join("logs"),
        _ => PathBuf::new(),
    };
    let crash_dumps = if user_data.as_os_str().is_empty() {
        PathBuf::new()
    } else {
        user_data.join("Crashpad")
    };
    put(&mut map, "appData", &base.app_data);
    put(&mut map, "userData", &user_data);
    put(&mut map, "sessionData", &user_data);
    put(&mut map, "temp", &base.temp);
    put(&mut map, "home", &base.home);
    put(&mut map, "desktop", &base.desktop);
    put(&mut map, "documents", &base.documents);
    put(&mut map, "downloads", &base.downloads);
    put(&mut map, "music", &base.music);
    put(&mut map, "pictures", &base.pictures);
    put(&mut map, "videos", &base.videos);
    put(&mut map, "logs", &logs);
    put(&mut map, "exe", &base.exe);
    put(&mut map, "crashDumps", &crash_dumps);
    if !base.resources.as_os_str().is_empty() {
        put(&mut map, "appPath", &app_path(&base.resources));
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> BaseDirs {
        BaseDirs {
            app_data: PathBuf::from("/Users/u/Library/Application Support"),
            home: PathBuf::from("/Users/u"),
            temp: PathBuf::from("/tmp"),
            desktop: PathBuf::from("/Users/u/Desktop"),
            documents: PathBuf::from("/Users/u/Documents"),
            downloads: PathBuf::from("/Users/u/Downloads"),
            music: PathBuf::from("/Users/u/Music"),
            pictures: PathBuf::from("/Users/u/Pictures"),
            videos: PathBuf::from("/Users/u/Movies"),
            exe: PathBuf::from("/Applications/X.app/Contents/MacOS/x"),
            resources: PathBuf::from("/Applications/X.app/Contents/Resources"),
        }
    }

    #[test]
    fn macos_layout() {
        // Joins use the host's separator, so compare as paths: every OS's
        // layout is computed on every host.
        let p = electron_paths(&base(), "Example App", TargetOs::Macos);
        let path = |key: &str| Path::new(&p[key]);
        assert_eq!(
            path("userData"),
            Path::new("/Users/u/Library/Application Support/Example App")
        );
        assert_eq!(p["sessionData"], p["userData"]);
        assert_eq!(path("logs"), Path::new("/Users/u/Library/Logs/Example App"));
        assert_eq!(
            path("crashDumps"),
            Path::new("/Users/u/Library/Application Support/Example App/Crashpad")
        );
        assert_eq!(
            path("appPath"),
            Path::new("/Applications/X.app/Contents/Resources/app")
        );
        assert_eq!(p.len(), 15);
    }

    #[test]
    fn linux_logs_under_user_data_and_missing_dirs_omitted() {
        let b = BaseDirs {
            app_data: PathBuf::from("/home/u/.config"),
            home: PathBuf::from("/home/u"),
            ..BaseDirs::default()
        };
        let p = electron_paths(&b, "A", TargetOs::Linux);
        assert_eq!(Path::new(&p["logs"]), Path::new("/home/u/.config/A/logs"));
        assert!(!p.contains_key("music"));
        assert!(!p.contains_key("appPath"));
    }

    #[test]
    fn node_names() {
        assert_eq!(TargetOs::Windows.node_platform(), "win32");
        assert_eq!(TargetOs::Macos.node_platform(), "darwin");
        assert!(!node_arch().is_empty());
    }
}
