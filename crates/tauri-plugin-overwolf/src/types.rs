//! Values the plugin's API takes and returns (DESIGN §3.3, §3.4). Their JSON
//! shapes are the JavaScript API's (`tauri-plugin-overwolf-api`).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// What `getInfo()` returns: the app's Overwolf identity without the
/// machine identifiers (those need `overwolf:machine-id`, see
/// [`MachineIds`]).
///
/// ```
/// use tauri_plugin_overwolf::{HostInfo, Info};
/// let info = Info::new("uid", "cuid", 42, None, false, true, "App", "1.0.0",
///     HostInfo::new("tauri", "2.12.1", "tauri-2.12.1"));
/// let json = serde_json::to_value(&info).unwrap();
/// assert_eq!(json["appCuid"], "cuid");
/// assert_eq!(json["host"]["owVersion"], "tauri-2.12.1");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Info {
    /// The app uid.
    pub uid: String,
    /// The computed uid (`app_cuid` of the analytics).
    pub app_cuid: String,
    /// The phase bucket of this machine, 0 to 99.
    pub phase_percent: u8,
    /// The UTM parameters stored at install, if any.
    pub utm_params: Option<Value>,
    /// Test ads are on.
    pub test_ad: bool,
    /// Ads can be shown on this platform and build.
    pub ads_supported: bool,
    /// `<PN>`, the ow-electron app name.
    pub name: String,
    /// The app version.
    pub version: String,
    /// The host the analytics and the guests report.
    pub host: HostInfo,
}

impl Info {
    /// An [`Info`] from its fields.
    #[expect(
        clippy::too_many_arguments,
        reason = "one argument per field of a non-exhaustive struct"
    )]
    #[must_use]
    pub fn new(
        uid: impl Into<String>,
        app_cuid: impl Into<String>,
        phase_percent: u8,
        utm_params: Option<Value>,
        test_ad: bool,
        ads_supported: bool,
        name: impl Into<String>,
        version: impl Into<String>,
        host: HostInfo,
    ) -> Self {
        Info {
            uid: uid.into(),
            app_cuid: app_cuid.into(),
            phase_percent,
            utm_params,
            test_ad,
            ads_supported,
            name: name.into(),
            version: version.into(),
            host,
        }
    }
}

/// The host label, version and Overwolf runtime version the analytics and
/// the guests report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HostInfo {
    /// `analytics.hostLabel` (`tauri`).
    pub label: String,
    /// The host version (the Tauri version unless configured).
    pub version: String,
    /// The Overwolf runtime version the guests report.
    pub ow_version: String,
}

impl HostInfo {
    /// A [`HostInfo`] from its fields.
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        version: impl Into<String>,
        ow_version: impl Into<String>,
    ) -> Self {
        HostInfo {
            label: label.into(),
            version: version.into(),
            ow_version: ow_version.into(),
        }
    }
}

/// What `getMachineIds()` returns (permission `overwolf:machine-id`).
///
/// ```
/// use tauri_plugin_overwolf::MachineIds;
/// let ids = MachineIds::new("v1", "v2");
/// assert_eq!(serde_json::to_value(&ids).unwrap(), serde_json::json!({ "muid": "v2", "muidV2": "v2" }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MachineIds {
    /// ow-electron's `app.overwolf.muid`: `muidV2` when present, else the
    /// first-generation muid.
    pub muid: String,
    /// The second-generation muid.
    pub muid_v2: String,
}

impl MachineIds {
    /// The ids as JavaScript reports them, from the first- and
    /// second-generation muids: `muid` is `muid_v2` when it is not empty
    /// (ow-electron's `app.overwolf.muid`).
    #[must_use]
    pub fn new(muid_v1: impl Into<String>, muid_v2: impl Into<String>) -> Self {
        let v1 = muid_v1.into();
        let v2 = muid_v2.into();
        MachineIds {
            muid: if v2.is_empty() { v1 } else { v2.clone() },
            muid_v2: v2,
        }
    }
}

/// The tab the ad privacy settings window opens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum CmpTab {
    /// Purposes.
    Purposes,
    /// Features.
    Features,
    /// Vendors.
    Vendors,
}

impl CmpTab {
    /// The wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            CmpTab::Purposes => "purposes",
            CmpTab::Features => "features",
            CmpTab::Vendors => "vendors",
        }
    }
}

