//! The pure rules of guest hosting (DESIGN §4.4, §4.9): the ads
//! environment's browser arguments, the guest builder's invariants, the
//! local-origin frame guard, the visibility formula, the recreate rate
//! guard and generations, the per-app external-open cap, the `WebView2`
//! floor and the `sessionStorage` carry-over of a recreate.
//!
//! The host (`host/ads.rs`) applies them; every rule here is unit-tested
//! without a webview.

use std::path::PathBuf;

use serde_json::Value;

/// The browser arguments of the ads environment (DESIGN §4.4.2, PAR-M8):
/// exactly ow-electron's ads-environment string. The three throttling
/// switches are guest parity. Consent windows share the environment, so
/// they use the same text (`WebView2` refuses a second environment on the
/// same data folder with other arguments). Any change needs a full Windows
/// lab re-proof (`reward-optin`, `perf-minimize`, `long`).
///
/// ```
/// use tauri_plugin_overwolf::ads::ADS_PARITY_ARGS;
/// assert!(ADS_PARITY_ARGS.starts_with("--disable-features=msWebOOUI"));
/// ```
pub const ADS_PARITY_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows --disable-web-security --allow-running-insecure-content";

/// The browser arguments of the ads environment: [`ADS_PARITY_ARGS`], then
/// `ads.browserArgs` in order.
///
/// ```
/// use tauri_plugin_overwolf::ads::{ADS_PARITY_ARGS, ads_browser_args};
/// assert_eq!(ads_browser_args(&[]), ADS_PARITY_ARGS);
/// assert_eq!(ads_browser_args(&["--lang=de".into()]), format!("{ADS_PARITY_ARGS} --lang=de"));
/// ```
#[must_use]
pub fn ads_browser_args(extra: &[String]) -> String {
    std::iter::once(ADS_PARITY_ARGS)
        .chain(extra.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The URL schemes a document or frame inside an ad guest or consent window
/// may load (DESIGN §4.4.9, SEC-B1). The app's own `tauri:` origin fails
/// this rule; `http(s)://*.localhost` fails [`frame_url_allowed`]'s host rule.
pub const GUEST_FRAME_SCHEMES: [&str; 5] = ["http", "https", "about", "data", "blob"];

/// Whether `host` is `localhost` or ends with `.localhost` (Tauri's
/// `http(s)://<scheme>.localhost` app origins, among them
/// `tauri.localhost`, `asset.localhost` and `ipc.localhost`), ignoring case
/// and a trailing dot.
///
/// ```
/// use tauri_plugin_overwolf::ads::is_localhost_host;
/// assert!(is_localhost_host("localhost"));
/// assert!(is_localhost_host("Tauri.LocalHost."));
/// assert!(!is_localhost_host("localhost.example"));
/// assert!(!is_localhost_host("127.0.0.1"));
/// ```
#[must_use]
pub fn is_localhost_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost" || host.ends_with(".localhost")
}

/// The local-origin frame guard (DESIGN §4.4.9): whether a guest or consent
/// window may load `url` in any frame. The scheme must be one of
/// [`GUEST_FRAME_SCHEMES`], the host must not be `localhost` or
/// `*.localhost`, and the origin must not be one of `blocked_origins` (the
/// app's own origins, such as a debug build's `devUrl`). A frame of the
/// app's origin inside a guest would pass Tauri's ACL as a local page.
///
/// ```
/// use tauri_plugin_overwolf::ads::frame_url_allowed;
/// let ok = |u: &str| frame_url_allowed(&u.parse().unwrap(), &[]);
/// assert!(ok("https://www.overwolf.com/monsdk/electron/latest/adview.html"));
/// assert!(ok("about:blank"));
/// assert!(!ok("tauri://localhost/index.html"));
/// assert!(!ok("http://tauri.localhost/index.html"));
/// assert!(!frame_url_allowed(&"http://127.0.0.1:1430/".parse().unwrap(), &["http://127.0.0.1:1430"]));
/// ```
#[must_use]
pub fn frame_url_allowed(url: &url::Url, blocked_origins: &[&str]) -> bool {
    if !GUEST_FRAME_SCHEMES.contains(&url.scheme()) {
        return false;
    }
    if url.host_str().is_some_and(is_localhost_host) {
        return false;
    }
    let origin = url.origin().ascii_serialization();
    !blocked_origins
        .iter()
        .any(|b| b.eq_ignore_ascii_case(&origin))
}

/// The visibility a guest's document reports (DESIGN §4.4.4, PAR-B1):
/// visible iff its element is visible and its window is neither hidden,
/// minimized nor closing.
///
/// ```
/// use tauri_plugin_overwolf::ads::guest_visible;
/// assert!(guest_visible(true, false, false, false));
/// assert!(!guest_visible(true, false, false, true));
/// ```
#[must_use]
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "the four independent inputs of the formula"
)]
pub fn guest_visible(
    element_visible: bool,
    embedder_hidden: bool,
    embedder_minimized: bool,
    closing_hidden: bool,
) -> bool {
    element_visible && !embedder_hidden && !embedder_minimized && !closing_hidden
}

