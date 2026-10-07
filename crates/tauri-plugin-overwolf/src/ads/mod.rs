//! The ads host: one native child webview per `<owadview>` element
//! (CONTRACT A.2.5, A.2.6, B.3, D; ADR 0003,
//! `docs/adr/0003-owadview-native-child-webviews.md`).
//!
//! This module holds the pure parts: the wire types of A.2.5 and A.2.6,
//! guest labels, the guest configuration (D.2), the per-guest limits (D.4),
//! the external-open rules (D.7) and the recovery policy. The plugin's host
//! creates and drives the guest webviews with them.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

/// The ad page every guest loads (CONTRACT D).
pub const ADVIEW_URL: &str = "https://www.overwolf.com/monsdk/electron/latest/adview.html";

/// The URL scope of the ad guests' capability and of their one command.
pub const ADVIEW_SCOPE: &str = "https://www.overwolf.com/monsdk/electron/";

/// The token `adview-host.js` carries in place of its configuration (D.1).
pub const ADVIEW_CONFIG_TOKEN: &str = "/*__OW_TAURI_ADVIEW_CONFIG__*/null";

/// Largest encoded `data` of one guest message (D.4, A.2.6).
pub const MAX_EVENT_DATA_BYTES: usize = 16 * 1024;

/// How long a guest's first navigation waits for the startup consent
/// window at most, from the mount (D.6.5).
pub const CONSENT_WAIT_MS: u64 = 3_000;

/// How long a guest may stay over its message limit before it is reloaded,
/// then closed (D.4).
pub const OVER_LIMIT_MS: u64 = 10_000;

/// Dropped-message counts are logged at most once per this interval (D.4).
pub const DROP_LOG_INTERVAL_MS: u64 = 60_000;

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
    #[serde(default)]
    pub custom_tracking: Value,
    /// Performance ad.
    #[serde(default)]
    pub performance: bool,
    /// Ad unit override.
    #[serde(default)]
    pub unit: Option<String>,
    /// The `pageurl` attribute, `""` when absent (D.2 `pageUrl`).
    #[serde(default)]
    pub pageurl: String,
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

/// Distinguishes an absent field (`None`) from `null` (`Some(Value::Null)`).
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

/// `Partial<AdviewAttributes>` of `adview_update` (A.2.5).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdviewAttributesPatch {
    /// New `cid`.
    pub cid: Option<String>,
    /// New `slotsize`.
    pub slotsize: Option<String>,
    /// New `adstyle`.
    pub adstyle: Option<String>,
    /// New `customTracking`; `Some(Value::Null)` clears it.
    #[serde(default, deserialize_with = "present")]
    pub custom_tracking: Option<Value>,
    /// New `performance`.
    pub performance: Option<bool>,
    /// New `unit`.
    #[serde(default, deserialize_with = "present")]
    pub unit: Option<Value>,
    /// New `pageurl`.
    pub pageurl: Option<String>,
}

/// `adview_update` (A.2.5).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdviewUpdate {
    /// The element.
    pub element_id: String,
    /// New rectangle.
    pub rect: Option<AdviewRect>,
    /// New visibility.
    pub visible: Option<bool>,
    /// Changed attributes.
    pub attributes: Option<AdviewAttributesPatch>,
}

/// The element methods of `adview_command` (B.3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdviewCommandName {
    /// `setAudioMuted(muted)`.
    SetAudioMuted,
    /// `reload()`.
    Reload,
    /// `setPageUrl(url)`.
    SetPageUrl,
    /// `sendCommand(...args)`.
    SendCommand,
}

/// A guest-to-host message of `adview_event` (A.2.6).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdviewEvent {
    /// The slot the shim claims; the caller label is authoritative.
    #[serde(default)]
    pub slot_id: Option<String>,
    /// Event name.
    pub name: String,
    /// Event data.
    #[serde(default)]
    pub data: Option<Value>,
}

/// Validates a mount: element id and rectangle.
///
/// ```
/// use tauri_plugin_overwolf::ads::{valid_element_id, valid_rect, AdviewRect};
/// assert!(valid_element_id("e12"));
/// assert!(!valid_element_id("e 1"));
/// let r = AdviewRect { x: 0.0, y: 0.0, width: 400.0, height: 600.0, device_pixel_ratio: 2.0 };
/// assert!(valid_rect(&r));
/// assert!(!valid_rect(&AdviewRect { width: f64::NAN, ..r }));
/// ```
#[must_use]
pub fn valid_element_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Whether every value of `rect` is finite, the size is at most 16384 and
/// not negative, and the pixel ratio is in `(0, 16]`.
#[must_use]
pub fn valid_rect(rect: &AdviewRect) -> bool {
    let finite = [
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        rect.device_pixel_ratio,
    ]
    .iter()
    .all(|v| v.is_finite());
    finite
        && (0.0..=16_384.0).contains(&rect.width)
        && (0.0..=16_384.0).contains(&rect.height)
        && rect.x.abs() <= 1.0e6
        && rect.y.abs() <= 1.0e6
        && rect.device_pixel_ratio > 0.0
        && rect.device_pixel_ratio <= 16.0
}

