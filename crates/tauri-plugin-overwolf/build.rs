//! Build script: generates the plugin's permission files (one
//! `allow-<command>` per command of `src/commands/list.rs`, plus the sets in
//! `permissions/`), registers `api-iife.js` as the plugin's global API
//! script (`withGlobalTauri`), and sets the release guard's cfgs
//! (DESIGN §6.3).

use std::path::Path;

include!("src/commands/list.rs");

/// Set when `OW_TAURI_ALLOW_DEV_FEATURES_IN_RELEASE=1`: a lab release build
/// may enable `lab` / `test-util`.
const ALLOW_DEV_FEATURES: &str = "ow_tauri_allow_dev_features";
/// Set for an optimised build (`PROFILE=release` or `OPT_LEVEL` other than
/// `0`), so a release profile with `debug-assertions = true` is still caught.
const RELEASE_PROFILE: &str = "ow_tauri_release_profile";
/// The environment switch behind [`ALLOW_DEV_FEATURES`].
const ALLOW_DEV_FEATURES_ENV: &str = "OW_TAURI_ALLOW_DEV_FEATURES_IN_RELEASE";
/// Set when feature `ads` is on and the target is Windows or macOS: Tauri's
/// `unstable` API (child webviews) is available through the shim crate.
const ADS_CFG: &str = "ow_tauri_ads";

/// Tells an app's build step (`build::run`, through the links metadata
/// `DEP_TAURI_PLUGIN_OVERWOLF_UPDATER` and `DEP_TAURI_PLUGIN_OVERWOLF_ADS`)
/// which features the app enabled: `updater` applies the updater's release
/// rules, `ads` the capability lint's `windows` selector warning.
fn feature_metadata() {
    if std::env::var_os("CARGO_FEATURE_UPDATER").is_some() {
        println!("cargo::metadata=updater=1");
    }
    if std::env::var_os("CARGO_FEATURE_ADS").is_some() {
        println!("cargo::metadata=ads=1");
    }
}

/// Emits [`ADS_CFG`].
fn ads_cfg() {
    println!("cargo::rustc-check-cfg=cfg({ADS_CFG})");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if std::env::var_os("CARGO_FEATURE_ADS").is_some() && (os == "windows" || os == "macos") {
        println!("cargo:rustc-cfg={ADS_CFG}");
    }
}

/// Emits the cfgs the release guard in `src/lib.rs` reads.
fn release_guard() {
    println!("cargo::rustc-check-cfg=cfg({ALLOW_DEV_FEATURES})");
    println!("cargo::rustc-check-cfg=cfg({RELEASE_PROFILE})");
    println!("cargo:rerun-if-env-changed={ALLOW_DEV_FEATURES_ENV}");
    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release")
        || std::env::var("OPT_LEVEL").is_ok_and(|o| o != "0");
    if release {
        println!("cargo:rustc-cfg={RELEASE_PROFILE}");
    }
    if std::env::var(ALLOW_DEV_FEATURES_ENV).is_ok_and(|v| v == "1") {
        println!("cargo:rustc-cfg={ALLOW_DEV_FEATURES}");
    }
}

/// The application manifest of this crate's own test executables on
/// Windows (MSVC): a dependency on Common Controls v6, as `tauri-build`
/// embeds in an app. Tauri imports `TaskDialogIndirect`, which only that
/// version exports, so without the manifest the loader refuses the
/// unit-test executable (`STATUS_ENTRYPOINT_NOT_FOUND`) before any test runs.
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
    release_guard();
    ads_cfg();
    feature_metadata();
    embed_test_manifest()?;
    tauri_plugin::Builder::new(COMMANDS)
        .global_api_script_path("./api-iife.js")
        .build();
    Ok(())
}
