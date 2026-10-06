//! `HostSnapshot` and the state sequence (CONTRACT A.2.1, B.1.6, C.6).
//!
//! The snapshot is injected into `ow-main` as
//! `window.__OW_TAURI_BOOTSTRAP__` and returned by `bootstrap`. Changes are
//! pushed as `state` host messages: dot-path patches with a sequence number
//! of their own.
//!
//! ```
//! use serde_json::json;
//! use tauri_plugin_overwolf::snapshot::StateHub;
//! let mut hub = StateHub::new(json!({ "seq": 0, "flags": { "adsFpdDisabled": false } }));
//! let msg = hub.patch(vec![("flags.adsFpdDisabled".into(), json!(true))]);
//! assert_eq!(hub.seq(), 1);
//! assert_eq!(hub.snapshot()["flags"]["adsFpdDisabled"], true);
//! assert_eq!(hub.snapshot()["seq"], 1);
//! assert!(matches!(msg, tauri_plugin_overwolf::ipc::messages::HostMessage::State { seq: 1, .. }));
//! ```

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::ipc::messages::{HostMessage, Patch};
use crate::manifest::EmbeddedManifest;
use crate::packages::PackagesSnapshot;
use crate::screen::{ElectronDisplay, Point};

/// `HostSnapshot.versions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Versions {
    /// ow-tauri version.
    pub ow_tauri: String,
    /// Tauri version.
    pub tauri: String,
    /// App version (manifest).
    pub app: String,
    /// Webview engine version.
    pub webview: String,
    /// OS release.
    pub os: String,
}

/// `HostSnapshot.identity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityInfo {
    /// The effective uid (G.2).
    pub uid: String,
    /// The computed uid.
    pub cuid: String,
    /// The muid (E.4).
    pub muid: String,
    /// Equals `muid` (OQ-02).
    pub muid_v2: String,
    /// Phase percent.
    pub phase_percent: u8,
}

/// `HostSnapshot.switches`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchesInfo {
    /// The process arguments.
    pub argv: Vec<String>,
    /// Test ad inventory.
    pub test_ad: bool,
}

/// `HostSnapshot.flags`: per-session switches (A.2.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Flags {
    /// `disableAnonymousAnalytics()` was called.
    pub anonymous_analytics_disabled: bool,
    /// `disableAdsOptimization()` was called.
    pub ads_optimization_disabled: bool,
    /// `disableAdsFPD()` was called.
    pub ads_fpd_disabled: bool,
}

/// `HostSnapshot.ipcLimits`: the limits the main runtime checks itself
/// (C.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcLimits {
    /// `ipc.maxMessageBytes` (A.1): the encoded size cap per message.
    pub max_message_bytes: usize,
}

/// The bootstrap snapshot.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSnapshot {
    /// Last applied state sequence number.
    pub seq: u64,
    /// Version strings.
    pub versions: Versions,
    /// The embedded manifest.
    pub manifest: EmbeddedManifest,
    /// App identity.
    pub identity: IdentityInfo,
    /// `ow-electron.json` `utmParams`; absent (`undefined` in JS) when there
    /// are none (F.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utm_params: Option<Value>,
    /// Process arguments and the ad mode.
    pub switches: SwitchesInfo,
    /// `app.getPath()` values plus `appPath` (addition).
    pub paths: BTreeMap<String, String>,
    /// Release build.
    pub is_packaged: bool,
    /// App locale (BCP 47).
    pub locale: String,
    /// Displays.
    pub displays: Vec<ElectronDisplay>,
    /// Primary display id.
    pub primary_display_id: u32,
    /// Package manager state.
    pub packages: PackagesSnapshot,
    /// Session switches.
    pub flags: Flags,
    /// Node `process.platform` (addition, used by the `process` shim).
    pub platform: String,
    /// Node `process.arch` (addition).
    pub arch: String,
    /// `app.overwolf.__settings__.firstLaunch`: `true` when `ow-electron.json`
    /// had no `firstLaunch` at setup, read before this launch writes it
    /// (F.2, B.1.1).
    pub first_launch: bool,
    /// The cursor in DIP when the snapshot was taken; absent when the host
    /// cannot query it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Point>,
    /// Limits the main runtime checks itself (`ipc.maxMessageBytes`).
    pub ipc_limits: IpcLimits,
}

