//! What ow-electron's harness reads from Electron internals, read here from
//! the outside of the plugin, through Tauri and `WebKit` only:
//!
//! - ad guest documents (`owad-*` webviews): each new document of the ad
//!   page is probed at load (`guest-<n>-dom-ready-<k>.json`, and again 10 s
//!   later), with the probe ow-electron's harness runs at `dom-ready`;
//! - consent page documents (`ow-cmp*` windows): `cmp-pages.jsonl`;
//! - cookies of the default website data store (macOS): `cookie-changes.jsonl`.
//!
//! The plugin's own lab trace (`OW_TAURI_LAB_DIR`) records what it sends
//! and does (`host-requests.jsonl`, `ipc.jsonl`, `wc-events.jsonl`, ...).
//! Probing runs the harness's own script in a guest's main world, as
//! ow-electron's harness does with `executeJavaScript`; it never sends input.

use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};
use tauri::{AppHandle, Manager, Wry};

use crate::harness::{self, lock, truncate};

/// How long an evaluation in a webview may take to answer.
const EVAL_WAIT: Duration = Duration::from_secs(5);

/// How often the documents are checked.
const POLL: Duration = Duration::from_millis(250);

/// How often the cookies are read.
const COOKIE_POLL: Duration = Duration::from_millis(500);

/// Ad guest webview labels.
pub fn is_guest(label: &str) -> bool {
    label.starts_with("owad-")
}

/// Consent window webview labels.
fn is_cmp(label: &str) -> bool {
    label.starts_with("ow-cmp")
}

/// The document a webview shows: its URL, its time origin (a new value is a
/// new document) and whether the DOM is ready.
const DOC_STATE: &str =
    "JSON.stringify([location.href, performance.timeOrigin, document.readyState])";

/// ow-electron's harness `GUEST_PROBE` (`app/main.cjs`), plus the page's
/// resource list (`labResources`, every request of the page so far, which
/// the fill-impression count reads).
pub const GUEST_PROBE: &str = r"(() => {
  const describe = (v, depth) => {
    if (v === null || v === undefined) return v === null ? null : { undefined: true };
    const t = typeof v;
    if (t === 'function') {
      const src = Function.prototype.toString.call(v);
      return { function: true, length: v.length, native: /\[native code\]/.test(src) };
    }
    if (t !== 'object') return v;
    if (depth > 5) return '[depth]';
    if (Array.isArray(v)) return v.map((x) => describe(x, depth + 1));
    const out = {};
    for (const k of Object.getOwnPropertyNames(v)) {
      try { out[k] = describe(v[k], depth + 1); } catch (e) { out[k] = { threw: String(e) }; }
    }
    return out;
  };
  const ow = window.__overwolf__;
  const descriptors = {};
  if (ow) for (const k of Object.getOwnPropertyNames(ow)) {
    const d = Object.getOwnPropertyDescriptor(ow, k);
    descriptors[k] = { writable: d.writable, configurable: d.configurable, enumerable: d.enumerable, accessor: !!(d.get || d.set) };
  }
  const calls = {};
  if (ow) for (const k of ['muid', 'getSystemInformation', 'getCustomTracking']) {
    if (typeof ow[k] === 'function') { try { calls[k] = describe(ow[k](), 0); } catch (e) { calls[k] = { threw: String(e) }; } }
  }
  const top = Object.getOwnPropertyDescriptor(window, '__overwolf__');
  let storage = {};
  try { for (let i = 0; i < localStorage.length; i++) { const k = localStorage.key(i); storage[k] = localStorage.getItem(k); } } catch (e) { storage = { error: String(e) }; }
  let labResources = [];
  try { labResources = performance.getEntriesByType('resource').map((e) => e.name); } catch (e) {}
  return {
    href: location.href, referrer: document.referrer, userAgent: navigator.userAgent,
    visibilityState: document.visibilityState, hasFocus: document.hasFocus(),
    cookie: document.cookie, localStorage: storage,
    gcType: typeof window.gc,
    overwolfPresent: !!ow,
    overwolfWindowDescriptor: top ? { writable: top.writable, configurable: top.configurable, enumerable: top.enumerable, accessor: !!(top.get || top.set) } : null,
    overwolfFrozen: ow ? Object.isFrozen(ow) : null,
    overwolf: describe(ow, 0),
    descriptors, calls,
    innerSize: [innerWidth, innerHeight], devicePixelRatio,
    labResources,
  };
})()";

/// ow-electron's harness `CMP_PROBE`: the globals a consent page sees.
const CMP_PROBE: &str = r"(() => {
  const fnInfo = (o) => {
    if (!o) return null;
    const out = {};
    for (const k of Object.getOwnPropertyNames(o)) {
      const v = o[k];
      out[k] = typeof v === 'function'
        ? { function: true, length: v.length, native: /\[native code\]/.test(Function.prototype.toString.call(v)) }
        : typeof v;
    }
    return out;
  };
  let storage = {};
  try { for (let i = 0; i < localStorage.length; i++) { const k = localStorage.key(i); storage[k] = (localStorage.getItem(k) || '').slice(0, 300); } } catch (e) { storage = { error: String(e) }; }
  return { href: location.href, cmp: fnInfo(window.cmp), privacy: fnInfo(window.privacy),
    overwolf: typeof window.overwolf, closeNative: /\[native code\]/.test(Function.prototype.toString.call(window.close)),
    cookie: document.cookie.slice(0, 2000), localStorage: storage, userAgent: navigator.userAgent };
})()";

