//! Stand-in plugin crate (W0c-B7). Code that needs `unstable` names the
//! package through the ordinary `tauri` name; the renamed `tauri-unstable`
//! dependency exists only to switch the feature on for Windows and macOS.

/// True when this build carries the `unstable` APIs (Windows/macOS + `ads`).
pub const HAS_UNSTABLE: bool = cfg!(all(feature = "ads", any(windows, target_os = "macos")));

/// Adds a child webview to `window` (needs `unstable`).
#[cfg(all(feature = "ads", any(windows, target_os = "macos")))]
pub fn add_guest<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    builder: tauri::webview::WebviewBuilder<R>,
    position: tauri::LogicalPosition<f64>,
    size: tauri::LogicalSize<f64>,
) -> tauri::Result<tauri::Webview<R>> {
    window.add_child(builder, position, size)
}
