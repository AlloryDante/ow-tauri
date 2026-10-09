//! The run: the configuration `run.mjs` wrote (`PARITY_HARNESS_CONFIG`),
//! the identity under test (`PARITY_HARNESS_PACKAGE_JSON`) and the files the
//! harness writes into the run directory, in the shapes the ow-electron
//! harness app (`app/main.cjs`) writes.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

/// The run configuration and the harness's own bookkeeping.
pub struct Harness {
    /// `config.json` of the run.
    pub config: Value,
    /// The run directory every record goes to.
    pub run_dir: PathBuf,
    /// When the process started: the origin of every record's `t`.
    started: Instant,
    /// The same moment as Unix ms.
    pub started_wall: u64,
    /// Answers the harness page sends back (`harness_reply`), by request id.
    replies: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    /// Request ids for [`Harness::ask`].
    next_id: AtomicU64,
    /// `overwolf.json`: snapshots and calls, as ow-electron's harness keeps them.
    pub overwolf: Mutex<OverwolfFile>,
}

/// The content of `overwolf.json` (see `app/main.cjs` `snapshotOverwolf`).
#[derive(Default)]
pub struct OverwolfFile {
    /// Fixed facts (`appName`, `appVersion`, `platform`, ...).
    pub facts: Map<String, Value>,
    /// Full first snapshot, then the changes of each later one.
    pub snapshots: Vec<Value>,
    /// The last full snapshot (for the changes of the next one).
    pub last: Option<Value>,
    /// Every recorded `app.overwolf` call.
    pub calls: Vec<Value>,
}

static HARNESS: OnceLock<Harness> = OnceLock::new();

/// Unix time in ms.
pub fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The run (set once in `main`).
///
/// # Panics
///
/// Never after [`install`]; `main` installs the run before anything else.
pub fn get() -> &'static Harness {
    HARNESS.get().unwrap_or_else(|| {
        eprintln!("parity harness: no run configuration");
        std::process::exit(2)
    })
}

/// Reads the run configuration and keeps it for the process.
///
/// # Errors
///
/// When `PARITY_HARNESS_CONFIG` is missing or is not a run configuration.
pub fn install() -> Result<&'static Harness, String> {
    let path =
        std::env::var_os("PARITY_HARNESS_CONFIG").ok_or("PARITY_HARNESS_CONFIG is not set")?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("config: {e}"))?;
    let config: Value = serde_json::from_str(&text).map_err(|e| format!("config: {e}"))?;
    let run_dir = config
        .get("runDir")
        .and_then(Value::as_str)
        .ok_or("config: runDir missing")?
        .into();
    let harness = Harness {
        config,
        run_dir,
        started: Instant::now(),
        started_wall: wall_ms(),
        replies: Mutex::new(HashMap::new()),
        next_id: AtomicU64::new(1),
        overwolf: Mutex::new(OverwolfFile::default()),
    };
    let _ = HARNESS.set(harness);
    Ok(get())
}

/// Locks `m`, ignoring poisoning (a panicked recorder must not stop the run).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Only plain file names in the run directory (probe labels such as
/// `perf+12s` keep their `+`, as ow-electron's harness names the files).
fn run_file(run_dir: &std::path::Path, name: &str, extension: &str) -> Result<PathBuf, String> {
    let ok = !name.is_empty()
        && name.len() <= 160
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
        && !name.starts_with('.')
        && name.ends_with(extension);
    if !ok {
        return Err(format!("bad run file name {name:?}"));
    }
    Ok(run_dir.join(name))
}

