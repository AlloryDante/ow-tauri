//! The platform part of `<UA>` (CONTRACT E.1, DESIGN §4.10): the per-OS
//! template and the shape check a natively read user agent must pass.
//!
//! ow-electron sends its default Chromium user agent whatever user agent
//! the app sets on its own windows (E.1). ow-tauri reads the user agent of
//! an app webview and accepts it only when it has the platform default's
//! shape, so an app-set user agent (`userAgent` in a window's
//! configuration, `WebviewBuilder::user_agent`) never reaches Overwolf's
//! servers. Anything else, and every case where nothing can be read, uses
//! the template, which equals what the platform webview reports by default
//! (verified on macOS 26 and on the Windows runner's `WebView2`).
//!
//! ```
//! use tauri_plugin_overwolf::analytics::user_agent::{accepts_native, template};
//! use tauri_plugin_overwolf::paths::TargetOs;
//! let ua = template(TargetOs::Windows, "141.0.3537.57", "x86_64");
//! assert!(ua.ends_with("Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0"));
//! assert!(accepts_native(TargetOs::Windows, &ua));
//! assert!(!accepts_native(TargetOs::Windows, "Custom/1.0"));
//! ```

use crate::paths::TargetOs;

/// The default `WKWebView` user agent (macOS; no Safari tokens: wry sets no
/// application name). A natively read macOS user agent must equal it.
pub const MACOS_TEMPLATE: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";

/// The platform webview's default user agent on `os`, built without a
/// webview: the user agent that `<UA>` is composed from when no app webview
/// can be read (DESIGN §4.10).
///
/// - Windows: `WebView2`'s reduced user agent, which carries only the
///   runtime's major version (`webview_version`, as `tauri::webview_version()`
///   reports it).
/// - macOS: [`MACOS_TEMPLATE`].
/// - Linux: the `WebKitGTK` form for `arch` (Rust's `std::env::consts::ARCH`).
///
/// ```
/// use tauri_plugin_overwolf::analytics::user_agent::template;
/// use tauri_plugin_overwolf::paths::TargetOs;
/// assert_eq!(
///     template(TargetOs::Windows, "153.0.4234.48", "x86_64"),
///     "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/153.0.0.0 Safari/537.36 Edg/153.0.0.0"
/// );
/// assert_eq!(
///     template(TargetOs::Linux, "2.48.0", "aarch64"),
///     "Mozilla/5.0 (X11; Linux aarch64) AppleWebKit/605.1.15 (KHTML, like Gecko)"
/// );
/// ```
#[must_use]
pub fn template(os: TargetOs, webview_version: &str, arch: &str) -> String {
    match os {
        TargetOs::Windows => {
            let major = webview_version.trim().split('.').next().unwrap_or_default();
            format!(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36 Edg/{major}.0.0.0"
            )
        }
        TargetOs::Macos => MACOS_TEMPLATE.to_owned(),
        TargetOs::Linux => {
            format!("Mozilla/5.0 (X11; Linux {arch}) AppleWebKit/605.1.15 (KHTML, like Gecko)")
        }
    }
}

/// Whether a user agent read from an app webview has the platform default's
/// shape and may be used for `<UA>` (DESIGN §4.10, PAR-M3a). Windows: the
/// reduced `WebView2` user agent,
/// `^Mozilla/5\.0 \(Windows NT [\d.]+; (Win64; x64|ARM64)\) AppleWebKit/537\.36 \(KHTML, like Gecko\) Chrome/\d+\.0\.0\.0 Safari/537\.36 Edg/\d+\.0\.0\.0$`;
/// macOS: exactly [`MACOS_TEMPLATE`]; Linux: never (no native read).
///
/// ```
/// use tauri_plugin_overwolf::analytics::user_agent::{accepts_native, MACOS_TEMPLATE};
/// use tauri_plugin_overwolf::paths::TargetOs;
/// assert!(accepts_native(TargetOs::Macos, MACOS_TEMPLATE));
/// assert!(!accepts_native(TargetOs::Macos, &format!("{MACOS_TEMPLATE} MyApp/1.0")));
/// ```
#[must_use]
pub fn accepts_native(os: TargetOs, ua: &str) -> bool {
    match os {
        TargetOs::Windows => windows_shape(ua),
        TargetOs::Macos => ua == MACOS_TEMPLATE,
        TargetOs::Linux => false,
    }
}