/// A guest's position and size in the embedder window's logical pixels
/// (A.2.5): `css * devicePixelRatio / scaleFactor` plus the embedder
/// webview's own position.
///
/// ```
/// use tauri_plugin_overwolf::ads::{logical_rect, AdviewRect};
/// let r = AdviewRect { x: 10.0, y: 20.0, width: 400.0, height: 600.0, device_pixel_ratio: 2.0 };
/// assert_eq!(logical_rect(&r, 2.0, (0.0, 30.0)), (10.0, 50.0, 400.0, 600.0));
/// // Page zoom 150 % on a 1x display.
/// let z = AdviewRect { device_pixel_ratio: 1.5, ..r };
/// assert_eq!(logical_rect(&z, 1.0, (0.0, 0.0)), (15.0, 30.0, 600.0, 900.0));
/// ```
#[must_use]
pub fn logical_rect(
    rect: &AdviewRect,
    scale_factor: f64,
    offset: (f64, f64),
) -> (f64, f64, f64, f64) {
    let k = if scale_factor > 0.0 {
        rect.device_pixel_ratio / scale_factor
    } else {
        rect.device_pixel_ratio
    };
    (
        rect.x * k + offset.0,
        rect.y * k + offset.1,
        (rect.width * k).max(0.0),
        (rect.height * k).max(0.0),
    )
}

/// The guest label `owad-<embedder>-<n>`.
///
/// ```
/// use tauri_plugin_overwolf::ads::{guest_label, parse_guest_label};
/// assert_eq!(guest_label("bw-3", 1), "owad-bw-3-1");
/// assert_eq!(parse_guest_label("owad-bw-3-1"), Some(("bw-3", 1)));
/// assert_eq!(parse_guest_label("owad-bw-3"), Some(("bw", 3)));
/// assert_eq!(parse_guest_label("owad-x"), None);
/// ```
#[must_use]
pub fn guest_label(embedder: &str, n: u32) -> String {
    format!("owad-{embedder}-{n}")
}

/// Splits a guest label into the embedder label and the counter.
#[must_use]
pub fn parse_guest_label(label: &str) -> Option<(&str, u32)> {
    let rest = label.strip_prefix("owad-")?;
    let (embedder, n) = rest.rsplit_once('-')?;
    if embedder.is_empty() || n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((embedder, n.parse().ok()?))
}

/// Whether `name` is a valid `adview_event` name: 1 to 64 characters of
/// `[A-Za-z0-9_:.-]` (A.2.6).
///
/// ```
/// use tauri_plugin_overwolf::ads::valid_event_name;
/// assert!(valid_event_name("display_ad_loaded"));
/// assert!(valid_event_name("__host:ready"));
/// assert!(!valid_event_name(""));
/// assert!(!valid_event_name("a b"));
/// assert!(!valid_event_name(&"x".repeat(65)));
/// ```
#[must_use]
pub fn valid_event_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'.' | b'-'))
}

/// Applies the 16 KiB rule to guest data (D.4): data whose JSON encoding
/// is larger becomes `{ truncated: true, bytes }`. Returns the value and
/// its encoded size.
///
/// ```
/// use serde_json::json;
/// use tauri_plugin_overwolf::ads::cap_event_data;
/// assert_eq!(cap_event_data(json!({"a": 1})).0, json!({"a": 1}));
/// let (v, n) = cap_event_data(json!("x".repeat(20_000)));
/// assert_eq!(v, json!({"truncated": true, "bytes": n}));
/// ```
#[must_use]
pub fn cap_event_data(data: Value) -> (Value, usize) {
    let bytes = serde_json::to_vec(&data).map_or(usize::MAX, |v| v.len());
    if bytes > MAX_EVENT_DATA_BYTES {
        let mut m = Map::new();
        m.insert("truncated".into(), Value::Bool(true));
        m.insert("bytes".into(), Value::from(bytes));
        (Value::Object(m), bytes)
    } else {
        (data, bytes)
    }
}