/// Whether a minimize also sends the guest `window-hidden` after
/// `window-minimized` (D.5): on macOS and Linux, not on Windows (observed in
/// both labs, `perf-minimize`).
pub const MINIMIZE_SENDS_WINDOW_HIDDEN: bool = !cfg!(windows);

/// Whether a minimize also hides the guest webviews natively until the
/// restore (Windows): a minimized ow-electron window has an empty client
/// area there, while a `WebView2` controller keeps rendering (observed).
pub const MINIMIZE_HIDES_NATIVELY: bool = cfg!(windows);

/// The native visibility a minimize or restore gives a guest webview, or
/// `None` to leave it: hidden on minimize; on restore shown again unless
/// the app hid the element meanwhile (`visible`).
///
/// ```
/// use tauri_plugin_overwolf::ads::native_visibility_on_minimize;
/// assert_eq!(native_visibility_on_minimize(true, true), Some(false));
/// assert_eq!(native_visibility_on_minimize(false, false), None);
/// ```
#[must_use]
pub fn native_visibility_on_minimize(minimized: bool, visible: bool) -> Option<bool> {
    if minimized {
        Some(false)
    } else {
        visible.then_some(true)
    }
}

/// How a window's close hides its guests' documents first (DESIGN §4.4.5,
/// W0c ruling 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseHide {
    /// macOS: at `WindowEvent::Destroyed`, through the plugin's own retained
    /// `WKWebView` handles (Tauri's `eval` no longer runs then). Covers
    /// `close()` and `destroy()`; a prevented close never gets there.
    AtDestroyed,
    /// Windows: at `CloseRequested` with the `closing_hidden` flag, cleared
    /// after [`CLOSE_GRACE_MS`] when the window is still alive, shown and not
    /// minimized (a prevented close).
    AtCloseRequested,
}

/// This platform's [`CloseHide`].
pub const CLOSE_HIDE: CloseHide = if cfg!(target_os = "macos") {
    CloseHide::AtDestroyed
} else {
    CloseHide::AtCloseRequested
};

/// The grace after a `CloseRequested` before a still-open window's guests
/// are shown again ([`CloseHide::AtCloseRequested`]).
pub const CLOSE_GRACE_MS: u64 = 1_000;

/// The recreate rate guard of one guest (DESIGN §4.4.6.2, R9): a reload
/// sooner than `min_interval_ms` after the guest's last recreate, or beyond
/// `max_per_hour` recreates in the last hour, reloads in place instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecreateLimiter {
    min_interval_ms: u64,
    max_per_hour: u32,
    recreates: Vec<u64>,
}

/// One hour in milliseconds.
const HOUR_MS: u64 = 3_600_000;

impl RecreateLimiter {
    /// A guard with no recreate yet.
    #[must_use]
    pub fn new(min_interval_ms: u64, max_per_hour: u32) -> Self {
        RecreateLimiter {
            min_interval_ms,
            max_per_hour,
            recreates: Vec::new(),
        }
    }

