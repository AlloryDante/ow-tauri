//! `shell` commands (CONTRACT A.2.3, A.2.3.2), through `tauri-plugin-opener`.

use std::path::{Path, PathBuf};

use tauri::{Runtime, State, Webview};

use super::{host, require_main};
use crate::error::{Error, Result};
use crate::ext::Overwolf;
use crate::fs_scope::{Access, FsScope};
use crate::paths::TargetOs;
use crate::shell::{
    DEFAULT_PATHEXT, PathFacts, is_executable, open_path_errors, validate_external_url,
};

#[tauri::command]
pub(crate) async fn shell_open_external<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    url: String,
) -> Result<()> {
    require_main(&webview)?;
    let url = validate_external_url(&url)?;
    host(&state).open_in_browser(&url)
}

/// The A.2.3.2 checks. Returns the path to open or the Electron-style error
/// string.
pub(crate) fn check_open_path(
    path: &str,
    scope: &FsScope,
    allow_executables: bool,
    os: TargetOs,
    pathext: &str,
) -> std::result::Result<PathBuf, &'static str> {
    let canonical =
        std::fs::canonicalize(Path::new(path)).map_err(|_| open_path_errors::NOT_FOUND)?;
    let meta = std::fs::metadata(&canonical).map_err(|_| open_path_errors::NOT_FOUND)?;
    match scope.check(&canonical, Access::Read) {
        Ok(crate::fs_scope::Resolved::File(_)) => {}
        Ok(crate::fs_scope::Resolved::Manifest) | Err(_) => {
            return Err(open_path_errors::OUT_OF_SCOPE);
        }
    }
    if !allow_executables {
        let facts = PathFacts {
            is_dir: meta.is_dir(),
            has_execute_bit: crate::platform::has_execute_bit(&meta),
        };
        if is_executable(&canonical, facts, os, pathext) {
            return Err(open_path_errors::EXECUTABLE);
        }
    }
    Ok(canonical)
}

#[tauri::command]
pub(crate) async fn shell_open_path<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    path: String,
) -> Result<String> {
    require_main(&webview)?;
    let host = host(&state);
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| DEFAULT_PATHEXT.to_owned());
    let checked = check_open_path(
        &path,
        &host.info.fs_scope,
        host.info.config.shell.open_path_allow_executables,
        host.info.os,
        &pathext,
    );
    Ok(match checked {
        Ok(target)
            if crate::lab::block_os_surface(
                "shell_open_path",
                || serde_json::json!({ "path": target.to_string_lossy() }),
            ) =>
        {
            String::new()
        }
        Ok(target) => match tauri_plugin_opener::open_path(&target, None::<&str>) {
            Ok(()) => String::new(),
            Err(_) => "Failed to open path".to_owned(),
        },
        Err(reason) => reason.to_owned(),
    })
}

#[tauri::command]
pub(crate) async fn shell_show_item_in_folder<R: Runtime>(
    webview: Webview<R>,
    _state: State<'_, Overwolf<R>>,
    path: String,
) -> Result<()> {
    require_main(&webview)?;
    let canonical = std::fs::canonicalize(Path::new(&path))
        .map_err(|e| Error::from_io("Resolving the path", &e))?;
    if crate::lab::block_os_surface(
        "shell_show_item_in_folder",
        || serde_json::json!({ "path": canonical.to_string_lossy() }),
    ) {
        return Ok(());
    }
    tauri_plugin_opener::reveal_item_in_dir(canonical)
        .map_err(|_| Error::io("The system could not show the item in its folder."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_dir;

    #[test]
    fn open_path_checks_follow_the_contract_order() {
        let dir = test_dir("open-path");
        let user = dir.join("user");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(dir.join("outside")).unwrap();
        std::fs::write(user.join("notes.txt"), "x").unwrap();
        std::fs::write(user.join("run.command"), "x").unwrap();
        std::fs::write(dir.join("outside/a.txt"), "x").unwrap();
        let scope = FsScope::new(
            user.clone(),
            dir.join("state"),
            Path::new("/virtual/app"),
            Vec::new(),
        );
        let os = TargetOs::Macos;
        let check = |p: &Path, allow| {
            check_open_path(&p.to_string_lossy(), &scope, allow, os, DEFAULT_PATHEXT)
        };
        assert!(check(&user.join("notes.txt"), false).is_ok());
        assert!(check(&user, false).is_ok());
        assert_eq!(
            check(&user.join("missing.txt"), false),
            Err(open_path_errors::NOT_FOUND)
        );
        assert_eq!(
            check(&dir.join("outside/a.txt"), false),
            Err(open_path_errors::OUT_OF_SCOPE)
        );
        assert_eq!(
            check(&user.join("run.command"), false),
            Err(open_path_errors::EXECUTABLE)
        );
        assert!(check(&user.join("run.command"), true).is_ok());
        assert_eq!(
            check(Path::new("/virtual/app/package.json"), false),
            Err(open_path_errors::NOT_FOUND)
        );
    }
}
