#[cfg(all(feature = "ads", any(windows, target_os = "macos")))]
pub fn add_guest<R: tauri::Runtime>(w: &tauri::Window<R>, b: tauri::webview::WebviewBuilder<R>) -> tauri::Result<tauri::Webview<R>> {
    w.add_child(b, tauri::LogicalPosition::new(0.0, 0.0), tauri::LogicalSize::new(1.0, 1.0))
}