/// Internal guest message names, handled by the host (D.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalEvent {
    /// `__host:ready`.
    Ready,
    /// `__host:gesture`.
    Gesture,
    /// `__host:focus`.
    Focus,
    /// `__host:setMute`.
    SetMute,
    /// `__host:applySetting`.
    ApplySetting,
    /// `__host:crash`.
    Crash,
    /// `__host:reload`.
    Reload,
    /// `__host:domReady` (the guest's `DOMContentLoaded`, B.3.5).
    DomReady,
}

impl InternalEvent {
    /// Parses an internal name; `None` for an app-facing event.
    ///
    /// ```
    /// use tauri_plugin_overwolf::ads::InternalEvent;
    /// assert_eq!(InternalEvent::parse("__host:reload"), Some(InternalEvent::Reload));
    /// assert_eq!(InternalEvent::parse("impression"), None);
    /// ```
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "__host:ready" => Self::Ready,
            "__host:gesture" => Self::Gesture,
            "__host:focus" => Self::Focus,
            "__host:setMute" => Self::SetMute,
            "__host:applySetting" => Self::ApplySetting,
            "__host:crash" => Self::Crash,
            "__host:reload" => Self::Reload,
            "__host:domReady" => Self::DomReady,
            _ => return None,
        })
    }

    /// Whether an unknown `__host:` name (dropped, never forwarded).
    #[must_use]
    pub fn is_reserved(name: &str) -> bool {
        name.starts_with("__host:")
    }
}

/// Whether `url` is an Overwolf page a guest may navigate to: `https` on
/// `overwolf.com` or a subdomain (D.7).
///
/// ```
/// use tauri_plugin_overwolf::ads::is_overwolf_url;
/// assert!(is_overwolf_url(&"https://www.overwolf.com/x".parse().unwrap()));
/// assert!(!is_overwolf_url(&"http://www.overwolf.com/x".parse().unwrap()));
/// assert!(!is_overwolf_url(&"https://overwolf.com.example/x".parse().unwrap()));
/// ```
#[must_use]
pub fn is_overwolf_url(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|h| h == "overwolf.com" || h.ends_with(".overwolf.com"))
}

/// Whether `url` may be opened in the system browser from a guest: `http`
/// or `https`, with a host and without credentials (D.7).
///
/// ```
/// use tauri_plugin_overwolf::ads::openable_url;
/// assert!(openable_url(&"https://advertiser.example/x".parse().unwrap()));
/// assert!(!openable_url(&"https://u:p@advertiser.example/".parse().unwrap()));
/// assert!(!openable_url(&"file:///etc/passwd".parse().unwrap()));
/// ```
#[must_use]
pub fn openable_url(url: &url::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some_and(|h| !h.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
}

/// What the message limiter decided (D.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// Deliver the message.
    Admit,
    /// Drop it.
    Drop,
    /// Drop it; the guest stayed over the limit for 10 s: reload it.
    Reload,
    /// Drop it; the guest stayed over the limit again after its reload:
    /// close it.
    Close,
}

/// A guest whose messages were not dropped for this long is no longer over
/// its limit (D.4).
const CALM_MS: u64 = 1000;

/// The per-guest token buckets of D.4: messages per second with a burst,
/// and encoded bytes per second.
#[derive(Debug, Clone, PartialEq)]
pub struct GuestLimiter {
    events_per_second: f64,
    burst: f64,
    bytes_per_second: f64,
    tokens: f64,
    byte_tokens: f64,
    last_ms: u64,
    over_since: Option<u64>,
    last_drop_ms: u64,
    reloaded: bool,
    dropped: u64,
    last_log_ms: Option<u64>,
}

impl GuestLimiter {
    /// A full bucket.
    #[must_use]
    pub fn new(limits: &crate::config::GuestLimits, now_ms: u64) -> Self {
        let burst = f64::from(limits.event_burst.max(1));
        #[expect(clippy::cast_precision_loss, reason = "byte rates are far below 2^52")]
        let bytes = limits.bytes_per_second.max(1) as f64;
        GuestLimiter {
            events_per_second: f64::from(limits.events_per_second.max(1)),
            burst,
            bytes_per_second: bytes,
            tokens: burst,
            byte_tokens: bytes,
            last_ms: now_ms,
            over_since: None,
            last_drop_ms: 0,
            reloaded: false,
            dropped: 0,
            last_log_ms: None,
        }
    }

