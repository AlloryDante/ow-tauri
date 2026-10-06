//! The ads host: one native child webview per `<owadview>` element
//! (CONTRACT A.2.5, B.3, D; ADR 0003, `docs/adr/0003-owadview-native-child-webviews.md`).
//!
//! Implemented in a later milestone: the `adview_*` commands, guest webview
//! creation and layout, the `adview-host.js` guest shim, consent gating,
//! click and navigation rules, rate limits and crash recovery. This module
//! holds the wire types of A.2.5 so other modules and apps can name them.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The ad page every guest loads (CONTRACT D).
pub const ADVIEW_URL: &str = "https://www.overwolf.com/monsdk/electron/latest/adview.html";

/// `AdviewMount` (A.2.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdviewMount {
    /// Runtime-assigned element id, unique per embedder webview.
    pub element_id: String,
    /// The element's attributes.
    pub attributes: AdviewAttributes,
    /// The element's rectangle in CSS pixels.
    pub rect: AdviewRect,
    /// Whether the element is visible.
    pub visible: bool,
}

/// `AdviewAttributes` (A.2.5, B.3.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdviewAttributes {
    /// Container id, trimmed, at most 20 characters.
    pub cid: String,
    /// Requested inventory `"WxH"`.
    pub slotsize: String,
    /// Style tokens, for example `"high-impact-ad;"`.
    pub adstyle: String,
    /// Parsed `customTracking` JSON object, or `null`.
    pub custom_tracking: Value,
    /// Performance ad.
    pub performance: bool,
    /// Ad unit override.
    pub unit: Option<String>,
    /// Page URL (`ads.experimentalElementApi` only).
    pub page_url: Option<String>,
}

/// `AdviewRect` (A.2.5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdviewRect {
    /// Left edge in CSS pixels.
    pub x: f64,
    /// Top edge in CSS pixels.
    pub y: f64,
    /// Width in CSS pixels.
    pub width: f64,
    /// Height in CSS pixels.
    pub height: f64,
    /// The embedder's `devicePixelRatio`.
    pub device_pixel_ratio: f64,
}
