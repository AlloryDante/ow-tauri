//! The package manager: `app.overwolf.packages` (CONTRACT A.2.4, B.1.3, H).
//!
//! No package runtime exists (CONTRACT H), so ow-tauri behaves on every OS
//! exactly as ow-electron behaves where packages are not available: no
//! package events, no package objects, `getChannel` resolves `{}`,
//! `setChannel` and `getAvailableChannels` reject with ow-electron's error
//! text, and `relaunch` does nothing. The interface a future runtime would
//! implement is the deferred design of CONTRACT Appendix P.1.
//!
//! ```
//! use tauri_plugin_overwolf::packages::{PackagesBackend, PackagesSnapshot};
//! let s = PackagesSnapshot::new(PackagesBackend::None, &["gep".into()], "/logs".into(), 7);
//! assert_eq!(serde_json::to_value(&s).unwrap()["backend"], "none");
//! assert_eq!(
//!     tauri_plugin_overwolf::packages::set_channel_error("gep").to_string(),
//!     "setChannel - package 'gep' is not registered in this app"
//! );
//! ```

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::error::Error;

/// `packagesBackend` (A.1, H.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackagesBackend {
    /// No package runtime: the H.1 behaviour (default).
    #[default]
    None,
    /// Reserved for a package runtime that implements Appendix P. No such
    /// runtime exists, so it behaves as `none` and logs one warning.
    Native,
}

/// An unknown `packagesBackend` string.
///
/// ```
/// use tauri_plugin_overwolf::packages::PackagesBackend;
/// let err = "simulated".parse::<PackagesBackend>().unwrap_err();
/// assert_eq!(err.to_string(), "unknown packages backend \"simulated\"; expected none or native");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown packages backend \"{0}\"; expected none or native")]
pub struct UnknownBackend(pub String);

impl FromStr for PackagesBackend {
    type Err = UnknownBackend;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(PackagesBackend::None),
            "native" => Ok(PackagesBackend::Native),
            other => Err(UnknownBackend(other.to_owned())),
        }
    }
}

impl PackagesBackend {
    /// The `PackagesSnapshot.backend` value.
    ///
    /// ```
    /// use tauri_plugin_overwolf::packages::PackagesBackend;
    /// assert_eq!(PackagesBackend::Native.wire_name(), "native");
    /// ```
    #[must_use]
    pub fn wire_name(self) -> &'static str {
        match self {
            PackagesBackend::None => "none",
            PackagesBackend::Native => "native",
        }
    }
}

/// `PackagesSnapshot` (A.2.4), part of the host snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackagesSnapshot {
    /// `none` or `native` (reserved).
    pub backend: String,
    /// `packages.logsFolderPath`: the literal ow-electron string (F.4).
    pub logs_folder_path: String,
    /// `packages.phasePercent` (E.4).
    pub phase_percent: u8,
    /// Manifest `overwolf.packages`.
    pub listed: Vec<String>,
    /// `hasPendingUpdates()`: always `{ hasPendingUpdate: false, details: [] }`.
    pub pending_updates: PendingUpdates,
}

/// `PendingUpdatesResult`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingUpdates {
    /// Whether any package has an update waiting for a restart.
    pub has_pending_update: bool,
    /// `{ name, version }` per pending package; always empty.
    pub details: Vec<Value>,
}

impl PackagesSnapshot {
    /// The snapshot for this session.
    ///
    /// ```
    /// use tauri_plugin_overwolf::packages::{PackagesBackend, PackagesSnapshot};
    /// let s = PackagesSnapshot::new(PackagesBackend::None, &["gep".into()], "/l".into(), 3);
    /// assert!(!s.pending_updates.has_pending_update);
    /// assert_eq!(s.listed, ["gep"]);
    /// ```
    #[must_use]
    pub fn new(
        backend: PackagesBackend,
        listed: &[String],
        logs_folder_path: String,
        phase_percent: u8,
    ) -> Self {
        PackagesSnapshot {
            backend: backend.wire_name().to_owned(),
            logs_folder_path,
            phase_percent,
            listed: listed.to_vec(),
            pending_updates: PendingUpdates::default(),
        }
    }
}

