//! The app's build step (DESIGN §4.15): [`run`], called from the app's
//! `src-tauri/build.rs` before `tauri_build::build()`.
//!
//! ```toml
//! [build-dependencies]
//! tauri-plugin-overwolf = { version = "0.1", default-features = false, features = ["build"] }
//! ```
//!
//! ```no_run
//! // src-tauri/build.rs, inside `fn main()`:
//! tauri_plugin_overwolf::build::run().expect("tauri-plugin-overwolf build step failed");
//! // tauri_build::build();
//! ```
//!
//! It reads the merged Tauri configuration exactly as `tauri-build` does
//! (`tauri.conf.json`, then the `tauri.<target>.conf.json` overlay, then
//! `TAURI_CONFIG`, each an RFC 7396 merge patch; DESIGN §3.1), validates
//! `plugins.overwolf` with the release-only rules, and resolves the app
//! identity the plugin resolves at run time, so the uid the installer
//! records always equals the runtime uid. The installer hooks, the
//! capability lint and the signed-build resource are added in a later
//! release of this step.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tauri_utils::platform::Target;

use crate::app_identity::AppIdentity;
use crate::config::{Config, ConfigError, Validation};

/// Why [`run`] failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    /// A build-script environment variable is missing.
    #[error("tauri-plugin-overwolf: {0} is not set (call build::run from a build script)")]
    Env(&'static str),
    /// A configuration file could not be read or parsed.
    #[error("tauri-plugin-overwolf: {path}: {message}")]
    File {
        /// The file.
        path: PathBuf,
        /// What failed.
        message: String,
    },
    /// `TAURI_CONFIG` is not a JSON object.
    #[error("tauri-plugin-overwolf: TAURI_CONFIG: {0}")]
    TauriConfig(String),
    /// `plugins.overwolf` is invalid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// No app name: neither `plugins.overwolf.name`, `productName` nor a
    /// Cargo package name.
    #[error("tauri-plugin-overwolf: set plugins.overwolf.name or productName in tauri.conf.json")]
    NoName,
}

/// The identity a build resolves (DESIGN §3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BuildIdentity {
    /// The effective uid.
    pub uid: String,
    /// The computed uid.
    pub cuid: String,
    /// `<PN>`.
    pub name: String,
    /// The uid formula's author (`"unknown"` when not configured).
    pub author: String,
    /// The app version, if any.
    pub version: Option<String>,
}

/// The merged configuration and the files it came from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Merged {
    /// The merged document, before Tauri's defaults.
    pub(crate) config: Value,
    /// The files read, base first.
    pub(crate) files: Vec<PathBuf>,
}

