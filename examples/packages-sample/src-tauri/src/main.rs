//! The ported sample's native shell. All app logic is the upstream
//! TypeScript, running in the plugin's hidden main webview; this file only
//! registers the plugins.

// No console window next to the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri_plugin_overwolf::OverwolfExt;

fn main() -> Result<(), tauri::Error> {
    let builder = tauri::Builder::default()
        // Single instance first (CONTRACT A.5): a second launch reaches the
        // running app as `app.on('second-instance')`.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            app.overwolf().emit_second_instance(argv, cwd);
        }))
        .plugin(
            tauri_plugin_overwolf::Builder::new()
                .manifest_json(tauri_plugin_overwolf::embedded_manifest!())
                .build(),
        );

    // macOS reports a crashed web content process only through this hook;
    // the plugin restarts the main webview, recovers ad guests and emits
    // `render-process-gone` for app windows (CONTRACT A.6).
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(|webview| {
        webview
            .overwolf()
            .report_web_content_terminated(webview.label());
    });

    builder.run(tauri::generate_context!())
}
