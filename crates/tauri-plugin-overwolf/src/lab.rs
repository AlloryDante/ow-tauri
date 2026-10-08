//! Lab mode: a trace of what the host sends and does, and invisible
//! windows, for the Tauri edition of the parity harness
//! (`tools/parity-harness/tauri-app`).
//!
//! It exists only with the Cargo feature `lab` (off by default; never enable
//! it in a shipped app). With the feature, nothing happens unless the
//! environment asks for it:
//!
//! - `OW_TAURI_LAB_DIR=<dir>` turns the trace on. The plugin appends JSON
//!   lines to files in `<dir>`, in the shapes the ow-electron harness writes,
//!   so the harness's `analyze.mjs` and `parity-diff.mjs` read both:
//!   - `host-requests.jsonl`: every host request (analytics, `cmp-eu-only`)
//!     as it leaves the plugin: method, URL, headers in order (with the
//!     final user agent), the names of any `cookie` header (none: host
//!     requests carry no cookies, as in ow-electron (observed)), body; then
//!     its status and the `Set-Cookie` names it was answered with.
//!   - `shaped-requests.jsonl`: each ad document navigation and the header
//!     fields the plugin puts on it (macOS and Linux: the load request;
//!     Windows: the request handler).
//!   - `wc-events.jsonl`: ad guest and consent window lifecycle (created,
//!     navigation, page load, crash, load failure, reload, close), each
//!     guest move or resize (`bounds`), and the guests' native state
//!     (`transparent`, `zorder`, `passthrough`).
//!   - `ipc.jsonl`: host-to-guest messages and calls, guest-to-host events,
//!     the element events the host dispatches, and each guest mute change
//!     (`set-muted`, with its cause).
//!   - `cookie-changes.jsonl`: changes of the ads data store's cookies
//!     (macOS: polled every 250 ms), with their attributes.
//!   - `state-writes.jsonl`: every state file write, with its bytes.
//!   - `guest-<n>-dom-ready-<k>.json`: what each ad guest's page sees after
//!     each load (`window.__overwolf__`, referrer, user agent, cookies).
//!   - `windows.jsonl`: every window the plugin builds and how it is shown.
//! - `OW_TAURI_LAB_INVISIBLE=1` makes every window the plugin builds
//!   invisible before it can appear: built hidden, not focusable and off the
//!   taskbar; then, on macOS, alpha 0 and click-through; then shown when the
//!   app asked for a visible window. The plugin never focuses a window and
//!   never builds a full-screen one in this mode. On macOS the app is also
//!   never activated, for the whole run and whoever asks: `NSApplication`
//!   activation does nothing and `makeKeyAndOrderFront:` orders the window
//!   front without making it key (`orderFrontRegardless`), so a later
//!   `BrowserWindow.show()` or `focus()` (the core window commands) and the
//!   ad privacy window keep the app in the background (`activation-suppressed`
//!   and `key-front-redirected` in `wc-events.jsonl`). On other platforms the
//!   windows stay hidden. Native dialogs, the file manager
//!   (`shell.openPath`, `showItemInFolder`) and the system browser do not
//!   open either: the command answers as if the user dismissed the dialog at
//!   once (or as if the OS opened the item) and records the request in
//!   `blocked.jsonl`.
//! - `OW_TAURI_LAB_CMP_EU_ONLY=<body>` answers the startup `cmp-eu-only`
//!   request with HTTP 200 and `<body>` instead of sending it, as the
//!   ow-electron harness's `--features` stand-in answers ow-electron's; the
//!   answer is recorded in `host-requests.jsonl` (`stubbed: true`).
//!
//! Without the feature every function here is an empty inline function.

use serde_json::Value;

/// Whether the trace is on (`OW_TAURI_LAB_DIR` is set, feature `lab`).
#[cfg_attr(
    not(feature = "plugin"),
    expect(dead_code, reason = "only plugin code asks")
)]
#[must_use]
pub(crate) fn trace_on() -> bool {
    imp::trace_on()
}

