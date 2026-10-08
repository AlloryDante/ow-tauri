//! Graphics facts of `systemInfo` (D.2) that Tauri does not report: the GPU
//! list and, on Windows, the monitor names a user sees.
//!
//! On Windows ow-electron lists one GPU per DXGI adapter (software adapters
//! included) with only the user-mode driver version filled in, and names a
//! display by its monitor's friendly name (`HyperVMonitor`, not
//! `\\.\DISPLAY1`) [OBS: Windows lab]. That is Chromium's own adapter list
//! (`EnumAdapters`, `CheckInterfaceSupport(IDXGIDevice)`) and display label
//! (`DisplayConfigGetDeviceInfo`).
#![allow(dead_code, reason = "the ads host (W2) reports the GPU drivers (D.2)")]

/// The `driverVersion` text of a user-mode driver version as
/// `CheckInterfaceSupport` returns it: its four 16-bit words, high to low,
/// joined by dots (Chromium's `DriverVersionToString`).
#[cfg_attr(not(any(test, windows)), expect(dead_code, reason = "Windows only"))]
pub(crate) fn driver_version_text(version: i64) -> String {
    let v = version.cast_unsigned();
    let word = |shift: u32| (v >> shift) & 0xFFFF;
    format!("{}.{}.{}.{}", word(48), word(32), word(16), word(0))
}

/// The `gpus` driver versions: one per adapter, `""` where unknown. Empty
/// when the platform reports no list (then `systemInfo` keeps one blank
/// entry, as on macOS).
pub(crate) fn gpu_driver_versions() -> Vec<String> {
    #[cfg(windows)]
    {
        windows_impl::gpu_driver_versions()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// The name a user sees for the display a window system calls `name`
/// (Windows: the monitor friendly name of a GDI device such as
/// `\\.\DISPLAY1`), or `None` to keep `name`.
pub(crate) fn display_friendly_name(name: &str) -> Option<String> {
    #[cfg(windows)]
    {
        windows_impl::display_friendly_name(name)
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        None
    }
}

#[cfg(windows)]
mod windows_impl {
    use windows::Win32::Devices::Display::{
        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
        DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
        DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
        DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
        QueryDisplayConfig,
    };
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIDevice, IDXGIFactory1};
    use windows::core::Interface as _;

    use super::driver_version_text;

    pub(super) fn gpu_driver_versions() -> Vec<String> {
        // SAFETY: plain COM factory creation and adapter queries; every
        // returned interface is owned and released by its wrapper.
        unsafe {
            let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
                return Vec::new();
            };
            let mut out = Vec::new();
            for i in 0.. {
                let Ok(adapter) = factory.EnumAdapters(i) else {
                    break;
                };
                out.push(
                    adapter
                        .CheckInterfaceSupport(&IDXGIDevice::IID)
                        .map(driver_version_text)
                        .unwrap_or_default(),
                );
            }
            out
        }
    }

    fn utf16(units: &[u16]) -> String {
        let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        String::from_utf16_lossy(&units[..end])
    }

    pub(super) fn display_friendly_name(gdi_name: &str) -> Option<String> {
        let mut paths_len = 0_u32;
        let mut modes_len = 0_u32;
        // SAFETY: the counts are written through valid pointers; the buffers
        // are sized from them and outlive the calls; each device-info
        // request carries its own size and type in its header.
        unsafe {
            if GetDisplayConfigBufferSizes(
                QDC_ONLY_ACTIVE_PATHS,
                &raw mut paths_len,
                &raw mut modes_len,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); paths_len as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); modes_len as usize];
            if QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &raw mut paths_len,
                paths.as_mut_ptr(),
                &raw mut modes_len,
                modes.as_mut_ptr(),
                None,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            paths.truncate(paths_len as usize);
            for path in &paths {
                let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                    header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                        r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                        size: u32::try_from(size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>()).ok()?,
                        adapterId: path.sourceInfo.adapterId,
                        id: path.sourceInfo.id,
                    },
                    ..Default::default()
                };
                if DisplayConfigGetDeviceInfo(&raw mut source.header) != 0
                    || !utf16(&source.viewGdiDeviceName).eq_ignore_ascii_case(gdi_name)
                {
                    continue;
                }
                let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
                    header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                        r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                        size: u32::try_from(size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>()).ok()?,
                        adapterId: path.targetInfo.adapterId,
                        id: path.targetInfo.id,
                    },
                    ..Default::default()
                };
                if DisplayConfigGetDeviceInfo(&raw mut target.header) != 0 {
                    return None;
                }
                let name = utf16(&target.monitorFriendlyDeviceName);
                return (!name.is_empty()).then_some(name);
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_versions_read_as_four_words() {
        // Windows lab: ow-electron's `driverVersion` on a Windows Server 2025
        // runner (UMD version 10.0.26100.33438).
        let v = 10 * (1_i64 << 48) + 26_100 * (1 << 16) + 33_438;
        assert_eq!(driver_version_text(v), "10.0.26100.33438");
        assert_eq!(driver_version_text(-1), "65535.65535.65535.65535");
        assert_eq!(driver_version_text(0), "0.0.0.0");
    }

    #[test]
    fn platform_queries_do_not_fail() {
        let versions = gpu_driver_versions();
        assert!(
            versions
                .iter()
                .all(|v| v.is_empty() || v.split('.').count() == 4)
        );
        assert_eq!(display_friendly_name("no such display"), None);
    }
}
