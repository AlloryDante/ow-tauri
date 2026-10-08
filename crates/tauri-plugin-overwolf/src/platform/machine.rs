//! Machine facts read from the OS: the platform id behind the machine-id
//! muid (CONTRACT E.4) and the CPU brand string of the guest `systemInfo`
//! (D.2).

use crate::identity::machine_muid;

/// The machine ids of this session (E.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MachineIds {
    /// `muid`.
    pub(crate) muid: String,
    /// `muidV2`.
    pub(crate) muid_v2: String,
    /// Windows: a value was missing from the registry, so both are written
    /// back at `RunEvent::Ready` ([`persist_machine_ids`]); nothing is
    /// written before (DESIGN §4.2).
    pub(crate) unsaved: bool,
}

impl MachineIds {
    /// `muid` and `muidV2` derived from one platform id.
    #[cfg_attr(
        all(windows, not(test)),
        expect(dead_code, reason = "Windows fills the ids in windows_ids")
    )]
    pub(crate) fn from_platform_id(id: &str) -> Self {
        let muid = machine_muid(id);
        MachineIds {
            muid_v2: muid.clone(),
            muid,
            unsaved: false,
        }
    }

    /// One per-install muid for both ids (`analytics.muidStrategy`
    /// `per-install`, or when the OS id is unavailable); it lives in
    /// `ow-tauri.json`, not the registry.
    pub(crate) fn per_install(muid: String) -> Self {
        MachineIds {
            muid_v2: muid.clone(),
            muid,
            unsaved: false,
        }
    }
}

/// Reads the machine ids, or says why they are unavailable.
///
/// macOS: `IOPlatformUUID` through `IOKit` (matches ow-electron, observed).
/// Windows: the registry values ow-electron apps share, else `muid` derived
/// from `MachineGuid` and `muidV2` from `new_install_id` (a random UUID v4,
/// as ow-electron creates it), written back at `RunEvent::Ready`
/// ([`windows_ids`], [`persist_machine_ids`]). Reading never writes. Linux
/// (inferred): `/etc/machine-id`, else `/var/lib/dbus/machine-id`.
pub(crate) fn machine_ids(new_install_id: impl FnOnce() -> String) -> Result<MachineIds, String> {
    #[cfg(not(windows))]
    let _ = new_install_id;
    #[cfg(target_os = "macos")]
    {
        macos::platform_uuid()
            .map(|id| MachineIds::from_platform_id(&id))
            .ok_or_else(|| "IOPlatformUUID could not be read".to_owned())
    }
    #[cfg(windows)]
    {
        windows::machine_ids(new_install_id)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux_machine_id(&[
            std::path::Path::new("/etc/machine-id"),
            std::path::Path::new("/var/lib/dbus/machine-id"),
        ])
        .map(|id| MachineIds::from_platform_id(&id))
        .ok_or_else(|| "no readable machine-id file".to_owned())
    }
}

/// The Windows machine ids (E.4) from the registry values ow-electron apps
/// share (`muid`, `muid_v2`), filling a missing `muid` from `machine_guid`
/// with the macOS formula and a missing `muidV2` with `new_install_id`: on
/// Windows ow-electron's `MUIDV2` is a separate per-install random UUID v4
/// (Windows lab: it differed on every runner while `MUID` did not).
#[cfg(any(test, windows))]
pub(crate) fn windows_ids(
    muid: Option<String>,
    muid_v2: Option<String>,
    machine_guid: impl FnOnce() -> Option<String>,
    new_install_id: impl FnOnce() -> String,
) -> Result<MachineIds, String> {
    let muid = match muid {
        Some(m) => m,
        None => {
            machine_muid(&machine_guid().ok_or_else(|| "MachineGuid could not be read".to_owned())?)
        }
    };
    Ok(MachineIds {
        muid,
        muid_v2: muid_v2.unwrap_or_else(new_install_id),
        unsaved: true,
    })
}

/// Writes machine ids that were missing from the registry (Windows, at
/// `RunEvent::Ready`; best effort: the uninstaller reads both values, I.6).
/// Nothing on other platforms, whose ids are derived from the OS.
pub(crate) fn persist_machine_ids(ids: &MachineIds) {
    #[cfg(windows)]
    if ids.unsaved {
        windows::persist(ids);
    }
    #[cfg(not(windows))]
    let _ = ids;
}

/// The first non-empty, trimmed content among `paths`.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
pub(crate) fn linux_machine_id(paths: &[&std::path::Path]) -> Option<String> {
    paths.iter().find_map(|p| {
        std::fs::read_to_string(p)
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    })
}

/// The CPU brand string, or `""` when it cannot be read.
#[allow(dead_code, reason = "the ads host (W2) reports it to the guests (D.2)")]
pub(crate) fn cpu_brand() -> String {
    #[cfg(target_os = "macos")]
    {
        macos::cpu_brand().unwrap_or_default()
    }
    #[cfg(windows)]
    {
        windows::cpu_brand().unwrap_or_default()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|text| cpuinfo_model_name(&text))
            .unwrap_or_default()
    }
}

