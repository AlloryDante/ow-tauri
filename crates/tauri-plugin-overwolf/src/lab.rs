//! Lab mode: a trace of what the plugin sends and does, and invisible
//! windows, for the Tauri edition of the parity harness.
//!
//! Only with the Cargo feature `lab` (refused in release builds, DESIGN
//! §6.3). Even then nothing happens unless the environment asks for it:
//!
//! - `OW_TAURI_LAB_DIR=<dir>` turns the trace on: JSON lines appended to
//!   files in `<dir>` (`host-requests.jsonl`: every host request as it
//!   leaves the plugin and its answer; `state-writes.jsonl`: every state
//!   file write), in the shapes the ow-electron harness writes.
//! - `OW_TAURI_LAB_INVISIBLE=1`: windows the plugin builds stay invisible
//!   and no OS surface (system browser) opens; [`block_os_surface`] records
//!   the request in `blocked.jsonl` instead.
//! - `OW_TAURI_LAB_CMP_EU_ONLY=<body>` answers the startup `cmp-eu-only`
//!   request with HTTP 200 and `<body>` instead of sending it.
//!
//! Without the feature every function here is an empty inline function. The
//! guest and window probes come back with the ads host (W2) and the harness
//! (W3).

use serde_json::Value;

/// Whether the trace is on (`OW_TAURI_LAB_DIR` is set, feature `lab`).
#[allow(
    dead_code,
    reason = "wired by the ads and consent hosts (W2) and the harness (W3)"
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

/// A process-wide sequence number for trace records (request ids, guest
/// numbers).
#[allow(
    dead_code,
    reason = "wired by the ads and consent hosts (W2) and the harness (W3)"
)]
#[must_use]
pub(crate) fn next_id() -> u64 {
    imp::next_id()
}

/// Whether lab windows are invisible (`OW_TAURI_LAB_INVISIBLE=1`, feature
/// `lab`).
#[allow(
    dead_code,
    reason = "wired by the ads and consent hosts (W2) and the harness (W3)"
)]
#[must_use]
pub(crate) fn invisible() -> bool {
    imp::invisible()
}

/// The body that answers the startup `cmp-eu-only` request instead of the
/// network (`OW_TAURI_LAB_CMP_EU_ONLY`, feature `lab`); `None` outside the
/// lab. Recorded in `host-requests.jsonl` when used.
#[allow(
    dead_code,
    reason = "wired by the ads and consent hosts (W2) and the harness (W3)"
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
#[allow(
    dead_code,
    reason = "wired by the ads and consent hosts (W2) and the harness (W3)"
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
#[allow(
    dead_code,
    reason = "wired by the ads and consent hosts (W2) and the harness (W3)"
)]
#[must_use]
pub(crate) fn may_focus() -> bool {
    !invisible()
}

/// The names of the cookies in a `cookie` header value, in order.
#[allow(
    dead_code,
    reason = "wired by the ads and consent hosts (W2) and the harness (W3)"
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
