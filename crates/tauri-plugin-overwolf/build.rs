//! Build script: generates the plugin's permission files (one
//! `allow-<command>` per command, plus the sets in `permissions/`) and stages
//! the runtime scripts the plugin embeds.

use std::path::Path;

include!("src/commands/list.rs");

/// Scripts the plugin embeds with `include_str!`. They are built from
/// `packages/ow-tauri` into `js/` by `npm run build:injected --workspace
/// ow-tauri`.
const SCRIPTS: &[&str] = &[
    "bootstrap.js",
    "adview-host.js",
    "cmp.js",
    "native-dialogs.js",
];

/// Whether a missing script fails the build. A release build without the
/// runtime would ship an app with no `ow-main` runtime and no IPC, so it
/// fails unless `OW_TAURI_ALLOW_MISSING_JS=1` (CI jobs that only test the
/// Rust side in release mode). `OW_TAURI_REQUIRE_JS=1` makes debug builds
/// strict too. docs.rs builds never fail.
fn scripts_required() -> bool {
    let flag = |name: &str| std::env::var(name).is_ok_and(|v| v == "1");
    if std::env::var_os("DOCS_RS").is_some() || flag("OW_TAURI_ALLOW_MISSING_JS") {
        return false;
    }
    flag("OW_TAURI_REQUIRE_JS") || std::env::var("PROFILE").is_ok_and(|p| p == "release")
}

fn stage_scripts() -> Result<(), String> {
    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        return Ok(());
    };
    println!("cargo:rerun-if-env-changed=OW_TAURI_REQUIRE_JS");
    println!("cargo:rerun-if-env-changed=OW_TAURI_ALLOW_MISSING_JS");
    let required = scripts_required();
    for name in SCRIPTS {
        let source = Path::new("js").join(name);
        println!("cargo:rerun-if-changed={}", source.display());
        let text = match std::fs::read_to_string(&source) {
            Ok(text) => text,
            Err(_) if required => {
                return Err(format!(
                    "tauri-plugin-overwolf: js/{name} is missing. Build the runtime first \
                     (`npm run build:injected --workspace ow-tauri`), or set \
                     OW_TAURI_ALLOW_MISSING_JS=1 for a build that never runs the app."
                ));
            }
            Err(_) => {
                println!(
                    "cargo:warning=js/{name} is missing; embedding a placeholder that only reports it \
                     (run `npm run build:injected --workspace ow-tauri`)"
                );
                format!(
                    "console.error('ow-tauri: js/{name} is missing from tauri-plugin-overwolf; build packages/ow-tauri first.');\n"
                )
            }
        };
        let target = Path::new(&out_dir).join(name);
        if std::fs::read_to_string(&target).ok().as_deref() != Some(text.as_str()) {
            std::fs::write(&target, text)
                .map_err(|e| format!("writing {}: {e}", target.display()))?;
        }
    }
    Ok(())
}

/// The application manifest of this crate's own test executables on
/// Windows (MSVC): a dependency on Common Controls v6, as `tauri-build`
/// embeds in an app. Tauri's dialog code imports `TaskDialogIndirect`,
/// which only that version exports, so without the manifest the loader
/// refuses the unit-test executable (`STATUS_ENTRYPOINT_NOT_FOUND`) before
/// any test runs.
const TEST_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
</assembly>
"#;

/// Embeds [`TEST_MANIFEST`] in the executables cargo links for this package
/// (its unit tests and doctests; it has no binaries or examples). Link
/// arguments of a library's build script never reach the crates that depend
/// on it, so an app's executable keeps the manifest its own build embeds.
fn embed_test_manifest() -> Result<(), String> {
    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    if !windows_msvc || std::env::var_os("CARGO_FEATURE_PLUGIN").is_none() {
        return Ok(());
    }
    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        return Ok(());
    };
    let manifest = Path::new(&out_dir).join("test-app.manifest");
    std::fs::write(&manifest, TEST_MANIFEST)
        .map_err(|e| format!("writing {}: {e}", manifest.display()))?;
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    Ok(())
}

fn main() -> Result<(), String> {
    stage_scripts()?;
    embed_test_manifest()?;
    tauri_plugin::Builder::new(COMMANDS).build();
    Ok(())
}
