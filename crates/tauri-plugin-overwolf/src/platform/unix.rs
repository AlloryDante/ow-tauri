//! Unix implementations (macOS, Linux).

use super::LocalTime;

/// The current local time.
pub(crate) fn local_time() -> LocalTime {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = libc::time_t::try_from(now.as_secs()).unwrap_or(libc::time_t::MAX);
    // SAFETY: `tm` is a plain C struct for which all-zero bytes are a valid
    // value; `localtime_r` only writes into it and is thread-safe.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the duration of the call.
    let ok = unsafe { !libc::localtime_r(&raw const secs, &raw mut tm).is_null() };
    if !ok {
        return LocalTime {
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            millis: 0,
        };
    }
    let field = |v: libc::c_int| u32::try_from(v).unwrap_or(0);
    LocalTime {
        year: tm.tm_year + 1900,
        month: field(tm.tm_mon + 1),
        day: field(tm.tm_mday),
        hour: field(tm.tm_hour),
        minute: field(tm.tm_min),
        second: field(tm.tm_sec),
        millis: now.subsec_millis(),
    }
}

/// `uname -r`: the Darwin or Linux kernel release, as Node's `os.release()`.
#[cfg(feature = "plugin")]
pub(crate) fn os_release() -> String {
    // SAFETY: `utsname` is a plain C struct; all-zero bytes are a valid value.
    let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
    // SAFETY: the pointer is valid for the duration of the call.
    if unsafe { libc::uname(&raw mut uts) } != 0 {
        return "unknown".into();
    }
    let bytes: Vec<u8> = uts
        .release
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| c.to_ne_bytes()[0])
        .collect();
    String::from_utf8(bytes).unwrap_or_else(|_| "unknown".into())
}
