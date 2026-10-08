//! OS facts and native webview operations the plugin needs: the kernel or
//! OS release (Node's `os.release()`), Safari's version for `<UA>`, machine
//! ids, displays, graphics and the guest webview hooks.

#[cfg(feature = "plugin")]
pub(crate) mod display;
#[cfg(feature = "plugin")]
pub(crate) mod graphics;
#[cfg(all(target_os = "macos", feature = "plugin"))]
pub(crate) mod input;
#[cfg(feature = "plugin")]
pub(crate) mod machine;
#[cfg(all(target_os = "macos", feature = "plugin"))]
pub(crate) mod terminate;
#[cfg(all(unix, feature = "plugin"))]
mod unix;
#[cfg(feature = "plugin")]
pub(crate) mod webview;
#[cfg(all(windows, feature = "plugin"))]
mod windows;

#[cfg(all(unix, feature = "plugin"))]
pub(crate) use unix::os_release;
#[cfg(all(windows, feature = "plugin"))]
pub(crate) use windows::os_release;

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

#[cfg(all(test, feature = "plugin"))]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
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
    #[cfg(target_os = "macos")]
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
    fn os_release_is_not_empty() {
        assert!(!os_release().is_empty());
    }
}
