//! The capabilities the plugin adds at run time (DESIGN §3.6, §4.5): one
//! guest command for the ad guests and one for the consent windows.
//!
//! Every capability names webview labels only (never window labels), so a
//! webview inside an app window never inherits a guest's capability and a
//! guest never inherits its window's. No `core:*` permission is granted.
//!
//! `Manager::add_capability` panics when a capability names a permission the
//! app's ACL does not know, so capabilities are built from `tauri-utils`
//! types (no parsing at run time) and only name this plugin's own sets.

use tauri::ipc::RuntimeCapability;
use tauri::utils::acl::Identifier;
use tauri::utils::acl::capability::{
    Capability, CapabilityFile, CapabilityRemote, PermissionEntry,
};

/// Identifier of the ad guests' capability.
pub(crate) const ADVIEW_GUEST_CAPABILITY: &str = "ow-tauri-adview-guest";
/// Identifier of the consent windows' capability.
pub(crate) const CMP_CAPABILITY: &str = "ow-tauri-cmp";

/// A capability built without string parsing at run time.
#[derive(Debug, Clone)]
pub(crate) struct Built(pub(crate) Capability);

impl RuntimeCapability for Built {
    fn build(self) -> CapabilityFile {
        CapabilityFile::Capability(self.0)
    }
}

fn entries(ids: &[&str]) -> Result<Vec<PermissionEntry>, String> {
    ids.iter()
        .map(|id| {
            Identifier::try_from((*id).to_owned())
                .map(PermissionEntry::PermissionRef)
                .map_err(|e| format!("{id}: {e}"))
        })
        .collect()
}

/// A capability for remote Overwolf pages: `webviews` and exact URL
/// patterns, `local: false`. Used by the ads and consent modules.
pub(crate) fn remote_capability(
    identifier: &str,
    webview_pattern: &str,
    url_pattern: &str,
    sets: &[&str],
) -> Result<Built, String> {
    Ok(Built(Capability {
        identifier: identifier.into(),
        description: format!("ow-tauri: {webview_pattern} pages on {url_pattern}."),
        remote: Some(CapabilityRemote {
            urls: vec![url_pattern.into()],
        }),
        local: false,
        windows: Vec::new(),
        webviews: vec![webview_pattern.into()],
        permissions: entries(sets)?,
        platforms: None,
    }))
}

/// `ow-tauri-adview-guest`: `owad-*` webviews on Overwolf's ad page get
/// `adview_event` only (A.2.6, ADR 0011).
pub(crate) fn adview_guest_capability() -> Result<Built, String> {
    remote_capability(
        ADVIEW_GUEST_CAPABILITY,
        "owad-*",
        "https://www.overwolf.com/monsdk/electron/*",
        &["overwolf:adview-guest"],
    )
}

/// `ow-tauri-cmp`: the consent windows on Overwolf's consent pages get
/// `cmp_event` only (A.2.7, D.6.4).
pub(crate) fn cmp_capability() -> Result<Built, String> {
    remote_capability(
        CMP_CAPABILITY,
        "ow-cmp*",
        "https://content.overwolf.com/monsdk/electron/*",
        &["overwolf:cmp-window"],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_capabilities_are_scoped() {
        let Built(c) = remote_capability(
            ADVIEW_GUEST_CAPABILITY,
            "owad-*",
            "https://www.overwolf.com/monsdk/electron/*",
            &["overwolf:default"],
        )
        .unwrap();
        assert!(!c.local);
        assert_eq!(
            c.remote.unwrap().urls,
            vec!["https://www.overwolf.com/monsdk/electron/*"]
        );
        assert_eq!(c.webviews, vec!["owad-*"]);
        assert!(c.windows.is_empty());
        let Built(g) = adview_guest_capability().unwrap();
        assert_eq!(g.webviews, vec!["owad-*"]);
        assert_eq!(g.permissions.len(), 1);
        let Built(m) = cmp_capability().unwrap();
        assert_eq!(m.webviews, vec!["ow-cmp*"]);
        assert!(!m.local);
        for Built(c) in [
            adview_guest_capability().unwrap(),
            cmp_capability().unwrap(),
        ] {
            let ids: Vec<String> = c
                .permissions
                .iter()
                .map(|p| match p {
                    PermissionEntry::PermissionRef(id) => id.get().to_owned(),
                    PermissionEntry::ExtendedPermission { identifier, .. } => {
                        identifier.get().to_owned()
                    }
                })
                .collect();
            assert!(ids.iter().all(|p| p.starts_with("overwolf:")), "{ids:?}");
        }
        assert!(
            remote_capability(
                CMP_CAPABILITY,
                "ow-cmp",
                "https://content.overwolf.com/monsdk/electron/*",
                &["bad id!"]
            )
            .is_err()
        );
    }
}
