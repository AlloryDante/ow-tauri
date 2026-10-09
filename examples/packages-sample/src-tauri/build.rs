//! The sample's build step: tauri-plugin-overwolf reads the merged
//! `tauri.conf.json` (platform overlay and `TAURI_CONFIG`), validates
//! `plugins.overwolf` and the capabilities, and on Windows writes the NSIS
//! installer hooks; then `tauri-build` runs.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tauri_plugin_overwolf::build::run()?;
    tauri_build::try_build(tauri_build::Attributes::new())?;
    Ok(())
}