impl Harness {
    /// Milliseconds since the process started.
    pub fn t(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// A string option of the configuration.
    pub fn str_opt(&self, key: &str) -> Option<&str> {
        self.config.get(key).and_then(Value::as_str)
    }

    /// A boolean option of the configuration (`false` when absent).
    pub fn flag(&self, key: &str) -> bool {
        self.config.get(key).and_then(Value::as_bool) == Some(true)
    }

    /// A number option of the configuration.
    pub fn num(&self, key: &str) -> Option<u64> {
        self.config.get(key).and_then(Value::as_u64)
    }

    /// Whether the run uses test ads.
    pub fn test_mode(&self) -> bool {
        self.str_opt("mode") == Some("test")
    }

    /// Appends `{t, ...entry}` as one line to `<runDir>/<file>` (`*.jsonl`).
    pub fn record(&self, file: &str, entry: Value) {
        let mut line = Map::new();
        line.insert("t".into(), json!(self.t()));
        if let Value::Object(fields) = entry {
            line.extend(fields);
        }
        let _ = self.append(file, &Value::Object(line).to_string());
    }

    /// Appends one line (already JSON) to `<runDir>/<file>` (`*.jsonl`).
    ///
    /// # Errors
    ///
    /// A bad file name or a failed write.
    pub fn append(&self, file: &str, line: &str) -> Result<(), String> {
        let path = run_file(&self.run_dir, file, ".jsonl")?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        // One write per line keeps lines whole next to the plugin's trace.
        f.write_all(format!("{line}\n").as_bytes())
            .map_err(|e| e.to_string())
    }

    /// Writes `<runDir>/<file>` (`*.json`), pretty-printed.
    pub fn write_json(&self, file: &str, value: &Value) {
        if let Ok(path) = run_file(&self.run_dir, file, ".json") {
            let text = serde_json::to_string_pretty(value).unwrap_or_default();
            let _ = std::fs::write(path, format!("{text}\n"));
        }
    }

    /// A new request id and the receiver its answer arrives on.
    pub fn ask(&self) -> (u64, mpsc::Receiver<Value>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        lock(&self.replies).insert(id, tx);
        (id, rx)
    }

    /// Delivers the answer to request `id` (see [`Harness::ask`]).
    pub fn answer(&self, id: u64, value: Value) {
        if let Some(tx) = lock(&self.replies).remove(&id) {
            let _ = tx.send(value);
        }
    }

    /// Forgets request `id` (its answer will not be awaited).
    pub fn forget(&self, id: u64) {
        lock(&self.replies).remove(&id);
    }

    /// Writes `overwolf.json` from [`Harness::overwolf`].
    pub fn write_overwolf(&self) {
        let file = lock(&self.overwolf);
        let mut out = file.facts.clone();
        out.insert("snapshots".into(), Value::Array(file.snapshots.clone()));
        out.insert("calls".into(), Value::Array(file.calls.clone()));
        drop(file);
        self.write_json("overwolf.json", &Value::Object(out));
    }

    /// Adds a snapshot of `full` under `label`: the whole of it the first
    /// time, else only what changed since the last one (as ow-electron's
    /// harness does).
    pub fn push_snapshot(&self, label: &str, mut full: Value) {
        let mut file = lock(&self.overwolf);
        if let Value::Object(m) = &mut full {
            m.insert("label".into(), json!(label));
        }
        let entry = match &file.last {
            None => full.clone(),
            Some(last) => {
                let mut changed = Map::new();
                for section in ["env", "members", "packages"] {
                    let now = full.get(section).and_then(Value::as_object);
                    let before = last.get(section).and_then(Value::as_object);
                    for (k, v) in now.into_iter().flatten() {
                        if before.and_then(|b| b.get(k)) != Some(v) {
                            changed.insert(format!("{section}.{k}"), v.clone());
                        }
                    }
                }
                json!({ "label": label, "t": self.t(), "changed": changed })
            }
        };
        file.snapshots.push(entry);
        file.last = Some(full);
        drop(file);
        self.write_overwolf();
    }

    /// Records one `app.overwolf` call (`overwolf.json` `calls`).
    pub fn push_call(&self, call: Value) {
        lock(&self.overwolf).calls.push(call);
        self.write_overwolf();
    }
}

/// Waits up to `limit` for an answer.
pub fn wait(rx: &mpsc::Receiver<Value>, limit: Duration) -> Option<Value> {
    rx.recv_timeout(limit).ok()
}

/// The identity under test: `run.mjs` writes it as an ow-electron
/// `package.json` (`PARITY_HARNESS_PACKAGE_JSON`); the Tauri app takes the
/// same facts from its configuration (`productName`, `version`,
/// `plugins.overwolf.{name, author, uid}`), so the identity never enters
/// the build.
pub struct Identity {
    /// ow-electron's app name: `build.productName`, `productName`, else `name`.
    pub product_name: String,
    /// The app version.
    pub version: String,
    /// The uid formula's author (`author.name` or `author`).
    pub author: Option<String>,
    /// `overwolf.uid`, when the run pins one.
    pub uid: Option<String>,
}

/// Reads the identity under test.
///
/// # Errors
///
/// When `PARITY_HARNESS_PACKAGE_JSON` is missing or unreadable.
pub fn identity() -> Result<Identity, String> {
    let path = std::env::var_os("PARITY_HARNESS_PACKAGE_JSON")
        .ok_or("PARITY_HARNESS_PACKAGE_JSON is not set")?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("package.json: {e}"))?;
    let pkg: Value = serde_json::from_str(&text).map_err(|e| format!("package.json: {e}"))?;
    let s = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_owned);
    let product_name = s(pkg.pointer("/build/productName"))
        .or_else(|| s(pkg.get("productName")))
        .or_else(|| s(pkg.get("name")))
        .ok_or("package.json: no name")?;
    let author = match pkg.get("author") {
        Some(Value::Object(a)) => s(a.get("name")),
        other => s(other),
    };
    Ok(Identity {
        product_name,
        version: s(pkg.get("version")).unwrap_or_else(|| "0.0.0".into()),
        author,
        uid: s(pkg.pointer("/overwolf/uid")),
    })
}

/// JSON text of `value`, cut at `limit` characters as the ow-electron
/// harness cuts long values (`…[<length>]`).
pub fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let cut: String = text.chars().take(limit).collect();
    format!("{cut}…[{}]", text.chars().count())
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncates_like_the_electron_harness() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abcdef", 3), "abc…[6]");
    }
}