/// What a UI window's bootstrap receives: the subset the `process` shim needs.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RendererBootstrap {
    /// Version strings.
    pub versions: Versions,
    /// Process arguments and the ad mode.
    pub switches: SwitchesInfo,
    /// Node `process.platform`.
    pub platform: String,
    /// Node `process.arch`.
    pub arch: String,
}

impl HostSnapshot {
    /// The renderer subset.
    #[must_use]
    pub fn renderer(&self) -> RendererBootstrap {
        RendererBootstrap {
            versions: self.versions.clone(),
            switches: self.switches.clone(),
            platform: self.platform.clone(),
            arch: self.arch.clone(),
        }
    }
}

/// Sets `value` at the dot `path` inside `root`, creating objects on the
/// way (a non-object in the way is replaced).
pub fn set_path(root: &mut Value, path: &str, value: Value) {
    let mut cur = root;
    let mut parts = path.split('.').peekable();
    while let Some(part) = parts.next() {
        if !cur.is_object() {
            *cur = Value::Object(Map::new());
        }
        let Value::Object(map) = cur else { return };
        if parts.peek().is_none() {
            map.insert(part.to_owned(), value);
            return;
        }
        cur = map
            .entry(part.to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
    }
}

/// Reads the dot `path` inside `root`.
#[must_use]
pub fn get_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(root, |cur, part| cur.get(part))
}

/// The snapshot as JSON plus its state sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct StateHub {
    value: Value,
    seq: u64,
}

impl StateHub {
    /// Wraps a serialised snapshot; its `seq` field is taken as the start.
    #[must_use]
    pub fn new(snapshot: Value) -> Self {
        let seq = snapshot.get("seq").and_then(Value::as_u64).unwrap_or(0);
        StateHub {
            value: snapshot,
            seq,
        }
    }

    /// The current sequence number.
    #[must_use]
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// The current snapshot, `seq` included.
    #[must_use]
    pub fn snapshot(&self) -> &Value {
        &self.value
    }

    /// Reads a value by dot path.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&Value> {
        get_path(&self.value, path)
    }

    /// Applies `patches` as one state step and returns the `state` message
    /// for `ow-main`.
    pub fn patch(&mut self, patches: Vec<(String, Value)>) -> HostMessage {
        self.seq += 1;
        let mut wire = Vec::with_capacity(patches.len());
        for (path, value) in patches {
            set_path(&mut self.value, &path, value.clone());
            wire.push(Patch { path, value });
        }
        set_path(&mut self.value, "seq", Value::from(self.seq));
        HostMessage::State {
            seq: self.seq,
            patches: wire,
        }
    }

    /// Like [`StateHub::patch`], but only when some value differs from the
    /// current one.
    pub fn patch_if_changed(&mut self, patches: Vec<(String, Value)>) -> Option<HostMessage> {
        let changed: Vec<(String, Value)> = patches
            .into_iter()
            .filter(|(path, value)| self.get(path) != Some(value))
            .collect();
        (!changed.is_empty()).then(|| self.patch(changed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dot_paths() {
        let mut v = json!({"a": {"b": 1}, "x": 3});
        set_path(&mut v, "a.c.d", json!(2));
        set_path(&mut v, "x.y", json!(4));
        assert_eq!(v, json!({"a": {"b": 1, "c": {"d": 2}}, "x": {"y": 4}}));
        assert_eq!(get_path(&v, "a.c.d"), Some(&json!(2)));
        assert_eq!(get_path(&v, "a.z"), None);
    }

    #[test]
    fn sequence_and_change_detection() {
        let mut hub = StateHub::new(json!({"seq": 5, "displays": []}));
        assert!(
            hub.patch_if_changed(vec![("displays".into(), json!([]))])
                .is_none()
        );
        let msg = hub
            .patch_if_changed(vec![("displays".into(), json!([1]))])
            .unwrap();
        assert_eq!(
            serde_json::to_value(msg).unwrap(),
            json!({"type":"state","seq":6,"patches":[{"path":"displays","value":[1]}]})
        );
        assert_eq!(hub.snapshot()["seq"], 6);
    }
}
