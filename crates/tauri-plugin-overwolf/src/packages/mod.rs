//! The package manager: `app.overwolf.packages` (CONTRACT A.2.4, B.1.3, H).
//!
//! This milestone provides backend selection (H.1) and the
//! [`PackagesSnapshot`] that the host snapshot carries, so the JavaScript side
//! sees every listed package from the first frame. Loading packages, the
//! commands of A.2.4, the [`PackageRuntime`] contract with its JSON-RPC
//! sidecar and C-ABI adapters are a later milestone.
//!
//! This release ships no simulated backends: without a registered native
//! runtime every listed package (gep, overlay, recorder, utility, crn)
//! reports as unavailable, as ow-electron reports a package it cannot load.
//!
//! ```
//! use tauri_plugin_overwolf::packages::{PackagesBackend, ResolvedBackend};
//! assert_eq!(PackagesBackend::Auto.resolve(false), ResolvedBackend::Failed("unsupported-host"));
//! assert_eq!(PackagesBackend::Auto.resolve(true), ResolvedBackend::Native);
//! ```

use std::collections::BTreeMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `packagesBackend` (A.1, H.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackagesBackend {
    /// Native if registered; else every listed package fails with
    /// `unsupported-host`.
    #[default]
    Auto,
    /// The registered native runtime.
    Native,
    /// Accepted for configuration compatibility. No simulated backends ship
    /// in this release, so it resolves like `auto` without a runtime.
    Simulated,
    /// Every listed package fails with `packages-disabled`.
    None,
}

/// An unknown `packagesBackend` string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown packages backend \"{0}\"; expected auto, native, simulated or none")]
pub struct UnknownBackend(pub String);

impl FromStr for PackagesBackend {
    type Err = UnknownBackend;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(PackagesBackend::Auto),
            "native" => Ok(PackagesBackend::Native),
            "simulated" => Ok(PackagesBackend::Simulated),
            "none" => Ok(PackagesBackend::None),
            other => Err(UnknownBackend(other.to_owned())),
        }
    }
}

/// The outcome of backend selection (H.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedBackend {
    /// Use the registered native runtime.
    Native,
    /// Every listed package fails with this reason.
    Failed(&'static str),
}

impl PackagesBackend {
    /// Applies the H.1 table.
    #[must_use]
    pub fn resolve(self, native_registered: bool) -> ResolvedBackend {
        match self {
            PackagesBackend::Native | PackagesBackend::Auto if native_registered => {
                ResolvedBackend::Native
            }
            PackagesBackend::Native => ResolvedBackend::Failed("no-native-runtime"),
            PackagesBackend::None => ResolvedBackend::Failed("packages-disabled"),
            PackagesBackend::Auto | PackagesBackend::Simulated => {
                ResolvedBackend::Failed("unsupported-host")
            }
        }
    }
}

impl ResolvedBackend {
    /// The `PackagesSnapshot.backend` value.
    #[must_use]
    pub fn wire_name(self) -> &'static str {
        match self {
            ResolvedBackend::Native => "native",
            ResolvedBackend::Failed(_) => "none",
        }
    }
}

/// `PackagesSnapshot` (A.2.4), part of the host snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackagesSnapshot {
    /// `native` or `none`.
    pub backend: String,
    /// The native runtime's name and version.
    pub runtime: Option<RuntimeInfo>,
    /// `packages.logsFolderPath` (F.4).
    pub logs_folder_path: String,
    /// `packages.phasePercent`.
    pub phase_percent: u8,
    /// Manifest `overwolf.packages`.
    pub listed: Vec<String>,
    /// Per-package state.
    pub packages: BTreeMap<String, PackageEntry>,
    /// `hasPendingUpdates()`.
    pub pending_updates: PendingUpdates,
    /// Persisted channel choices (`ow-tauri.json` `packageChannels`).
    pub channels: BTreeMap<String, String>,
    /// Per package: member paths the runtime provides.
    pub members: BTreeMap<String, Vec<String>>,
    /// Per-package sync caches (H.4).
    pub package_state: BTreeMap<String, Value>,
}

/// A runtime's name and version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeInfo {
    /// Runtime name.
    pub name: String,
    /// Runtime version.
    pub version: String,
}