/// Evaluates `code` (an expression whose value is JSON text) in every
/// webview whose label passes `filter` and returns `(label, value)` pairs.
/// Blocks for at most [`EVAL_WAIT`]; call off the main thread.
pub fn eval_each(
    app: &AppHandle<Wry>,
    filter: impl Fn(&str) -> bool,
    code: &str,
) -> Vec<(String, Value)> {
    let (tx, rx) = mpsc::channel();
    let mut asked = 0;
    for (label, webview) in app.webviews() {
        if !filter(&label) {
            continue;
        }
        let tx = tx.clone();
        let name = label.clone();
        if webview
            .eval_with_callback(code, move |result| {
                let value = serde_json::from_str::<Value>(&result).unwrap_or(Value::String(result));
                // A JSON string result (JSON.stringify) arrives quoted.
                let value = match value {
                    Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
                    other => other,
                };
                let _ = tx.send((name.clone(), value));
            })
            .is_ok()
        {
            asked += 1;
        }
    }
    drop(tx);
    let mut out = Vec::new();
    while out.len() < asked {
        match rx.recv_timeout(EVAL_WAIT) {
            Ok(v) => out.push(v),
            Err(_) => break,
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Per guest webview label: its number (in order of first load), the time
/// origin of the document last seen and how many documents it loaded.
#[derive(Default)]
struct Guests {
    seen: HashMap<String, GuestDoc>,
    count: u64,
}

struct GuestDoc {
    index: u64,
    origin: f64,
    loads: u64,
}

static GUESTS: std::sync::Mutex<Option<Guests>> = std::sync::Mutex::new(None);

/// Probes guest `label` (`guest-<n>-<phase>.json`) and records the probe.
pub fn probe_guest(app: &AppHandle<Wry>, label: &str, phase: &str) {
    let index = lock(&GUESTS)
        .as_ref()
        .and_then(|g| g.seen.get(label).map(|d| d.index));
    let Some(index) = index else { return };
    let h = harness::get();
    let code = format!("JSON.stringify({GUEST_PROBE})");
    let result = eval_each(app, |l| l == label, &code);
    match result.into_iter().next() {
        Some((_, Value::Object(mut probe))) => {
            let href = probe.get("href").cloned().unwrap_or(Value::Null);
            probe.insert("label".into(), json!(label));
            // Labels name files: anything else than [A-Za-z0-9_.+-] becomes `_`.
            let name: String = phase
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || "-_.+".contains(c) { c } else { '_' })
                .collect();
            h.write_json(&format!("guest-{index}-{name}.json"), &Value::Object(probe));
            h.record(
                "events.jsonl",
                json!({ "kind": "guest-probe", "webContentsId": label, "label": phase, "href": href }),
            );
        }
        other => h.record(
            "events.jsonl",
            json!({ "kind": "guest-probe-failed", "webContentsId": label, "label": phase, "error": format!("{other:?}") }),
        ),
    }
}

/// Probes every guest seen so far.
pub fn probe_all(app: &AppHandle<Wry>, phase: &str) {
    let labels: Vec<String> = lock(&GUESTS)
        .as_ref()
        .map(|g| g.seen.keys().cloned().collect())
        .unwrap_or_default();
    let live: Vec<String> = app.webviews().into_keys().collect();
    for label in labels.iter().filter(|l| live.contains(l)) {
        probe_guest(app, label, phase);
    }
}

/// Whether a URL is the ad page (ow-electron's harness `isAdGuest`).
fn is_ad_page(href: &str) -> bool {
    href.contains("overwolf.com/") && href.to_lowercase().contains("adview")
}

/// One pass over the guests: a new ad page document (ready DOM) is a load
/// (`dom-ready`); it is probed now and 10 s later.
fn check_guests(app: &AppHandle<Wry>) {
    for (label, state) in eval_each(app, is_guest, DOC_STATE) {
        let Some([href, origin, ready]) = state
            .as_array()
            .and_then(|a| <&[Value; 3]>::try_from(a.as_slice()).ok())
        else {
            continue;
        };
        let (Some(href), Some(origin)) = (href.as_str(), origin.as_f64()) else {
            continue;
        };
        if ready.as_str() == Some("loading") || !is_ad_page(href) {
            continue;
        }
        let phase = {
            let mut g = lock(&GUESTS);
            let g = g.get_or_insert_with(Guests::default);
            let next = g.count + 1;
            let doc = g.seen.entry(label.clone()).or_insert(GuestDoc {
                index: next,
                origin: f64::NAN,
                loads: 0,
            });
            if doc.index == next {
                g.count = next;
            }
            if doc.origin.to_bits() == origin.to_bits() {
                continue;
            }
            doc.origin = origin;
            let phase = format!("dom-ready-{}", doc.loads);
            doc.loads += 1;
            phase
        };
        probe_guest(app, &label, &phase);
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(10));
            if app.webviews().contains_key(&label) {
                probe_guest(&app, &label, "after-10s");
            }
        });
    }
}