    /// Admits or drops one message of `bytes` encoded bytes.
    ///
    /// ```
    /// use tauri_plugin_overwolf::ads::{Admission, GuestLimiter};
    /// use tauri_plugin_overwolf::config::GuestLimits;
    /// let limits = GuestLimits { events_per_second: 1, event_burst: 2, ..GuestLimits::default() };
    /// let mut l = GuestLimiter::new(&limits, 0);
    /// assert_eq!(l.admit(0, 10), Admission::Admit);
    /// assert_eq!(l.admit(0, 10), Admission::Admit);
    /// assert_eq!(l.admit(0, 10), Admission::Drop);
    /// assert_eq!(l.admit(1_000, 10), Admission::Admit);
    /// ```
    pub fn admit(&mut self, now_ms: u64, bytes: usize) -> Admission {
        #[expect(clippy::cast_precision_loss, reason = "millisecond deltas")]
        let elapsed = now_ms.saturating_sub(self.last_ms) as f64 / 1000.0;
        self.last_ms = self.last_ms.max(now_ms);
        self.tokens = (self.tokens + elapsed * self.events_per_second).min(self.burst);
        self.byte_tokens =
            (self.byte_tokens + elapsed * self.bytes_per_second).min(self.bytes_per_second);
        #[expect(clippy::cast_precision_loss, reason = "capped at 16 KiB")]
        let cost = bytes.min(MAX_EVENT_DATA_BYTES + 64) as f64;
        if self.tokens >= 1.0 && self.byte_tokens >= cost {
            self.tokens -= 1.0;
            self.byte_tokens -= cost;
            // A guest is over the limit while its messages keep being
            // dropped; a second without drops ends the period.
            if now_ms.saturating_sub(self.last_drop_ms) >= CALM_MS {
                self.over_since = None;
            }
            return Admission::Admit;
        }
        self.dropped += 1;
        if now_ms.saturating_sub(self.last_drop_ms) >= CALM_MS {
            self.over_since = None;
        }
        self.last_drop_ms = now_ms;
        let since = *self.over_since.get_or_insert(now_ms);
        if now_ms.saturating_sub(since) >= OVER_LIMIT_MS {
            self.over_since = None;
            if self.reloaded {
                return Admission::Close;
            }
            self.reloaded = true;
            self.tokens = self.burst;
            self.byte_tokens = self.bytes_per_second;
            return Admission::Reload;
        }
        Admission::Drop
    }

    /// The dropped count to log now, at most once per minute (D.4).
    pub fn take_drop_log(&mut self, now_ms: u64) -> Option<u64> {
        if self.dropped == 0 {
            return None;
        }
        if self
            .last_log_ms
            .is_some_and(|t| now_ms.saturating_sub(t) < DROP_LOG_INTERVAL_MS)
        {
            return None;
        }
        self.last_log_ms = Some(now_ms);
        Some(std::mem::take(&mut self.dropped))
    }
}

/// Why an external open was refused (D.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenRefusal {
    /// Not `http` / `https`, or with credentials.
    Url,
    /// No user gesture in the window.
    NoGesture,
    /// Over `externalOpensPerMinute`.
    RateLimited,
}

/// The external-open budget of one guest (D.7): one gesture allows one
/// open within `gestureWindowMs`, and at most `externalOpensPerMinute`
/// opens per minute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenBudget {
    window_ms: u64,
    per_minute: u32,
    gesture_at: Option<u64>,
    opens: Vec<u64>,
}

impl OpenBudget {
    /// A budget with no gesture.
    #[must_use]
    pub fn new(window_ms: u64, per_minute: u32) -> Self {
        OpenBudget {
            window_ms,
            per_minute,
            gesture_at: None,
            opens: Vec::new(),
        }
    }

    /// Records a gesture.
    pub fn gesture(&mut self, now_ms: u64) {
        self.gesture_at = Some(now_ms);
    }

    /// Asks to open `url`. `user_initiated` is the platform's own gesture
    /// flag for popups (WebView2 `IsUserInitiated`); `None` uses the
    /// reported gesture.
    ///
    /// ```
    /// use tauri_plugin_overwolf::ads::{OpenBudget, OpenRefusal};
    /// let url: url::Url = "https://advertiser.example/".parse().unwrap();
    /// let mut b = OpenBudget::new(1_500, 5);
    /// assert_eq!(b.try_open(0, &url, None), Err(OpenRefusal::NoGesture));
    /// b.gesture(100);
    /// assert_eq!(b.try_open(200, &url, None), Ok(()));
    /// // One gesture, one open.
    /// assert_eq!(b.try_open(300, &url, None), Err(OpenRefusal::NoGesture));
    /// ```
    ///
    /// # Errors
    ///
    /// The reason the open is refused.
    pub fn try_open(
        &mut self,
        now_ms: u64,
        url: &url::Url,
        user_initiated: Option<bool>,
    ) -> Result<(), OpenRefusal> {
        if !openable_url(url) {
            return Err(OpenRefusal::Url);
        }
        let gesture = match user_initiated {
            Some(true) => true,
            Some(false) => false,
            None => self
                .gesture_at
                .is_some_and(|t| now_ms.saturating_sub(t) <= self.window_ms),
        };
        if !gesture {
            return Err(OpenRefusal::NoGesture);
        }
        self.opens.retain(|t| now_ms.saturating_sub(*t) < 60_000);
        if self.opens.len() >= self.per_minute as usize {
            return Err(OpenRefusal::RateLimited);
        }
        self.opens.push(now_ms);
        if user_initiated.is_none() {
            self.gesture_at = None;
        }
        Ok(())
    }
}

