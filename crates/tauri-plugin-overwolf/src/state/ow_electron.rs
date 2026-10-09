//! `ow-electron.json`, shared with ow-electron (CONTRACT F.2).
//!
//! ow-tauri reads `firstLaunch`, `cmp.*` and `utmParams`, and writes only
//! `firstLaunch`, `cmp.*` and `eHashes` (the last email hashes an app set,
//! as ow-electron does (observed)). It never writes `utmParams` and never removes
//! keys; every other key keeps its value (and, with `serde_json`'s
//! `preserve_order`, its position). Writes are read-modify-write under an
//! in-process lock, through a temp file renamed over the original. Nothing
//! is written before `RunEvent::Ready`.
//!
//! A file that does not parse as a JSON object (garbage, truncated, empty,
//! `null`, `[]`) or whose shared keys have the wrong type
//! ([`FileStatus::Invalid`]) is reset the way ow-electron resets an
//! unparseable file: it reads as a first launch, and the next write starts a
//! new object, with no backup copy (W4 ruling L3). ow-electron itself keeps a
//! `[]` or wrong-typed file forever and re-sends `app_first_launch` on every
//! launch; resetting it once is a listed deviation (PARITY).
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
    /// Present but not a JSON object, a JSON object whose shared keys have
    /// the wrong type, or unreadable; the next write of a file that could
    /// be read starts a new object (no backup, as ow-electron).
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
    /// Reading or writing failed.
    #[error("ow-electron.json: {0}")]
    Io(#[from] std::io::Error),
}

/// What the file held.
#[derive(Debug)]
enum Loaded {
    Missing,
    Valid(Map<String, Value>),
    /// Not a JSON object, or a shared key of the wrong type.
    Unparseable,
    Unreadable(std::io::Error),
}

