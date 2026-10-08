//! `ow-tauri.json`, owned by ow-tauri (CONTRACT F.3).
//!
//! Loading never writes: a file that is not valid JSON is moved aside by
//! [`OwTauriFile::repair`] at `RunEvent::Ready` (DESIGN §4.2, §4.12).
//!
//! ```
//! use tauri_plugin_overwolf::state::ow_tauri::OwTauriState;
//! let state: OwTauriState = serde_json::from_str(r#"{ "schema": 1, "anonymousAnalytics": false }"#).unwrap();
//! assert_eq!(state.anonymous_analytics, Some(false));
//! ```

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
    /// Updater staging bucket seed (I.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staging_id: Option<String>,
    /// Consent page ad-optimisation toggle (D.6.6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ad_optimization: Option<bool>,
    /// The persisted anonymous-analytics preference, applied at the next
    /// `RunEvent::Ready` (`set_anonymous_analytics_preference`, (R10)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anonymous_analytics: Option<bool>,
    /// App-level analytics switch (`analytics.userSwitch` only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analytics_user_enabled: Option<bool>,
    /// The `per-install` muid (E.4, ow-tauri option).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muid: Option<String>,
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
            staging_id: None,
            ad_optimization: None,
            anonymous_analytics: None,
            analytics_user_enabled: None,
            muid: None,
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

/// Parses `ow-tauri.json` field by field: a known field with an unexpected
/// type (for example from a newer schema) is kept unchanged in
/// [`OwTauriState::extra`] instead of failing the whole file, so values such
/// as the per-install muid survive. `None` when the text is not a JSON
/// object.
///
/// ```
/// use tauri_plugin_overwolf::state::ow_tauri::parse_lenient;
/// let s = parse_lenient(br#"{"muid":"M","anonymousAnalytics":7}"#).unwrap();
/// assert_eq!(s.muid.as_deref(), Some("M"));
/// assert!(s.anonymous_analytics.is_none());
/// assert_eq!(s.extra["anonymousAnalytics"], 7);
/// assert!(parse_lenient(b"garbage").is_none());
/// ```
#[must_use]
pub fn parse_lenient(bytes: &[u8]) -> Option<OwTauriState> {
    let Value::Object(object) = serde_json::from_slice::<Value>(bytes).ok()? else {
        return None;
    };
    if let Ok(state) = serde_json::from_value::<OwTauriState>(Value::Object(object.clone())) {
        return Some(state);
    }
    let empty = || serde_json::from_value::<OwTauriState>(Value::Object(Map::new()));
    let mut state = empty().ok()?;
    let mut kept = Map::new();
    let mut mistyped = Map::new();
    for (key, value) in object {
        let mut one = Map::new();
        one.insert(key.clone(), value.clone());
        if serde_json::from_value::<OwTauriState>(Value::Object(one)).is_ok() {
            kept.insert(key, value);
        } else {
            mistyped.insert(key, value);
        }
    }
    if let Ok(parsed) = serde_json::from_value::<OwTauriState>(Value::Object(kept)) {
        state = parsed;
    }
    state.extra.extend(mistyped);
    Some(state)
}

/// The names the known fields of `state` serialise to.
fn known_keys(state: &OwTauriState) -> Vec<String> {
    let bare = OwTauriState {
        extra: Map::new(),
        ..state.clone()
    };
    match serde_json::to_value(bare) {
        Ok(Value::Object(m)) => m.into_iter().map(|(k, _)| k).collect(),
        _ => Vec::new(),
    }
}

/// Renames an unparseable state file to `<name>.corrupt-<ms since 1970>`
/// so nothing is lost; returns the new path when the rename worked.
fn move_aside(path: &Path) -> Option<PathBuf> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let mut name = path.file_name()?.to_os_string();
    name.push(format!(".corrupt-{millis}"));
    let target = path.with_file_name(name);
    std::fs::rename(path, &target).ok().map(|()| target)
}

impl OwTauriFile {
    /// Loads `path` without writing anything. A missing file starts from
    /// defaults. A file whose known fields have unexpected types keeps them
    /// in `extra` ([`parse_lenient`]). A file that is not a JSON object
    /// starts from defaults ([`OwTauriFile::corrupt_at_load`] is then
    /// `true`) and stays on disk until [`OwTauriFile::repair`].
    #[must_use]
    pub fn load(path: PathBuf) -> Self {
        let (state, corrupt) = match std::fs::read(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (OwTauriState::default(), false),
            Err(_) => (OwTauriState::default(), true),
            Ok(bytes) => match parse_lenient(&bytes) {
                Some(s) => (s, false),
                None => (OwTauriState::default(), true),
            },
        };
        OwTauriFile {
            path,
            state: Mutex::new(state),
            corrupt_at_load: corrupt,
        }
    }

    /// Moves a file that was not valid JSON at load to
    /// `ow-tauri.json.corrupt-<ms since 1970>`, so nothing is lost and the
    /// next write starts clean. Called once at `RunEvent::Ready`, never
    /// before (DESIGN §4.2). Returns the new path when a file was moved.
    pub fn repair(&self) -> Option<PathBuf> {
        if !self.corrupt_at_load || !self.path.exists() {
            return None;
        }
        // Only a file that still fails to parse is moved: another instance
        // may have rewritten it since this one loaded it.
        let bytes = std::fs::read(&self.path).ok()?;
        if parse_lenient(&bytes).is_some() {
            return None;
        }
        move_aside(&self.path)
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
        // A known field that was kept in `extra` because it had an
        // unexpected type yields to the value this build sets.
        let known = known_keys(&guard);
        guard.extra.retain(|k, _| !known.contains(k));
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
    fn mistyped_known_fields_are_kept_and_yield_to_new_values() {
        let dir = test_dir("owt-lenient");
        let path = dir.join("ow-tauri.json");
        std::fs::write(
            &path,
            r#"{"schema":2,"muid":"M","stagingId":{"v":2},"futureKey":true}"#,
        )
        .unwrap();
        let file = OwTauriFile::load(path.clone());
        assert!(!file.corrupt_at_load);
        assert_eq!(file.get().muid.as_deref(), Some("M"), "the muid survives");
        file.update(|s| s.ad_optimization = Some(true)).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["stagingId"], serde_json::json!({"v":2}), "kept as found");
        assert_eq!(v["futureKey"], true);
        file.update(|s| s.staging_id = Some("S".into())).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("stagingId").count(), 1, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["stagingId"], "S");
        let _ = std::fs::remove_dir_all(&dir);
    }

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
        file.update(|s| s.ad_optimization = Some(true)).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["schema"], 3, "never downgraded");
        assert_eq!(v["futureKey"], serde_json::json!({"a":1}));
        assert_eq!(v["adOptimization"], true);
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
        assert!(path.exists(), "loading never moves the file");
        let backup = corrupt.repair().unwrap();
        assert_eq!(
            std::fs::read(&backup).unwrap(),
            b"garbage",
            "the old file is kept"
        );
        assert!(!path.exists());
        assert!(corrupt.repair().is_none(), "moved once");
        corrupt.update(|s| s.muid = Some("X".into())).unwrap();
        let reloaded = OwTauriFile::load(path);
        assert_eq!(reloaded.get().muid.as_deref(), Some("X"));
        assert!(reloaded.get().created_by.starts_with("ow-tauri "));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