/// The first `model name` value of `/proc/cpuinfo` text.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
pub(crate) fn cpuinfo_model_name(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "model name").then(|| value.trim().to_owned())
    })
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{c_char, c_void};

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFAllocatorRef = *const c_void;
    type CFMutableDictionaryRef = *mut c_void;
    type IoObject = u32;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    /// `kIOMainPortDefault` (`MACH_PORT_NULL`).
    const K_IO_MAIN_PORT_DEFAULT: u32 = 0;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
        fn IOServiceGetMatchingService(
            main_port: u32,
            matching: CFMutableDictionaryRef,
        ) -> IoObject;
        fn IORegistryEntryCreateCFProperty(
            entry: IoObject,
            key: CFStringRef,
            allocator: CFAllocatorRef,
            options: u32,
        ) -> CFTypeRef;
        fn IOObjectRelease(object: IoObject) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            alloc: CFAllocatorRef,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFStringGetCString(
            string: CFStringRef,
            buffer: *mut c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> u8;
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFRelease(cf: CFTypeRef);
    }

    /// `IOPlatformUUID` of `IOPlatformExpertDevice`.
    pub(super) fn platform_uuid() -> Option<String> {
        // SAFETY: IOServiceMatching takes a NUL-terminated C string and
        // returns a dictionary (or null) whose reference
        // IOServiceGetMatchingService consumes.
        let service = unsafe {
            let matching = IOServiceMatching(c"IOPlatformExpertDevice".as_ptr());
            if matching.is_null() {
                return None;
            }
            IOServiceGetMatchingService(K_IO_MAIN_PORT_DEFAULT, matching)
        };
        if service == 0 {
            return None;
        }
        // SAFETY: `service` is a valid registry entry until released below;
        // the key string is created and released here; the returned property
        // is owned by us (Create rule) and released after reading.
        unsafe {
            let key = CFStringCreateWithCString(
                std::ptr::null(),
                c"IOPlatformUUID".as_ptr(),
                K_CF_STRING_ENCODING_UTF8,
            );
            let value = if key.is_null() {
                std::ptr::null()
            } else {
                let v = IORegistryEntryCreateCFProperty(service, key, std::ptr::null(), 0);
                CFRelease(key);
                v
            };
            IOObjectRelease(service);
            if value.is_null() {
                return None;
            }
            let out = cf_string(value);
            CFRelease(value);
            out
        }
    }

    /// Copies a `CFString` into a Rust string.
    ///
    /// # Safety
    ///
    /// `value` must be a valid, non-null CF object.
    unsafe fn cf_string(value: CFTypeRef) -> Option<String> {
        // SAFETY: the caller guarantees `value` is a valid CF object; the
        // buffer and its length describe writable memory for the call.
        unsafe {
            if CFGetTypeID(value) != CFStringGetTypeID() {
                return None;
            }
            let mut buf = [0 as c_char; 256];
            let len = isize::try_from(buf.len()).ok()?;
            if CFStringGetCString(value, buf.as_mut_ptr(), len, K_CF_STRING_ENCODING_UTF8) == 0 {
                return None;
            }
            std::ffi::CStr::from_ptr(buf.as_ptr())
                .to_str()
                .ok()
                .map(str::to_owned)
        }
    }

    /// `sysctl machdep.cpu.brand_string`.
    pub(super) fn cpu_brand() -> Option<String> {
        let mut buf = [0_u8; 256];
        let mut len = buf.len();
        // SAFETY: the name is a NUL-terminated C string; `buf` and `len`
        // describe a writable buffer that outlives the call; no new value is
        // set (null pointer, length 0).
        let rc = unsafe {
            libc::sysctlbyname(
                c"machdep.cpu.brand_string".as_ptr(),
                buf.as_mut_ptr().cast(),
                &raw mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return None;
        }
        let text = std::str::from_utf8(buf.get(..len)?).ok()?;
        Some(text.trim_end_matches('\0').trim().to_owned())
    }
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
        REG_SZ, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegCloseKey, RegCreateKeyExW, RegGetValueW,
        RegSetValueExW,
    };

    use super::MachineIds;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Reads a `REG_SZ` value, from the 64-bit view.
    fn read_string(root: HKEY, subkey: &str, value: &str) -> Option<String> {
        let subkey = wide(subkey);
        let value = wide(value);
        let mut buf = vec![0_u16; 512];
        let mut size = u32::try_from(buf.len() * 2).ok()?;
        // SAFETY: the key and value names are NUL-terminated UTF-16; `buf`
        // and `size` describe a writable buffer that outlives the call.
        let rc = unsafe {
            RegGetValueW(
                root,
                subkey.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &raw mut size,
            )
        };
        if rc != ERROR_SUCCESS {
            return None;
        }
        let chars = usize::try_from(size).ok()? / 2;
        let text = String::from_utf16_lossy(buf.get(..chars)?);
        let text = text.trim_end_matches('\0').trim().to_owned();
        (!text.is_empty()).then_some(text)
    }

    /// Writes a `REG_SZ` value under `HKCU\<subkey>`, creating the key.
    fn write_string(subkey: &str, value: &str, data: &str) -> bool {
        let subkey = wide(subkey);
        let value = wide(value);
        let data = wide(data);
        let Ok(bytes) = u32::try_from(data.len() * 2) else {
            return false;
        };
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: names are NUL-terminated UTF-16; `key` receives an open
        // handle that is closed below; `data` outlives the call.
        unsafe {
            if RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &raw mut key,
                std::ptr::null_mut(),
            ) != ERROR_SUCCESS
            {
                return false;
            }
            let rc = RegSetValueExW(key, value.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), bytes);
            RegCloseKey(key);
            rc == ERROR_SUCCESS
        }
    }

    pub(super) fn machine_ids(
        new_install_id: impl FnOnce() -> String,
    ) -> Result<MachineIds, String> {
        let muid = read_string(HKEY_CURRENT_USER, "Software\\OverwolfElectron", "MUID");
        let muid_v2 = read_string(HKEY_CURRENT_USER, "Software\\OverwolfPersist", "MUIDV2");
        if let (Some(muid), Some(muid_v2)) = (&muid, &muid_v2) {
            return Ok(MachineIds {
                muid: muid.clone(),
                muid_v2: muid_v2.clone(),
                unsaved: false,
            });
        }
        super::windows_ids(
            muid,
            muid_v2,
            || {
                read_string(
                    HKEY_LOCAL_MACHINE,
                    "SOFTWARE\\Microsoft\\Cryptography",
                    "MachineGuid",
                )
            },
            new_install_id,
        )
    }

    /// Writes both ids to the registry values ow-electron apps share.
    pub(super) fn persist(ids: &MachineIds) {
        let _ = write_string("Software\\OverwolfElectron", "MUID", &ids.muid);
        let _ = write_string("Software\\OverwolfPersist", "MUIDV2", &ids.muid_v2);
    }

    pub(super) fn cpu_brand() -> Option<String> {
        read_string(
            HKEY_LOCAL_MACHINE,
            "HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0",
            "ProcessorNameString",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivation() {
        let ids = MachineIds::from_platform_id("DA3889E5-CB8A-8A15-CD1B-DCE6B5A71203");
        assert_eq!(ids.muid, "601860a3-90c7-b77b-a42e-636035921a81");
        assert_eq!(ids.muid_v2, ids.muid);
    }

    /// Regression (Windows lab): ow-electron's `MUIDV2` on Windows is a
    /// per-install random id, not the machine-derived `muid`.
    #[test]
    fn windows_ids_keep_the_shared_values_and_fill_the_missing_ones() {
        let guid = || Some("DA3889E5-CB8A-8A15-CD1B-DCE6B5A71203".to_owned());
        let install = || "0f0e0d0c-0b0a-4908-8706-050403020100".to_owned();
        let shared = windows_ids(Some("m".into()), Some("v".into()), || None, install).unwrap();
        assert_eq!((shared.muid.as_str(), shared.muid_v2.as_str()), ("m", "v"));
        // Filled values are written back at Ready, never while reading.
        assert!(shared.unsaved && !MachineIds::from_platform_id("x").unsaved);
        persist_machine_ids(&MachineIds::from_platform_id("x"));
        let fresh = windows_ids(None, None, guid, install).unwrap();
        assert_eq!(fresh.muid, "601860a3-90c7-b77b-a42e-636035921a81");
        assert_eq!(fresh.muid_v2, install());
        let v2_only = windows_ids(None, Some("v".into()), guid, || unreachable!()).unwrap();
        assert_eq!(v2_only.muid_v2, "v");
        assert!(windows_ids(None, None, || None, install).is_err());
    }

    #[test]
    fn linux_files() {
        let dir = crate::state::test_dir("machine-id");
        std::fs::create_dir_all(&dir).unwrap();
        let empty = dir.join("empty");
        let full = dir.join("full");
        std::fs::write(&empty, "  \n").unwrap();
        std::fs::write(&full, "0123abcd\n").unwrap();
        let missing = dir.join("missing");
        assert_eq!(
            linux_machine_id(&[&missing, &empty, &full]).as_deref(),
            Some("0123abcd")
        );
        assert_eq!(linux_machine_id(&[&missing]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cpuinfo() {
        let text =
            "processor\t: 0\nvendor_id\t: GenuineIntel\nmodel name\t: Example CPU @ 3.00GHz\n";
        assert_eq!(
            cpuinfo_model_name(text).as_deref(),
            Some("Example CPU @ 3.00GHz")
        );
        assert_eq!(cpuinfo_model_name("x"), None);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_reads_the_platform_uuid() {
        let ids = machine_ids(String::new).unwrap();
        assert_eq!(ids.muid.len(), 36);
        assert_eq!(ids.muid, ids.muid.to_lowercase());
        assert!(!cpu_brand().is_empty());
    }
}