    /// Whether a recreate may run at `now_ms`; if so it is counted.
    ///
    /// ```
    /// use tauri_plugin_overwolf::ads::RecreateLimiter;
    /// let mut l = RecreateLimiter::new(30_000, 30);
    /// assert!(l.try_recreate(1_000));
    /// assert!(!l.try_recreate(20_000)); // in place: under 30 s
    /// assert!(l.try_recreate(31_000));
    /// ```
    pub fn try_recreate(&mut self, now_ms: u64) -> bool {
        self.recreates
            .retain(|&t| now_ms.saturating_sub(t) < HOUR_MS);
        let too_soon = self
            .recreates
            .last()
            .is_some_and(|&t| now_ms.saturating_sub(t) < self.min_interval_ms);
        let capped = self.recreates.len() >= self.max_per_hour as usize;
        if too_soon || capped {
            return false;
        }
        self.recreates.push(now_ms);
        true
    }

    /// Takes back the last counted recreate: one that reloaded in place
    /// after all (a `sessionStorage` too large to carry, ruling 8) spends
    /// neither the hourly budget nor the interval.
    ///
    /// ```
    /// use tauri_plugin_overwolf::ads::RecreateLimiter;
    /// let mut l = RecreateLimiter::new(30_000, 30);
    /// assert!(l.try_recreate(1_000));
    /// l.forget_last();
    /// assert!(l.try_recreate(2_000));
    /// ```
    pub fn forget_last(&mut self) {
        self.recreates.pop();
    }
}

/// The native webview generations of one guest (DESIGN §4.4.6.3): a
/// recreate retires the current instance, and events from it are dropped
/// until the new instance starts its first load. Timers and callbacks keep
/// the generation they were started under and are dropped when it is no
/// longer the current one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generations {
    current: u64,
    live: bool,
}

impl Default for Generations {
    fn default() -> Self {
        Generations {
            current: 1,
            live: true,
        }
    }
}

impl Generations {
    /// The current generation.
    #[must_use]
    pub fn current(&self) -> u64 {
        self.current
    }

    /// Retires the current instance: a new generation that does not accept
    /// events yet. Returns it.
    ///
    /// ```
    /// use tauri_plugin_overwolf::ads::Generations;
    /// let mut g = Generations::default();
    /// let old = g.current();
    /// let new = g.retire();
    /// assert!(!g.admits());
    /// assert!(!g.is_current(old));
    /// assert!(g.go_live(new));
    /// assert!(g.admits());
    /// ```
    pub fn retire(&mut self) -> u64 {
        self.current += 1;
        self.live = false;
        self.current
    }

    /// The instance of `generation` started its first load: its events are
    /// accepted from now on. `false` (and no change) for a stale generation.
    pub fn go_live(&mut self, generation: u64) -> bool {
        if generation != self.current {
            return false;
        }
        self.live = true;
        true
    }

    /// Whether events of the guest are accepted now.
    #[must_use]
    pub fn admits(&self) -> bool {
        self.live
    }

    /// Whether `generation` is the current one.
    #[must_use]
    pub fn is_current(&self, generation: u64) -> bool {
        generation == self.current
    }
}

/// The per-app external-open cap (DESIGN §4.9:
/// `guestLimits.externalOpensPerMinuteApp`): at most `per_minute` opens per
/// minute across every guest of the app, a budget that survives guest
/// recreation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppOpenCap {
    per_minute: u32,
    opens: Vec<u64>,
}

impl AppOpenCap {
    /// A cap with no opens yet.
    #[must_use]
    pub fn new(per_minute: u32) -> Self {
        AppOpenCap {
            per_minute,
            opens: Vec::new(),
        }
    }

    /// Whether one more open fits at `now_ms` (nothing is counted).
    #[must_use]
    pub fn allows(&self, now_ms: u64) -> bool {
        self.opens
            .iter()
            .filter(|&&t| now_ms.saturating_sub(t) < 60_000)
            .count()
            < self.per_minute as usize
    }

    /// Counts one open at `now_ms`.
    pub fn record(&mut self, now_ms: u64) {
        self.opens.retain(|&t| now_ms.saturating_sub(t) < 60_000);
        self.opens.push(now_ms);
    }
}

/// The oldest `WebView2` Runtime the ads support (W0c ruling 5): the
/// version that brought `ICoreWebView2Frame2`, which the per-frame guard of
/// §4.4.9 needs. Below it ads are `unsupported`.
pub const WEBVIEW2_MINIMUM: [u32; 4] = [98, 0, 1108, 44];