/// Appends one JSON line to `file` in the trace directory, adding `t` (ms
/// since the trace started) and `wall` (Unix ms). `entry` runs only when
/// the trace is on.
pub(crate) fn record(file: &str, entry: impl FnOnce() -> Value) {
    imp::record(file, entry);
}

/// Writes `value` as a pretty JSON file in the trace directory.
#[cfg(all(feature = "lab", feature = "plugin"))]
pub(crate) fn write_json(file: &str, value: impl FnOnce() -> Value) {
    imp::write_json(file, value);
}

/// A process-wide sequence number for trace records (request ids, guest
/// numbers).
#[cfg_attr(
    not(feature = "plugin"),
    expect(dead_code, reason = "only plugin code numbers records")
)]
#[must_use]
pub(crate) fn next_id() -> u64 {
    imp::next_id()
}

/// Whether lab windows are invisible (`OW_TAURI_LAB_INVISIBLE=1`, feature
/// `lab`).
#[cfg_attr(
    not(any(feature = "plugin", test)),
    expect(dead_code, reason = "only plugin code builds windows")
)]
#[must_use]
pub(crate) fn invisible() -> bool {
    imp::invisible()
}

/// The body that answers the startup `cmp-eu-only` request instead of the
/// network (`OW_TAURI_LAB_CMP_EU_ONLY`, feature `lab`); `None` outside the
/// lab. Recorded in `host-requests.jsonl` when used.
#[cfg_attr(
    not(feature = "plugin"),
    expect(dead_code, reason = "only the consent host asks")
)]
#[must_use]
pub(crate) fn cmp_eu_only_stub() -> Option<Vec<u8>> {
    let body = imp::cmp_eu_only_stub()?;
    record("host-requests.jsonl", || {
        serde_json::json!({
            "phase": "stubbed",
            "url": "https://features.overwolf.com/experiments/cmp-eu-only",
            "stubbed": true,
            "status": 200,
            "responseBody": String::from_utf8_lossy(&body),
        })
    });
    Some(body)
}

/// Lab windows are invisible, so nothing else may appear either: returns
/// `true` (and records `{ kind, detail }` in `blocked.jsonl`) when an OS
/// surface the app asked for (a file or message dialog, the file manager,
/// the system browser) must not open. The caller then answers as the OS
/// would when the user dismisses it at once. Always `false` outside the lab.
#[cfg_attr(
    not(any(feature = "plugin", test)),
    expect(dead_code, reason = "only plugin commands open OS surfaces")
)]
#[must_use]
pub(crate) fn block_os_surface(kind: &str, detail: impl FnOnce() -> Value) -> bool {
    if !invisible() {
        return false;
    }
    record("blocked.jsonl", || {
        let mut entry = serde_json::Map::new();
        entry.insert("kind".into(), Value::from(kind));
        entry.insert("detail".into(), detail());
        Value::Object(entry)
    });
    true
}

/// Whether the plugin may focus a window (always, unless lab windows are
/// invisible).
#[cfg_attr(
    not(any(feature = "plugin", test)),
    expect(dead_code, reason = "only plugin code focuses windows")
)]
#[must_use]
pub(crate) fn may_focus() -> bool {
    !invisible()
}

/// The names of the cookies in a `cookie` header value, in order.
#[cfg_attr(
    not(any(feature = "plugin", test)),
    expect(dead_code, reason = "only the host request trace lists cookie names")
)]
pub(crate) fn cookie_names(header: &str) -> Vec<String> {
    header
        .split(';')
        .filter_map(|pair| {
            let name = pair.split('=').next()?.trim();
            (!name.is_empty()).then(|| name.to_owned())
        })
        .collect()
}

#[cfg(feature = "plugin")]
pub(crate) use cookies::start;
#[cfg(all(feature = "lab", feature = "plugin"))]
pub(crate) use pages::probe_guests;
#[cfg(feature = "plugin")]
pub(crate) use pages::{navigation, page_load};
#[cfg(feature = "plugin")]
pub(crate) use window_guard::{after_build, window_builder};

