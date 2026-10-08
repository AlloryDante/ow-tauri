//! The per-app state directory (CONTRACT F; ADR 0007).
//!
//! `<appData>/ow-electron/<uid>/` is the directory ow-electron uses for the
//! same uid, so a user moving from the ow-electron build of an app keeps the
//! uid, consent, first-launch state and UTM parameters:
//!
//! | File | Module |
//! |---|---|
//! | `ow-electron.json` (shared keys only) | [`ow_electron`] |
//! | `ow-tauri.json` | [`ow_tauri`] |
//!
//! Nothing here is written before `RunEvent::Ready` (DESIGN §4.2).
//!
//! ```
//! use tauri_plugin_overwolf::state::StateDir;
//! let dir = StateDir::new("/home/u/.config".as_ref(), "abc");
//! assert!(dir.root().ends_with("ow-electron/abc"));
//! assert!(dir.ow_tauri_json().ends_with("ow-electron/abc/ow-tauri.json"));
//! ```

pub mod ow_electron;
pub mod ow_tauri;

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Paths inside the per-app state directory (F.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateDir {
    root: PathBuf,
}

impl StateDir {
    /// `<app_data>/ow-electron/<uid>`.
    #[must_use]
    pub fn new(app_data: &Path, uid: &str) -> Self {
        StateDir {
            root: app_data.join("ow-electron").join(uid),
        }
    }

    /// The directory itself.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `ow-electron.json`, shared with ow-electron.
    #[must_use]
    pub fn ow_electron_json(&self) -> PathBuf {
        self.root.join("ow-electron.json")
    }

    /// `ow-tauri.json`, owned by ow-tauri.
    #[must_use]
    pub fn ow_tauri_json(&self) -> PathBuf {
        self.root.join("ow-tauri.json")
    }
}

/// Writes `bytes` to `path` atomically: a temp file in the same directory,
/// flushed, then renamed over the original. Creates parent directories.
///
/// # Errors
///
/// Any I/O error; the original file is untouched when one occurs.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    crate::lab::record("state-writes.jsonl", || {
        serde_json::json!({
            "path": path.to_string_lossy(),
            "ok": result.is_ok(),
            "text": String::from_utf8_lossy(bytes),
        })
    });
    result
}

static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ow-tauri-test-{name}-{}-{}",
        std::process::id(),
        TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_and_creates_parents() {
        let dir = test_dir("atomic");
        let file = dir.join("a/b/c.json");
        write_atomic(&file, b"one").unwrap();
        write_atomic(&file, b"two").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"two");
        let leftovers: Vec<_> = std::fs::read_dir(dir.join("a/b"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn layout() {
        let d = StateDir::new(Path::new("/x"), "uid1");
        assert_eq!(d.root(), Path::new("/x/ow-electron/uid1"));
        assert_eq!(
            d.ow_electron_json(),
            Path::new("/x/ow-electron/uid1/ow-electron.json")
        );
        assert_eq!(
            d.ow_tauri_json(),
            Path::new("/x/ow-electron/uid1/ow-tauri.json")
        );
    }
}
