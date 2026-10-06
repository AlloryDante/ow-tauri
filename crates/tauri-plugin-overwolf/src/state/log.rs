//! `logs/ow-tauri.log` (CONTRACT F.4).
//!
//! One line per entry, `[YYYY-MM-DD HH:MM:SS.mmm] [<level>] <message>` in
//! local time. The file rolls at 5 MiB and three files are kept:
//! `ow-tauri.log`, `ow-tauri.log.1` and `ow-tauri.log.2`. Line breaks inside a
//! message are escaped, so one entry is always one line.
//!
//! ```
//! # let dir = std::env::temp_dir().join(format!("owlog-doc-{}", std::process::id()));
//! use tauri_plugin_overwolf::state::log::{LogLevel, Logger};
//! let logger = Logger::open(dir.join("logs/ow-tauri.log"));
//! logger.write(LogLevel::Info, "hello\nworld");
//! let text = std::fs::read_to_string(dir.join("logs/ow-tauri.log")).unwrap();
//! assert!(text.ends_with("[info] hello\\nworld\n"));
//! # let _ = std::fs::remove_dir_all(&dir);
//! ```

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Size at which the file rolls.
pub const ROLL_BYTES: u64 = 5 * 1024 * 1024;
/// Number of files kept, the current one included.
pub const KEEP_FILES: usize = 3;

/// A log level, as the `log` command accepts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// `debug`.
    Debug,
    /// `info`.
    Info,
    /// `warn`.
    Warn,
    /// `error`.
    Error,
}

impl LogLevel {
    /// The level as written in the file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

#[derive(Debug)]
struct Sink {
    file: File,
    size: u64,
}

/// The ow-tauri log file. Writing never fails the caller: if the file cannot
/// be opened the logger is silent.
#[derive(Debug)]
pub struct Logger {
    path: PathBuf,
    sink: Mutex<Option<Sink>>,
    enabled: bool,
}

/// Formats one log line (without the trailing newline).
#[must_use]
pub fn format_line(timestamp: &str, level: LogLevel, message: &str) -> String {
    let escaped = message.replace('\r', "\\r").replace('\n', "\\n");
    format!("[{timestamp}] [{}] {escaped}", level.as_str())
}

fn rolled(path: &Path, n: usize) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{n}"));
    PathBuf::from(name)
}

fn open_append(path: &Path) -> Option<Sink> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()?;
    let size = file.metadata().map_or(0, |m| m.len());
    Some(Sink { file, size })
}

impl Logger {
    /// Opens (or creates) the log file at `path`, creating its directory.
    #[must_use]
    pub fn open(path: PathBuf) -> Self {
        let sink = open_append(&path);
        Logger {
            path,
            sink: Mutex::new(sink),
            enabled: true,
        }
    }

    /// A logger that never creates or writes the file (`logging.enabled`
    /// false, the default, F.4).
    ///
    /// ```
    /// # let dir = std::env::temp_dir().join(format!("owlog-off-{}", std::process::id()));
    /// use tauri_plugin_overwolf::state::log::{LogLevel, Logger};
    /// let logger = Logger::disabled(dir.join("logs/ow-tauri.log"));
    /// logger.write(LogLevel::Info, "dropped");
    /// assert!(!dir.join("logs").exists());
    /// ```
    #[must_use]
    pub fn disabled(path: PathBuf) -> Self {
        Logger {
            path,
            sink: Mutex::new(None),
            enabled: false,
        }
    }

    /// Whether entries are written.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The current log file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one entry stamped with the current local time.
    pub fn write(&self, level: LogLevel, message: &str) {
        let stamp = crate::platform::local_time().format();
        self.write_line(&format_line(&stamp, level, message));
    }

    fn write_line(&self, line: &str) {
        if !self.enabled {
            return;
        }
        let mut guard = self
            .sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let bytes = line.len() as u64 + 1;
        if guard
            .as_ref()
            .is_some_and(|s| s.size > 0 && s.size + bytes > ROLL_BYTES)
        {
            *guard = None;
            self.roll();
            *guard = open_append(&self.path);
        }
        if let Some(sink) = guard.as_mut()
            && writeln!(sink.file, "{line}").is_ok()
        {
            sink.size += bytes;
        }
    }

    fn roll(&self) {
        for n in (1..KEEP_FILES).rev() {
            let from = if n == 1 {
                self.path.clone()
            } else {
                rolled(&self.path, n - 1)
            };
            let _ = std::fs::rename(&from, rolled(&self.path, n));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_dir;

    #[test]
    fn line_format() {
        assert_eq!(
            format_line("2026-10-06 10:00:00.000", LogLevel::Warn, "a\r\nb"),
            "[2026-10-06 10:00:00.000] [warn] a\\r\\nb"
        );
    }

    #[test]
    fn rolls_and_keeps_three_files() {
        let dir = test_dir("log-roll");
        let path = dir.join("ow-tauri.log");
        let logger = Logger::open(path.clone());
        let big = "x".repeat(1024 * 1024);
        for _ in 0..20 {
            logger.write(LogLevel::Info, &big);
        }
        assert!(path.exists());
        assert!(rolled(&path, 1).exists());
        assert!(rolled(&path, 2).exists());
        assert!(!rolled(&path, 3).exists());
        for p in [path.clone(), rolled(&path, 1), rolled(&path, 2)] {
            assert!(std::fs::metadata(&p).unwrap().len() <= ROLL_BYTES);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unopenable_path_is_silent() {
        let dir = test_dir("log-bad");
        let blocker = dir.join("file");
        std::fs::write(&blocker, b"x").unwrap();
        let logger = Logger::open(blocker.join("sub/ow-tauri.log"));
        logger.write(LogLevel::Error, "dropped");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