/// Whether the `WebView2` Runtime `version` (`tauri::webview_version()`,
/// `<major>.<minor>.<build>.<patch>`, an optional channel suffix after a
/// space) is at least [`WEBVIEW2_MINIMUM`]. An unreadable version is not.
///
/// ```
/// use tauri_plugin_overwolf::ads::webview2_supported;
/// assert!(webview2_supported("153.0.4234.48"));
/// assert!(webview2_supported("98.0.1108.44"));
/// assert!(!webview2_supported("98.0.1108.43"));
/// assert!(!webview2_supported(""));
/// ```
#[must_use]
pub fn webview2_supported(version: &str) -> bool {
    let numbers: Option<Vec<u32>> = version
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .split('.')
        .map(|p| p.parse().ok())
        .collect();
    let Some(numbers) = numbers else {
        return false;
    };
    if numbers.len() != 4 {
        return false;
    }
    numbers.as_slice() >= WEBVIEW2_MINIMUM.as_slice()
}

/// The next free guest label (SEC-m2): `owad-<n>` for the first `n` after
/// `last` whose label `taken` does not report (another webview or window
/// may already use it). Returns `n` and the label.
///
/// ```
/// use tauri_plugin_overwolf::ads::next_guest_label;
/// assert_eq!(next_guest_label(0, |_| false), (1, "owad-1".to_owned()));
/// assert_eq!(next_guest_label(0, |l| l == "owad-1"), (2, "owad-2".to_owned()));
/// ```
pub fn next_guest_label(last: u32, taken: impl Fn(&str) -> bool) -> (u32, String) {
    let mut n = last;
    loop {
        n = n.wrapping_add(1).max(1);
        let label = super::guest_label(n);
        if !taken(&label) {
            return (n, label);
        }
    }
}

/// The guest webview builder's settings (DESIGN §4.4.1). Mount, recreate
/// and crash recovery build every guest from one of these, and the host
/// maps each field onto Tauri's `WebviewBuilder`; the unit tests pin the
/// invariants.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one field per independent builder setting of DESIGN §4.4.1"
)]
pub struct GuestBuilderSpec {
    /// `owad-<n>`.
    pub label: String,
    /// The first document: `about:blank` (the host starts the ad document's
    /// shaped load itself, D.6.5, D.8.3).
    pub url: &'static str,
    /// Never focused at creation (SPA F2, SEC-M6): a focused child takes the
    /// keyboard from the app mid-typing.
    pub focused: bool,
    /// `ads.transparentGuests`.
    pub transparent: bool,
    /// `<UA>` (DESIGN §4.10).
    pub user_agent: String,
    /// The initialization scripts, in order: the guest shim with its
    /// configuration, then (a recreate only) the one-shot
    /// `sessionStorage` restore.
    pub init_scripts: Vec<String>,
    /// Page zoom hotkeys: always off.
    pub zoom_hotkeys: bool,
    /// The page starts muted (the host mutes it right after creation and on
    /// every load, as ow-electron does).
    pub muted_at_start: bool,
    /// Windows only: the ads environment (its data folder and browser
    /// arguments).
    pub environment: Option<(PathBuf, String)>,
}

/// The [`GuestBuilderSpec`] of guest `label` (DESIGN §4.4.1).
///
/// ```
/// use tauri_plugin_overwolf::ads::guest_builder_spec;
/// let spec = guest_builder_spec("owad-1", true, "UA", vec!["shim".into()], std::path::Path::new("/x"), &[]);
/// assert!(!spec.focused && !spec.zoom_hotkeys && spec.muted_at_start);
/// assert_eq!(spec.url, "about:blank");
/// ```
#[must_use]
pub fn guest_builder_spec(
    label: &str,
    transparent: bool,
    user_agent: &str,
    init_scripts: Vec<String>,
    ads_data_dir: &std::path::Path,
    browser_args: &[String],
) -> GuestBuilderSpec {
    GuestBuilderSpec {
        label: label.to_owned(),
        url: "about:blank",
        focused: false,
        transparent,
        user_agent: user_agent.to_owned(),
        init_scripts,
        zoom_hotkeys: false,
        muted_at_start: true,
        environment: cfg!(windows)
            .then(|| (ads_data_dir.to_path_buf(), ads_browser_args(browser_args))),
    }
}

