//! Compiles the fixture's ACL.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tauri_build::try_build(tauri_build::Attributes::new())?;
    Ok(())
}
