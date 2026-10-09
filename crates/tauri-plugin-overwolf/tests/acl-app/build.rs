//! Compiles the fixture's ACL.
//!
//! Windows (MSVC): the application manifest (Common Controls v6, which Tauri
//! links against) is embedded into every linked target, not only the
//! `fake-installer` binary. With a `[[bin]]` in the package, tauri-build's
//! resource reaches binaries only, and the test executables then fail to
//! start (`STATUS_ENTRYPOINT_NOT_FOUND`).

use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut attributes = tauri_build::Attributes::new();
    let windows = env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|e| e == "msvc");
    if windows && msvc {
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        let manifest =
            PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::try_build(attributes)?;
    Ok(())
}
