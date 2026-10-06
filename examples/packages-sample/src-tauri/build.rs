//! Embeds the app manifest (`../package.json`, CONTRACT G.3), writes the NSIS
//! installer hooks (CONTRACT I.6) and runs `tauri-build`.

use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tauri_plugin_overwolf::build::embed_manifest("../package.json")?;

    // Overwolf's install and uninstall work, as ow-electron-builder's NSIS
    // script does it. `tauri.conf.json` points `installerHooks` here.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let manifest_dir = Path::new(&manifest_dir);
    tauri_plugin_overwolf::build::write_nsis_installer_hooks(
        &manifest_dir.join("../package.json"),
        None,
        "tauri",
        &manifest_dir.join("windows/hooks.nsh"),
    )?;

    tauri_build::try_build(tauri_build::Attributes::new())?;
    Ok(())
}
