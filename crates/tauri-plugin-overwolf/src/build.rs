//! Build-script helper that embeds the app manifest (CONTRACT G.3).
//!
//! Use it from the app's `src-tauri/build.rs`, before `tauri_build::build()`.
//! The build dependency does not need the plugin itself, so turn the default
//! features off to keep build scripts fast:
//!
//! ```toml
//! [build-dependencies]
//! tauri-plugin-overwolf = { version = "0.1", default-features = false }
//! ```
//!
//! ```no_run
//! // src-tauri/build.rs, in `fn main()`:
//! tauri_plugin_overwolf::build::embed_manifest("../package.json")
//!     .expect("package.json overwolf manifest");
//! // tauri_build::build();
//! ```
//!
//! The app then passes the embedded manifest to the plugin:
//!
//! ```ignore
//! tauri_plugin_overwolf::Builder::new()
//!     .manifest_json(tauri_plugin_overwolf::embedded_manifest!())
//!     .build()
//! ```

use std::path::{Path, PathBuf};

use crate::manifest::{
    ManifestError, ManifestWarning, parse_package_json, signing_warnings, tauri_conf_warnings,
};

/// File name of the embedded manifest inside `OUT_DIR`.
pub const MANIFEST_FILE: &str = "ow-tauri-manifest.json";

/// File name of the embedded `dev-app-update.yml` copy inside `OUT_DIR`
/// (empty when there is none, or in release builds).
pub const DEV_APP_UPDATE_FILE: &str = "ow-tauri-dev-app-update.yml";

/// Why [`embed_manifest`] failed.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// Reading `package.json` or writing into `OUT_DIR` failed.
    #[error("{action} {path}: {source}")]
    Io {
        /// What was being done.
        action: &'static str,
        /// The file involved.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// `package.json` failed validation.
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    /// `OUT_DIR` is not set: the function was not called from a build script.
    #[error("OUT_DIR is not set; call embed_manifest from a Cargo build script")]
    NotInBuildScript,
}

/// What [`embed_manifest_to`] produced, for callers and tests.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbedOutput {
    /// The manifest JSON file that was written.
    pub manifest_path: PathBuf,
    /// The `dev-app-update.yml` copy that was written (possibly empty).
    pub dev_app_update_path: PathBuf,
    /// Whether a `dev-app-update.yml` was found and embedded.
    pub dev_app_update_embedded: bool,
    /// Every warning, in report order.
    pub warnings: Vec<ManifestWarning>,
    /// Files whose change must re-run the build script.
    pub rerun_if_changed: Vec<PathBuf>,
}

