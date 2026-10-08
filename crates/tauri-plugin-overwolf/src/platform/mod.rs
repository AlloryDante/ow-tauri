//! Small OS facts the plugin needs: local time for log lines, the kernel or OS
//! release (Node's `os.release()`), the macOS major version (A.6 liveness),
//! and whether a file carries an execute bit (A.2.3.2).

#[cfg(feature = "plugin")]
pub(crate) mod graphics;
#[cfg(feature = "plugin")]
pub(crate) mod js_dialogs;
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

/// Where the installed Safari bundle lives: `/Applications/Safari.app` (on
/// macOS 13 and newer a link into the Safari cryptex), then the cryptex path
/// itself.
#[cfg(all(target_os = "macos", feature = "plugin"))]
const SAFARI_BUNDLES: [&str; 2] = [
    "/Applications/Safari.app",
    "/System/Cryptexes/App/System/Applications/Safari.app",
];

/// Safari's `Version/` token for `<UA>` (E.1): the installed Safari's
/// `CFBundleShortVersionString`, major and minor only, read once. `None` off
/// macOS (WebView2 and WebKitGTK bring their own browser tokens) or when no
/// Safari bundle can be read.
///
/// The bundle's `Info.plist` is read through `NSBundle`, which accepts both
/// property list encodings (XML and binary); system bundles ship either.
#[cfg(feature = "plugin")]
pub(crate) fn safari_version() -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        static VERSION: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        VERSION
            .get_or_init(|| {
                SAFARI_BUNDLES.iter().find_map(|path| {
                    let short = bundle_short_version(path)?;
                    crate::analytics::safari_ua_version(&short)
                })
            })
            .as_deref()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// The `CFBundleShortVersionString` of the bundle at `path`, in whichever
/// encoding its `Info.plist` uses.
#[cfg(all(target_os = "macos", feature = "plugin"))]
fn bundle_short_version(path: &str) -> Option<String> {
    use objc2_foundation::{NSBundle, NSString};
    let bundle = NSBundle::bundleWithPath(&NSString::from_str(path))?;
    let value =
        bundle.objectForInfoDictionaryKey(&NSString::from_str("CFBundleShortVersionString"))?;
    Some(value.downcast::<NSString>().ok()?.to_string())
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
    #[cfg(all(target_os = "macos", feature = "plugin"))]
    fn safari_version_is_read() {
        // Safari is part of every macOS install, but a stripped-down image
        // (some CI runners) can lack the bundle. Where an `Info.plist` is on
        // disk, its version must be read, whatever its encoding. CI's macOS
        // job sets `OW_TAURI_EXPECT_SAFARI=1` and runs this test with
        // `--nocapture`, so its log shows that a plist was read.
        let on_disk: Vec<_> = SAFARI_BUNDLES
            .iter()
            .map(|path| std::path::Path::new(path).join("Contents/Info.plist"))
            .filter(|plist| plist.is_file())
            .collect();
        let version = safari_version();
        println!("safari_version_is_read: version {version:?} from {on_disk:?}");
        if std::env::var_os("OW_TAURI_EXPECT_SAFARI").is_some() {
            assert!(
                version.is_some(),
                "OW_TAURI_EXPECT_SAFARI is set but no Safari version was read"
            );
        }
        match version {
            Some(v) => assert_eq!(v.split('.').count(), 2, "{v}"),
            None => assert!(
                on_disk.is_empty(),
                "Safari's Info.plist exists but its version was not read: {:?}",
                on_disk
                    .iter()
                    .map(|plist| (plist, std::fs::read(plist).map(|b| b.len())))
                    .collect::<Vec<_>>()
            ),
        }
    }

    #[test]
    #[cfg(all(target_os = "macos", feature = "plugin"))]
    fn bundle_versions_are_read_from_binary_plists() {
        let dir = std::env::temp_dir().join(format!("ow-tauri-bundle-{}", std::process::id()));
        let contents = dir.join("Test.app/Contents");
        std::fs::create_dir_all(&contents).expect("temp bundle");
        let xml = contents.join("Info.plist");
        std::fs::write(
            &xml,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>test.ow-tauri.bundle</string><key>CFBundleShortVersionString</key><string>18.6.1</string></dict></plist>\n",
        )
        .expect("Info.plist");
        let converted = std::process::Command::new("/usr/bin/plutil")
            .args(["-convert", "binary1"])
            .arg(&xml)
            .status()
            .expect("plutil");
        assert!(converted.success());
        assert_eq!(
            std::fs::read(&xml).expect("binary plist").get(..8),
            Some(&b"bplist00"[..])
        );
        let version = bundle_short_version(&dir.join("Test.app").to_string_lossy());
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(version.as_deref(), Some("18.6.1"));
    }

    #[test]
    #[cfg(feature = "plugin")]
    fn os_release_is_not_empty() {
        assert!(!os_release().is_empty());
    }
}