/// The largest `sessionStorage` snapshot a recreate carries, in UTF-16
/// code units of its JSON text (W0c ruling 8): larger storage is never
/// truncated; the reload then runs in place instead.
pub const MAX_SNAPSHOT_UNITS: usize = 2_000_000;

/// How long a recreate waits for the `sessionStorage` snapshot (DESIGN
/// §4.4.6.1).
pub const SNAPSHOT_WAIT_MS: u64 = 200;

/// The origin whose `sessionStorage` a recreate carries.
pub const SNAPSHOT_ORIGIN: &str = "https://www.overwolf.com";

/// The token `session-restore.js` carries in place of the snapshot.
pub const SESSION_SNAPSHOT_TOKEN: &str = "/*__OW_TAURI_SESSION_SNAPSHOT__*/null";

/// The comment the host puts first in the restore prelude, so the prelude
/// can be found among the webview's user scripts and removed after the
/// recreated document loaded (one-shot, W0c ruling 8).
pub const SESSION_RESTORE_MARKER: &str = "/*ow-tauri:session-restore*/";

/// The script that reads the top frame's `sessionStorage` as JSON text when
/// the frame is on [`SNAPSHOT_ORIGIN`]: `null` elsewhere,
/// `"TOO_BIG:<units>"` above [`MAX_SNAPSHOT_UNITS`].
#[must_use]
pub fn snapshot_script() -> String {
    format!(
        "(function(){{try{{if(window.top!==window||location.origin!=={origin})return null;var o={{}};for(var i=0;i<sessionStorage.length;i++){{var k=sessionStorage.key(i);o[k]=sessionStorage.getItem(k);}}var s=JSON.stringify(o);return s.length>{max}?'TOO_BIG:'+s.length:s;}}catch(e){{return null;}}}})()",
        origin = Value::from(SNAPSHOT_ORIGIN),
        max = MAX_SNAPSHOT_UNITS,
    )
}

/// What a recreate does with the snapshot it read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Snapshot {
    /// Carry this JSON object text into the new webview.
    Carry(String),
    /// Nothing to carry (another origin, an empty storage, no answer in
    /// time, or an unreadable answer): recreate without a prelude.
    Nothing,
    /// The storage is larger than [`MAX_SNAPSHOT_UNITS`]: reload in place.
    TooBig,
}

/// Classifies the answer of [`snapshot_script`] (`None`: no answer).
///
/// ```
/// use tauri_plugin_overwolf::ads::{snapshot_outcome, Snapshot};
/// assert_eq!(snapshot_outcome(Some(r#"{"a":"1"}"#)), Snapshot::Carry(r#"{"a":"1"}"#.into()));
/// assert_eq!(snapshot_outcome(Some("{}")), Snapshot::Nothing);
/// assert_eq!(snapshot_outcome(Some("TOO_BIG:2000001")), Snapshot::TooBig);
/// assert_eq!(snapshot_outcome(None), Snapshot::Nothing);
/// ```
#[must_use]
pub fn snapshot_outcome(answer: Option<&str>) -> Snapshot {
    let Some(text) = answer else {
        return Snapshot::Nothing;
    };
    if text.starts_with("TOO_BIG:") || text.encode_utf16().count() > MAX_SNAPSHOT_UNITS {
        return Snapshot::TooBig;
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) if !m.is_empty() => Snapshot::Carry(text.to_owned()),
        _ => Snapshot::Nothing,
    }
}