/// `\d+` followed by `rest`: the digits are at least one ASCII digit.
fn digits_then<'a>(s: &'a str, rest: &str) -> Option<&'a str> {
    let n = s.bytes().take_while(u8::is_ascii_digit).count();
    (n > 0).then(|| s[n..].strip_prefix(rest)).flatten()
}

fn windows_shape(ua: &str) -> bool {
    let check = || -> Option<()> {
        let s = ua.strip_prefix("Mozilla/5.0 (Windows NT ")?;
        let n = s
            .bytes()
            .take_while(|b| b.is_ascii_digit() || *b == b'.')
            .count();
        if n == 0 {
            return None;
        }
        let s = s[n..].strip_prefix("; ")?;
        let s = s
            .strip_prefix("Win64; x64)")
            .or_else(|| s.strip_prefix("ARM64)"))?;
        let s = s.strip_prefix(" AppleWebKit/537.36 (KHTML, like Gecko) Chrome/")?;
        let s = digits_then(s, ".0.0.0 Safari/537.36 Edg/")?;
        let s = digits_then(s, ".0.0.0")?;
        s.is_empty().then_some(())
    };
    check().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DESIGN §7.1: the templates and the shape rule per OS.
    #[test]
    fn shape_table() {
        let win = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/153.0.0.0 Safari/537.36 Edg/153.0.0.0";
        let cases: &[(TargetOs, &str, bool)] = &[
            (TargetOs::Windows, win, true),
            // The runner's WebView2 (W0c B2).
            (TargetOs::Windows, &win.replace("153", "141"), true),
            (TargetOs::Windows, &win.replace("Win64; x64", "ARM64"), true),
            (TargetOs::Windows, &win.replace("10.0;", "6.3.9600;"), true),
            // Full (unreduced) versions, app tokens and custom strings fail.
            (
                TargetOs::Windows,
                &win.replace("Chrome/153.0.0.0", "Chrome/153.0.4234.48"),
                false,
            ),
            (TargetOs::Windows, &format!("{win} MyApp/1.0"), false),
            (
                TargetOs::Windows,
                &win.replace("Edg/153.0.0.0", "Edg/"),
                false,
            ),
            (TargetOs::Windows, &win.replace("NT 10.0", "NT "), false),
            (TargetOs::Windows, "Custom/1.0", false),
            (TargetOs::Windows, "", false),
            (TargetOs::Macos, MACOS_TEMPLATE, true),
            (TargetOs::Macos, "CustomApp/9.9 (Macintosh)", false),
            (TargetOs::Macos, win, false),
            (
                TargetOs::Linux,
                &template(TargetOs::Linux, "", "x86_64"),
                false,
            ),
        ];
        for (os, ua, accepted) in cases {
            assert_eq!(accepts_native(*os, ua), *accepted, "{os:?} {ua}");
        }
    }

    #[test]
    fn templates_pass_their_own_shape_check() {
        for v in ["98.0.1108.44", "141.0.3537.57", "153.0.4234.48"] {
            assert!(accepts_native(
                TargetOs::Windows,
                &template(TargetOs::Windows, v, "x86_64")
            ));
        }
        assert!(accepts_native(
            TargetOs::Macos,
            &template(TargetOs::Macos, "", "aarch64")
        ));
        assert_eq!(
            template(TargetOs::Linux, "", "x86_64"),
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)"
        );
    }
}