fn file_error(path: &Path, message: impl std::fmt::Display) -> BuildError {
    BuildError::File {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

/// Reads the merged configuration of the Tauri folder `dir` for `target`,
/// with `tauri_config` (the `TAURI_CONFIG` value) merged last.
pub(crate) fn merged_config(
    dir: &Path,
    target: Target,
    tauri_config: Option<&str>,
) -> Result<Merged, BuildError> {
    let (mut config, files) = tauri_utils::config::parse::read_from(target, dir)
        .map_err(|e| file_error(&dir.join("tauri.conf.json"), e))?;
    if let Some(text) = tauri_config {
        let patch: Value =
            serde_json::from_str(text).map_err(|e| BuildError::TauriConfig(e.to_string()))?;
        if !patch.is_object() {
            return Err(BuildError::TauriConfig("must be a JSON object".into()));
        }
        json_patch::merge(&mut config, &patch);
    }
    Ok(Merged { config, files })
}

/// `plugins.overwolf` of a merged configuration (an empty block when
/// absent).
pub(crate) fn plugin_config(merged: &Value) -> Result<Config, ConfigError> {
    match merged.get("plugins").and_then(|p| p.get("overwolf")) {
        Some(block) if !block.is_null() => Config::from_value(block),
        _ => Ok(Config::default()),
    }
}

fn non_empty(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// The Cargo `[package]` name and version of a `Cargo.toml` text (enough
/// for a build step's fallbacks; the real values come from cargo's
/// environment).
#[cfg(test)]
fn cargo_package(toml: &str) -> (Option<String>, Option<String>) {
    let (mut section, mut name, mut version) = (String::new(), None, None);
    for raw in toml.lines() {
        let line = raw.split(" #").next().unwrap_or_default().trim();
        if line.starts_with('[') {
            section = line
                .trim_matches(|c| c == '[' || c == ']')
                .trim()
                .to_owned();
            continue;
        }
        if section != "package" {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_owned();
            match key.trim() {
                "name" => name = Some(value),
                "version" => version = Some(value),
                _ => {}
            }
        }
    }
    (name, version)
}

/// The app version: `version` as written, or, when it names a file of the
/// Tauri folder, that JSON file's `version`; else the Cargo version.
fn resolve_version(
    merged: &Value,
    dir: &Path,
    cargo_version: Option<&str>,
) -> Result<Option<String>, BuildError> {
    let Some(version) = non_empty(merged.get("version")) else {
        return Ok(cargo_version.map(str::to_owned));
    };
    let path = dir.join(&version);
    if path.is_file() {
        let text = std::fs::read_to_string(&path).map_err(|e| file_error(&path, e))?;
        let file: Value = serde_json::from_str(&text).map_err(|e| file_error(&path, e))?;
        return match file.get("version").and_then(Value::as_str) {
            Some(v) => Ok(Some(v.to_owned())),
            None => Err(file_error(
                &path,
                "\"version\" must be a string (tauri.conf.json > version names this file)",
            )),
        };
    }
    Ok(Some(version))
}

/// The identity of a merged configuration, as the plugin resolves it at run
/// time (`productName`, else the Cargo package name, is Tauri's
/// `PackageInfo::name`).
pub(crate) fn resolve_identity(
    merged: &Value,
    config: &Config,
    dir: &Path,
    cargo_name: Option<&str>,
    cargo_version: Option<&str>,
) -> Result<BuildIdentity, BuildError> {
    let product_name = non_empty(merged.get("productName"))
        .or_else(|| cargo_name.filter(|n| !n.is_empty()).map(str::to_owned));
    let has_name = config.name.as_deref().is_some_and(|n| !n.is_empty());
    let Some(product_name) = product_name.or_else(|| has_name.then(String::new)) else {
        return Err(BuildError::NoName);
    };
    let version = resolve_version(merged, dir, cargo_version)?;
    let app = AppIdentity::resolve(
        config,
        &product_name,
        version.as_deref().unwrap_or_default(),
    );
    Ok(BuildIdentity {
        uid: app.uid,
        cuid: app.cuid,
        name: app.name,
        author: app.author,
        version,
    })
}

fn env(key: &'static str) -> Result<String, BuildError> {
    std::env::var(key).map_err(|_| BuildError::Env(key))
}

/// The plugin's links metadata: set when the app enabled the `updater`
/// feature (`DEP_TAURI_PLUGIN_OVERWOLF_UPDATER`, emitted by this crate's
/// own build script).
const UPDATER_METADATA: &str = "DEP_TAURI_PLUGIN_OVERWOLF_UPDATER";

/// The build step (DESIGN §4.15): reads the merged configuration of the
/// app's Tauri folder (`CARGO_MANIFEST_DIR`) for the build's target,
/// validates `plugins.overwolf` (with the release-only rules in a release
/// build) and resolves the app identity. Prints `cargo:rerun-if-changed` for
/// every configuration file read and `cargo:rerun-if-env-changed` for
/// `TAURI_CONFIG`, and one `cargo:warning` per defaulted input.
///
/// ```no_run
/// // src-tauri/build.rs
/// tauri_plugin_overwolf::build::run().expect("tauri-plugin-overwolf build step failed");
/// ```
///
/// # Errors
///
/// A missing build-script variable, an unreadable configuration file, an
/// invalid `TAURI_CONFIG`, an invalid `plugins.overwolf`, or no app name.
#[expect(
    clippy::print_stdout,
    reason = "cargo reads a build script's instructions from stdout"
)]
pub fn run() -> Result<(), BuildError> {
    let dir = PathBuf::from(env("CARGO_MANIFEST_DIR")?);
    let target = Target::from_triple(&env("TARGET")?);
    println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
    let tauri_config = std::env::var("TAURI_CONFIG").ok();
    let merged = merged_config(&dir, target, tauri_config.as_deref())?;
    for file in &merged.files {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    let mut config = plugin_config(&merged.config)?;
    for warning in config.normalize() {
        println!("cargo:warning={warning}");
    }
    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release");
    let updater = std::env::var_os(UPDATER_METADATA).is_some();
    config.validate(Validation::build_step(release, updater))?;
    let cargo_name = std::env::var("CARGO_PKG_NAME").ok();
    let cargo_version = std::env::var("CARGO_PKG_VERSION").ok();
    // The installer hooks (W3) are written from this identity.
    let _identity = resolve_identity(
        &merged.config,
        &config,
        &dir,
        cargo_name.as_deref(),
        cargo_version.as_deref(),
    )?;
    if config.author.as_deref().is_none_or(str::is_empty) {
        println!(
            "cargo:warning=plugins.overwolf.author is not set; the uid uses \"unknown\" (debug builds only, a release build fails)"
        );
    }
    if config.signing.enabled {
        // `ow-tauri sign` writes it next to the Tauri folder; only watched
        // when signing is on (cargo re-runs a build script on every build
        // while a watched file is missing).
        println!("cargo:rerun-if-changed=../signed/sign-result.json");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(name: &str) -> Target {
        match name {
            "windows" => Target::Windows,
            "macos" => Target::MacOS,
            "linux" => Target::Linux,
            "android" => Target::Android,
            "ios" => Target::Ios,
            other => panic!("unknown target {other}"),
        }
    }

    /// Golden cases shared with the CLI (`fixtures/config-merge`, W1-B
    /// CR 7): the merged document and the identity equal `expected.json`.
    #[test]
    fn config_merge_goldens() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/config-merge");
        if !root.is_dir() {
            // Built outside the repository (crates.io, docs.rs).
            return;
        }
        let mut cases = 0;
        for entry in std::fs::read_dir(&root).unwrap() {
            let dir = entry.unwrap().path();
            if !dir.join("case.json").is_file() {
                continue;
            }
            let read = |name: &str| -> Value {
                serde_json::from_str(&std::fs::read_to_string(dir.join(name)).unwrap()).unwrap()
            };
            let case = read("case.json");
            let expected = read("expected.json");
            let merged = merged_config(
                &dir,
                target(case["target"].as_str().unwrap()),
                case.get("tauriConfig").and_then(Value::as_str),
            )
            .unwrap();
            assert_eq!(merged.config, expected["config"], "{}", dir.display());
            let (cargo_name, cargo_version) =
                cargo_package(&std::fs::read_to_string(dir.join("Cargo.toml")).unwrap());
            let config = plugin_config(&merged.config).unwrap();
            let id = resolve_identity(
                &merged.config,
                &config,
                &dir,
                cargo_name.as_deref(),
                cargo_version.as_deref(),
            )
            .unwrap();
            let want = &expected["identity"];
            assert_eq!(id.uid, want["uid"], "{}", dir.display());
            assert_eq!(id.cuid, want["cuid"], "{}", dir.display());
            assert_eq!(id.name, want["name"], "{}", dir.display());
            assert_eq!(id.author, want["author"], "{}", dir.display());
            assert_eq!(
                id.version.as_deref(),
                want["version"].as_str(),
                "{}",
                dir.display()
            );
            cases += 1;
        }
        assert!(cases >= 9, "every fixture ran ({cases})");
    }

    #[test]
    fn tauri_config_must_be_an_object() {
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/config-merge/base-only");
        if !dir.is_dir() {
            return;
        }
        assert!(matches!(
            merged_config(&dir, Target::Linux, Some("[1]")),
            Err(BuildError::TauriConfig(_))
        ));
        assert!(matches!(
            merged_config(&dir, Target::Linux, Some("{")),
            Err(BuildError::TauriConfig(_))
        ));
        assert!(matches!(
            merged_config(Path::new("/nonexistent-ow-tauri"), Target::Linux, None),
            Err(BuildError::File { .. })
        ));
    }

    #[test]
    fn identity_needs_a_name() {
        let merged = serde_json::json!({});
        let err =
            resolve_identity(&merged, &Config::default(), Path::new("."), None, None).unwrap_err();
        assert!(matches!(err, BuildError::NoName));
        let config =
            Config::from_value(&serde_json::json!({ "name": "App", "author": "A" })).unwrap();
        let id = resolve_identity(&merged, &config, Path::new("."), None, Some("1.0.0")).unwrap();
        assert_eq!(id.name, "App");
        assert_eq!(id.version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn cargo_toml_package_fields() {
        let (name, version) = cargo_package(
            "[package]\nname = \"a\" # x\nversion = '1.2.3'\n[dependencies]\nname = \"b\"\n",
        );
        assert_eq!(name.as_deref(), Some("a"));
        assert_eq!(version.as_deref(), Some("1.2.3"));
    }
}