/// `sessionTS` of a crash report: whole seconds since the guest's last load
/// or recovery (E.2 #8).
#[must_use]
pub fn session_secs(now_ms: u64, last_load_ms: u64) -> u64 {
    now_ms.saturating_sub(last_load_ms) / 1000
}

/// Whether a guest that has been recovered `recoveries` times may be
/// recovered again (`ads.maxRecoveries`, D.7).
///
/// ```
/// use tauri_plugin_overwolf::ads::may_recover;
/// assert!(may_recover(None, 1_000));
/// assert!(may_recover(Some(2), 1));
/// assert!(!may_recover(Some(2), 2));
/// ```
#[must_use]
pub fn may_recover(max_recoveries: Option<u32>, recoveries: u32) -> bool {
    max_recoveries.is_none_or(|max| recoveries < max)
}

/// The facts the guest configuration (D.2) is built from.
#[derive(Debug, Clone, PartialEq)]
pub struct GuestFacts<'a> {
    /// `<muid>`.
    pub muid: &'a str,
    /// `<uid>`.
    pub uid: &'a str,
    /// `<PN>`.
    pub name: &'a str,
    /// `<owVersion>` or `ads.owVersionOverride`.
    pub ow_version: &'a str,
    /// `<ver>`.
    pub version: &'a str,
    /// The embedder window's analytics name.
    pub window_name: &'a str,
    /// The embedder window's document title.
    pub window_title: &'a str,
    /// Embedder focus at mount.
    pub window_focused: bool,
    /// Test mode.
    pub test_ad: bool,
    /// `disableOptimization`.
    pub disable_optimization: bool,
    /// `<muidV2>`.
    pub muid_v2: &'a str,
    /// `phasePercent`.
    pub phase_percent: u8,
    /// `consent` and `consentFull`: the URL-encoded unified consent string
    /// stored at launch (`cmp%3D...`), or `""` before the first consent, as
    /// in ow-electron (observed).
    pub consent: &'a str,
    /// `systemInfo`.
    pub system_info: Value,
    /// The element's attributes at mount.
    pub attributes: &'a AdviewAttributes,
    /// The guest label (`slotId` of the shim's messages).
    pub slot_id: &'a str,
}

/// The guest configuration spliced into `adview-host.js`: the D.2 data keys
/// in their order (the shim inserts the functions after `muid`), plus the
/// shim's own `slotId` and `visibilityState`.
///
/// ```
/// use serde_json::json;
/// use tauri_plugin_overwolf::ads::{guest_config, AdviewAttributes, GuestFacts};
/// let attributes = AdviewAttributes {
///     cid: "c".into(), slotsize: "400x600".into(), adstyle: String::new(),
///     custom_tracking: json!(null), performance: false, unit: Some("u1".into()), pageurl: String::new(),
/// };
/// let facts = GuestFacts {
///     muid: "m", uid: "u", name: "App", ow_version: "tauri-2.12.1", version: "1.0.0",
///     window_name: "index", window_title: "App", window_focused: false, test_ad: true,
///     disable_optimization: false, muid_v2: "m", phase_percent: 7, consent: "",
///     system_info: json!({}),
///     attributes: &attributes, slot_id: "owad-bw-1-1",
/// };
/// let c = guest_config(&facts, true);
/// let keys: Vec<&str> = c.as_object().unwrap().keys().map(String::as_str).collect();
/// assert_eq!(&keys[..4], ["muid", "uid", "name", "owVersion"]);
/// // `unit` is passed through verbatim, in test mode too (observed).
/// assert_eq!(c["unit"], "u1");
/// ```
#[must_use]
pub fn guest_config(facts: &GuestFacts<'_>, visible: bool) -> Value {
    let a = facts.attributes;
    // Verbatim in both modes, as ow-electron forwards it (observed: the ad
    // library gets `forceAdUnit:<unit>`); `testAd` already selects test
    // demand.
    let unit = a.unit.clone().unwrap_or_default();
    let mut settings = Map::new();
    settings.insert(
        "disableOptimization".into(),
        Value::Bool(facts.disable_optimization),
    );
    settings.insert("anonymous".into(), Value::Bool(false));
    let mut m = Map::new();
    let mut put = |k: &str, v: Value| {
        m.insert(k.to_owned(), v);
    };
    put("muid", facts.muid.into());
    put("uid", facts.uid.into());
    put("name", facts.name.into());
    put("owVersion", facts.ow_version.into());
    put("version", facts.version.into());
    put("windowName", facts.window_name.into());
    put("windowTitle", facts.window_title.into());
    put("windowFocused", facts.window_focused.into());
    put("testAd", facts.test_ad.into());
    put("consent", facts.consent.into());
    put("consentFull", facts.consent.into());
    put("slotSize", a.slotsize.clone().into());
    put(
        "containerId",
        a.cid.chars().take(20).collect::<String>().into(),
    );
    put("systemInfo", facts.system_info.clone());
    put("settings", Value::Object(settings));
    put("muidV2", facts.muid_v2.into());
    put("phasePercent", facts.phase_percent.into());
    put("pageUrl", a.pageurl.clone().into());
    put("performanceAd", a.performance.into());
    put("adStyle", a.adstyle.clone().into());
    put("unit", unit.into());
    put(
        "customTracking",
        if a.custom_tracking.is_object() {
            a.custom_tracking.clone()
        } else {
            Value::Null
        },
    );
    put("slotId", facts.slot_id.into());
    put(
        "visibilityState",
        if visible { "visible" } else { "hidden" }.into(),
    );
    Value::Object(m)
}

