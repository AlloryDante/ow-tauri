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
//! records always equals the runtime uid. Then:
//!
//! - it lints the capabilities (a remote URL covering Overwolf pages fails
//!   the build; a `windows` selector while `ads` is on warns; SEC-B1);
//! - for a Windows target it writes `gen/overwolf/overwolf-hooks.nsh`
//!   (macros `OW_TAURI_HOOK_POSTINSTALL` and `OW_TAURI_HOOK_POSTUNINSTALL`:
//!   the install record `Software\OverwolfElectron\<uid>`, and on a real
//!   uninstall, never for `/UPDATE`, the uninstall Counter and the state
//!   folder removal; CONTRACT I.6) and `gen/overwolf/installer-hooks.nsh`
//!   (the `NSIS_HOOK_*` macros from those), for
//!   `bundle.windows.nsis.installerHooks`. With
//!   `bundle.windows.nsis.installMode` `perMachine` or `both`, the record
//!   goes under `HKLM` for an all-users install, as Overwolf's builder does;
//! - with `signing.enabled`, a Windows release build links the
//!   `OWEINTEGRITY/OWE` resource after checking the `ow-tauri sign` output
//!   against the merged uid, and fails without that output when
//!   `signing.requireSigning` (the default).

use std::path::{Path, PathBuf};

use serde_json::Value;
use tauri_utils::platform::Target;

use crate::app_identity::AppIdentity;
use crate::config::{Config, ConfigError, Validation};

mod lint;
mod nsis;
mod owe;

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
    /// A capability lets Overwolf ad pages call the app (SEC-B1).
    #[error("tauri-plugin-overwolf: {0}")]
    Capability(String),
    /// The signed build cannot be completed.
    #[error("tauri-plugin-overwolf: signing: {0}")]
    Signing(String),
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
/// As [`UPDATER_METADATA`], for the `ads` feature.
const ADS_METADATA: &str = "DEP_TAURI_PLUGIN_OVERWOLF_ADS";

/// Whether the build is optimised: `PROFILE=release`, or an `OPT_LEVEL`
/// other than `0` (a custom release profile).
fn is_release(profile: Option<&str>, opt_level: Option<&str>) -> bool {
    profile == Some("release") || opt_level.is_some_and(|o| o != "0")
}

/// What one build step decided, for [`run`] to print (and the tests to
/// read).
#[derive(Debug, Default, PartialEq, Eq)]
struct Plan {
    /// `cargo:rerun-if-changed` paths.
    rerun: Vec<PathBuf>,
    /// `cargo:warning` lines.
    warnings: Vec<String>,
    /// The resource script to link.
    owe_rc: Option<PathBuf>,
}

/// The inputs of one build step.
struct Inputs<'a> {
    dir: &'a Path,
    target: Target,
    tauri_config: Option<&'a str>,
    release: bool,
    updater: bool,
    ads: bool,
    cargo_name: Option<&'a str>,
    cargo_version: Option<&'a str>,
    out_dir: Option<&'a Path>,
}

