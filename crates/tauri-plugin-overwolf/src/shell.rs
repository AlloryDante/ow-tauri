//! Checks for `shell_open_external` and `shell_open_path` (CONTRACT A.2.3,
//! A.2.3.2). The opening itself goes through `tauri-plugin-opener`; this module
//! only decides whether a URL or path may be handed to it.
//!
//! ```
//! use tauri_plugin_overwolf::shell::validate_external_url;
//! assert!(validate_external_url("https://www.overwolf.com/").is_ok());
//! assert!(validate_external_url("mailto:someone@example.com").is_ok());
//! assert!(validate_external_url("file:///etc/passwd").is_err());
//! assert!(validate_external_url("https://user:pw@example.com/").is_err());
//! assert!(validate_external_url("/relative").is_err());
//! ```

use std::path::Path;

use url::Url;

use crate::error::Error;
use crate::paths::TargetOs;

/// The schemes `shell.openExternal` accepts.
pub const EXTERNAL_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// Parses `input` with the WHATWG URL parser and checks it for
/// `shell_open_external`.
///
/// # Errors
///
/// `invalid-argument` when it is not an absolute URL, its scheme is not
/// `http`, `https` or `mailto`, or it carries credentials.
pub fn validate_external_url(input: &str) -> Result<Url, Error> {
    let url = Url::parse(input).map_err(|_| Error::invalid_argument("not an absolute URL"))?;
    if !EXTERNAL_SCHEMES.contains(&url.scheme()) {
        return Err(Error::invalid_argument(
            "only http, https and mailto URLs can be opened",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::invalid_argument(
            "URLs with credentials cannot be opened",
        ));
    }
    if url.scheme() != "mailto" && url.host_str().is_none_or(str::is_empty) {
        return Err(Error::invalid_argument("not an absolute URL"));
    }
    Ok(url)
}

/// Windows launcher extensions refused besides `PATHEXT`.
pub const WINDOWS_EXTRA_EXTENSIONS: [&str; 11] = [
    ".lnk", ".url", ".scf", ".ps1", ".msi", ".msp", ".reg", ".hta", ".cpl", ".jar", ".exe",
];
/// macOS launcher extensions.
pub const MACOS_EXTENSIONS: [&str; 7] = [
    ".app",
    ".command",
    ".tool",
    ".terminal",
    ".workflow",
    ".pkg",
    ".mpkg",
];
/// Linux launcher extensions.
pub const LINUX_EXTENSIONS: [&str; 2] = [".desktop", ".appimage"];

/// The `PATHEXT` default used when the variable is unset.
pub const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC";

/// What is known about a path when deciding whether it is an executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathFacts {
    /// The path is a directory.
    pub is_dir: bool,
    /// The path is a file with an execute bit (Unix).
    pub has_execute_bit: bool,
}

fn lower_extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase()))
}

/// Whether `path` is an executable or a launcher on `os` (A.2.3.2).
/// Directories are allowed, except macOS `.app` bundles, which are launchers.
///
/// ```
/// use std::path::Path;
/// use tauri_plugin_overwolf::paths::TargetOs;
/// use tauri_plugin_overwolf::shell::{is_executable, PathFacts};
/// let file = PathFacts { is_dir: false, has_execute_bit: false };
/// assert!(is_executable(Path::new("C:/x/setup.EXE"), file, TargetOs::Windows, ".COM;.EXE"));
/// assert!(is_executable(Path::new("C:/x/link.lnk"), file, TargetOs::Windows, ".COM;.EXE"));
/// assert!(!is_executable(Path::new("C:/x/notes.txt"), file, TargetOs::Windows, ".COM;.EXE"));
/// ```
#[must_use]
pub fn is_executable(path: &Path, facts: PathFacts, os: TargetOs, pathext: &str) -> bool {
    let ext = lower_extension(path);
    match os {
        TargetOs::Windows => {
            if facts.is_dir {
                return false;
            }
            let Some(ext) = ext else { return false };
            pathext
                .split(';')
                .map(|e| e.trim().to_ascii_lowercase())
                .filter(|e| !e.is_empty())
                .any(|e| e == ext)
                || WINDOWS_EXTRA_EXTENSIONS.contains(&ext.as_str())
        }
        TargetOs::Macos => {
            if ext.as_deref() == Some(".app") {
                return true;
            }
            if facts.is_dir {
                return false;
            }
            facts.has_execute_bit || ext.is_some_and(|e| MACOS_EXTENSIONS.contains(&e.as_str()))
        }
        TargetOs::Linux => {
            if facts.is_dir {
                return false;
            }
            facts.has_execute_bit || ext.is_some_and(|e| LINUX_EXTENSIONS.contains(&e.as_str()))
        }
    }
}