#[cfg(feature = "lab")]
mod imp {
    use std::collections::HashMap;
    use std::fs::{File, OpenOptions};
    use std::io::Write as _;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock, PoisonError};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    use serde_json::Value;

    struct Trace {
        dir: PathBuf,
        t0: Instant,
        files: Mutex<HashMap<String, File>>,
    }

    static TRACE: OnceLock<Option<Trace>> = OnceLock::new();
    static INVISIBLE: OnceLock<bool> = OnceLock::new();
    static IDS: AtomicU64 = AtomicU64::new(1);

    fn trace() -> Option<&'static Trace> {
        TRACE
            .get_or_init(|| {
                let dir = std::env::var_os("OW_TAURI_LAB_DIR").filter(|d| !d.is_empty())?;
                let dir = PathBuf::from(dir);
                std::fs::create_dir_all(&dir).ok()?;
                Some(Trace {
                    dir,
                    t0: Instant::now(),
                    files: Mutex::new(HashMap::new()),
                })
            })
            .as_ref()
    }

    /// Only plain file names: no directories, nothing hidden.
    fn safe_name(file: &str) -> bool {
        !file.is_empty()
            && !file.starts_with('.')
            && file
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    }

    fn wall_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }

    pub(super) fn trace_on() -> bool {
        trace().is_some()
    }

    pub(super) fn record(file: &str, entry: impl FnOnce() -> Value) {
        let Some(trace) = trace() else { return };
        if !safe_name(file) {
            return;
        }
        let t = u64::try_from(trace.t0.elapsed().as_millis()).unwrap_or(u64::MAX);
        let mut line = serde_json::Map::new();
        line.insert("t".into(), Value::from(t));
        line.insert("wall".into(), Value::from(wall_ms()));
        match entry() {
            Value::Object(m) => line.extend(m),
            other => {
                line.insert("value".into(), other);
            }
        }
        let Ok(mut text) = serde_json::to_string(&Value::Object(line)) else {
            return;
        };
        text.push('\n');
        let mut files = trace.files.lock().unwrap_or_else(PoisonError::into_inner);
        if !files.contains_key(file) {
            let Ok(f) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(trace.dir.join(file))
            else {
                return;
            };
            files.insert(file.to_owned(), f);
        }
        if let Some(f) = files.get_mut(file) {
            let _ = f.write_all(text.as_bytes());
        }
    }

    pub(super) fn write_json(file: &str, value: impl FnOnce() -> Value) {
        let Some(trace) = trace() else { return };
        if !safe_name(file) {
            return;
        }
        if let Ok(mut text) = serde_json::to_string_pretty(&value()) {
            text.push('\n');
            let _ = std::fs::write(trace.dir.join(file), text);
        }
    }

    pub(super) fn next_id() -> u64 {
        IDS.fetch_add(1, Ordering::Relaxed)
    }

    pub(super) fn invisible() -> bool {
        *INVISIBLE
            .get_or_init(|| std::env::var("OW_TAURI_LAB_INVISIBLE").is_ok_and(|v| v.trim() == "1"))
    }

    pub(super) fn cmp_eu_only_stub() -> Option<Vec<u8>> {
        std::env::var("OW_TAURI_LAB_CMP_EU_ONLY")
            .ok()
            .filter(|b| !b.is_empty())
            .map(String::into_bytes)
    }

    #[cfg(test)]
    mod tests {
        use super::safe_name;

        #[test]
        fn only_plain_file_names() {
            assert!(safe_name("host-requests.jsonl"));
            assert!(safe_name("guest-1-dom-ready-0.json"));
            assert!(!safe_name("../x.jsonl"));
            assert!(!safe_name("a/b.jsonl"));
            assert!(!safe_name(".hidden"));
            assert!(!safe_name(""));
        }
    }
}

#[cfg(not(feature = "lab"))]
mod imp {
    use serde_json::Value;

