//! Installing a verified update (CONTRACT I.4).
//!
//! This module holds the parts that need no Tauri: the Windows installer
//! command line, the staging file names, and the atomic replacement of a
//! macOS `.app` bundle or a Linux `AppImage`. The plugin runs them at exit.

use std::path::{Path, PathBuf};

use crate::error::Error;

use super::InstallerKind;

/// A program to start for a Windows install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsInstall {
    /// The program: the installer, or `msiexec`.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Start it elevated (`runas`): the feed entry has
    /// `IsAdminRightsRequired: true`.
    pub elevate: bool,
}

/// The Windows install command (I.4).
///
/// NSIS (Tauri's template): `/S` when `silent`, `/UPDATE`, and `/R` when
/// `force_run_after`, which makes the installer start the app when it
/// finishes (Tauri's template honours `/R` in silent and passive mode; an
/// interactive installer shows its own "run" option instead).
/// `installer_args` (`updater.installerArgs`) replaces these arguments.
/// MSI: `msiexec /i "<file>" /quiet /norestart`.
///
/// ```
/// use std::path::Path;
/// use tauri_plugin_overwolf::updater::{install::windows_install, InstallerKind};
/// let i = windows_install(InstallerKind::Nsis, Path::new("C:/c/setup.exe"), true, true, None, false);
/// assert_eq!(i.args, ["/S", "/UPDATE", "/R"]);
/// let i = windows_install(InstallerKind::Nsis, Path::new("C:/c/setup.exe"), false, false, None, true);
/// assert_eq!((i.args.as_slice(), i.elevate), (&["/UPDATE".to_owned()][..], true));
/// ```
#[must_use]
pub fn windows_install(
    kind: InstallerKind,
    file: &Path,
    silent: bool,
    force_run_after: bool,
    installer_args: Option<&[String]>,
    admin_rights_required: bool,
) -> WindowsInstall {
    if kind == InstallerKind::Msi {
        return WindowsInstall {
            program: PathBuf::from("msiexec.exe"),
            args: vec![
                "/i".to_owned(),
                file.to_string_lossy().into_owned(),
                "/quiet".to_owned(),
                "/norestart".to_owned(),
            ],
            elevate: admin_rights_required,
        };
    }
    let args = if let Some(custom) = installer_args {
        custom.to_vec()
    } else {
        let mut args = Vec::new();
        if silent {
            args.push("/S".to_owned());
        }
        args.push("/UPDATE".to_owned());
        if force_run_after {
            args.push("/R".to_owned());
        }
        args
    };
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
/// assert_eq!(download_file_name("https://x.example/a/..", ".zip"), "update.zip");
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

/// The file extension the update client expects for `kind`.
#[must_use]
pub fn extension(kind: InstallerKind) -> &'static str {
    match kind {
        InstallerKind::Nsis => ".exe",
        InstallerKind::Msi => ".msi",
        InstallerKind::MacZip => ".zip",
        InstallerKind::AppImage => ".AppImage",
    }
}

/// The `.app` bundle that contains `exe` (`…/Name.app/Contents/MacOS/bin`).
///
/// ```
/// use std::path::Path;
/// use tauri_plugin_overwolf::updater::install::bundle_of;
/// let b = bundle_of(Path::new("/Applications/Demo.app/Contents/MacOS/demo")).unwrap();
/// assert_eq!(b, Path::new("/Applications/Demo.app"));
/// assert!(bundle_of(Path::new("/usr/local/bin/demo")).is_none());
/// ```
#[must_use]
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .skip(1)
        .find(|p| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"))
                && exe.starts_with(p.join("Contents"))
        })
        .map(Path::to_path_buf)
}

/// The single `.app` bundle at the top of an unpacked update.
///
/// # Errors
///
/// `backend` when the folder holds no `.app` or more than one.
pub fn find_app_bundle(dir: &Path) -> Result<PathBuf, Error> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::from_io("Reading the update", &e))?;
    let mut apps = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")) && p.is_dir());
    match (apps.next(), apps.next()) {
        (Some(app), None) => Ok(app),
        _ => Err(Error::backend(
            "The update archive must hold exactly one .app bundle.",
        )),
    }
}

/// A sibling path of `target` for a staged copy or a backup:
/// `<dir>/.<name>.<tag>`.
#[must_use]
pub fn sibling(target: &Path, tag: &str) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!(".{name}.{tag}"))
}