/// The error strings `shell_open_path` returns (Electron returns a string
/// instead of throwing).
pub mod open_path_errors {
    /// The path does not exist.
    pub const NOT_FOUND: &str = "path does not exist";
    /// The path is outside the `fs_*` scope.
    pub const OUT_OF_SCOPE: &str = "path is outside the allowed scope";
    /// The path is an executable or launcher and executables are disabled.
    pub const EXECUTABLE: &str = "opening executables is disabled";
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: PathFacts = PathFacts {
        is_dir: false,
        has_execute_bit: false,
    };
    const DIR: PathFacts = PathFacts {
        is_dir: true,
        has_execute_bit: false,
    };
    const EXEC: PathFacts = PathFacts {
        is_dir: false,
        has_execute_bit: true,
    };

    #[test]
    fn urls() {
        for ok in [
            "http://a.example/x?y#z",
            "https://a.example",
            "mailto:a@b.example",
            "HTTPS://A.EXAMPLE",
        ] {
            assert!(validate_external_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "javascript:alert(1)",
            "file:///C:/Windows/system32/calc.exe",
            "ftp://a.example",
            "https://u@a.example",
            "https://:p@a.example",
            "ms-settings:privacy",
            "a.example",
            "data:text/html,x",
        ] {
            assert!(validate_external_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn windows_denylist() {
        let pathext = DEFAULT_PATHEXT;
        for p in [
            "a.exe", "a.BAT", "a.cmd", "a.js", "a.lnk", "a.url", "a.ps1", "a.msi", "a.reg",
            "a.hta", "a.cpl", "a.jar", "a.scf", "a.msp",
        ] {
            assert!(
                is_executable(Path::new(p), FILE, TargetOs::Windows, pathext),
                "{p}"
            );
        }
        for p in ["a.txt", "a.png", "a", "a.exe.txt"] {
            assert!(
                !is_executable(Path::new(p), FILE, TargetOs::Windows, pathext),
                "{p}"
            );
        }
        assert!(!is_executable(
            Path::new("folder.exe"),
            DIR,
            TargetOs::Windows,
            pathext
        ));
        // A custom PATHEXT entry is honoured.
        assert!(is_executable(
            Path::new("a.py"),
            FILE,
            TargetOs::Windows,
            ".PY"
        ));
    }

    #[test]
    fn macos_denylist() {
        assert!(is_executable(Path::new("/A.app"), DIR, TargetOs::Macos, ""));
        for p in [
            "/a.command",
            "/a.tool",
            "/a.terminal",
            "/a.workflow",
            "/a.pkg",
            "/a.mpkg",
        ] {
            assert!(
                is_executable(Path::new(p), FILE, TargetOs::Macos, ""),
                "{p}"
            );
        }
        assert!(is_executable(
            Path::new("/bin-file"),
            EXEC,
            TargetOs::Macos,
            ""
        ));
        assert!(!is_executable(
            Path::new("/Users/u/Pictures"),
            DIR,
            TargetOs::Macos,
            ""
        ));
        assert!(!is_executable(
            Path::new("/a.png"),
            FILE,
            TargetOs::Macos,
            ""
        ));
    }

    #[test]
    fn linux_denylist() {
        assert!(is_executable(
            Path::new("/a.desktop"),
            FILE,
            TargetOs::Linux,
            ""
        ));
        assert!(is_executable(
            Path::new("/a.AppImage"),
            FILE,
            TargetOs::Linux,
            ""
        ));
        assert!(is_executable(
            Path::new("/script"),
            EXEC,
            TargetOs::Linux,
            ""
        ));
        assert!(!is_executable(Path::new("/dir"), DIR, TargetOs::Linux, ""));
        assert!(!is_executable(
            Path::new("/a.txt"),
            FILE,
            TargetOs::Linux,
            ""
        ));
    }
}