/// `packages.logsFolderPath` as ow-electron builds it (F.4): the userData
/// path, the literal `/..\ow-electron/`, the uid and `/logs`, with mixed
/// separators on every OS (matches ow-electron, observed).
///
/// ```
/// use tauri_plugin_overwolf::packages::logs_folder_path;
/// assert_eq!(
///     logs_folder_path("/Users/a/Library/Application Support/My App", "abc"),
///     "/Users/a/Library/Application Support/My App/..\\ow-electron/abc/logs"
/// );
/// ```
#[must_use]
pub fn logs_folder_path(user_data: &str, uid: &str) -> String {
    format!("{user_data}/..\\ow-electron/{uid}/logs")
}

fn not_registered(method: &str, name: &str) -> Error {
    let message = format!("{method} - package '{name}' is not registered in this app");
    Error::NotFound {
        message: message.clone(),
        data: Some(json!({ "message": message })),
    }
}

/// The `packages_set_channel` error (H.1): `not-found` whose message and
/// `data.message` are ow-electron's text.
///
/// ```
/// let e = tauri_plugin_overwolf::packages::set_channel_error("overlay");
/// assert_eq!(serde_json::to_value(&e).unwrap()["data"]["message"],
///     "setChannel - package 'overlay' is not registered in this app");
/// ```
#[must_use]
pub fn set_channel_error(name: &str) -> Error {
    not_registered("setChannel", name)
}

/// The result of `packages_get_available_channels` (H.1): `not-found` with
/// the first name, or `{}` when no name is given (interim, R3-9).
///
/// # Errors
///
/// Always, when `names` is not empty.
///
/// ```
/// use tauri_plugin_overwolf::packages::get_available_channels;
/// assert_eq!(get_available_channels(&[]).unwrap(), serde_json::json!({}));
/// let e = get_available_channels(&["gep".into(), "overlay".into()]).unwrap_err();
/// assert_eq!(e.to_string(), "getAvailableChannels - package 'gep' is not registered in this app");
/// ```
pub fn get_available_channels(names: &[String]) -> Result<Value, Error> {
    match names.first() {
        Some(first) => Err(not_registered("getAvailableChannels", first)),
        None => Ok(Value::Object(Map::new())),
    }
}

/// The result of `packages_get_channel` (H.1): `{}` for any arguments.
///
/// ```
/// assert_eq!(tauri_plugin_overwolf::packages::get_channel(&["gep".into()]), serde_json::json!({}));
/// ```
#[must_use]
pub fn get_channel(_names: &[String]) -> Value {
    Value::Object(Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_serde() {
        for (s, b) in [
            ("none", PackagesBackend::None),
            ("native", PackagesBackend::Native),
        ] {
            assert_eq!(s.parse::<PackagesBackend>().unwrap(), b);
            assert_eq!(serde_json::to_value(b).unwrap(), s);
        }
        for removed in ["auto", "simulated", "None", ""] {
            assert!(removed.parse::<PackagesBackend>().is_err(), "{removed}");
            assert!(serde_json::from_value::<PackagesBackend>(json!(removed)).is_err());
        }
        assert_eq!(PackagesBackend::default(), PackagesBackend::None);
    }

    #[test]
    fn snapshot_shape() {
        let s = PackagesSnapshot::new(
            PackagesBackend::None,
            &["gep".into(), "overlay".into()],
            "/l".into(),
            3,
        );
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            json!({
                "backend": "none",
                "logsFolderPath": "/l",
                "phasePercent": 3,
                "listed": ["gep", "overlay"],
                "pendingUpdates": {"hasPendingUpdate": false, "details": []}
            })
        );
    }

    #[test]
    fn errors_match_ow_electron() {
        let e = serde_json::to_value(set_channel_error("gep")).unwrap();
        assert_eq!(
            e,
            json!({
                "code": "not-found",
                "message": "setChannel - package 'gep' is not registered in this app",
                "data": {"message": "setChannel - package 'gep' is not registered in this app"}
            })
        );
        let e = get_available_channels(&["recorder".into()]).unwrap_err();
        assert_eq!(
            serde_json::to_value(e).unwrap()["data"]["message"],
            "getAvailableChannels - package 'recorder' is not registered in this app"
        );
        assert_eq!(get_channel(&[]), json!({}));
    }
}
