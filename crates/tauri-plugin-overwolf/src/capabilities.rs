//! The capabilities the plugin adds at runtime (ARCHITECTURE 5.2;
//! ADR 0010, 0011).
//!
//! Every capability names webview labels only (never window labels), so a
//! child webview inside a `bw-*` window, an ad guest or a remote page, does
//! not inherit its window's capability. No `core:event:*` permission is
//! granted anywhere.
//!
//! `Manager::add_capability` panics when a capability names a permission the
//! app's ACL does not know, so capabilities are built from `tauri-utils`
//! types (no parsing at runtime) and only name permissions that every app
//! built with `tauri-build` has: the core sets and this plugin's own.

use tauri::ipc::RuntimeCapability;
use tauri::utils::acl::Identifier;
use tauri::utils::acl::capability::{
    Capability, CapabilityFile, CapabilityRemote, PermissionEntry,
};

use crate::ipc::router::MAIN_LABEL;

/// Identifier of the main webview's capability.
pub(crate) const MAIN_CAPABILITY: &str = "ow-tauri-main";
/// Identifier of the ad guests' capability (added once `adview_event` exists).
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used once the ads lane registers adview_event")
)]
pub(crate) const ADVIEW_GUEST_CAPABILITY: &str = "ow-tauri-adview-guest";
/// Identifier of the consent window's capability (added once `cmp_event` exists).
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used once the consent lane registers cmp_event")
)]
pub(crate) const CMP_CAPABILITY: &str = "ow-tauri-cmp";

/// Core permissions of `ow-main`: the default sets plus the window and
/// webview commands the `BrowserWindow` facade calls. Creating windows or
/// webviews and destroying them go through plugin commands instead.
pub(crate) const MAIN_CORE_PERMISSIONS: &[&str] = &[
    "core:app:default",
    "core:path:default",
    "core:window:default",
    "core:webview:default",
    "core:window:allow-center",
    "core:window:allow-close",
    "core:window:allow-hide",
    "core:window:allow-show",
    "core:window:allow-maximize",
    "core:window:allow-minimize",
    "core:window:allow-unmaximize",
    "core:window:allow-unminimize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-request-user-attention",
    "core:window:allow-set-always-on-top",
    "core:window:allow-set-background-color",
    "core:window:allow-set-closable",
    "core:window:allow-set-content-protected",
    "core:window:allow-set-decorations",
    "core:window:allow-set-enabled",
    "core:window:allow-set-focus",
    "core:window:allow-set-focusable",
    "core:window:allow-set-fullscreen",
    "core:window:allow-set-ignore-cursor-events",
    "core:window:allow-set-max-size",
    "core:window:allow-set-maximizable",
    "core:window:allow-set-min-size",
    "core:window:allow-set-minimizable",
    "core:window:allow-set-position",
    "core:window:allow-set-progress-bar",
    "core:window:allow-set-resizable",
    "core:window:allow-set-shadow",
    "core:window:allow-set-size",
    "core:window:allow-set-skip-taskbar",
    "core:window:allow-set-title",
    "core:window:allow-set-visible-on-all-workspaces",
    "core:window:allow-start-dragging",
    "core:webview:allow-set-webview-zoom",
    "core:webview:allow-set-webview-focus",
];

/// Plugin permission sets of `ow-main`.
pub(crate) const MAIN_PLUGIN_PERMISSIONS: &[&str] = &["overwolf:main"];

/// A capability built without string parsing at runtime.
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

/// `ow-tauri-main`: webview `ow-main`, local content only.
pub(crate) fn main_capability() -> Result<Built, String> {
    let mut permissions = entries(MAIN_PLUGIN_PERMISSIONS)?;
    permissions.extend(entries(MAIN_CORE_PERMISSIONS)?);
    Ok(Built(Capability {
        identifier: MAIN_CAPABILITY.into(),
        description: "ow-tauri: the hidden main webview that runs the app's main-process code."
            .into(),
        remote: None,
        local: true,
        windows: Vec::new(),
        webviews: vec![MAIN_LABEL.into()],
        permissions,
        platforms: None,
    }))
}

/// A capability for remote Overwolf pages: `webviews` and exact URL
/// patterns, `local: false`. Used by the ads and consent modules.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "added once the ads and consent lanes register their commands"
    )
)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_capability_matches_webview_only() {
        let Built(c) = main_capability().unwrap();
        assert_eq!(c.webviews, vec!["ow-main"]);
        assert!(c.windows.is_empty());
        assert!(c.local && c.remote.is_none());
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
        assert!(ids.contains(&"overwolf:main".to_owned()));
        assert!(
            !ids.iter().any(|p| p.starts_with("core:event")),
            "no event permissions"
        );
        for forbidden in [
            "core:window:allow-create",
            "core:webview:allow-create-webview",
            "core:webview:allow-create-webview-window",
        ] {
            assert!(!ids.iter().any(|p| p == forbidden), "{forbidden}");
        }
    }

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
