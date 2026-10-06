//! Small OS facts the plugin needs: local time for log lines, the kernel or OS
//! release (Node's `os.release()`), the macOS major version (A.6 liveness),
//! and whether a file carries an execute bit (A.2.3.2).

#[cfg(feature = "plugin")]
pub(crate) mod machine;
#[cfg(unix)]
mod unix;
#[cfg(feature = "plugin")]
pub(crate) mod webview;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use unix::local_time;
#[cfg(all(unix, feature = "plugin"))]
pub(crate) use unix::os_release;
#[cfg(windows)]
pub(crate) use windows::local_time;
#[cfg(all(windows, feature = "plugin"))]
pub(crate) use windows::os_release;

/// Broken-down local time, enough for `[YYYY-MM-DD HH:MM:SS.mmm]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocalTime {
    pub(crate) year: i32,
    pub(crate) month: u32,
    pub(crate) day: u32,
    pub(crate) hour: u32,
    pub(crate) minute: u32,
    pub(crate) second: u32,
    pub(crate) millis: u32,
}

impl LocalTime {
    /// `YYYY-MM-DD HH:MM:SS.mmm` (F.4).
    pub(crate) fn format(self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            self.year, self.month, self.day, self.hour, self.minute, self.second, self.millis
        )
    }
}

/// The macOS major version (for example 14), or `None` when it cannot be
/// read. Reads the `kern.osproductversion` sysctl, which works in sandboxed
/// and hardened processes (no child process).
#[cfg(all(target_os = "macos", feature = "plugin"))]
pub(crate) fn macos_major_version() -> Option<u32> {
    let mut buf = [0_u8; 32];
    let mut len = buf.len();
    // SAFETY: the name is a NUL-terminated C string; `buf` and `len`
    // describe a writable buffer that outlives the call; no new value is
    // set (null pointer, length 0).
    let rc = unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            buf.as_mut_ptr().cast(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    parse_major_version(buf.get(..len)?)
}

/// The major version from `"14.5"` style text (a trailing NUL is ignored).
#[cfg(any(test, all(target_os = "macos", feature = "plugin")))]
fn parse_major_version(bytes: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(bytes).ok()?;
    text.trim_end_matches('\0')
        .trim()
        .split('.')
        .next()?
        .parse()
        .ok()
}

/// Whether `ow-main` can rely on `BackgroundThrottlingPolicy::Disabled`
/// (macOS 14 and newer) instead of the technically visible 1 x 1 window.
#[cfg(feature = "plugin")]
pub(crate) fn hidden_main_webview_keeps_timers() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos_major_version().is_some_and(|v| v >= 14)
    }
    #[cfg(windows)]
    {
        // The shared browser arguments disable throttling (A.1.1).
        true
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        false
    }
}

/// Whether `meta` describes a file with any execute bit set (Unix only).
#[cfg(all(unix, feature = "plugin"))]
pub(crate) fn has_execute_bit(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    meta.is_file() && meta.permissions().mode() & 0o111 != 0
}

/// Whether `meta` describes a file with any execute bit set (Unix only).
#[cfg(all(not(unix), feature = "plugin"))]
pub(crate) fn has_execute_bit(_meta: &std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_time_formats() {
        let t = LocalTime {
            year: 2026,
            month: 10,
            day: 6,
            hour: 9,
            minute: 5,
            second: 3,
            millis: 7,
        };
        assert_eq!(t.format(), "2026-10-06 09:05:03.007");
        let now = local_time();
        assert!(now.year >= 2024);
        assert!((1..=12).contains(&now.month));
    }

    #[test]
    fn major_version_parses() {
        assert_eq!(parse_major_version(b"14.5\0"), Some(14));
        assert_eq!(parse_major_version(b"26.0"), Some(26));
        assert_eq!(parse_major_version(b"x"), None);
    }

    #[test]
    #[cfg(all(target_os = "macos", feature = "plugin"))]
    fn macos_version_is_read() {
        assert!(macos_major_version().is_some_and(|v| v >= 11));
    }

    #[test]
    #[cfg(feature = "plugin")]
    fn os_release_is_not_empty() {
        assert!(!os_release().is_empty());
    }
}
