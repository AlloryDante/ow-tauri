//! `window_create` and `window_load` arguments (CONTRACT A.2.3, B.2.2).
//!
//! ```
//! use tauri_plugin_overwolf::window::options::WindowCreateRequest;
//! let req: WindowCreateRequest = serde_json::from_str(
//!     r#"{ "options": { "width": 1200, "height": 700, "frame": false, "parentId": null }, "preload": "preload/preload.js", "windowClass": "ui" }"#,
//! ).unwrap();
//! assert_eq!(req.options.size(), (1200.0, 700.0));
//! assert_eq!(req.options.frame, Some(false));
//! req.validate().unwrap();
//! ```

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::error::Error;

/// Electron's default window size.
pub const DEFAULT_SIZE: (f64, f64) = (800.0, 600.0);

/// `webPreferences`, supported subset.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebPreferencesWire {
    /// `devTools`.
    pub dev_tools: Option<bool>,
    /// `zoomFactor`.
    pub zoom_factor: Option<f64>,
    /// Every other key, for warnings.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// The supported subset of `BrowserWindowConstructorOptions`, with `parent`
/// replaced by `parentId`. Unknown keys are accepted and reported.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[expect(
    missing_docs,
    reason = "each field is the Electron option of the same name"
)]
pub struct BrowserWindowOptionsWire {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub center: Option<bool>,
    pub min_width: Option<f64>,
    pub min_height: Option<f64>,
    pub max_width: Option<f64>,
    pub max_height: Option<f64>,
    pub use_content_size: Option<bool>,
    pub show: Option<bool>,
    pub title: Option<String>,
    pub resizable: Option<bool>,
    pub movable: Option<bool>,
    pub minimizable: Option<bool>,
    pub maximizable: Option<bool>,
    pub closable: Option<bool>,
    pub focusable: Option<bool>,
    pub always_on_top: Option<bool>,
    pub fullscreen: Option<bool>,
    pub skip_taskbar: Option<bool>,
    pub transparent: Option<bool>,
    pub background_color: Option<String>,
    pub parent_id: Option<u32>,
    pub modal: Option<bool>,
    pub frame: Option<bool>,
    pub name: Option<String>,
    pub web_preferences: Option<WebPreferencesWire>,
    /// Every other key, for warnings.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

fn finite_positive(name: &str, v: Option<f64>) -> Result<(), Error> {
    match v {
        Some(v) if !v.is_finite() || v < 0.0 => Err(Error::invalid_argument(format!(
            "options.{name} must be a non-negative number."
        ))),
        _ => Ok(()),
    }
}

impl BrowserWindowOptionsWire {
    /// `(width, height)` with Electron's defaults.
    #[must_use]
    pub fn size(&self) -> (f64, f64) {
        (
            self.width.unwrap_or(DEFAULT_SIZE.0),
            self.height.unwrap_or(DEFAULT_SIZE.1),
        )
    }

    /// Checks numeric fields.
    ///
    /// # Errors
    ///
    /// `invalid-argument` naming the field.
    pub fn validate(&self) -> Result<(), Error> {
        for (name, v) in [
            ("width", self.width),
            ("height", self.height),
            ("minWidth", self.min_width),
            ("minHeight", self.min_height),
            ("maxWidth", self.max_width),
            ("maxHeight", self.max_height),
        ] {
            finite_positive(name, v)?;
        }
        for (name, v) in [("x", self.x), ("y", self.y)] {
            if v.is_some_and(|v| !v.is_finite()) {
                return Err(Error::invalid_argument(format!(
                    "options.{name} must be a finite number."
                )));
            }
        }
        if let Some(z) = self.web_preferences.as_ref().and_then(|w| w.zoom_factor)
            && (!z.is_finite() || z <= 0.0)
        {
            return Err(Error::invalid_argument(
                "options.webPreferences.zoomFactor must be positive.",
            ));
        }
        Ok(())
    }

    /// Option names that are accepted but have no effect (B.2.2), for one
    /// warning per window.
    #[must_use]
    pub fn ignored_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.other.keys().cloned().collect();
        if let Some(w) = &self.web_preferences {
            keys.extend(w.other.keys().map(|k| format!("webPreferences.{k}")));
        }
        keys.retain(|k| k != "webPreferences.preload");
        keys
    }
}

