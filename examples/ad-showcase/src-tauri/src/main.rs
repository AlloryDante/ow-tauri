//! The ad showcase's native shell. All app logic is the showcase's
//! TypeScript, running in the plugin's hidden main webview (the main process)
//! and in one window; this file only registers the plugins (and, with the
//! `lab` feature, the invisible lab of `e2e/README.md`).

// No console window next to the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "lab")]
mod lab;

use tauri_plugin_overwolf::OverwolfExt;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = tauri_plugin_overwolf::embedded_manifest!();
    #[cfg(feature = "lab")]
    let manifest = lab::manifest(manifest)?;

    let builder = tauri::Builder::default()
        // Single instance first (CONTRACT A.5): a second launch reaches the
        // running app as `app.on('second-instance')`.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            app.overwolf().emit_second_instance(argv, cwd);
        }))
        .plugin(
            tauri_plugin_overwolf::Builder::new()
                .manifest_json(manifest)
                .build(),
        );

    #[cfg(feature = "lab")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        lab::e2e_config,
        lab::e2e_record,
        lab::e2e_probe_guests,
        lab::e2e_native_probe,
        lab::e2e_still
    ]);
    // An invisible lab app never comes to the front: activated at launch, it
    // would take the keyboard from the app the user is typing in.
    #[cfg(all(feature = "lab", target_os = "macos"))]
    let builder = builder.activate_ignoring_other_apps(!lab::invisible());

    // macOS reports a crashed web content process only through this hook;
    // the plugin restarts the main webview, recovers ad guests and emits
    // `render-process-gone` for app windows (CONTRACT A.6).
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(|webview| {
        webview
            .overwolf()
            .report_web_content_terminated(webview.label());
    });

    #[allow(unused_mut, reason = "mutated by the macOS lab only")]
    let mut app = builder.build(tauri::generate_context!())?;
    // The lab keeps the app out of the Dock and the app switcher before the
    // event loop starts.
    #[cfg(all(feature = "lab", target_os = "macos"))]
    if lab::invisible() {
        app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
    app.run(|_, _| {});
    Ok(())
}
