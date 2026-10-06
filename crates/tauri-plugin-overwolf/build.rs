//! Build script: generates the plugin's permission files (one
//! `allow-<command>` per command, plus the sets in `permissions/`) and stages
//! the runtime scripts the plugin embeds.

use std::path::Path;

include!("src/commands/list.rs");

/// Scripts the plugin embeds with `include_str!`. They are built from
/// `packages/ow-tauri` into `js/`; until a build has produced one, a
/// placeholder that reports the missing build is embedded instead.
const SCRIPTS: &[&str] = &["bootstrap.js"];

fn stage_scripts() -> std::io::Result<()> {
    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        return Ok(());
    };
    for name in SCRIPTS {
        let source = Path::new("js").join(name);
        println!("cargo:rerun-if-changed={}", source.display());
        let text = std::fs::read_to_string(&source).unwrap_or_else(|_| {
            format!(
                "console.error('ow-tauri: js/{name} is missing from tauri-plugin-overwolf; build packages/ow-tauri first.');\n"
            )
        });
        let target = Path::new(&out_dir).join(name);
        if std::fs::read_to_string(&target).ok().as_deref() != Some(text.as_str()) {
            std::fs::write(&target, text)?;
        }
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    stage_scripts()?;
    tauri_plugin::Builder::new(COMMANDS).build();
    Ok(())
}