    #[inline]
    pub(super) fn trace_on() -> bool {
        false
    }

    #[inline]
    pub(super) fn record(_file: &str, _entry: impl FnOnce() -> Value) {}

    #[inline]
    pub(super) fn next_id() -> u64 {
        0
    }

    #[inline]
    pub(super) fn invisible() -> bool {
        false
    }

    #[inline]
    pub(super) fn cmp_eu_only_stub() -> Option<Vec<u8>> {
        None
    }
}

#[cfg(feature = "plugin")]
mod cookies {
    use tauri::{AppHandle, Runtime};

    /// Starts the lab's cookie watcher (macOS, trace on): polls the ads
    /// data store every 250 ms and writes each change to
    /// `cookie-changes.jsonl` (`cause` `inserted`, `overwrite` or
    /// `deleted`), and the latest jar to `cookies-end.json`.
    pub(crate) fn start<R: Runtime>(app: &AppHandle<R>) {
        if !super::trace_on() {
            return;
        }
        #[cfg(all(target_os = "macos", feature = "lab"))]
        {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                watch(app).await;
            });
        }
        #[cfg(not(all(target_os = "macos", feature = "lab")))]
        {
            let _ = app;
        }
    }

    #[cfg(all(target_os = "macos", feature = "lab"))]
    async fn watch<R: Runtime>(app: AppHandle<R>) {
        use std::collections::BTreeMap;

        use serde_json::{Value, json};

        let key = |c: &Value| {
            format!(
                "{}|{}|{}",
                c["name"].as_str().unwrap_or_default(),
                c["domain"].as_str().unwrap_or_default(),
                c["path"].as_str().unwrap_or_default()
            )
        };
        let mut last: BTreeMap<String, Value> = BTreeMap::new();
        loop {
            let (tx, rx) = tokio::sync::oneshot::channel();
            if app
                .run_on_main_thread(move || {
                    crate::platform::webview::default_store_cookie_details(move |list| {
                        let _ = tx.send(list);
                    });
                })
                .is_err()
            {
                return;
            }
            if let Ok(Ok(list)) = tokio::time::timeout(std::time::Duration::from_secs(2), rx).await
            {
                let now: BTreeMap<String, Value> = list.into_iter().map(|c| (key(&c), c)).collect();
                let mut changed = false;
                for (k, c) in &now {
                    let cause = match last.get(k) {
                        None => "inserted",
                        Some(old) if old != c => "overwrite",
                        Some(_) => continue,
                    };
                    changed = true;
                    super::record(
                        "cookie-changes.jsonl",
                        || json!({ "cause": cause, "removed": false, "cookie": truncated(c) }),
                    );
                }
                for (k, c) in &last {
                    if !now.contains_key(k) {
                        changed = true;
                        super::record(
                            "cookie-changes.jsonl",
                            || json!({ "cause": "deleted", "removed": true, "cookie": truncated(c) }),
                        );
                    }
                }
                if changed {
                    let jar: Vec<Value> = now.values().map(truncated).collect();
                    super::write_json(
                        "cookies-end.json",
                        || json!([{ "storagePath": "default WKWebsiteDataStore", "cookies": jar }]),
                    );
                }
                last = now;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }

    /// The cookie with a long value cut, as the ow-electron harness does.
    #[cfg(all(target_os = "macos", feature = "lab"))]
    fn truncated(c: &serde_json::Value) -> serde_json::Value {
        let mut c = c.clone();
        if let Some(v) = c["value"].as_str()
            && v.chars().count() > 300
        {
            let head: String = v.chars().take(300).collect();
            c["value"] = format!("{head}…[{}]", v.chars().count()).into();
        }
        c
    }
}

#[cfg(feature = "plugin")]
mod pages {
    use serde_json::json;
    use tauri::webview::PageLoadEvent;
    use tauri::{Runtime, Webview};
    use url::Url;

    use crate::window::{WebviewClass, classify};

    /// What an ad guest's page sees, as the ow-electron harness records it
    /// (`guest-<n>-dom-ready-<k>.json`).
    #[cfg(feature = "lab")]
    const GUEST_PROBE: &str = r"JSON.stringify((() => {
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
    labViewport: { outer: [outerWidth, outerHeight], client: [document.documentElement.clientWidth, document.documentElement.clientHeight], visual: window.visualViewport ? [visualViewport.width, visualViewport.height, visualViewport.offsetTop] : null, screen: [screenX, screenY] },
    labResources: (performance[Symbol.for('ow-tauri-lab')] || []).slice(),
  };
})())";

    /// Lab only: records the URLs the guest page and its same-origin frames
    /// request (Resource Timing, at most 2000, cut at 800 characters except
    /// the ad library frame's, whose query is its `options`; what the
    /// ow-electron harness reads from the developer tools protocol, where the
    /// fill impression is sent from a same-origin frame), for the live fill
    /// check and the ad library options (lab check L10). Read back by
    /// [`GUEST_PROBE`] as `labResources`.
    #[cfg(feature = "lab")]
    const GUEST_TAP: &str = r"(() => {
  const key = Symbol.for('ow-tauri-lab');
  if (performance[key]) return;
  const seen = [];
  Object.defineProperty(performance, key, { value: seen });
  // The ad library frame's URL carries its whole `options` (L10): kept in full.
  const keep = (e) => {
    if (seen.length < 2000) seen.push(e.name.slice(0, /oam\/releases\//.test(e.name) ? 16000 : 800));
  };
  const tapped = new WeakSet();
  const tap = (w) => {
    try {
      if (tapped.has(w)) return;
      tapped.add(w);
      new w.PerformanceObserver((list) => list.getEntries().forEach(keep)).observe({ type: 'resource', buffered: true });
    } catch (e) {}
  };
  const walk = (w, depth) => {
    tap(w);
    if (depth > 4) return;
    for (let i = 0; i < w.frames.length; i++) {
      try {
        const f = w.frames[i];
        void f.document;
        walk(f, depth + 1);
      } catch (e) {}
    }
  };
  walk(window, 0);
  setInterval(() => walk(window, 0), 250);
})()";

    /// What a consent page sees (`cmp-pages.jsonl`).
    #[cfg(feature = "lab")]
    const CMP_PROBE: &str = r"JSON.stringify((() => {
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
})())";

    fn class_name(label: &str) -> &'static str {
        match classify(label) {
            WebviewClass::Main => "main",
            WebviewClass::Ui(_) => "window",
            WebviewClass::Remote(_) => "remote",
            WebviewClass::AdviewGuest => "owadview",
            WebviewClass::Cmp => "cmp",
            WebviewClass::Other => "other",
        }
    }

    /// Lab trace: a navigation request of a webview and the policy's answer.
    pub(crate) fn navigation(label: &str, url: &Url, allowed: bool) {
        super::record(
            "wc-events.jsonl",
            || json!({ "kind": "navigation", "label": label, "type": class_name(label), "url": url.as_str(), "allowed": allowed }),
        );
    }

    /// Lab trace: a page load of a webview; when an ad guest or consent page
    /// finished loading, what its page sees.
    pub(crate) fn page_load<R: Runtime>(webview: &Webview<R>, event: PageLoadEvent, url: &Url) {
        if !super::trace_on() {
            return;
        }
        let label = webview.label().to_owned();
        let finished = event == PageLoadEvent::Finished;
        super::record("wc-events.jsonl", || {
            json!({
                "kind": if finished { "did-finish-load" } else { "did-start-loading" },
                "label": label,
                "type": class_name(&label),
                "url": url.as_str(),
            })
        });
        #[cfg(feature = "lab")]
        if finished && url.scheme() == "https" {
            probe(webview, &label);
        }
    }

    /// Guest numbers in first-seen order, and how often each was probed
    /// after a load: `label -> (n, loads)`.
    #[cfg(feature = "lab")]
    static GUESTS: std::sync::Mutex<Option<std::collections::HashMap<String, (u64, u64)>>> =
        std::sync::Mutex::new(None);

    /// The guest number of `label` and, when `load` is set, the index of this
    /// load (the ow-electron harness's `dom-ready-<k>`).
    #[cfg(feature = "lab")]
    fn guest_number(label: &str, load: bool) -> (u64, u64) {
        use std::sync::PoisonError;
        let mut guard = GUESTS.lock().unwrap_or_else(PoisonError::into_inner);
        let map = guard.get_or_insert_with(std::collections::HashMap::new);
        let next = u64::try_from(map.len()).unwrap_or(0) + 1;
        let entry = map.entry(label.to_owned()).or_insert((next, 0));
        let out = *entry;
        if load {
            entry.1 += 1;
        }
        out
    }

    /// The callback of `eval_with_callback` gets the JSON of the returned
    /// string; the probes return JSON text.
    #[cfg(feature = "lab")]
    fn parse(result: &str) -> serde_json::Value {
        serde_json::from_str::<String>(result)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| json!({ "raw": result }))
    }

    /// Evaluates the guest probe in `webview` and writes
    /// `guest-<n>-<phase>.json`.
    #[cfg(feature = "lab")]
    fn probe_guest<R: Runtime>(webview: &Webview<R>, n: u64, phase: &str) {
        let name = format!("guest-{n}-{phase}.json");
        let label = webview.label().to_owned();
        let _ = webview.eval_with_callback(GUEST_PROBE, move |result| {
            let mut value = parse(&result);
            if let serde_json::Value::Object(m) = &mut value {
                m.insert("label".into(), label.clone().into());
            }
            super::write_json(&name, || value);
        });
    }

    /// Probes every live ad guest of `app` now (`guest-<n>-<phase>.json`),
    /// as the ow-electron harness's `probe-guests` action does.
    #[cfg(feature = "lab")]
    pub(crate) fn probe_guests<R: Runtime>(app: &tauri::AppHandle<R>, phase: &str) {
        use tauri::Manager as _;
        if !super::trace_on() {
            return;
        }
        for (label, webview) in app.webviews() {
            if classify(&label) == WebviewClass::AdviewGuest {
                let (n, _) = guest_number(&label, false);
                probe_guest(&webview, n, phase);
            }
        }
    }

    /// Evaluates the probe of an ad guest (now and 10 s later, as the
    /// ow-electron harness does) or consent page and writes it.
    #[cfg(feature = "lab")]
    fn probe<R: Runtime>(webview: &Webview<R>, label: &str) {
        match classify(label) {
            WebviewClass::AdviewGuest => {
                let (n, k) = guest_number(label, true);
                let _ = webview.eval(GUEST_TAP);
                probe_guest(webview, n, &format!("dom-ready-{k}"));
                let later = webview.clone();
                let _ = std::thread::Builder::new()
                    .name("ow-tauri-lab-probe".into())
                    .spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(10));
                        probe_guest(&later, n, "after-10s");
                    });
            }
            WebviewClass::Cmp => {
                let label = label.to_owned();
                let _ = webview.eval_with_callback(CMP_PROBE, move |result| {
                    let mut value = parse(&result);
                    if let serde_json::Value::Object(m) = &mut value {
                        m.insert("label".into(), label.clone().into());
                        m.insert("type".into(), "cmp".into());
                    }
                    super::record("cmp-pages.jsonl", || value);
                });
            }
            _ => {}
        }
    }
}

