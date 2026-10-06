//! `ow-tauri.json`, owned by ow-tauri (CONTRACT F.3).
//!
//! ```
//! use tauri_plugin_overwolf::state::ow_tauri::OwTauriState;
//! let state: OwTauriState = serde_json::from_str(r#"{ "schema": 1, "packageChannels": { "gep": "beta" } }"#).unwrap();
//! assert_eq!(state.package_channels["gep"], "beta");
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::write_atomic;

/// The schema version this build writes.
pub const SCHEMA: u32 = 1;

/// The contents of `ow-tauri.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwTauriState {
    /// Schema version; a newer value is kept on write (never downgraded).
    #[serde(default = "default_schema")]
    pub schema: u32,
    /// The `per-install` muid (E.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muid: Option<String>,
    /// Updater staging bucket seed (I.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staging_id: Option<String>,
    /// `setChannel` persistence.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub package_channels: BTreeMap<String, String>,
    /// Consent page ad-optimisation toggle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ad_optimization: Option<bool>,
    /// App-level analytics switch (`analytics.userSwitch` only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analytics_user_enabled: Option<bool>,
    /// Browser switches recorded by app code, applied from the next launch (A.1.1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_browser_args: Vec<String>,
    /// Shared `ow-electron.json` values kept here when that file is not valid
    /// JSON (F.2); never read back into `ow-electron.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ow_electron_fallback: Option<Value>,
    /// `ow-tauri <version>` of the build that created the file.
    #[serde(default)]
    pub created_by: String,
    /// Keys from newer schemas, preserved.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn default_schema() -> u32 {
    SCHEMA
}

impl Default for OwTauriState {
    fn default() -> Self {
        OwTauriState {
            schema: SCHEMA,
            muid: None,
            staging_id: None,
            package_channels: BTreeMap::new(),
            ad_optimization: None,
            analytics_user_enabled: None,
            pending_browser_args: Vec::new(),
            ow_electron_fallback: None,
            created_by: format!("ow-tauri {}", env!("CARGO_PKG_VERSION")),
            extra: Map::new(),
        }
    }
}

/// Access to one `ow-tauri.json`, cached in memory.
#[derive(Debug)]
pub struct OwTauriFile {
    path: PathBuf,
    state: Mutex<OwTauriState>,
    /// Whether the file on disk could not be parsed at load.
    pub corrupt_at_load: bool,
}

impl OwTauriFile {
    /// Loads `path`. A missing file starts from defaults; an unreadable or
    /// invalid one also starts from defaults and is replaced on the next
    /// write ([`OwTauriFile::corrupt_at_load`] is then `true`).
    #[must_use]
    pub fn load(path: PathBuf) -> Self {
        let (state, corrupt) = match std::fs::read(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (OwTauriState::default(), false),
            Err(_) => (OwTauriState::default(), true),
            Ok(bytes) => match serde_json::from_slice::<OwTauriState>(&bytes) {
                Ok(s) => (s, false),
                Err(_) => (OwTauriState::default(), true),
            },
        };
        OwTauriFile {
            path,
            state: Mutex::new(state),
            corrupt_at_load: corrupt,
        }
    }

    /// The file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A copy of the current state.
    #[must_use]
    pub fn get(&self) -> OwTauriState {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Applies `edit` and writes the file atomically. The schema is never
    /// lowered.
    ///
    /// # Errors
    ///
    /// The I/O error of the write; the in-memory state keeps the edit.
    pub fn update(&self, edit: impl FnOnce(&mut OwTauriState)) -> std::io::Result<()> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        edit(&mut guard);
        guard.schema = guard.schema.max(SCHEMA);
        if guard.created_by.is_empty() {
            guard.created_by = format!("ow-tauri {}", env!("CARGO_PKG_VERSION"));
        }
        let mut bytes = serde_json::to_vec_pretty(&*guard).map_err(std::io::Error::other)?;
        bytes.push(b'\n');
        write_atomic(&self.path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_dir;

    #[test]
    fn round_trip_and_newer_schema() {
        let dir = test_dir("owt");
        let path = dir.join("ow-tauri.json");
        std::fs::write(
            &path,
            r#"{"schema":3,"muid":"M","futureKey":{"a":1},"createdBy":"ow-tauri 9.0.0"}"#,
        )
        .unwrap();
        let file = OwTauriFile::load(path.clone());
        assert!(!file.corrupt_at_load);
        assert_eq!(file.get().schema, 3);
        file.update(|s| {
            s.package_channels.insert("gep".into(), "beta".into());
        })
        .unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["schema"], 3, "never downgraded");
        assert_eq!(v["futureKey"], serde_json::json!({"a":1}));
        assert_eq!(v["packageChannels"]["gep"], "beta");
        assert_eq!(v["createdBy"], "ow-tauri 9.0.0");
        assert_eq!(v["muid"], "M");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_and_missing() {
        let dir = test_dir("owt-corrupt");
        let path = dir.join("ow-tauri.json");
        let missing = OwTauriFile::load(path.clone());
        assert!(!missing.corrupt_at_load);
        assert_eq!(missing.get().schema, SCHEMA);
        std::fs::write(&path, b"garbage").unwrap();
        let corrupt = OwTauriFile::load(path.clone());
        assert!(corrupt.corrupt_at_load);
        corrupt.update(|s| s.muid = Some("X".into())).unwrap();
        let reloaded = OwTauriFile::load(path);
        assert_eq!(reloaded.get().muid.as_deref(), Some("X"));
        assert!(reloaded.get().created_by.starts_with("ow-tauri "));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