/// Parses and validates `package.json`, prints `cargo:rerun-if-changed` and
/// `cargo:warning` lines, and writes `$OUT_DIR/ow-tauri-manifest.json` for
/// [`embedded_manifest!`](crate::embedded_manifest).
///
/// `path` is relative to the build script's package directory. When a
/// `tauri.conf.json` sits next to `Cargo.toml`, its `productName` and
/// `version` are compared with the manifest. In debug builds a
/// `dev-app-update.yml` next to `package.json` is embedded as well
/// ([`embedded_dev_app_update!`](crate::embedded_dev_app_update)).
///
/// # Errors
///
/// [`BuildError`] when the file cannot be read, fails validation (the error
/// names the field path), or `OUT_DIR` is not set.
///
/// ```no_run
/// // build.rs of the app (`tauri-plugin-overwolf` in [build-dependencies]
/// // with `default-features = false`):
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     tauri_plugin_overwolf::build::embed_manifest("../package.json")?;
///     Ok(())
/// }
/// ```
#[expect(
    clippy::print_stdout,
    reason = "cargo reads build-script directives from stdout"
)]
pub fn embed_manifest(path: impl AsRef<Path>) -> Result<(), BuildError> {
    let out_dir = std::env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or(BuildError::NotInBuildScript)?;
    let manifest_dir =
        std::env::var_os("CARGO_MANIFEST_DIR").map_or_else(PathBuf::new, PathBuf::from);
    let package_json = manifest_dir.join(path.as_ref());
    let tauri_conf = manifest_dir.join("tauri.conf.json");
    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release");
    let out = embed_manifest_to(&package_json, Some(&tauri_conf), &out_dir, release)?;
    for file in &out.rerun_if_changed {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    for warning in &out.warnings {
        println!("cargo:warning={warning}");
    }
    Ok(())
}

/// The testable core of [`embed_manifest`]: no environment, no printing.
///
/// # Errors
///
/// As [`embed_manifest`].
///
/// ```
/// use tauri_plugin_overwolf::build::embed_manifest_to;
/// let dir = std::env::temp_dir().join(format!("ow-tauri-doc-embed-{}", std::process::id()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let package_json = dir.join("package.json");
/// std::fs::write(&package_json, r#"{"name":"demo","productName":"Demo","version":"1.0.0","author":"Example Studio"}"#).unwrap();
/// let out = embed_manifest_to(&package_json, None, &dir, false).unwrap();
/// let text = std::fs::read_to_string(&out.manifest_path).unwrap();
/// assert!(text.contains("Example Studio"));
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn embed_manifest_to(
    package_json: &Path,
    tauri_conf: Option<&Path>,
    out_dir: &Path,
    release: bool,
) -> Result<EmbedOutput, BuildError> {
    let text = std::fs::read_to_string(package_json).map_err(|source| BuildError::Io {
        action: "reading",
        path: package_json.to_path_buf(),
        source,
    })?;
    let parsed = parse_package_json(&text)?;
    let mut warnings = parsed.warnings;
    warnings.extend(signing_warnings(&parsed.manifest.build_overwolf, release));
    let mut rerun = vec![package_json.to_path_buf()];

    if let Some(conf_path) = tauri_conf
        && let Ok(conf_text) = std::fs::read_to_string(conf_path)
    {
        rerun.push(conf_path.to_path_buf());
        if let Ok(conf) = serde_json::from_str::<serde_json::Value>(&conf_text) {
            warnings.extend(tauri_conf_warnings(&parsed.manifest, &conf));
        }
    }

    let json = serde_json::to_string(&parsed.manifest).map_err(|e| {
        BuildError::Manifest(ManifestError {
            path: "$".into(),
            message: e.to_string(),
        })
    })?;
    let manifest_path = out_dir.join(MANIFEST_FILE);
    write(&manifest_path, json.as_bytes())?;

    let dev_update_source = package_json
        .parent()
        .map(|dir| dir.join("dev-app-update.yml"));
    if let Some(source) = &dev_update_source {
        rerun.push(source.clone());
    }
    let dev_update = if release {
        None
    } else {
        dev_update_source.and_then(|p| std::fs::read(p).ok())
    };
    let dev_app_update_path = out_dir.join(DEV_APP_UPDATE_FILE);
    write(
        &dev_app_update_path,
        dev_update.as_deref().unwrap_or_default(),
    )?;

    Ok(EmbedOutput {
        manifest_path,
        dev_app_update_path,
        dev_app_update_embedded: dev_update.is_some(),
        warnings,
        rerun_if_changed: rerun,
    })
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), BuildError> {
    // Only rewrite on change, so Cargo does not rebuild needlessly.
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    std::fs::write(path, bytes).map_err(|source| BuildError::Io {
        action: "writing",
        path: path.to_path_buf(),
        source,
    })
}

/// Expands to the manifest JSON that [`embed_manifest`] wrote, as a
/// `&'static str`. Use it in the app crate whose build script called
/// [`embed_manifest`].
///
/// ```ignore
/// let plugin = tauri_plugin_overwolf::Builder::new()
///     .manifest_json(tauri_plugin_overwolf::embedded_manifest!())
///     .build();
/// ```
#[macro_export]
macro_rules! embedded_manifest {
    () => {
        include_str!(concat!(env!("OUT_DIR"), "/ow-tauri-manifest.json"))
    };
}

/// Expands to the embedded `dev-app-update.yml` (`&'static str`, empty when
/// there is none or in release builds), for `forceDevUpdateConfig` (CONTRACT I.1).
#[macro_export]
macro_rules! embedded_dev_app_update {
    () => {
        include_str!(concat!(env!("OUT_DIR"), "/ow-tauri-dev-app-update.yml"))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ow-tauri-build-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_manifest_and_dev_update() {
        let dir = temp_dir("ok");
        std::fs::write(
            dir.join("package.json"),
            r#"{"name":"demo","version":"1.0.0","author":"Studio","overwolf":{"packages":["gep"]}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("dev-app-update.yml"), "provider: generic\n").unwrap();
        std::fs::write(
            dir.join("tauri.conf.json"),
            r#"{"productName":"other","version":"1.0.0"}"#,
        )
        .unwrap();
        let out = embed_manifest_to(
            &dir.join("package.json"),
            Some(&dir.join("tauri.conf.json")),
            &dir,
            false,
        )
        .unwrap();
        let json = std::fs::read_to_string(&out.manifest_path).unwrap();
        let m = crate::manifest::EmbeddedManifest::from_embedded_json(&json).unwrap();
        assert_eq!(m.product_name, "demo");
        assert!(out.dev_app_update_embedded);
        assert_eq!(
            std::fs::read_to_string(&out.dev_app_update_path).unwrap(),
            "provider: generic\n"
        );
        let paths: Vec<&str> = out.warnings.iter().map(|w| w.path.as_str()).collect();
        assert_eq!(paths, ["overwolf.packages", "productName"]);
        assert_eq!(out.rerun_if_changed.len(), 3);

        // Release builds embed no dev update config and warn about signing.
        let out = embed_manifest_to(&dir.join("package.json"), None, &dir, true).unwrap();
        assert!(!out.dev_app_update_embedded);
        assert_eq!(std::fs::read(&out.dev_app_update_path).unwrap(), b"");
        assert!(
            out.warnings
                .iter()
                .any(|w| w.path == "build.overwolf.requireSigning")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn errors() {
        let dir = temp_dir("err");
        let missing = embed_manifest_to(&dir.join("nope.json"), None, &dir, false).unwrap_err();
        assert!(matches!(
            missing,
            BuildError::Io {
                action: "reading",
                ..
            }
        ));
        std::fs::write(
            dir.join("package.json"),
            r#"{"name":"x","overwolf":{"packages":[3]}}"#,
        )
        .unwrap();
        let invalid = embed_manifest_to(&dir.join("package.json"), None, &dir, false).unwrap_err();
        assert!(
            invalid.to_string().contains("overwolf.packages[0]"),
            "{invalid}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
