//! Windows implementations.

use super::LocalTime;

/// The current local time.
pub(crate) fn local_time() -> LocalTime {
    let mut st = windows_sys::Win32::Foundation::SYSTEMTIME::default();
    // SAFETY: `st` is a valid, writable SYSTEMTIME for the duration of the call.
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&raw mut st) };
    LocalTime {
        year: i32::from(st.wYear),
        month: u32::from(st.wMonth),
        day: u32::from(st.wDay),
        hour: u32::from(st.wHour),
        minute: u32::from(st.wMinute),
        second: u32::from(st.wSecond),
        millis: u32::from(st.wMilliseconds),
    }
}

/// `<major>.<minor>.<build>`, as Node's `os.release()` on Windows.
#[cfg(feature = "plugin")]
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