fn remove_any(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Replaces `target` (a file or a folder) with `staged`, which must be on
/// the same volume (a sibling from [`sibling`]): `target` is renamed to a
/// backup, `staged` to `target`, then the backup is removed. When the second
/// rename fails the backup is put back, so `target` is never left missing.
///
/// # Errors
///
/// `io` when a rename fails (the original is in place again).
pub fn swap_into_place(staged: &Path, target: &Path) -> Result<(), Error> {
    let backup = sibling(target, "ow-tauri-old");
    remove_any(&backup).map_err(|e| Error::from_io("Clearing the update backup", &e))?;
    std::fs::rename(target, &backup).map_err(|e| Error::from_io("Moving the old version", &e))?;
    if let Err(e) = std::fs::rename(staged, target) {
        let _ = std::fs::rename(&backup, target);
        return Err(Error::from_io("Moving the new version into place", &e));
    }
    // The old copy is no longer needed; a failure only leaves it behind.
    let _ = remove_any(&backup);
    Ok(())
}

/// Copies a verified `AppImage` next to `target`, makes it executable and
/// swaps it in (I.4 Linux).
///
/// # Errors
///
/// `io` when the copy or the swap fails; `target` is unchanged then.
pub fn replace_file(source: &Path, target: &Path) -> Result<(), Error> {
    let staged = sibling(target, "ow-tauri-new");
    remove_any(&staged).map_err(|e| Error::from_io("Clearing the staged update", &e))?;
    std::fs::copy(source, &staged).map_err(|e| Error::from_io("Staging the update", &e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(target).map_or(0o755, |m| m.permissions().mode()) | 0o111;
        if let Err(e) = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(mode)) {
            let _ = std::fs::remove_file(&staged);
            return Err(Error::from_io("Marking the update executable", &e));
        }
    }
    swap_into_place(&staged, target).inspect_err(|_| {
        let _ = std::fs::remove_file(&staged);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("ow-tauri-install-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn nsis_and_msi_commands() {
        let file = Path::new("C:/cache/setup.exe");
        let custom = ["/S".to_owned(), "/D=C:\\x".to_owned()];
        let i = windows_install(InstallerKind::Nsis, file, false, true, Some(&custom), false);
        assert_eq!(i.args, custom);
        let i = windows_install(InstallerKind::Nsis, file, true, false, None, false);
        assert_eq!(i.args, ["/S", "/UPDATE"]);
        let i = windows_install(
            InstallerKind::Msi,
            Path::new("C:/c/a.msi"),
            false,
            true,
            Some(&custom),
            true,
        );
        assert_eq!(i.program, Path::new("msiexec.exe"));
        assert_eq!(i.args, ["/i", "C:/c/a.msi", "/quiet", "/norestart"]);
        assert!(i.elevate);
    }

    #[test]
    fn file_names() {
        assert_eq!(
            download_file_name("a/b%20c.AppImage", ".AppImage"),
            "b_c.AppImage"
        );
        assert_eq!(download_file_name("a/%zz.exe", ".exe"), "_zz.exe");
        assert_eq!(
            download_file_name(&format!("{}.exe", "x".repeat(200)), ".exe"),
            "update.exe"
        );
        assert_eq!(download_file_name("", ".zip"), "update.zip");
        assert_eq!(extension(InstallerKind::AppImage), ".AppImage");
    }

    #[test]
    fn bundle_lookup() {
        let d = dir("bundle");
        let exe = d.join("Demo.app/Contents/MacOS/demo");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        assert_eq!(bundle_of(&exe).unwrap(), d.join("Demo.app"));
        let unpacked = dir("unpacked");
        assert!(find_app_bundle(&unpacked).is_err());
        std::fs::create_dir_all(unpacked.join("New.app/Contents")).unwrap();
        std::fs::write(unpacked.join("readme.txt"), "x").unwrap();
        assert_eq!(
            find_app_bundle(&unpacked).unwrap(),
            unpacked.join("New.app")
        );
        std::fs::create_dir_all(unpacked.join("Other.app")).unwrap();
        assert!(find_app_bundle(&unpacked).is_err());
        assert!(find_app_bundle(&unpacked.join("missing")).is_err());
    }

    #[test]
    fn swaps_files_and_folders() {
        let d = dir("swap");
        let target = d.join("App.AppImage");
        std::fs::write(&target, "old").unwrap();
        let source = d.join("download.AppImage");
        std::fs::write(&source, "new").unwrap();
        replace_file(&source, &target).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert!(!sibling(&target, "ow-tauri-old").exists());
        assert!(!sibling(&target, "ow-tauri-new").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_ne!(
                std::fs::metadata(&target).unwrap().permissions().mode() & 0o111,
                0
            );
        }

        let bundle = d.join("Demo.app");
        std::fs::create_dir_all(bundle.join("Contents")).unwrap();
        std::fs::write(bundle.join("Contents/v"), "1").unwrap();
        let staged = sibling(&bundle, "ow-tauri-new");
        std::fs::create_dir_all(staged.join("Contents")).unwrap();
        std::fs::write(staged.join("Contents/v"), "2").unwrap();
        swap_into_place(&staged, &bundle).unwrap();
        assert_eq!(
            std::fs::read_to_string(bundle.join("Contents/v")).unwrap(),
            "2"
        );

        // A failed second rename restores the original.
        let missing = d.join("missing-stage");
        assert!(swap_into_place(&missing, &bundle).is_err());
        assert_eq!(
            std::fs::read_to_string(bundle.join("Contents/v")).unwrap(),
            "2"
        );
        assert!(replace_file(&d.join("nope"), &target).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
    }
}
