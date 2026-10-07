//! Embeds the staged app manifest (`../.stage/package.json`, written by
//! `scripts/stage.mjs` with the local identity merged in; CONTRACT G.3),
//! writes the NSIS installer hooks (CONTRACT I.6) and runs `tauri-build`.

use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let manifest_dir = Path::new(&manifest_dir);
    let staged = manifest_dir.join("../.stage/package.json");
    if !staged.exists() {
        return Err(format!(
            "{} is missing: run `npm run stage` (or `node scripts/stage.mjs --host tauri`) in examples/ad-showcase first",
            staged.display()
        )
        .into());
    }
    tauri_plugin_overwolf::build::embed_manifest(&staged)?;
    tauri_plugin_overwolf::build::write_nsis_installer_hooks(
        &staged,
        None,
        "tauri",
        &manifest_dir.join("windows/hooks.nsh"),
    )?;

    tauri_build::try_build(tauri_build::Attributes::new())?;
    Ok(())
}