/// JSON safe to splice into a script: `<`, U+2028 and U+2029 escaped (D.1).
///
/// ```
/// use serde_json::json;
/// use tauri_plugin_overwolf::ads::script_json;
/// assert_eq!(
///     script_json(&json!({"a": "</script>\u{2028}"})),
///     r#"{"a":"\u003c/script>\u2028"}"#
/// );
/// ```
#[must_use]
pub fn script_json(value: &Value) -> String {
    value
        .to_string()
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Replaces `token` in `script` with `config` (D.1). The token must occur
/// exactly once; otherwise `None`.
///
/// ```
/// use serde_json::json;
/// use tauri_plugin_overwolf::ads::splice_config;
/// let s = splice_config("const C = /*T*/null;", "/*T*/null", &json!({"x": 1})).unwrap();
/// assert_eq!(s, r#"const C = {"x":1};"#);
/// assert!(splice_config("no token", "/*T*/null", &json!(1)).is_none());
/// ```
#[must_use]
pub fn splice_config(script: &str, token: &str, config: &Value) -> Option<String> {
    if script.matches(token).count() != 1 {
        return None;
    }
    Some(script.replacen(token, &script_json(config), 1))
}

/// Electron's `render-process-gone` reason for a platform termination
/// (E.2 #8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoneReason {
    /// `killed`.
    Killed,
    /// `crashed`.
    Crashed,
    /// `oom`.
    Oom,
    /// `abnormal-exit`.
    AbnormalExit,
    /// `launch-failed`.
    LaunchFailed,
}

impl GoneReason {
    /// The wire string.
    ///
    /// ```
    /// use tauri_plugin_overwolf::ads::GoneReason;
    /// assert_eq!(GoneReason::Killed.as_str(), "killed");
    /// ```
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            GoneReason::Killed => "killed",
            GoneReason::Crashed => "crashed",
            GoneReason::Oom => "oom",
            GoneReason::AbnormalExit => "abnormal-exit",
            GoneReason::LaunchFailed => "launch-failed",
        }
    }
}

/// Host lifecycle event data for `did-fail-load` (B.3.5).
#[must_use]
pub fn fail_load_data(error_code: i64, description: &str, url: &str, main_frame: bool) -> Value {
    let mut m = Map::new();
    m.insert("errorCode".into(), error_code.into());
    m.insert("errorDescription".into(), description.into());
    m.insert("validatedURL".into(), url.into());
    m.insert("isMainFrame".into(), main_frame.into());
    m.insert("frameProcessId".into(), 0.into());
    m.insert("frameRoutingId".into(), 0.into());
    Value::Object(m)
}

/// Host lifecycle event data for `render-process-gone` (B.3.5).
#[must_use]
pub fn gone_data(reason: GoneReason, exit_code: i64) -> Value {
    let mut details = Map::new();
    details.insert("reason".into(), reason.as_str().into());
    details.insert("exitCode".into(), exit_code.into());
    let mut m = Map::new();
    m.insert("details".into(), Value::Object(details));
    Value::Object(m)
}

