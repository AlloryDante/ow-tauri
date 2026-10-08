//! Ad guests (`<owadview>`, DESIGN §4.4).
//!
//! W1 holds the frozen shape: the mounts keyed by embedder webview and
//! element (the §4.5 rule that only the mounting webview may act on its
//! element), the email hashes the guests receive, and the hooks
//! [`dispatch`](super::dispatch) calls. Guest creation, placement,
//! visibility, recreate and crash recovery arrive in W2; until then
//! `adview_mount` answers `unsupported` and no guest exists.

#![allow(
    dead_code,
    clippy::unused_self,
    reason = "the ads host (W2) reads the remaining state"
)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use tauri::webview::PageLoadEvent;
use tauri::{Runtime, Webview, WindowEvent};
use url::Url;

use super::{Core, lock};
use crate::identity::EmailHashes;

/// One mounted element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mount {
    /// The guest webview's label (`owad-<n>`).
    pub(crate) guest: String,
}

/// The ads service of one app.
#[derive(Debug, Default)]
pub(crate) struct AdsCore {
    /// Mounted elements by (embedder webview label, element id).
    mounts: Mutex<BTreeMap<(String, String), Mount>>,
    /// The hashes of the last `setUserEmailHashes()`, for the guests.
    email_hashes: Mutex<Option<EmailHashes>>,
}

impl AdsCore {
    /// Whether ad guests can be shown in this build on this platform.
    pub(crate) fn supported(&self) -> bool {
        false
    }

    /// The mount of element `element_id` of webview `embedder`; `None`
    /// when that webview did not mount it (another webview's element is
    /// never found, DESIGN §4.5).
    pub(crate) fn mount_of(&self, embedder: &str, element_id: &str) -> Option<Mount> {
        lock(&self.mounts)
            .get(&(embedder.to_owned(), element_id.to_owned()))
            .cloned()
    }

    /// Stores the hashes the guests receive; `None` clears them.
    pub(crate) fn set_email_hashes(&self, hashes: Option<EmailHashes>) {
        *lock(&self.email_hashes) = hashes;
    }

    /// The hashes the guests receive.
    pub(crate) fn email_hashes(&self) -> Option<EmailHashes> {
        lock(&self.email_hashes).clone()
    }

    /// `on_webview_ready` (W2: guest hooks).
    pub(crate) fn webview_ready<R: Runtime>(&self, _core: &Arc<Core<R>>, _webview: &Webview<R>) {}

    /// A page load of any webview (W2: guest loads; an embedder's new
    /// document closes its guests).
    pub(crate) fn page_load<R: Runtime>(
        &self,
        _core: &Arc<Core<R>>,
        _webview: &Webview<R>,
        _event: PageLoadEvent,
        _url: &Url,
    ) {
    }

    /// The navigation policy of guest `label` (W2). No guest exists yet:
    /// a webview with a guest label the plugin did not create may not
    /// navigate anywhere.
    pub(crate) fn guest_navigation<R: Runtime>(
        &self,
        _core: &Arc<Core<R>>,
        _label: &str,
        _url: &Url,
    ) -> bool {
        false
    }

    /// A window event (W2: focus, close-hide, destroyed embedders).
    pub(crate) fn window_event<R: Runtime>(
        &self,
        _core: &Arc<Core<R>>,
        _label: &str,
        _event: &WindowEvent,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elements_belong_to_their_webview() {
        let ads = AdsCore::default();
        lock(&ads.mounts).insert(
            ("main".into(), "e1".into()),
            Mount {
                guest: "owad-1".into(),
            },
        );
        assert_eq!(ads.mount_of("main", "e1").unwrap().guest, "owad-1");
        assert!(ads.mount_of("settings", "e1").is_none());
        assert!(!ads.supported());
    }
}
