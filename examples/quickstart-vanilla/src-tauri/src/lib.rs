use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        // Register single-instance first: a second launch focuses this app, and the overwolf plugin
        // writes nothing in the second process before it exits (README, "Other plugins").
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // `get_window`: a window that hosts an ad has more than one webview.
            if let Some(window) = app.get_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        // Remember the window's size and position, but not the plugin's consent windows (README).
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_filter(|label| !label.starts_with("ow-cmp"))
                .build(),
        )
        // Register the log plugin before the overwolf plugin so the overwolf plugin's setup messages reach it (§4.16).
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_overwolf::init());

    // macOS only: lets the plugin recover crashed ad guests and report them (D13a).
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(
        tauri_plugin_overwolf::web_content_process_terminate_hook(),
    );

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
