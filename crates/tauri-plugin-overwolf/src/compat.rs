//! Window and webview lookups that work with and without Tauri's
//! `unstable` feature (DESIGN §4.5, SPA F3).
//!
//! With `unstable` (feature `ads` on Windows and macOS) a window that hosts
//! an ad guest has several webviews, and `Manager::get_webview_window`
//! returns `None` for it; the plugin therefore looks windows and webviews
//! up with `get_window` / `get_webview` there, and through the
//! window's single webview otherwise.

use tauri::{Manager, Runtime, Webview, Window};

/// The window `label`.
#[cfg(ow_tauri_ads)]
pub(crate) fn window<R: Runtime, M: Manager<R>>(manager: &M, label: &str) -> Option<Window<R>> {
    manager.get_window(label)
}

/// The window `label`.
#[cfg(not(ow_tauri_ads))]
pub(crate) fn window<R: Runtime, M: Manager<R>>(manager: &M, label: &str) -> Option<Window<R>> {
    manager
        .get_webview_window(label)
        .map(|w| AsRef::<Webview<R>>::as_ref(&w).window())
}

/// The webview `label`.
#[cfg(ow_tauri_ads)]
#[allow(dead_code, reason = "the ads and consent hosts (W2) look webviews up")]
pub(crate) fn webview<R: Runtime, M: Manager<R>>(manager: &M, label: &str) -> Option<Webview<R>> {
    manager.get_webview(label)
}

/// The webview `label`.
#[cfg(not(ow_tauri_ads))]
#[allow(dead_code, reason = "the ads and consent hosts (W2) look webviews up")]
pub(crate) fn webview<R: Runtime, M: Manager<R>>(manager: &M, label: &str) -> Option<Webview<R>> {
    manager
        .get_webview_window(label)
        .map(|w| AsRef::<Webview<R>>::as_ref(&w).clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups_find_app_windows() {
        let app = tauri::test::mock_app();
        tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        assert_eq!(window(&app, "main").unwrap().label(), "main");
        assert_eq!(webview(&app, "main").unwrap().label(), "main");
        assert!(window(&app, "missing").is_none());
    }
}
