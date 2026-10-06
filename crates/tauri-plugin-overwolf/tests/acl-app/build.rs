//! Embeds the fixture manifest and compiles the fixture's ACL.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tauri_plugin_overwolf::build::embed_manifest("package.json")?;
    tauri_build::try_build(tauri_build::Attributes::new())?;
    Ok(())
}