/// The one-shot restore prelude: `restore_js` (`session-restore.js`) with
/// the snapshot spliced in as a JSON string literal (parsed by the prelude,
/// never evaluated), after [`SESSION_RESTORE_MARKER`]. `None` when the
/// script lacks its token.
///
/// ```
/// use tauri_plugin_overwolf::ads::{restore_prelude, SESSION_RESTORE_MARKER};
/// let p = restore_prelude("f(/*__OW_TAURI_SESSION_SNAPSHOT__*/null)", r#"{"k":"</script>"}"#).unwrap();
/// assert!(p.starts_with(SESSION_RESTORE_MARKER));
/// assert!(p.contains(r#"f("{\"k\":\"\u003c/script>\"}")"#));
/// ```
#[must_use]
pub fn restore_prelude(restore_js: &str, snapshot_json: &str) -> Option<String> {
    let spliced = super::splice_config(
        restore_js,
        SESSION_SNAPSHOT_TOKEN,
        &Value::from(snapshot_json),
    )?;
    Some(format!("{SESSION_RESTORE_MARKER}{spliced}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DESIGN §4.4.2: the string is pinned; a change needs a Windows lab
    /// re-proof.
    #[test]
    fn the_ads_parity_arguments_are_pinned() {
        assert_eq!(
            ADS_PARITY_ARGS,
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
             --disable-background-timer-throttling --disable-renderer-backgrounding \
             --disable-backgrounding-occluded-windows --disable-web-security \
             --allow-running-insecure-content"
        );
        assert_eq!(
            ads_browser_args(&["--a".into(), "--b".into()]),
            format!("{ADS_PARITY_ARGS} --a --b")
        );
    }

    /// DESIGN §4.4.1: the builder invariants.
    #[test]
    fn the_guest_builder_invariants_are_pinned() {
        let dir = std::path::Path::new("ads-data");
        let spec = guest_builder_spec(
            "owad-7",
            true,
            "Mozilla/5.0 UA",
            vec!["shim".into(), "prelude".into()],
            dir,
            &["--lang=de".into()],
        );
        assert_eq!(spec.label, "owad-7");
        assert_eq!(spec.url, "about:blank");
        assert!(
            !spec.focused,
            "a focused guest takes the app's keyboard (SPA F2)"
        );
        assert!(spec.transparent);
        assert_eq!(spec.user_agent, "Mozilla/5.0 UA");
        assert_eq!(spec.init_scripts, ["shim", "prelude"]);
        assert!(!spec.zoom_hotkeys);
        assert!(spec.muted_at_start);
        if cfg!(windows) {
            assert_eq!(
                spec.environment,
                Some((dir.to_path_buf(), format!("{ADS_PARITY_ARGS} --lang=de")))
            );
        } else {
            assert_eq!(spec.environment, None);
        }
        let opaque = guest_builder_spec("owad-8", false, "UA", Vec::new(), dir, &[]);
        assert!(!opaque.transparent && !opaque.focused);
    }

    /// SEC-m2: labels already taken are skipped.
    #[test]
    fn guest_labels_skip_taken_ones() {
        let taken = ["owad-3", "owad-4"];
        assert_eq!(
            next_guest_label(2, |l| taken.contains(&l)),
            (5, "owad-5".to_owned())
        );
        assert_eq!(
            next_guest_label(u32::MAX, |_| false),
            (1, "owad-1".to_owned())
        );
    }

    /// DESIGN §4.4.9: the local-frame invariant table.
    #[test]
    fn the_local_frame_invariant_table() {
        let blocked = ["http://127.0.0.1:1430"];
        let cases: &[(&str, bool)] = &[
            (
                "https://www.overwolf.com/monsdk/electron/latest/adview.html",
                true,
            ),
            ("http://ads.example/frame.html", true),
            ("https://securepubads.g.doubleclick.net/x", true),
            ("about:blank", true),
            ("about:srcdoc", true),
            ("data:text/html,<p>x</p>", true),
            ("blob:https://www.overwolf.com/1234", true),
            ("http://127.0.0.1:8080/fixture.html", true),
            ("tauri://localhost/index.html", false),
            ("http://tauri.localhost/index.html", false),
            ("https://tauri.localhost/", false),
            ("http://asset.localhost/C:/secret.txt", false),
            ("http://ipc.localhost/plugin%3Aoverwolf%7Cget_info", false),
            ("http://localhost:1420/", false),
            ("http://LOCALHOST./", false),
            ("https://app.localhost/", false),
            ("http://127.0.0.1:1430/index.html", false),
            ("file:///etc/passwd", false),
            ("asset://localhost/x", false),
            ("javascript:alert(1)", false),
            ("ftp://ads.example/x", false),
            ("ws://ads.example/", false),
        ];
        for (url, want) in cases {
            let parsed: url::Url = url.parse().unwrap();
            assert_eq!(frame_url_allowed(&parsed, &blocked), *want, "{url}");
        }
    }

    /// DESIGN §4.4.4: the visibility formula, `closing_hidden` included.
    #[test]
    fn the_visibility_formula() {
        for bits in 0_u8..16 {
            let (e, h, m, c) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
            assert_eq!(guest_visible(e, h, m, c), e && !h && !m && !c, "{bits:04b}");
        }
        assert_eq!(MINIMIZE_SENDS_WINDOW_HIDDEN, !cfg!(windows));
        assert_eq!(MINIMIZE_HIDES_NATIVELY, cfg!(windows));
        assert_eq!(native_visibility_on_minimize(true, false), Some(false));
        assert_eq!(native_visibility_on_minimize(false, true), Some(true));
        assert_eq!(
            CLOSE_HIDE,
            if cfg!(target_os = "macos") {
                CloseHide::AtDestroyed
            } else {
                CloseHide::AtCloseRequested
            }
        );
    }

    /// DESIGN §4.4.6.2: the interval and the hourly cap.
    #[test]
    fn the_recreate_rate_guard() {
        let mut l = RecreateLimiter::new(30_000, 3);
        assert!(l.try_recreate(0));
        assert!(!l.try_recreate(29_999));
        assert!(l.try_recreate(30_000));
        assert!(l.try_recreate(60_000));
        // The hourly cap: three in the last hour.
        assert!(!l.try_recreate(90_000));
        assert!(!l.try_recreate(3_599_999));
        // The first one left the hour.
        assert!(l.try_recreate(3_600_000));
        let mut never = RecreateLimiter::new(0, 0);
        assert!(!never.try_recreate(0));
        // A recreate taken back spends nothing.
        let mut back = RecreateLimiter::new(30_000, 1);
        assert!(back.try_recreate(0));
        back.forget_last();
        assert!(back.try_recreate(1));
        assert!(!back.try_recreate(30_001), "the hourly cap of one");
        back.forget_last();
        back.forget_last();
        assert!(back.try_recreate(2), "taking back nothing is harmless");
    }

    /// DESIGN §4.4.6.3: generation ids.
    #[test]
    fn generations_drop_events_of_a_retired_instance() {
        let mut g = Generations::default();
        assert!(g.admits());
        let first = g.current();
        let second = g.retire();
        assert_ne!(first, second);
        assert!(!g.admits(), "the old instance's events are dropped");
        assert!(!g.go_live(first), "a stale instance never goes live");
        assert!(!g.admits());
        let third = g.retire();
        assert!(!g.go_live(second));
        assert!(g.go_live(third));
        assert!(g.admits());
        assert!(g.is_current(third) && !g.is_current(second));
    }

    #[test]
    fn the_app_open_cap() {
        let mut cap = AppOpenCap::new(2);
        assert!(cap.allows(0));
        cap.record(0);
        cap.record(10);
        assert!(!cap.allows(59_999));
        assert!(cap.allows(60_010));
    }

    #[test]
    fn the_webview2_floor() {
        assert!(webview2_supported("98.0.1108.44"));
        assert!(webview2_supported("98.0.1109.0"));
        assert!(webview2_supported("130.0.2849.80 beta"));
        assert!(!webview2_supported("97.0.1072.54"));
        assert!(!webview2_supported("98.0.1108"));
        assert!(!webview2_supported("x.y"));
    }

    #[test]
    fn snapshots() {
        let script = snapshot_script();
        assert!(script.contains(r#"location.origin!=="https://www.overwolf.com""#));
        assert!(script.contains("2000000"));
        let big = format!(r#"{{"k":"{}"}}"#, "x".repeat(MAX_SNAPSHOT_UNITS));
        assert_eq!(snapshot_outcome(Some(&big)), Snapshot::TooBig);
        assert_eq!(snapshot_outcome(Some("[1]")), Snapshot::Nothing);
        assert_eq!(snapshot_outcome(Some("not json")), Snapshot::Nothing);
        let restore = include_str!("../../js/session-restore.js");
        let prelude = restore_prelude(restore, r#"{"a":"b"}"#).unwrap();
        assert!(prelude.starts_with(SESSION_RESTORE_MARKER));
        assert!(!prelude.contains(SESSION_SNAPSHOT_TOKEN));
        assert!(prelude.contains(r#""{\"a\":\"b\"}""#));
        assert!(restore_prelude("no token", "{}").is_none());
    }
}