/// A host message as the guest receives it (D.5): `{ type }`, plus `data`
/// only when there is some (ow-electron sends `window-hidden` without a
/// `data` key (observed)).
///
/// ```
/// use serde_json::json;
/// use tauri_plugin_overwolf::ads::host_message;
/// assert_eq!(host_message("window-hidden", None), json!({ "type": "window-hidden" }));
/// assert_eq!(host_message("consent", Some(&json!("x"))), json!({ "type": "consent", "data": "x" }));
/// ```
#[must_use]
pub fn host_message(kind: &str, data: Option<&Value>) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), kind.into());
    if let Some(d) = data {
        m.insert("data".into(), d.clone());
    }
    Value::Object(m)
}

/// The JavaScript that delivers a host message to the guest (D.5).
/// `host_key` is the guest's random host API property (`hostKey` of its
/// configuration).
///
/// ```
/// use serde_json::json;
/// use tauri_plugin_overwolf::ads::deliver_script;
/// assert_eq!(
///     deliver_script("_k1", "window-hidden", None),
///     r#"(function(h){h&&h.deliver({"type":"window-hidden"})})(window["_k1"])"#
/// );
/// ```
#[must_use]
pub fn deliver_script(host_key: &str, kind: &str, data: Option<&Value>) -> String {
    host_call_script(host_key, "deliver", &host_message(kind, data))
}