/// Consent page documents already probed (label -> time origin).
static CMP_DOCS: std::sync::Mutex<BTreeMap<String, u64>> = std::sync::Mutex::new(BTreeMap::new());

/// One pass over the consent windows: each new consent page document is
/// probed once (`cmp-pages.jsonl`).
fn check_cmp(app: &AppHandle<Wry>) {
    for (label, state) in eval_each(app, is_cmp, DOC_STATE) {
        let Some(a) = state.as_array() else { continue };
        let (Some(href), Some(origin)) = (
            a.first().and_then(Value::as_str),
            a.get(1).and_then(Value::as_f64),
        ) else {
            continue;
        };
        if a.get(2).and_then(Value::as_str) == Some("loading") || !href.contains("/cmp/") {
            continue;
        }
        if lock(&CMP_DOCS).insert(label.clone(), origin.to_bits()) == Some(origin.to_bits()) {
            continue;
        }
        let code = format!("JSON.stringify({CMP_PROBE})");
        if let Some((_, Value::Object(probe))) =
            eval_each(app, |l| l == label, &code).into_iter().next()
        {
            let mut entry = serde_json::Map::new();
            entry.insert("webContentsId".into(), json!(label));
            entry.insert("type".into(), json!("cmp"));
            entry.extend(probe);
            harness::get().record("cmp-pages.jsonl", Value::Object(entry));
        }
    }
}

/// Cookies seen last (name, domain, path) -> the record's cookie.
static COOKIES: std::sync::Mutex<BTreeMap<(String, String, String), Value>> =
    std::sync::Mutex::new(BTreeMap::new());

/// Records what changed since the last read, in the shape of ow-electron's
/// `cookies.on('changed')` records (with `wall`, the time of the read).
fn diff_cookies(now: Vec<Value>) {
    let h = harness::get();
    let wall = harness::wall_ms();
    let mut seen = lock(&COOKIES);
    let mut current = BTreeMap::new();
    for mut c in now {
        let key = |k: &str| {
            c.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let id = (key("name"), key("domain"), key("path"));
        if let Some(Value::String(v)) = c.get_mut("value") {
            *v = truncate(v, 300);
        }
        current.insert(id, c);
    }
    for (id, c) in &current {
        let cause = match seen.get(id) {
            None => "inserted",
            Some(old) if old != c => "overwrite",
            Some(_) => continue,
        };
        h.record(
            "cookie-changes.jsonl",
            json!({ "wall": wall, "cause": cause, "removed": false, "cookie": c }),
        );
    }
    for (id, c) in seen.iter() {
        if !current.contains_key(id) {
            h.record(
                "cookie-changes.jsonl",
                json!({ "wall": wall, "cause": "explicit", "removed": true, "cookie": c }),
            );
        }
    }
    *seen = current;
}

/// Reads the cookies once (macOS: the default data store; elsewhere the
/// cookies of an ad guest or consent webview through Tauri, whose domain
/// loses its leading dot).
fn check_cookies(app: &AppHandle<Wry>) {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = mpsc::channel();
        if app
            .run_on_main_thread(move || crate::macos_lab::all_cookies(move |c| drop(tx.send(c))))
            .is_ok()
            && let Ok(cookies) = rx.recv_timeout(EVAL_WAIT)
        {
            diff_cookies(cookies);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let Some(webview) = app
            .webviews()
            .into_iter()
            .find(|(l, _)| is_guest(l) || is_cmp(l))
            .map(|(_, w)| w)
        else {
            return;
        };
        let Ok(cookies) = webview.cookies() else {
            return;
        };
        diff_cookies(
            cookies
                .iter()
                .map(|c| {
                    json!({
                        "name": c.name(),
                        "value": c.value(),
                        "domain": c.domain(),
                        "path": c.path(),
                        "secure": c.secure().unwrap_or(false),
                        "httpOnly": c.http_only().unwrap_or(false),
                        "session": c.expires().is_none_or(|e| e.is_session()),
                        "sameSite": c.same_site().map(|s| s.to_string().to_lowercase()),
                        "expirationDate": c.expires_datetime().map(|d| d.unix_timestamp()),
                    })
                })
                .collect(),
        );
    }
}

/// Starts the observers on their own threads (never the main thread).
pub fn start(app: &AppHandle<Wry>) {
    let docs = app.clone();
    std::thread::spawn(move || {
        loop {
            check_guests(&docs);
            check_cmp(&docs);
            std::thread::sleep(POLL);
        }
    });
    let cookies = app.clone();
    std::thread::spawn(move || {
        loop {
            check_cookies(&cookies);
            std::thread::sleep(COOKIE_POLL);
        }
    });
}
