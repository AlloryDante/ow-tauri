//! The sample's native side: the plugins and the window. Everything
//! Overwolf-specific (ads, consent, analytics, identity, the Windows
//! updater) is `tauri-plugin-overwolf`; the page calls it directly through
//! `tauri-plugin-overwolf-api`.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::webview::PageLoadEvent;
use tauri::window::Color;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, Window};

/// The label of the sample window and of its webview (the capability in
/// `capabilities/default.json` names this webview).
pub const MAIN: &str = "main";
/// The window size (it fits the ads tester's widest layout).
const SIZE: (f64, f64) = (1280.0, 860.0);
/// The window's minimum size.
const MIN_SIZE: (f64, f64) = (1000.0, 680.0);
/// The window background while the page loads (the page's dark theme).
const BACKGROUND: Color = Color(0x10, 0x10, 0x10, 0xff);

/// Starts the app.
///
/// # Errors
///
/// Tauri could not build or run the app.
pub fn run() -> tauri::Result<()> {
    #[cfg(feature = "lab")]
    crate::lab::hold_app_back();
    let builder = tauri::Builder::default()
        // Single instance first: a second launch focuses this app and the
        // Overwolf plugin writes nothing in the second process.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_window(MAIN) {
                let _ = window.unminimize();
                show(&window);
                let _ = window.set_focus();
            }
        }))
        // Log plugin before the Overwolf plugin, so its setup messages
        // reach it.
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(overwolf())
        .setup(|app| {
            create_window(app.handle())?;
            Ok(())
        });

    // The lab driver's commands (the sample itself has none).
    #[cfg(feature = "lab")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        crate::lab::e2e_config,
        crate::lab::e2e_record,
        crate::lab::e2e_window,
        crate::lab::e2e_quit,
    ]);
    // An invisible lab app is never activated (it would take the keyboard
    // from the app the user is typing in).
    #[cfg(all(feature = "lab", target_os = "macos"))]
    let builder = builder.activate_ignoring_other_apps(!crate::lab::invisible());

    // macOS: lets the plugin recover crashed ad guests and report them.
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(
        tauri_plugin_overwolf::web_content_process_terminate_hook(),
    );

    #[allow(unused_mut, reason = "mutated by the macOS lab only")]
    let mut app = builder.build(tauri::generate_context!())?;
    // The lab keeps the app out of the Dock and the app switcher.
    #[cfg(all(feature = "lab", target_os = "macos"))]
    if crate::lab::invisible() {
        app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
    app.run(|_, _| {});
    Ok(())
}

/// The Overwolf plugin: its configuration is `plugins.overwolf` in
/// `tauri.conf.json`; the lab adds loopback endpoints.
fn overwolf<R: Runtime>() -> tauri::plugin::TauriPlugin<R, Option<tauri_plugin_overwolf::Config>> {
    let builder = tauri_plugin_overwolf::Builder::new();
    #[cfg(feature = "lab")]
    let builder = crate::lab::overwolf_builder(builder);
    builder.build()
}

/// Builds the sample window, hidden until its page has loaded (no white
/// flash before the dark page).
fn create_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let title = app.package_info().name.clone();
    let window = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(SIZE.0, SIZE.1)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .background_color(BACKGROUND)
        .visible(false)
        .on_page_load(|webview, payload| {
            static SHOWN: AtomicBool = AtomicBool::new(false);
            if payload.event() == PageLoadEvent::Finished && !SHOWN.swap(true, Ordering::SeqCst) {
                show(&webview.as_ref().window());
            }
        })
        .build()?;
    #[cfg(feature = "lab")]
    crate::lab::prepare_window(&window.as_ref().window());
    #[cfg(not(feature = "lab"))]
    let _ = window;
    Ok(())
}

/// Shows a window (in the invisible lab: on screen at alpha 0, without
/// becoming key or activating the app).
fn show<R: Runtime>(window: &Window<R>) {
    #[cfg(feature = "lab")]
    if crate::lab::invisible() {
        crate::lab::order_front(window);
        return;
    }
    let _ = window.show();
}
