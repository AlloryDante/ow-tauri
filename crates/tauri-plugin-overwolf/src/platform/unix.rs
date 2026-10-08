//! Unix implementations (macOS, Linux).

/// `uname -r`: the Darwin or Linux kernel release, as Node's `os.release()`.
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
