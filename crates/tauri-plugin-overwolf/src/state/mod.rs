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
//! Nothing here is written before `RunEvent::Ready` (DESIGN §4.2). A file
//! that does not parse is moved to `<name>.corrupt-<ms since 1970>` before
//! the next write recreates it; the newest
//! [`CORRUPT_KEPT`] such copies are kept (DESIGN §4.12, SEC-m7).
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

/// How many `<name>.corrupt-<ms>` copies of a state file are kept.
pub const CORRUPT_KEPT: usize = 3;

/// How many times a refused rename is retried (Windows: another process,
/// such as an antivirus scanner, holds the file open), and the pause
/// between attempts (DESIGN §4.12).
pub const RENAME_RETRIES: u32 = 3;

/// See [`RENAME_RETRIES`].
pub const RENAME_BACKOFF: std::time::Duration = std::time::Duration::from_millis(50);

/// Writes `bytes` to `path` atomically: a temp file in the same directory,
/// flushed, then renamed over the original. Creates parent directories. On
/// Windows a rename refused with "access denied" is retried
/// [`RENAME_RETRIES`] times, [`RENAME_BACKOFF`] apart.
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
        let retries = if cfg!(windows) { RENAME_RETRIES } else { 0 };
        retry_denied(retries, RENAME_BACKOFF, || std::fs::rename(&tmp, path))
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

/// Runs `op`, and again up to `retries` times, `backoff` apart, while it
/// fails with [`std::io::ErrorKind::PermissionDenied`] (Windows
/// `ERROR_ACCESS_DENIED`). Other errors return at once.
pub(crate) fn retry_denied<T>(
    retries: u32,
    backoff: std::time::Duration,
    mut op: impl FnMut() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let mut left = retries;
    loop {
        match op() {
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && left > 0 => {
                left -= 1;
                std::thread::sleep(backoff);
            }
            other => return other,
        }
    }
}

/// The time stamp of a `<name>.corrupt-<ms>` copy of the file `name`.
fn corrupt_stamp(file_name: &str, name: &str) -> Option<u128> {
    file_name
        .strip_prefix(name)?
        .strip_prefix(".corrupt-")?
        .parse()
        .ok()
}

/// Renames a state file that does not parse to `<name>.corrupt-<ms since
/// 1970>`, so nothing is lost, and deletes all but the newest
/// [`CORRUPT_KEPT`] such copies. Returns the new path when the rename
/// worked.
pub(crate) fn move_aside(path: &Path) -> Option<PathBuf> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let name = path.file_name()?.to_string_lossy().into_owned();
    // Two repairs in the same millisecond still keep both copies.
    let mut stamp = millis;
    let mut target = path.with_file_name(format!("{name}.corrupt-{stamp}"));
    while target.exists() {
        stamp += 1;
        target = path.with_file_name(format!("{name}.corrupt-{stamp}"));
    }
    retry_denied(
        if cfg!(windows) { RENAME_RETRIES } else { 0 },
        RENAME_BACKOFF,
        || std::fs::rename(path, &target),
    )
    .ok()?;
    prune_corrupt(path, CORRUPT_KEPT);
    Some(target)
}

/// Deletes all but the newest `keep` `<name>.corrupt-<ms>` copies of
/// `path`.
fn prune_corrupt(path: &Path, keep: usize) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return;
    };
    let name = name.to_string_lossy();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut copies: Vec<(u128, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let stamp = corrupt_stamp(&e.file_name().to_string_lossy(), &name)?;
            Some((stamp, e.path()))
        })
        .collect();
    copies.sort_by_key(|c| std::cmp::Reverse(c.0));
    for (_, old) in copies.into_iter().skip(keep) {
        let _ = std::fs::remove_file(old);
    }
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
    fn corrupt_copies_keep_the_newest_three() {
        let dir = test_dir("corrupt-keep");
        let file = dir.join("ow-electron.json");
        std::fs::write(dir.join("ow-electron.json.corrupt-5"), b"old").unwrap();
        std::fs::write(dir.join("ow-tauri.json.corrupt-1"), b"other file").unwrap();
        let mut moved = Vec::new();
        for i in 0..4 {
            std::fs::write(&file, format!("bad {i}")).unwrap();
            moved.push(move_aside(&file).unwrap());
            assert!(!file.exists());
        }
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left.len(), 4, "{left:?}");
        assert!(left.contains(&"ow-tauri.json.corrupt-1".to_owned()));
        for m in &moved[1..] {
            assert!(m.exists(), "{}", m.display());
        }
        assert!(!moved[0].exists(), "the oldest of ours went");
        assert!(!dir.join("ow-electron.json.corrupt-5").exists());
        assert_eq!(std::fs::read(&moved[3]).unwrap(), b"bad 3");
        assert_eq!(corrupt_stamp("a.json.corrupt-12", "a.json"), Some(12));
        assert_eq!(corrupt_stamp("a.json.corrupt-x", "a.json"), None);
        assert_eq!(corrupt_stamp("b.json.corrupt-1", "a.json"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn denied_renames_are_retried_three_times() {
        let denied = || std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let backoff = std::time::Duration::from_millis(1);
        let mut calls = 0;
        let r = retry_denied(RENAME_RETRIES, backoff, || {
            calls += 1;
            if calls < 3 { Err(denied()) } else { Ok(calls) }
        });
        assert_eq!(r.unwrap(), 3);
        let mut calls = 0;
        let r: std::io::Result<()> = retry_denied(RENAME_RETRIES, backoff, || {
            calls += 1;
            Err(denied())
        });
        assert!(r.is_err());
        assert_eq!(calls, 4, "one attempt and three retries");
        let mut calls = 0;
        let r: std::io::Result<()> = retry_denied(RENAME_RETRIES, backoff, || {
            calls += 1;
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        });
        assert!(r.is_err());
        assert_eq!(calls, 1, "other errors are not retried");
        assert_eq!(RENAME_BACKOFF, std::time::Duration::from_millis(50));
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
