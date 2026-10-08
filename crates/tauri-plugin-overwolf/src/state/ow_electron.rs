//! `ow-electron.json`, shared with ow-electron (CONTRACT F.2).
//!
//! ow-tauri reads `firstLaunch`, `cmp.*` and `utmParams`, and writes only
//! `firstLaunch`, `cmp.*` and `eHashes` (the last email hashes an app set,
//! as ow-electron does (observed)). It never writes `utmParams` and never removes
//! keys; every other key keeps its value (and, with `serde_json`'s
//! `preserve_order`, its position). Writes are read-modify-write under an
//! in-process lock, through a temp file renamed over the original. A file
//! that is not valid JSON is never touched.
//!
//! ```
//! # let dir = std::env::temp_dir().join(format!("owe-doc-{}", std::process::id()));
//! use tauri_plugin_overwolf::state::ow_electron::{CmpBlock, OwElectronFile};
//! let file = OwElectronFile::new(dir.join("ow-electron.json"));
//! assert!(!file.read().state.first_launch);
//! file.set_first_launch().unwrap();
//! file.write_cmp(&CmpBlock { cmp_string: Some("CPX".into()), time_stamp: Some(1), unified_consent_string: None }).unwrap();
//! assert_eq!(std::fs::read_to_string(file.path()).unwrap(), r#"{"firstLaunch":true,"cmp":{"cmpString":"CPX","timeStamp":1}}"#);
//! let read = file.read();
//! assert!(read.state.first_launch);
//! assert_eq!(read.state.cmp.unwrap().cmp_string.as_deref(), Some("CPX"));
//! # let _ = std::fs::remove_dir_all(&dir);
//! ```

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::write_atomic;

/// The `cmp` block (F.2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CmpBlock {
    /// TCF v2 string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmp_string: Option<String>,
    /// Unix seconds, refreshed on every launch by the startup consent flow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_stamp: Option<u64>,
    /// Stored URL-encoded: `cmp%3D<tcf>%26ac%3D<ac>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unified_consent_string: Option<String>,
}

/// The shared keys ow-tauri reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SharedState {
    /// `firstLaunch`: the first-launch event was already recorded.
    pub first_launch: bool,
    /// `cmp`, when present and an object.
    pub cmp: Option<CmpBlock>,
    /// `utmParams`, written by Overwolf's installer; `None` when absent.
    pub utm_params: Option<Value>,
}

/// Whether the file exists and parses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// No file yet.
    Missing,
    /// A JSON object.
    Valid,
    /// Present but not a JSON object (or unreadable); never written.
    Invalid,
}

/// The result of [`OwElectronFile::read`].
#[derive(Debug, Clone, PartialEq)]
pub struct ReadOutcome {
    /// File status.
    pub status: FileStatus,
    /// The shared keys (defaults when missing or invalid).
    pub state: SharedState,
}

/// Why a write did not happen.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// The existing file is not valid JSON; it was left untouched.
    #[error("ow-electron.json is not valid JSON; left untouched")]
    InvalidExisting,
    /// Reading or writing failed.
    #[error("ow-electron.json: {0}")]
    Io(#[from] std::io::Error),
}

/// Access to one `ow-electron.json`.
#[derive(Debug)]
pub struct OwElectronFile {
    path: PathBuf,
    lock: Mutex<()>,
}

