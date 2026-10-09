//! Starting a verified update (CONTRACT I.4, DESIGN §4.14): the NSIS
//! command line, the staging file name and Windows argument quoting. The
//! engine runs them; nothing here needs an OS.

use std::path::{Path, PathBuf};

/// A program to start for a Windows install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsInstall {
    /// The installer.
    pub program: PathBuf,
    /// Its arguments; always with `/UPDATE`.
    pub args: Vec<String>,
    /// Start it elevated (`runas`): the feed entry has
    /// `IsAdminRightsRequired: true`.
    pub elevate: bool,
}

/// The NSIS command (I.4). `/UPDATE` is always present (DX-minor-8), so
/// Tauri's template keeps the app data and the plugin's hooks skip the
/// uninstall work.
///
/// - `silent` (the install at exit): `/S /UPDATE`.
/// - otherwise (an app's `install()`): `/UPDATE /R`; the interactive
///   installer starts the app when it finishes, as electron-updater's
///   `quitAndInstall()` does by default.
/// - `installer_args` (`updater.installerArgs`) replace these arguments;
///   `/UPDATE` is added when they lack it.
///
/// ```
/// use std::path::Path;
/// use tauri_plugin_overwolf::updater::install::nsis_command;
/// let file = Path::new("C:/cache/setup.exe");
/// assert_eq!(nsis_command(file, None, true, false).args, ["/S", "/UPDATE"]);
/// assert_eq!(nsis_command(file, None, false, true).args, ["/UPDATE", "/R"]);
/// let custom = ["/P".to_owned()];
/// assert_eq!(nsis_command(file, Some(&custom), true, false).args, ["/P", "/UPDATE"]);
/// ```
#[must_use]
pub fn nsis_command(
    file: &Path,
    installer_args: Option<&[String]>,
    silent: bool,
    admin_rights_required: bool,
) -> WindowsInstall {
    let mut args = if let Some(custom) = installer_args {
        custom.to_vec()
    } else if silent {
        vec!["/S".to_owned(), "/UPDATE".to_owned()]
    } else {
        vec!["/UPDATE".to_owned(), "/R".to_owned()]
    };
    if !args.iter().any(|a| a.eq_ignore_ascii_case("/UPDATE")) {
        args.push("/UPDATE".to_owned());
    }
    WindowsInstall {
        program: file.to_path_buf(),
        args,
        elevate: admin_rights_required,
    }
}

/// A safe local name for a downloaded file: the last segment of its URL
/// path, limited to `[A-Za-z0-9._-]`, never starting with `.`, at most 128
/// characters, else `update<ext>`.
///
/// ```
/// use tauri_plugin_overwolf::updater::install::download_file_name;
/// assert_eq!(download_file_name("https://x.example/a/My App Setup 1.2.exe?x=1", ".exe"), "My_App_Setup_1.2.exe");
/// assert_eq!(download_file_name("https://x.example/a/..", ".exe"), "update.exe");
/// assert_eq!(download_file_name("https://x.example/a/setup.msi", ".exe"), "update.exe");
/// ```
#[must_use]
pub fn download_file_name(url: &str, ext: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or_default();
    let last = path.rsplit('/').next().unwrap_or_default();
    let decoded = percent_decode(last);
    let cleaned: String = decoded
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let ok = !cleaned.is_empty()
        && !cleaned.starts_with('.')
        && cleaned.len() <= 128
        && cleaned
            .to_ascii_lowercase()
            .ends_with(&ext.to_ascii_lowercase());
    if ok { cleaned } else { format!("update{ext}") }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = bytes.get(i + 1..i + 3)
            && let Ok(hex) = std::str::from_utf8(hex)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Quotes one argument for a Windows command line (`CommandLineToArgvW`
/// rules), for the elevated start through `ShellExecuteW`.
///
/// ```
/// use tauri_plugin_overwolf::updater::install::quote_windows_arg;
/// assert_eq!(quote_windows_arg("/S"), "/S");
/// assert_eq!(quote_windows_arg("/D=C:\\Program Files\\A"), "\"/D=C:\\Program Files\\A\"");
/// ```
#[must_use]
pub fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_is_always_passed() {
        let file = Path::new("C:/cache/setup.exe");
        for custom in [
            vec!["/S".to_owned()],
            vec!["/S".to_owned(), "/update".to_owned()],
            vec![],
        ] {
            for silent in [true, false] {
                let c = nsis_command(file, Some(&custom), silent, false);
                assert_eq!(
                    c.args
                        .iter()
                        .filter(|a| a.eq_ignore_ascii_case("/UPDATE"))
                        .count(),
                    1,
                    "{custom:?}"
                );
            }
        }
        let c = nsis_command(file, None, false, true);
        assert_eq!((c.program.as_path(), c.elevate), (file, true));
    }

    #[test]
    fn file_names() {
        assert_eq!(download_file_name("a/b%20c.exe", ".exe"), "b_c.exe");
        assert_eq!(download_file_name("a/%zz.exe", ".exe"), "_zz.exe");
        assert_eq!(
            download_file_name(&format!("{}.exe", "x".repeat(200)), ".exe"),
            "update.exe"
        );
        assert_eq!(download_file_name("", ".exe"), "update.exe");
    }

    #[test]
    fn windows_quoting() {
        assert_eq!(quote_windows_arg("/S"), "/S");
        assert_eq!(quote_windows_arg(""), "\"\"");
        assert_eq!(
            quote_windows_arg("/D=C:\\Program Files\\A"),
            "\"/D=C:\\Program Files\\A\""
        );
        assert_eq!(quote_windows_arg("a \"b\""), "\"a \\\"b\\\"\"");
        assert_eq!(quote_windows_arg("x y\\"), "\"x y\\\\\"");
    }
}