#[cfg(feature = "plugin")]
mod window_guard {
    use serde_json::json;
    use tauri::{Manager, Runtime, WebviewWindow, WebviewWindowBuilder};

    /// Sets the builder's visibility. Lab windows are always built hidden,
    /// not focusable, off the taskbar and never full screen; [`after_build`]
    /// shows them.
    pub(crate) fn window_builder<R: Runtime, M: Manager<R>>(
        builder: WebviewWindowBuilder<'_, R, M>,
        visible: bool,
    ) -> WebviewWindowBuilder<'_, R, M> {
        if super::invisible() {
            // A full-screen window gets its own Space on macOS, which the
            // user would see even at alpha 0.
            builder
                .visible(false)
                .focusable(false)
                .skip_taskbar(true)
                .fullscreen(false)
        } else {
            builder.visible(visible)
        }
    }

    /// Lab windows: makes `window` invisible (macOS: alpha 0, click-through)
    /// and then shows it when `visible`; records it in `windows.jsonl`.
    /// Does nothing outside the lab.
    pub(crate) fn after_build<R: Runtime>(window: &WebviewWindow<R>, visible: bool) {
        super::record("windows.jsonl", || {
            json!({
                "kind": "created",
                "label": window.label(),
                "visibleRequested": visible,
                "invisible": super::invisible(),
                "bounds": window.outer_position().ok().map(|p| [p.x, p.y]),
                "size": window.outer_size().ok().map(|s| [s.width, s.height]),
            })
        });
        if !super::invisible() {
            return;
        }
        let _ = window.set_ignore_cursor_events(true);
        #[cfg(target_os = "macos")]
        {
            let Ok(ns_window) = window.ns_window() else {
                return;
            };
            // The pointer crosses to the main thread as an address.
            let address = ns_window as usize;
            let w = window.clone();
            let _ = window.run_on_main_thread(move || {
                set_alpha_zero(address);
                if visible {
                    // Not `show()`: `makeKeyAndOrderFront:` activates the app.
                    order_front_regardless(address);
                    super::record(
                        "windows.jsonl",
                        || json!({ "kind": "shown", "label": w.label(), "alpha": 0 }),
                    );
                }
            });
        }
        #[cfg(not(target_os = "macos"))]
        {
            // No alpha-0 window here: lab windows stay hidden.
            super::record(
                "windows.jsonl",
                || json!({ "kind": "kept-hidden", "label": window.label() }),
            );
        }
    }

    /// `-[NSWindow orderFrontRegardless]`: shown without becoming key or
    /// activating the app (the invisible lab app never comes to the front).
    #[cfg(target_os = "macos")]
    fn order_front_regardless(address: usize) {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        if address == 0 {
            return;
        }
        // SAFETY: `address` is the live `NSWindow*` Tauri handed out for this
        // window, used on the main thread.
        let obj: &AnyObject = unsafe { &*(address as *const AnyObject) };
        // SAFETY: a public NSWindow method without arguments.
        let () = unsafe { msg_send![obj, orderFrontRegardless] };
    }

    /// `-[NSWindow setAlphaValue:0]` and `setIgnoresMouseEvents:YES`.
    #[cfg(target_os = "macos")]
    fn set_alpha_zero(address: usize) {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        if address == 0 {
            return;
        }
        // SAFETY: `address` is the live `NSWindow*` Tauri handed out for this
        // window, used on the main thread.
        let obj: &AnyObject = unsafe { &*(address as *const AnyObject) };
        // SAFETY: public NSWindow setters taking a CGFloat and a BOOL.
        let () = unsafe { msg_send![obj, setAlphaValue: 0.0_f64] };
        // SAFETY: as above.
        let () = unsafe { msg_send![obj, setIgnoresMouseEvents: true] };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_names_in_order() {
        assert_eq!(
            cookie_names("euconsent-v2=CQ; acconsent=x;  _pubcid=1"),
            ["euconsent-v2", "acconsent", "_pubcid"]
        );
        assert!(cookie_names("").is_empty());
    }

    #[test]
    fn may_focus_outside_the_lab() {
        // The test process never sets OW_TAURI_LAB_INVISIBLE.
        assert!(may_focus());
    }

    #[test]
    fn os_surfaces_open_outside_the_lab() {
        // Dialogs, the file manager and the browser are blocked only in an
        // invisible lab; the detail is not even computed otherwise.
        assert!(!block_os_surface("dialog_message", || unreachable!(
            "no detail outside the lab"
        )));
    }
}
