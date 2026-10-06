//! Fixture for the plugin's mock-runtime tests: the compiled context (ACL from
//! the plugin's permission sets and `capabilities/renderer.json`) and the
//! embedded fixture manifest.

/// The fixture app's context with its compiled ACL.
#[must_use]
pub fn context() -> tauri::Context<tauri::test::MockRuntime> {
    tauri::generate_context!(test = true)
}

/// The embedded fixture manifest (`package.json` here).
#[must_use]
pub fn manifest() -> &'static str {
    tauri_plugin_overwolf::embedded_manifest!()
}