/// `CMPWindowOptions`: how the ad privacy settings window opens. Every field
/// is optional.
///
/// ```
/// use tauri_plugin_overwolf::{CmpTab, CmpWindowOptions};
/// let o: CmpWindowOptions = serde_json::from_value(serde_json::json!({
///     "tab": "vendors", "modal": true, "parent": "main", "cmpURL": "https://content.overwolf.com/x.html"
/// })).unwrap();
/// assert_eq!(o.tab, Some(CmpTab::Vendors));
/// assert_eq!(o.parent.as_deref(), Some("main"));
/// assert!(o.cmp_url.is_some());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[non_exhaustive]
pub struct CmpWindowOptions {
    /// The tab to open.
    pub tab: Option<CmpTab>,
    /// Owned by the parent window and kept above it.
    pub modal: Option<bool>,
    /// The parent window's Tauri label; with `modal` and no parent, the
    /// caller's window.
    pub parent: Option<String>,
    /// Centre the window.
    pub center: Option<bool>,
    /// Window background colour.
    pub background_color: Option<String>,
    /// Spinner colour of the preloader.
    pub pre_loader_spinner_color: Option<String>,
    /// Width (default 800).
    pub width: Option<f64>,
    /// Height (default 800).
    pub height: Option<f64>,
    /// Left edge.
    pub x: Option<f64>,
    /// Top edge.
    pub y: Option<f64>,
    /// Consent page URL override. From JavaScript it must match
    /// `consent.allowedCmpOrigins`.
    #[serde(rename = "cmpURL")]
    pub cmp_url: Option<String>,
    /// Page language.
    pub language: Option<String>,
}

impl CmpWindowOptions {
    /// Default options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets [`CmpWindowOptions::tab`].
    #[must_use]
    pub fn tab(mut self, tab: CmpTab) -> Self {
        self.tab = Some(tab);
        self
    }

    /// Sets [`CmpWindowOptions::modal`] and [`CmpWindowOptions::parent`].
    #[must_use]
    pub fn modal_to(mut self, parent: impl Into<String>) -> Self {
        self.modal = Some(true);
        self.parent = Some(parent.into());
        self
    }

    /// Sets [`CmpWindowOptions::cmp_url`].
    #[must_use]
    pub fn cmp_url(mut self, url: impl Into<String>) -> Self {
        self.cmp_url = Some(url.into());
        self
    }
}

/// `ExternalPaymentUserIdOptions` for
/// `Overwolf::set_external_payment_user_id`.
///
/// ```
/// use tauri_plugin_overwolf::PaymentUserIdOptions;
/// let map = PaymentUserIdOptions::new("user-1").provider("tebex").into_map();
/// assert_eq!(serde_json::Value::Object(map), serde_json::json!({ "providerName": "tebex", "userId": "user-1" }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PaymentUserIdOptions {
    /// The payment provider (`tebex` when absent).
    pub provider_name: Option<String>,
    /// The user id at the provider.
    pub user_id: String,
    /// An optional payment id.
    pub payment_id: Option<String>,
}

impl PaymentUserIdOptions {
    /// Options for `user_id`.
    #[must_use]
    pub fn new(user_id: impl Into<String>) -> Self {
        PaymentUserIdOptions {
            provider_name: None,
            user_id: user_id.into(),
            payment_id: None,
        }
    }

    /// Sets the provider.
    #[must_use]
    pub fn provider(mut self, name: impl Into<String>) -> Self {
        self.provider_name = Some(name.into());
        self
    }

    /// Sets the payment id.
    #[must_use]
    pub fn payment(mut self, id: impl Into<String>) -> Self {
        self.payment_id = Some(id.into());
        self
    }

    /// The options object in JavaScript's key order (`providerName`,
    /// `userId`, `paymentId`); the order is part of the request sent.
    #[must_use]
    pub fn into_map(self) -> Map<String, Value> {
        let mut map = Map::new();
        if let Some(p) = self.provider_name {
            map.insert("providerName".into(), p.into());
        }
        map.insert("userId".into(), self.user_id.into());
        if let Some(p) = self.payment_id {
            map.insert("paymentId".into(), p.into());
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_ids_follow_ow_electron() {
        let ids = MachineIds::new("v1", "");
        assert_eq!(ids.muid, "v1");
        assert_eq!(ids.muid_v2, "");
    }

    #[test]
    fn cmp_options_refuse_unknown_keys() {
        let err = serde_json::from_value::<CmpWindowOptions>(serde_json::json!({ "parentId": 3 }));
        assert!(err.is_err());
        let o = CmpWindowOptions::new()
            .tab(CmpTab::Features)
            .modal_to("main")
            .cmp_url("https://content.overwolf.com/x");
        let json = serde_json::to_value(&o).unwrap();
        assert_eq!(json["tab"], "features");
        assert_eq!(json["cmpURL"], "https://content.overwolf.com/x");
        assert_eq!(CmpTab::Purposes.as_str(), "purposes");
    }

    #[test]
    fn payment_options_keep_key_order() {
        let map = PaymentUserIdOptions::new("u").payment("p").into_map();
        let keys: Vec<&String> = map.keys().collect();
        assert_eq!(keys, ["userId", "paymentId"]);
    }
}