/// One listed package in [`PackagesSnapshot`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageEntry {
    /// `pending`, `loading`, `ready` or `failed`.
    pub state: String,
    /// Loaded version.
    pub version: Option<String>,
    /// Failure details.
    pub failure: Option<Value>,
}

/// `PendingUpdatesResult`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingUpdates {
    /// Whether any package has an update waiting for a restart.
    pub has_pending_update: bool,
    /// `{ name, version }` per pending package.
    pub details: Vec<PendingDetail>,
}

/// One pending package update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingDetail {
    /// Package name.
    pub name: String,
    /// Version waiting for the restart.
    pub version: String,
}

impl PackagesSnapshot {
    /// The snapshot at setup: every listed package `pending`.
    ///
    /// ```
    /// use tauri_plugin_overwolf::packages::{PackagesSnapshot, ResolvedBackend};
    /// let s = PackagesSnapshot::initial(ResolvedBackend::Native, &["gep".into()], "/logs".into(), 7, Default::default());
    /// assert_eq!(s.packages["gep"].state, "pending");
    /// assert_eq!(s.backend, "native");
    /// ```
    #[must_use]
    pub fn initial(
        backend: ResolvedBackend,
        listed: &[String],
        logs_folder_path: String,
        phase_percent: u8,
        channels: BTreeMap<String, String>,
    ) -> Self {
        PackagesSnapshot {
            backend: backend.wire_name().to_owned(),
            runtime: None,
            logs_folder_path,
            phase_percent,
            listed: listed.to_vec(),
            packages: listed
                .iter()
                .map(|name| {
                    (
                        name.clone(),
                        PackageEntry {
                            state: "pending".into(),
                            version: None,
                            failure: None,
                        },
                    )
                })
                .collect(),
            pending_updates: PendingUpdates::default(),
            channels,
            members: BTreeMap::new(),
            package_state: BTreeMap::new(),
        }
    }
}

/// A component that runs Overwolf packages for the host (CONTRACT H.2).
///
/// Only the identity method exists in this milestone; the full contract
/// (initialize, load, call, handles, events, channels) is implemented in a
/// later milestone, so registering a runtime today only makes `auto` and
/// `native` select it.
pub trait PackageRuntime: Send + Sync + 'static {
    /// Name and version reported in `PackagesSnapshot.runtime`.
    fn info(&self) -> RuntimeInfo;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_table() {
        use PackagesBackend::{Auto, Native, None, Simulated};
        use ResolvedBackend as R;
        let cases = [
            (Native, true, R::Native),
            (Native, false, R::Failed("no-native-runtime")),
            (Simulated, false, R::Failed("unsupported-host")),
            (Simulated, true, R::Failed("unsupported-host")),
            (None, true, R::Failed("packages-disabled")),
            (Auto, true, R::Native),
            (Auto, false, R::Failed("unsupported-host")),
        ];
        for (b, native, want) in cases {
            assert_eq!(b.resolve(native), want, "{b:?} {native}");
        }
    }

    #[test]
    fn parse_and_serde() {
        for (s, b) in [
            ("auto", PackagesBackend::Auto),
            ("native", PackagesBackend::Native),
            ("simulated", PackagesBackend::Simulated),
            ("none", PackagesBackend::None),
        ] {
            assert_eq!(s.parse::<PackagesBackend>().unwrap(), b);
            assert_eq!(serde_json::to_value(b).unwrap(), s);
        }
        assert!("Auto".parse::<PackagesBackend>().is_err());
    }

    #[test]
    fn snapshot_shape() {
        let s = PackagesSnapshot::initial(
            ResolvedBackend::Failed("unsupported-host"),
            &["gep".into(), "overlay".into()],
            "/l".into(),
            3,
            BTreeMap::new(),
        );
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["backend"], "none");
        assert_eq!(
            v["pendingUpdates"],
            serde_json::json!({"hasPendingUpdate": false, "details": []})
        );
        assert_eq!(v["listed"], serde_json::json!(["gep", "overlay"]));
        for key in [
            "runtime",
            "logsFolderPath",
            "phasePercent",
            "packages",
            "channels",
            "members",
            "packageState",
        ] {
            assert!(v.get(key).is_some(), "{key}");
        }
    }
}