/// Whether the shared keys ow-electron reads have a type ow-electron writes:
/// `firstLaunch` a boolean, `cmp` an object, `utmParams` an object or
/// `null`. Absent keys are fine. `eHashes` may hold any JSON value
/// (`setUserEmailHashes` stores what the app passes, L1), and other keys are
/// not looked at.
fn shared_keys_well_typed(map: &Map<String, Value>) -> bool {
    let ok = |key: &str, accept: fn(&Value) -> bool| map.get(key).is_none_or(accept);
    ok("firstLaunch", Value::is_boolean)
        && ok("cmp", Value::is_object)
        && ok("utmParams", |v| v.is_object() || v.is_null())
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
        match self.load_raw() {
            Loaded::Missing => (FileStatus::Missing, Map::new()),
            Loaded::Valid(map) => (FileStatus::Valid, map),
            Loaded::Unparseable | Loaded::Unreadable(_) => (FileStatus::Invalid, Map::new()),
        }
    }

    fn load_raw(&self) -> Loaded {
        match std::fs::read(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Loaded::Missing,
            Err(e) => Loaded::Unreadable(e),
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(Value::Object(map)) if shared_keys_well_typed(&map) => Loaded::Valid(map),
                _ => Loaded::Unparseable,
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
    /// [`WriteError::Io`] when reading or writing fails. A file that is
    /// [`FileStatus::Invalid`] but readable is replaced by a new object.
    pub fn update(&self, edit: impl FnOnce(&mut Map<String, Value>)) -> Result<(), WriteError> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut map = match self.load_raw() {
            Loaded::Valid(map) => map,
            Loaded::Unreadable(e) => return Err(WriteError::Io(e)),
            // A missing file starts a new object; so does an unparseable
            // one, as ow-electron resets it: no backup copy (L3). The
            // startup read logged it once.
            Loaded::Missing | Loaded::Unparseable => Map::new(),
        };
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

    /// Stores `eHashes` as `setUserEmailHashes(value)` does in ow-electron
    /// (W4 ruling L1, observed): `Some(value)` stores the value as given
    /// (`null`, `{}`, `""`, partial or extra keys, in the app's key order),
    /// and `None` (JavaScript `undefined`) removes the key, so the file
    /// returns to the bytes it had before the hashes were set. Removing a key
    /// the file does not have writes nothing.
    ///
    /// # Errors
    ///
    /// As [`OwElectronFile::update`].
    pub fn write_e_hashes(&self, value: Option<&Value>) -> Result<(), WriteError> {
        let Some(value) = value else {
            let (status, map) = {
                let _guard = self
                    .lock
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                self.load()
            };
            if status != FileStatus::Valid || !map.contains_key("eHashes") {
                return Ok(());
            }
            return self.update(|map| {
                map.remove("eHashes");
            });
        };
        self.update(|map| {
            map.insert("eHashes".into(), value.clone());
        })
    }

    /// Removes `eHashes` (`clearUserEmailHashes()`, SEC-M9): the same as
    /// [`OwElectronFile::write_e_hashes`] with `None`.
    ///
    /// # Errors
    ///
    /// As [`OwElectronFile::update`].
    pub fn clear_e_hashes(&self) -> Result<(), WriteError> {
        self.write_e_hashes(None)
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
        file.write_e_hashes(Some(
            &serde_json::json!({"sha1":"a","md5":"b","sha256":"c"}),
        ))
        .unwrap();
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
        file.write_e_hashes(Some(&serde_json::json!({"sha1":"old"})))
            .unwrap();
        file.write_e_hashes(Some(
            &serde_json::json!({"sha1":"s1","md5":"m5","sha256":"s256"}),
        ))
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(file.path()).unwrap(),
            r#"{"firstLaunch":true,"cmp":{"cmpString":"CQ","timeStamp":1},"eHashes":{"sha1":"s1","md5":"m5","sha256":"s256"}}"#
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// L1 (observed on ow-electron 42.11.4): the value is stored as given,
    /// in the app's key order; `None` (`undefined`) removes the key and the
    /// file returns to its bytes before the set.
    #[test]
    fn e_hashes_store_the_value_as_given_and_undefined_removes_it() {
        let dir = test_dir("owe-ehash-values");
        let file = OwElectronFile::new(dir.join("ow-electron.json"));
        file.set_first_launch().unwrap();
        let before = std::fs::read(file.path()).unwrap();
        let cases: [(Value, &str); 6] = [
            (Value::Null, r#""eHashes":null"#),
            (serde_json::json!({}), r#""eHashes":{}"#),
            (Value::String(String::new()), r#""eHashes":"""#),
            (
                serde_json::json!({"sha256":"z9","extra":"x","md5":"m5"}),
                r#""eHashes":{"sha256":"z9","extra":"x","md5":"m5"}"#,
            ),
            (Value::String("abc".into()), r#""eHashes":"abc""#),
            (serde_json::json!(["x"]), r#""eHashes":["x"]"#),
        ];
        for (value, expected) in cases {
            file.write_e_hashes(Some(&value)).unwrap();
            let text = std::fs::read_to_string(file.path()).unwrap();
            assert_eq!(text, format!(r#"{{"firstLaunch":true,{expected}}}"#));
            file.write_e_hashes(None).unwrap();
            assert_eq!(std::fs::read(file.path()).unwrap(), before);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// L3: every kind of corruption reads as a first launch (defaults) and
    /// the next write starts a new object, with no backup copy; reading
    /// never changes the file.
    #[test]
    fn corrupt_files_reset_like_ow_electron_without_a_backup() {
        let kinds: [&[u8]; 9] = [
            b"this is not json\n",
            b"{\"firstLaunch\":fal",
            b"",
            b"null",
            b"[]",
            b"[1,2]",
            br#"{"firstLaunch":"no","cmp":42,"eHashes":"x","utmParams":[]}"#,
            br#"{"cmp":"weird"}"#,
            br#"{"utmParams":7}"#,
        ];
        for (i, bytes) in kinds.iter().enumerate() {
            let dir = test_dir(&format!("owe-corrupt-{i}"));
            let path = dir.join("ow-electron.json");
            std::fs::write(&path, bytes).unwrap();
            let file = OwElectronFile::new(path.clone());
            let read = file.read();
            assert_eq!(read.status, FileStatus::Invalid, "case {i}");
            assert_eq!(read.state, SharedState::default(), "case {i}");
            assert_eq!(std::fs::read(&path).unwrap(), *bytes, "reads write nothing");
            file.set_first_launch().unwrap();
            file.write_cmp(&CmpBlock {
                cmp_string: Some("CQ".into()),
                time_stamp: Some(1),
                unified_consent_string: None,
            })
            .unwrap();
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                r#"{"firstLaunch":true,"cmp":{"cmpString":"CQ","timeStamp":1}}"#,
                "case {i}"
            );
            let siblings: Vec<_> = std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            assert_eq!(siblings, ["ow-electron.json"], "no backup copy (case {i})");
            assert!(file.read().state.first_launch, "the next launch is normal");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Wrong types only in keys ow-tauri does not read keep the file valid,
    /// and an `eHashes` of any type is what ow-electron itself writes.
    #[test]
    fn other_keys_and_any_e_hashes_keep_the_file_valid() {
        let dir = test_dir("owe-valid-shapes");
        let path = dir.join("ow-electron.json");
        std::fs::write(
            &path,
            r#"{"firstLaunch":true,"eHashes":["x"],"other":42,"utmParams":null}"#,
        )
        .unwrap();
        let file = OwElectronFile::new(path);
        assert_eq!(file.read().status, FileStatus::Valid);
        assert!(file.read().state.first_launch);
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
}
