//! Consent (CMP) windows and storage, email hashes and the FPD and
//! ad-optimisation switches (CONTRACT A.2.2, A.2.7, D.6).
//!
//! Implemented in a later milestone: `is_cmp_required`, the consent window
//! and the `cmp.js` guest shim, consent storage in `ow-electron.json` (the
//! state module already reads and writes the shared `cmp` block, F.2), and
//! the `disable_*` / email-hash commands. The email hash function itself is
//! [`crate::identity::email_hashes`]. This module holds the option types of
//! A.2.2.

use serde::{Deserialize, Serialize};

/// The default consent page (D.6, OQ-07).
pub const DEFAULT_CMP_URL: &str =
    "https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/cmp.html";

/// `CMPWindowOptions` with `parent` replaced by `parentId` (A.2.2).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CmpWindowOptions {
    /// `purposes`, `features` or `vendors`.
    pub tab: Option<String>,
    /// Owned by the parent and kept above it.
    pub modal: Option<bool>,
    /// The parent window's id.
    pub parent_id: Option<u32>,
    /// Centre the window.
    pub center: Option<bool>,
    /// Window background colour.
    pub background_color: Option<String>,
    /// Spinner colour of the consent page.
    pub pre_loader_spinner_color: Option<String>,
    /// Width (default 800).
    pub width: Option<u32>,
    /// Height (default 800).
    pub height: Option<u32>,
    /// Left edge.
    pub x: Option<i32>,
    /// Top edge.
    pub y: Option<i32>,
    /// Consent page URL override (D.6 scope only).
    #[serde(rename = "cmpURL")]
    pub cmp_url: Option<String>,
    /// Page language.
    pub language: Option<String>,
}

/// `ExternalPaymentUserIdOptions` (A.2.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalPaymentUserIdOptions {
    /// Payment provider; `tebex` when empty.
    #[serde(default)]
    pub provider_name: String,
    /// The user id at the provider.
    pub user_id: String,
    /// Optional payment id.
    #[serde(default)]
    pub payment_id: Option<String>,
}