/// `windowClass` of `window_create`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowClassWire {
    /// A UI window.
    Ui,
    /// An overlay window (overlay package backend only).
    Overlay,
}

/// `window_create` arguments.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowCreateRequest {
    /// Constructor options.
    #[serde(default)]
    pub options: BrowserWindowOptionsWire,
    /// App-asset path of the preload bundle.
    #[serde(default)]
    pub preload: Option<String>,
    /// `ui` or `overlay`.
    pub window_class: WindowClassWire,
    /// Overlay options (overlay backend).
    #[serde(default)]
    pub overlay_options: Option<Value>,
}

impl WindowCreateRequest {
    /// Validates options and the preload path.
    ///
    /// # Errors
    ///
    /// `invalid-argument`.
    pub fn validate(&self) -> Result<(), Error> {
        self.options.validate()?;
        if let Some(p) = &self.preload {
            validate_asset_path(p)
                .map_err(|_| Error::invalid_argument("preload must be an app-asset path."))?;
        }
        Ok(())
    }
}

/// `window_load` target.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LoadTarget {
    /// `loadFile(path, { query, hash })`.
    File {
        /// App-asset path.
        path: String,
        /// Query parameters.
        #[serde(default)]
        query: Option<BTreeMap<String, String>>,
        /// Fragment.
        #[serde(default)]
        hash: Option<String>,
    },
    /// `loadURL(url)`.
    Url {
        /// Absolute URL.
        url: String,
    },
}

/// Checks that `path` is a relative app-asset path without `..`, a scheme or
/// backslashes.
///
/// # Errors
///
/// `invalid-argument`.
pub fn validate_asset_path(path: &str) -> Result<(), Error> {
    let p = path.trim_start_matches('/');
    if p.is_empty()
        || p.contains('\\')
        || p.contains("://")
        || p.contains(':')
        || p.split('/').any(|s| s == ".." || s == ".")
    {
        return Err(Error::invalid_argument("not an app-asset path"));
    }
    Ok(())
}

/// Where a load goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedLoad {
    /// An app asset, loaded in the existing webview.
    App(Url),
    /// A remote `http(s)` page: the window becomes `remote` (A.2.3.1).
    Remote(Url),
}

/// Resolves a load target against the app origin.
///
/// ```
/// use url::Url;
/// use tauri_plugin_overwolf::window::options::{resolve_load, LoadTarget, ResolvedLoad};
/// let origin = Url::parse("tauri://localhost").unwrap();
/// let file = LoadTarget::File { path: "windows/main.html".into(), query: None, hash: Some("top".into()) };
/// assert_eq!(resolve_load(&file, &origin).unwrap(), ResolvedLoad::App(Url::parse("tauri://localhost/windows/main.html#top").unwrap()));
/// let remote = LoadTarget::Url { url: "https://example.com/".into() };
/// assert!(matches!(resolve_load(&remote, &origin).unwrap(), ResolvedLoad::Remote(_)));
/// ```
///
/// # Errors
///
/// `invalid-argument` for a bad path or URL, or a URL that is neither the app
/// origin nor `http(s)`.
pub fn resolve_load(target: &LoadTarget, app_origin: &Url) -> Result<ResolvedLoad, Error> {
    match target {
        LoadTarget::File { path, query, hash } => {
            validate_asset_path(path)?;
            let mut url = app_origin
                .join(path.trim_start_matches('/'))
                .map_err(|_| Error::invalid_argument("not an app-asset path"))?;
            if let Some(q) = query.as_ref().filter(|q| !q.is_empty()) {
                url.query_pairs_mut().extend_pairs(q.iter());
            }
            if let Some(h) = hash.as_ref().filter(|h| !h.is_empty()) {
                url.set_fragment(Some(h.trim_start_matches('#')));
            }
            Ok(ResolvedLoad::App(url))
        }
        LoadTarget::Url { url } => {
            let parsed =
                Url::parse(url).map_err(|_| Error::invalid_argument("not an absolute URL"))?;
            if same_origin(&parsed, app_origin) {
                Ok(ResolvedLoad::App(parsed))
            } else if matches!(parsed.scheme(), "http" | "https") {
                Ok(ResolvedLoad::Remote(parsed))
            } else {
                Err(Error::invalid_argument(
                    "only app assets and http(s) URLs can be loaded",
                ))
            }
        }
    }
}

