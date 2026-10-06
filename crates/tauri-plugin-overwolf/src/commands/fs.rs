//! Scoped file commands for main-process code (CONTRACT A.2.3).

use std::io::ErrorKind;
use std::path::Path;

use tauri::{Runtime, State, Webview};

use super::{host, require_main};
use crate::error::{Error, Result};
use crate::ext::Overwolf;
use crate::fs_scope::{Access, Resolved};
use crate::state::write_atomic;

#[tauri::command]
pub(crate) async fn fs_read_text<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    path: String,
) -> Result<Option<String>> {
    require_main(&webview)?;
    let host = host(&state);
    match host.info.fs_scope.check(Path::new(&path), Access::Read)? {
        Resolved::Manifest => Ok(Some(host.info.manifest.package_json_text())),
        Resolved::File(file) => match std::fs::read_to_string(&file) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::from_io("Reading the file", &e)),
        },
    }
}

#[tauri::command]
pub(crate) async fn fs_write_text<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    path: String,
    data: String,
) -> Result<()> {
    require_main(&webview)?;
    let host = host(&state);
    let Resolved::File(file) = host.info.fs_scope.check(Path::new(&path), Access::Write)? else {
        return Err(Error::forbidden("the app manifest is read-only"));
    };
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::from_io("Creating the directory", &e))?;
    }
    write_atomic(&file, data.as_bytes()).map_err(|e| Error::from_io("Writing the file", &e))
}

#[tauri::command]
pub(crate) async fn fs_exists<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    path: String,
) -> Result<bool> {
    require_main(&webview)?;
    match host(&state)
        .info
        .fs_scope
        .check(Path::new(&path), Access::Read)
    {
        Ok(Resolved::Manifest) => Ok(true),
        Ok(Resolved::File(file)) => Ok(file.exists()),
        Err(e) if e.code() == crate::ErrorCode::Io => Ok(false),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub(crate) async fn fs_mkdir<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    path: String,
    recursive: Option<bool>,
) -> Result<()> {
    require_main(&webview)?;
    let Resolved::File(dir) = host(&state)
        .info
        .fs_scope
        .check(Path::new(&path), Access::Write)?
    else {
        return Err(Error::forbidden("the app manifest is read-only"));
    };
    if dir.is_dir() {
        return Ok(());
    }
    let result = if recursive.unwrap_or(false) {
        std::fs::create_dir_all(&dir)
    } else {
        std::fs::create_dir(&dir)
    };
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists && dir.is_dir() => Ok(()),
        Err(e) => Err(Error::from_io("Creating the directory", &e)),
    }
}