/// The build step without the environment: reads, validates, lints and
/// writes; returns what to print.
fn plan(inputs: &Inputs<'_>) -> Result<Plan, BuildError> {
    let mut out = Plan::default();
    let merged = merged_config(inputs.dir, inputs.target, inputs.tauri_config)?;
    out.rerun.extend(merged.files.iter().cloned());
    let mut config = plugin_config(&merged.config)?;
    out.warnings.extend(config.normalize());
    config.validate(Validation::build_step(inputs.release, inputs.updater))?;
    let identity = resolve_identity(
        &merged.config,
        &config,
        inputs.dir,
        inputs.cargo_name,
        inputs.cargo_version,
    )?;
    if config.author.as_deref().is_none_or(str::is_empty) {
        out.warnings.push(
            "plugins.overwolf.author is not set; the uid uses \"unknown\" (debug builds only, a release build fails)"
                .into(),
        );
    }
    out.rerun.push(inputs.dir.join("capabilities"));
    let mut errors = Vec::new();
    for finding in lint::lint_capabilities(inputs.dir, &merged.config, inputs.ads) {
        if finding.error {
            errors.push(finding.message);
        } else {
            out.warnings.push(finding.message);
        }
    }
    if !errors.is_empty() {
        return Err(BuildError::Capability(errors.join("; ")));
    }
    let windows = inputs.target == Target::Windows;
    if windows {
        let written = nsis::write_hooks(inputs.dir, &identity, &config.analytics.host_label)?;
        out.rerun.extend(written);
        out.warnings
            .extend(nsis::hooks_config_warning(inputs.dir, &merged.config));
    }
    if config.signing.enabled {
        // Only watched when signing is on: cargo re-runs a build script on
        // every build while a watched file is missing.
        let signed = inputs.dir.join(owe::SIGNED_DIR);
        out.rerun.push(signed.join(owe::SIGN_RESULT_FILE));
        if windows && inputs.release {
            match owe::check_signed(&signed, &identity)? {
                owe::Signed::Ready { warnings } => {
                    out.warnings.extend(warnings);
                    let out_dir = inputs.out_dir.ok_or(BuildError::Env("OUT_DIR"))?;
                    out.owe_rc = Some(owe::owe_resource(&identity.uid, out_dir)?);
                    if !owe::ships_integrity_dll(&merged.config) {
                        out.warnings.push(
                            "bundle.resources does not ship ../signed/integrity.dll; add it next to the app executable (CONTRACT G.4)"
                                .into(),
                        );
                    }
                }
                owe::Signed::Missing(why) if config.signing.require_signing => {
                    return Err(BuildError::Signing(why));
                }
                owe::Signed::Missing(why) => {
                    out.warnings.push(format!(
                        "{why} (signing.requireSigning is off: building unsigned)"
                    ));
                }
            }
        }
    }
    Ok(out)
}