/// The JavaScript that calls a shim-internal host function (D.5) on the
/// guest's host API property `host_key`.
///
/// ```
/// use serde_json::json;
/// use tauri_plugin_overwolf::ads::host_call_script;
/// assert_eq!(
///     host_call_script("_k1", "setVisibility", &json!("hidden")),
///     r#"(function(h){h&&h.setVisibility("hidden")})(window["_k1"])"#
/// );
/// ```
#[must_use]
pub fn host_call_script(host_key: &str, function: &str, arg: &Value) -> String {
    format!(
        "(function(h){{h&&h.{function}({})}})(window[{}])",
        script_json(arg),
        script_json(&Value::from(host_key))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn attribute_wire_shapes() {
        let m: AdviewMount = serde_json::from_value(json!({
            "elementId": "e1",
            "attributes": {
                "cid": "c", "slotsize": "400x600", "adstyle": "", "customTracking": {"a": 1},
                "performance": false, "unit": null, "pageurl": "https://example.com/p"
            },
            "rect": {"x": 0, "y": 0, "width": 400, "height": 600, "devicePixelRatio": 1},
            "visible": true
        }))
        .unwrap();
        assert_eq!(m.attributes.pageurl, "https://example.com/p");
        let u: AdviewUpdate = serde_json::from_value(json!({
            "elementId": "e1", "attributes": {"customTracking": null}
        }))
        .unwrap();
        assert_eq!(
            u.attributes.unwrap().custom_tracking,
            Some(Value::Null),
            "null is a change, absent is not"
        );
        let u: AdviewUpdate =
            serde_json::from_value(json!({"elementId": "e1", "attributes": {}})).unwrap();
        assert_eq!(u.attributes.unwrap().custom_tracking, None);
        let c: AdviewCommandName = serde_json::from_value(json!("setPageUrl")).unwrap();
        assert_eq!(c, AdviewCommandName::SetPageUrl);
    }

    #[test]
    fn limiter_reloads_then_closes() {
        let limits = crate::config::GuestLimits {
            events_per_second: 1,
            event_burst: 1,
            ..crate::config::GuestLimits::default()
        };
        let mut l = GuestLimiter::new(&limits, 0);
        assert_eq!(l.admit(0, 1), Admission::Admit);
        // Flood: 100 messages per 100 ms for 10 s.
        let mut actions = Vec::new();
        for t in (0..=20_500).step_by(10) {
            let a = l.admit(t, 1);
            if matches!(a, Admission::Reload | Admission::Close) {
                actions.push((t, a));
            }
        }
        assert_eq!(actions.first().map(|a| a.1), Some(Admission::Reload));
        assert!(actions.iter().any(|a| a.1 == Admission::Close));
        assert!(l.take_drop_log(20_500).is_some());
        assert!(l.take_drop_log(20_600).is_none(), "once per minute");
    }

    #[test]
    fn byte_budget() {
        let limits = crate::config::GuestLimits {
            bytes_per_second: 100,
            ..crate::config::GuestLimits::default()
        };
        let mut l = GuestLimiter::new(&limits, 0);
        assert_eq!(l.admit(0, 80), Admission::Admit);
        assert_eq!(l.admit(0, 80), Admission::Drop);
        assert_eq!(l.admit(1_000, 80), Admission::Admit);
    }

    #[test]
    fn open_budget_rate_limit() {
        let url: url::Url = "https://advertiser.example/".parse().unwrap();
        let mut b = OpenBudget::new(1_500, 2);
        assert_eq!(b.try_open(0, &url, Some(true)), Ok(()));
        assert_eq!(b.try_open(1, &url, Some(true)), Ok(()));
        assert_eq!(
            b.try_open(2, &url, Some(true)),
            Err(OpenRefusal::RateLimited)
        );
        assert_eq!(b.try_open(60_001, &url, Some(true)), Ok(()));
        assert_eq!(
            b.try_open(60_002, &url, Some(false)),
            Err(OpenRefusal::NoGesture)
        );
        b.gesture(70_000);
        assert_eq!(
            b.try_open(71_501, &url, None),
            Err(OpenRefusal::NoGesture),
            "window passed"
        );
        let bad: url::Url = "javascript:alert(1)".parse().unwrap();
        assert_eq!(b.try_open(0, &bad, Some(true)), Err(OpenRefusal::Url));
    }

    #[test]
    fn config_keys_in_d2_order() {
        let attributes = AdviewAttributes {
            cid: "123456789012345678901234".into(),
            slotsize: "400x600".into(),
            adstyle: "high-impact-ad;".into(),
            custom_tracking: json!({"k": "v"}),
            performance: true,
            unit: Some("live-unit".into()),
            pageurl: "https://example.com/".into(),
        };
        let facts = GuestFacts {
            muid: "m",
            uid: "u",
            name: "Parity Harness",
            ow_version: "tauri-2.12.1",
            version: "1.0.0",
            window_name: "index",
            window_title: "Parity Harness",
            window_focused: false,
            test_ad: false,
            disable_optimization: true,
            muid_v2: "m2",
            phase_percent: 80,
            consent: "cmp%3DCQ%26ac%3D2~1",
            system_info: json!({"gpus": [], "cpu": "x", "displays": []}),
            attributes: &attributes,
            slot_id: "owad-bw-1-1",
        };
        let c = guest_config(&facts, false);
        let keys: Vec<&str> = c.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "muid",
                "uid",
                "name",
                "owVersion",
                "version",
                "windowName",
                "windowTitle",
                "windowFocused",
                "testAd",
                "consent",
                "consentFull",
                "slotSize",
                "containerId",
                "systemInfo",
                "settings",
                "muidV2",
                "phasePercent",
                "pageUrl",
                "performanceAd",
                "adStyle",
                "unit",
                "customTracking",
                "slotId",
                "visibilityState"
            ]
        );
        assert_eq!(c["containerId"], "12345678901234567890");
        // The consent stored at launch (regression: always "").
        assert_eq!(c["consent"], "cmp%3DCQ%26ac%3D2~1");
        assert_eq!(c["consentFull"], "cmp%3DCQ%26ac%3D2~1");
        assert_eq!(c["unit"], "live-unit", "live mode passes the unit through");
        let test_mode = guest_config(
            &GuestFacts {
                test_ad: true,
                ..facts.clone()
            },
            false,
        );
        assert_eq!(test_mode["testAd"], true);
        assert_eq!(
            test_mode["unit"], "live-unit",
            "test mode passes the unit through too (no testAd rewrite)"
        );
        assert_eq!(
            c["settings"],
            json!({"disableOptimization": true, "anonymous": false})
        );
        assert_eq!(c["visibilityState"], "hidden");
        let mut no_unit = attributes.clone();
        no_unit.unit = None;
        no_unit.custom_tracking = json!("not an object");
        let c = guest_config(
            &GuestFacts {
                attributes: &no_unit,
                test_ad: true,
                ..facts
            },
            true,
        );
        assert_eq!(c["unit"], "");
        assert_eq!(c["customTracking"], Value::Null);
    }

    #[test]
    fn host_event_data() {
        assert_eq!(
            fail_load_data(-105, "ERR_NAME_NOT_RESOLVED", "https://x/", true),
            json!({"errorCode": -105, "errorDescription": "ERR_NAME_NOT_RESOLVED",
                "validatedURL": "https://x/", "isMainFrame": true, "frameProcessId": 0, "frameRoutingId": 0})
        );
        assert_eq!(
            gone_data(GoneReason::Killed, 0),
            json!({"details": {"reason": "killed", "exitCode": 0}})
        );
        assert!(InternalEvent::is_reserved("__host:other"));
        assert_eq!(session_secs(25_999, 5_000), 20);
    }
}
