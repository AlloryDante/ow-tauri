#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        // Register the log plugin first so the overwolf plugin's setup messages reach it.
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_overwolf::init());

    // macOS only: lets the plugin recover crashed ad guests and report them.
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(
        tauri_plugin_overwolf::web_content_process_terminate_hook(),
    );

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