impl OwElectronFile {
    /// Wraps the file at `path` (it need not exist).
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        OwElectronFile {
            path,
            lock: Mutex::new(()),
        }
    }

    /// The file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load(&self) -> (FileStatus, Map<String, Value>) {
        match std::fs::read(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (FileStatus::Missing, Map::new()),
            Err(_) => (FileStatus::Invalid, Map::new()),
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(Value::Object(map)) => (FileStatus::Valid, map),
                _ => (FileStatus::Invalid, Map::new()),
            },
        }
    }

    /// Reads the shared keys.
    #[must_use]
    pub fn read(&self) -> ReadOutcome {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (status, map) = self.load();
        let first_launch = map
            .get("firstLaunch")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let cmp = map
            .get("cmp")
            .filter(|v| v.is_object())
            .and_then(|v| serde_json::from_value::<CmpBlock>(v.clone()).ok());
        let utm_params = map.get("utmParams").filter(|v| !v.is_null()).cloned();
        ReadOutcome {
            status,
            state: SharedState {
                first_launch,
                cmp,
                utm_params,
            },
        }
    }

    /// Read-modify-write. `edit` receives the current object; every key it
    /// does not touch is written back unchanged.
    ///
    /// # Errors
    ///
    /// [`WriteError::InvalidExisting`] when the file is not a JSON object;
    /// [`WriteError::Io`] when writing fails.
    pub fn update(&self, edit: impl FnOnce(&mut Map<String, Value>)) -> Result<(), WriteError> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (status, mut map) = self.load();
        if status == FileStatus::Invalid {
            return Err(WriteError::InvalidExisting);
        }
        edit(&mut map);
        // Compact, no trailing newline: ow-electron's exact encoding (F.2).
        let bytes = serde_json::to_vec(&Value::Object(map)).map_err(std::io::Error::other)?;
        write_atomic(&self.path, &bytes)?;
        Ok(())
    }

    /// Sets `firstLaunch: true`.
    ///
    /// # Errors
    ///
    /// As [`OwElectronFile::update`].
    pub fn set_first_launch(&self) -> Result<(), WriteError> {
        self.update(|map| {
            map.insert("firstLaunch".into(), Value::Bool(true));
        })
    }

    /// Writes `eHashes: { sha1, md5, sha256 }`, replacing an earlier value
    /// (ow-electron stores the hashes of every `setUserEmailHashes()`
    /// (observed)).
    ///
    /// # Errors
    ///
    /// As [`OwElectronFile::update`].
    pub fn write_e_hashes(&self, sha1: &str, md5: &str, sha256: &str) -> Result<(), WriteError> {
        self.update(|map| {
            let mut hashes = Map::new();
            hashes.insert("sha1".into(), Value::String(sha1.to_owned()));
            hashes.insert("md5".into(), Value::String(md5.to_owned()));
            hashes.insert("sha256".into(), Value::String(sha256.to_owned()));
            map.insert("eHashes".into(), Value::Object(hashes));
        })
    }

    /// Removes `eHashes` (`clearUserEmailHashes`, SEC-M9); nothing is
    /// written when the file has none.
    ///
    /// # Errors
    ///
    /// As [`OwElectronFile::update`].
    pub fn clear_e_hashes(&self) -> Result<(), WriteError> {
        let (status, map) = {
            let _guard = self
                .lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.load()
        };
        if status != FileStatus::Invalid && !map.contains_key("eHashes") {
            return Ok(());
        }
        self.update(|map| {
            map.remove("eHashes");
        })
    }

    /// Writes the fields of `cmp` that are set, keeping other keys of the
    /// existing `cmp` object.
    ///
    /// # Errors
    ///
    /// As [`OwElectronFile::update`].
    pub fn write_cmp(&self, cmp: &CmpBlock) -> Result<(), WriteError> {
        self.update(|map| {
            let entry = map
                .entry("cmp")
                .or_insert_with(|| Value::Object(Map::new()));
            if !entry.is_object() {
                *entry = Value::Object(Map::new());
            }
            if let Value::Object(obj) = entry {
                // New keys go in ow-electron's order; existing keys keep
                // their position.
                if let Some(s) = &cmp.cmp_string {
                    obj.insert("cmpString".into(), Value::String(s.clone()));
                }
                if let Some(t) = cmp.time_stamp {
                    obj.insert("timeStamp".into(), Value::from(t));
                }
                if let Some(s) = &cmp.unified_consent_string {
                    obj.insert("unifiedConsentString".into(), Value::String(s.clone()));
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_dir;

    #[test]
    fn clearing_hashes_keeps_other_keys_and_writes_only_when_needed() {
        let dir = test_dir("owe-clear");
        let file = OwElectronFile::new(dir.join("ow-electron.json"));
        file.clear_e_hashes().unwrap();
        assert!(!file.path().exists(), "nothing to clear writes nothing");
        file.set_first_launch().unwrap();
        file.write_e_hashes("a", "b", "c").unwrap();
        file.clear_e_hashes().unwrap();
        let text = std::fs::read_to_string(file.path()).unwrap();
        assert_eq!(text, r#"{"firstLaunch":true}"#);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preserves_unknown_keys_and_order() {
        let dir = test_dir("owe-preserve");
        let path = dir.join("ow-electron.json");
        std::fs::write(
            &path,
            r#"{"zeta":{"nested":[1,2,{"k":"v"}]},"utmParams":{"utm_source":"store"},"alpha":1.5,"cmp":{"extra":"keep","cmpString":"OLD"}}"#,
        )
        .unwrap();
        let file = OwElectronFile::new(path.clone());
        let before = file.read();
        assert_eq!(before.status, FileStatus::Valid);
        assert_eq!(
            before.state.utm_params,
            Some(serde_json::json!({"utm_source":"store"}))
        );
        assert_eq!(
            before.state.cmp.as_ref().unwrap().cmp_string.as_deref(),
            Some("OLD")
        );

        file.set_first_launch().unwrap();
        file.write_cmp(&CmpBlock {
            cmp_string: None,
            time_stamp: Some(1_759_750_000),
            unified_consent_string: Some("cmp%3DX%26ac%3DY".into()),
        })
        .unwrap();

        let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["zeta", "utmParams", "alpha", "cmp", "firstLaunch"]);
        assert_eq!(value["zeta"], serde_json::json!({"nested":[1,2,{"k":"v"}]}));
        assert_eq!(value["alpha"], 1.5);
        assert_eq!(
            value["utmParams"],
            serde_json::json!({"utm_source":"store"})
        );
        assert_eq!(
            value["cmp"],
            serde_json::json!({"extra":"keep","cmpString":"OLD","timeStamp":1_759_750_000_u64,"unifiedConsentString":"cmp%3DX%26ac%3DY"})
        );
        assert!(file.read().state.first_launch);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exact_bytes_match_ow_electron() {
        let dir = test_dir("owe-bytes");
        let file = OwElectronFile::new(dir.join("ow-electron.json"));
        file.set_first_launch().unwrap();
        file.write_cmp(&CmpBlock {
            cmp_string: Some("CQTEST".into()),
            time_stamp: Some(1_791_302_123),
            unified_consent_string: Some("cmp%3DCQTEST%26ac%3D".into()),
        })
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(file.path()).unwrap(),
            r#"{"firstLaunch":true,"cmp":{"cmpString":"CQTEST","timeStamp":1791302123,"unifiedConsentString":"cmp%3DCQTEST%26ac%3D"}}"#
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn e_hashes_follow_cmp_as_in_ow_electron() {
        let dir = test_dir("owe-ehashes");
        let file = OwElectronFile::new(dir.join("ow-electron.json"));
        file.set_first_launch().unwrap();
        file.write_cmp(&CmpBlock {
            cmp_string: Some("CQ".into()),
            time_stamp: Some(1),
            unified_consent_string: None,
        })
        .unwrap();
        file.write_e_hashes("old", "old", "old").unwrap();
        file.write_e_hashes("s1", "m5", "s256").unwrap();
        assert_eq!(
            std::fs::read_to_string(file.path()).unwrap(),
            r#"{"firstLaunch":true,"cmp":{"cmpString":"CQ","timeStamp":1},"eHashes":{"sha1":"s1","md5":"m5","sha256":"s256"}}"#
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_file_is_never_written() {
        let dir = test_dir("owe-invalid");
        let path = dir.join("ow-electron.json");
        std::fs::write(&path, b"{not json").unwrap();
        let file = OwElectronFile::new(path.clone());
        let read = file.read();
        assert_eq!(read.status, FileStatus::Invalid);
        assert_eq!(read.state, SharedState::default());
        assert!(matches!(
            file.set_first_launch(),
            Err(WriteError::InvalidExisting)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"{not json");
        std::fs::write(&path, b"[1,2]").unwrap();
        assert!(matches!(
            file.set_first_launch(),
            Err(WriteError::InvalidExisting)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_created() {
        let dir = test_dir("owe-missing");
        let file = OwElectronFile::new(dir.join("sub/ow-electron.json"));
        assert_eq!(file.read().status, FileStatus::Missing);
        file.set_first_launch().unwrap();
        assert_eq!(file.read().status, FileStatus::Valid);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_object_cmp_is_ignored_on_read_and_replaced_on_write() {
        let dir = test_dir("owe-cmp");
        let path = dir.join("ow-electron.json");
        std::fs::write(&path, r#"{"cmp":"weird","utmParams":null}"#).unwrap();
        let file = OwElectronFile::new(path);
        let read = file.read();
        assert!(read.state.cmp.is_none());
        assert!(read.state.utm_params.is_none());
        file.write_cmp(&CmpBlock {
            cmp_string: Some("A".into()),
            ..CmpBlock::default()
        })
        .unwrap();
        assert_eq!(
            file.read().state.cmp.unwrap().cmp_string.as_deref(),
            Some("A")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