/// Whether `url` has the same origin as `origin`.
#[must_use]
pub fn same_origin(url: &Url, origin: &Url) -> bool {
    url.scheme() == origin.scheme()
        && url.host_str() == origin.host_str()
        && url.port_or_known_default() == origin.port_or_known_default()
}

/// An RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

/// Parses Electron's `backgroundColor`: `#RGB`, `#RGBA`, `#RRGGBB`,
/// `#RRGGBBAA` or `transparent`.
///
/// ```
/// use tauri_plugin_overwolf::window::options::{parse_color, Rgba};
/// assert_eq!(parse_color("#fff"), Some(Rgba(255, 255, 255, 255)));
/// assert_eq!(parse_color("#11223380"), Some(Rgba(0x11, 0x22, 0x33, 0x80)));
/// assert_eq!(parse_color("transparent"), Some(Rgba(0, 0, 0, 0)));
/// assert_eq!(parse_color("red"), None);
/// ```
#[must_use]
pub fn parse_color(s: &str) -> Option<Rgba> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("transparent") {
        return Some(Rgba(0, 0, 0, 0));
    }
    let hex = s.strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let nib = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok().map(|v| v * 17);
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        3 => Some(Rgba(nib(0)?, nib(1)?, nib(2)?, 255)),
        4 => Some(Rgba(nib(0)?, nib(1)?, nib(2)?, nib(3)?)),
        6 => Some(Rgba(byte(0)?, byte(2)?, byte(4)?, 255)),
        8 => Some(Rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_validation_and_ignored_keys() {
        let o: BrowserWindowOptionsWire = serde_json::from_value(serde_json::json!({
            "width": -1, "kiosk": true, "webPreferences": { "preload": "x", "sandbox": true }
        }))
        .unwrap();
        assert!(o.validate().is_err());
        let mut keys = o.ignored_keys();
        keys.sort();
        assert_eq!(keys, vec!["kiosk", "webPreferences.sandbox"]);
        let ok: BrowserWindowOptionsWire =
            serde_json::from_value(serde_json::json!({ "x": -100, "y": 5 })).unwrap();
        ok.validate().unwrap();
        assert_eq!(ok.size(), DEFAULT_SIZE);
    }

    #[test]
    fn asset_paths() {
        for ok in ["index.html", "/windows/a.html", "a/b/c.js"] {
            assert!(validate_asset_path(ok).is_ok(), "{ok}");
        }
        for bad in ["", "../x", "a/../../x", "C:/x", "https://x", "a\\b", "./a"] {
            assert!(validate_asset_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn loads() {
        let origin = Url::parse("http://tauri.localhost").unwrap();
        let mut q = BTreeMap::new();
        q.insert("a b".to_owned(), "1&2".to_owned());
        let t = LoadTarget::File {
            path: "index.html".into(),
            query: Some(q),
            hash: Some("#h".into()),
        };
        let ResolvedLoad::App(u) = resolve_load(&t, &origin).unwrap() else {
            panic!()
        };
        assert_eq!(u.as_str(), "http://tauri.localhost/index.html?a+b=1%262#h");
        let same = LoadTarget::Url {
            url: "http://tauri.localhost/x.html".into(),
        };
        assert!(matches!(
            resolve_load(&same, &origin).unwrap(),
            ResolvedLoad::App(_)
        ));
        for bad in ["file:///etc/hosts", "javascript:alert(1)", "relative.html"] {
            let t = LoadTarget::Url { url: bad.into() };
            assert!(resolve_load(&t, &origin).is_err(), "{bad}");
        }
        let dev = Url::parse("http://localhost:1420").unwrap();
        assert!(!same_origin(
            &Url::parse("http://localhost:1421/").unwrap(),
            &dev
        ));
        assert!(same_origin(
            &Url::parse("http://localhost:1420/a").unwrap(),
            &dev
        ));
    }

    #[test]
    fn colors() {
        assert_eq!(parse_color("#000000"), Some(Rgba(0, 0, 0, 255)));
        assert_eq!(parse_color("#1234"), Some(Rgba(0x11, 0x22, 0x33, 0x44)));
        assert_eq!(parse_color("#12345"), None);
        assert_eq!(parse_color("#zzzzzz"), None);
    }
}
