//! Windows implementations.

/// `<major>.<minor>.<build>`, as Node's `os.release()` on Windows.
pub(crate) fn os_release() -> String {
    let mut info = windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW {
        dwOSVersionInfoSize: u32::try_from(size_of::<
            windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW,
        >())
        .unwrap_or(0),
        ..Default::default()
    };
    // SAFETY: `info` is a valid OSVERSIONINFOW with its size field set;
    // RtlGetVersion only writes into it.
    let status = unsafe { windows_sys::Wdk::System::SystemServices::RtlGetVersion(&raw mut info) };
    if status != 0 {
        return "unknown".into();
    }
    format!(
        "{}.{}.{}",
        info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
    )
}