/// The build step (DESIGN §4.15): reads the merged configuration of the
/// app's Tauri folder (`CARGO_MANIFEST_DIR`) for the build's target,
/// validates `plugins.overwolf` (with the release-only rules in a release
/// build), resolves the app identity, lints the capabilities, writes the
/// Windows installer hooks and, for a signed Windows release, links the
/// `OWEINTEGRITY/OWE` resource. Prints `cargo:rerun-if-changed` for every
/// file read or written, `cargo:rerun-if-env-changed` for `TAURI_CONFIG`,
/// and one `cargo:warning` per finding.
///
/// ```no_run
/// // src-tauri/build.rs
/// tauri_plugin_overwolf::build::run().expect("tauri-plugin-overwolf build step failed");
/// ```
///
/// # Errors
///
/// A missing build-script variable, an unreadable configuration file, an
/// invalid `TAURI_CONFIG`, an invalid `plugins.overwolf`, no app name, a
/// capability that covers Overwolf pages, or a signed Windows release
/// without matching `ow-tauri sign` output.
#[expect(
    clippy::print_stdout,
    reason = "cargo reads a build script's instructions from stdout"
)]
pub fn run() -> Result<(), BuildError> {
    let dir = PathBuf::from(env("CARGO_MANIFEST_DIR")?);
    let target = Target::from_triple(&env("TARGET")?);
    println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
    let tauri_config = std::env::var("TAURI_CONFIG").ok();
    let cargo_name = std::env::var("CARGO_PKG_NAME").ok();
    let cargo_version = std::env::var("CARGO_PKG_VERSION").ok();
    let out_dir = std::env::var_os("OUT_DIR").map(PathBuf::from);
    let plan = plan(&Inputs {
        dir: &dir,
        target,
        tauri_config: tauri_config.as_deref(),
        release: is_release(
            std::env::var("PROFILE").ok().as_deref(),
            std::env::var("OPT_LEVEL").ok().as_deref(),
        ),
        updater: std::env::var_os(UPDATER_METADATA).is_some(),
        ads: std::env::var_os(ADS_METADATA).is_some(),
        cargo_name: cargo_name.as_deref(),
        cargo_version: cargo_version.as_deref(),
        out_dir: out_dir.as_deref(),
    })?;
    for path in &plan.rerun {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    for warning in &plan.warnings {
        println!("cargo:warning={warning}");
    }
    if let Some(rc) = &plan.owe_rc {
        owe::link(rc)?;
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

    /// NSIS golden over every `fixtures/config-merge` case: the install
    /// record, the uninstall Counter and the removed state folder all name
    /// the merged-config uid (the uid the CLI merge resolves), and the
    /// uninstall work sits under `$UpdateMode <> 1`, so `/UPDATE` skips it.
    #[test]
    fn nsis_goldens_use_the_merged_uid() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/config-merge");
        if !root.is_dir() {
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
            let want = read("expected.json")["identity"].clone();
            let merged = merged_config(
                &dir,
                target(case["target"].as_str().unwrap()),
                case.get("tauriConfig").and_then(Value::as_str),
            )
            .unwrap();
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
            let uid = want["uid"].as_str().unwrap();
            let nsh = nsis::overwolf_hooks(&id, &config.analytics.host_label);
            let key = format!(r"Software\OverwolfElectron\{uid}");
            let post = nsis::tests::macro_body(&nsh, "OW_TAURI_HOOK_POSTINSTALL");
            assert_eq!(post.matches(&key).count(), 3, "{}", dir.display());
            let version = want["version"].as_str().unwrap_or_default();
            assert!(
                post.contains(&format!(r#""version" "{version}""#)),
                "{}",
                dir.display()
            );
            let un = nsis::tests::macro_body(&nsh, "OW_TAURI_HOOK_POSTUNINSTALL");
            let guarded = un.split_once("${If} $UpdateMode <> 1\n").map_or_else(
                || panic!("no /UPDATE guard: {}", dir.display()),
                |(_, rest)| &rest[..rest.rfind("${EndIf}").unwrap()],
            );
            assert!(guarded.contains(&format!("DeleteRegKey SHCTX \"{key}\"")));
            assert!(guarded.contains(&format!(r#"RMDir /r "$APPDATA\ow-electron\{uid}""#)));
            assert!(guarded.contains(&format!("%22app_id%22%3A%22{uid}%22")));
            assert!(guarded.contains("Name=ow_tauri_app_uninstall"));
            // Header comment, three record values, the key removal, the
            // state folder and the Counter: no other uid.
            assert_eq!(nsh.matches(uid).count(), 7, "{}", dir.display());
            cases += 1;
        }
        assert!(cases >= 9, "every fixture ran ({cases})");
    }

    /// A copy of the `windows-overlay` fixture in a fresh temp folder.
    fn project(name: &str) -> Option<PathBuf> {
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/config-merge/windows-overlay");
        if !src.is_dir() {
            return None;
        }
        let dir = std::env::temp_dir()
            .join(format!("ow-tauri-plan-{}-{name}", std::process::id()))
            .join("src-tauri");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        for file in ["Cargo.toml", "tauri.conf.json", "tauri.windows.conf.json"] {
            std::fs::copy(src.join(file), dir.join(file)).unwrap();
        }
        Some(dir)
    }

    fn inputs<'a>(
        dir: &'a Path,
        target: Target,
        tauri_config: Option<&'a str>,
        release: bool,
        out_dir: &'a Path,
    ) -> Inputs<'a> {
        Inputs {
            dir,
            target,
            tauri_config,
            release,
            updater: false,
            ads: true,
            cargo_name: None,
            cargo_version: None,
            out_dir: Some(out_dir),
        }
    }

    const WINDOWS_OVERLAY_UID: &str = "aejkligdodglhcjinbhdcnlohocenfkpdihjacdg";

    #[test]
    fn plan_writes_the_windows_hooks() {
        let Some(dir) = project("hooks") else { return };
        let out = dir.join("out");
        let plan = plan(&inputs(&dir, Target::Windows, None, false, &out)).unwrap();
        let gen_dir = dir.join(nsis::GEN_DIR);
        let macros = gen_dir.join(nsis::OVERWOLF_HOOKS_FILE);
        let wrapper = gen_dir.join(nsis::INSTALLER_HOOKS_FILE);
        // rerun-if-changed on the outputs (a deleted or edited file is
        // written again) and on the inputs.
        for path in [
            &macros,
            &wrapper,
            &dir.join("capabilities"),
            &dir.join("tauri.conf.json"),
        ] {
            assert!(
                plan.rerun.contains(path),
                "{} in {:?}",
                path.display(),
                plan.rerun
            );
        }
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
        assert_eq!(plan.owe_rc, None);
        let nsh = std::fs::read_to_string(&macros).unwrap();
        assert!(nsh.contains(WINDOWS_OVERLAY_UID));
        assert_eq!(
            std::fs::read_to_string(&wrapper).unwrap(),
            nsis::INSTALLER_HOOKS
        );
        // Another target writes nothing.
        std::fs::remove_dir_all(&gen_dir).unwrap();
        let plan = super::plan(&inputs(&dir, Target::Linux, None, false, &out)).unwrap();
        assert!(!gen_dir.exists());
        assert!(!plan.rerun.contains(&macros));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn plan_fails_on_a_capability_that_covers_overwolf() {
        let Some(dir) = project("caps") else { return };
        let out = dir.join("out");
        std::fs::create_dir_all(dir.join("capabilities")).unwrap();
        std::fs::write(
            dir.join("capabilities/default.json"),
            r#"{ "identifier": "main", "windows": ["*"], "permissions": ["overwolf:default"] }"#,
        )
        .unwrap();
        let plan = plan(&inputs(&dir, Target::Windows, None, false, &out)).unwrap();
        assert_eq!(plan.warnings.len(), 1, "{:?}", plan.warnings);
        std::fs::write(
            dir.join("capabilities/web.json"),
            r#"{ "identifier": "web", "webviews": ["main"], "remote": { "urls": ["https://*.overwolf.com/*"] } }"#,
        )
        .unwrap();
        let err = super::plan(&inputs(&dir, Target::Windows, None, false, &out)).unwrap_err();
        assert!(matches!(err, BuildError::Capability(_)), "{err}");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn plan_links_the_owe_resource_for_a_signed_release() {
        let Some(dir) = project("signed") else { return };
        let out = dir.join("out");
        let signed_config = r#"{ "plugins": { "overwolf": { "name": "Parity Harness", "signing": { "enabled": true } } } }"#;
        let optional = r#"{ "plugins": { "overwolf": { "name": "Parity Harness", "signing": { "enabled": true, "requireSigning": false } } } }"#;
        // No `ow-tauri sign` output: a failed build, or a warning.
        let err = plan(&inputs(
            &dir,
            Target::Windows,
            Some(signed_config),
            true,
            &out,
        ))
        .unwrap_err();
        assert!(matches!(err, BuildError::Signing(_)), "{err}");
        let soft = plan(&inputs(&dir, Target::Windows, Some(optional), true, &out)).unwrap();
        assert!(
            soft.warnings
                .iter()
                .any(|w| w.contains("building unsigned")),
            "{:?}",
            soft.warnings
        );
        assert_eq!(soft.owe_rc, None);
        // A debug build and another target do not check.
        assert!(
            plan(&inputs(
                &dir,
                Target::Windows,
                Some(signed_config),
                false,
                &out
            ))
            .is_ok()
        );
        assert!(
            plan(&inputs(
                &dir,
                Target::MacOS,
                Some(signed_config),
                true,
                &out
            ))
            .is_ok()
        );

        let signed = dir.join(owe::SIGNED_DIR);
        std::fs::create_dir_all(&signed).unwrap();
        let write_result = |uid: &str| {
            std::fs::write(
                signed.join(owe::SIGN_RESULT_FILE),
                serde_json::json!({ "uid": uid, "version": "1.0.0" }).to_string(),
            )
            .unwrap();
        };
        write_result(WINDOWS_OVERLAY_UID);
        let ready = plan(&inputs(
            &dir,
            Target::Windows,
            Some(signed_config),
            true,
            &out,
        ))
        .unwrap();
        assert!(ready.rerun.contains(&signed.join(owe::SIGN_RESULT_FILE)));
        assert_eq!(ready.owe_rc, Some(out.join(owe::OWE_RC_FILE)));
        assert!(
            ready.warnings.iter().any(|w| w.contains("integrity.dll")),
            "{:?}",
            ready.warnings
        );
        assert_eq!(
            std::fs::read_to_string(out.join(owe::OWE_JSON_FILE)).unwrap(),
            format!(r#"{{"appUid":"{WINDOWS_OVERLAY_UID}"}}"#)
        );
        // The console signed another uid (PAR-B2): fail, naming the fix.
        write_result("abcdefghijklmnopabcdefghijklmnop");
        let err = plan(&inputs(
            &dir,
            Target::Windows,
            Some(signed_config),
            true,
            &out,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("--write-uid"), "{err}");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn release_detection() {
        assert!(is_release(Some("release"), Some("0")));
        assert!(is_release(Some("debug"), Some("3")));
        assert!(!is_release(Some("debug"), Some("0")));
        assert!(!is_release(None, None));
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
