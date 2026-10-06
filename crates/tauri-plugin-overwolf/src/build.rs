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

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::identity::{is_valid_uid, resolve_uid};
use crate::manifest::{
    EmbeddedManifest, ManifestError, ManifestWarning, parse_package_json, sign_command_warnings,
    signing_warnings, tauri_conf_warnings,
};

/// File name of the embedded manifest inside `OUT_DIR`.
pub const MANIFEST_FILE: &str = "ow-tauri-manifest.json";

/// File name of the embedded `dev-app-update.yml` copy inside `OUT_DIR`
/// (empty when there is none, or in release builds).
pub const DEV_APP_UPDATE_FILE: &str = "ow-tauri-dev-app-update.yml";

/// The folder `ow-tauri sign` writes, next to `package.json` (CONTRACT G.4):
/// the signed `package.json`, `_metadata.json`, `integrity.dll`,
/// `owe.json` and `sign-result.json`.
pub const SIGNED_DIR: &str = "ow-tauri-signed";

/// File name of the `OWEINTEGRITY/OWE` resource data inside `OUT_DIR`.
pub const OWE_JSON_FILE: &str = "ow-tauri-owe.json";

/// File name of the resource script for [`OWE_JSON_FILE`] inside `OUT_DIR`.
pub const OWE_RC_FILE: &str = "ow-tauri-owe.rc";

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
    /// `package.json` (or the signed copy) failed validation.
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    /// `OUT_DIR` is not set: the function was not called from a build script.
    #[error("OUT_DIR is not set; call embed_manifest from a Cargo build script")]
    NotInBuildScript,
    /// The `OWEINTEGRITY/OWE` resource could not be compiled (no resource
    /// compiler for the Windows target, or it failed).
    #[error("compiling the OWEINTEGRITY/OWE resource: {0}")]
    Resource(String),
}

/// What the build targets, for [`embed_manifest_to`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BuildTarget {
    /// A release build (`PROFILE=release`): the signed output is applied and
    /// no `dev-app-update.yml` is embedded.
    pub release: bool,
    /// A Windows target (`CARGO_CFG_TARGET_OS=windows`): the builder's
    /// signing gate and the `OWEINTEGRITY/OWE` resource apply.
    pub windows: bool,
}

impl BuildTarget {
    /// The target of the running build script, from Cargo's environment.
    #[must_use]
    pub fn from_env() -> Self {
        BuildTarget {
            release: std::env::var("PROFILE").is_ok_and(|p| p == "release"),
            windows: std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows"),
        }
    }
}

/// The `OWEINTEGRITY/OWE` PE resource files that [`owe_resource`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OweResource {
    /// The resource data, `{"appUid":"<uid>"}`.
    pub json_path: PathBuf,
    /// The resource script that names it.
    pub rc_path: PathBuf,
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
    /// Whether the output of `ow-tauri sign` was applied (release builds).
    pub signed: bool,
    /// The `OWEINTEGRITY/OWE` resource to link, for a signed Windows
    /// release build.
    pub owe_resource: Option<OweResource>,
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
/// `version` are compared with the manifest.
///
/// In debug builds a `dev-app-update.yml` next to `package.json` is embedded
/// as well ([`embedded_dev_app_update!`](crate::embedded_dev_app_update)).
/// Only an existing file is watched, so after adding one, touch
/// `package.json` (or `cargo clean`) to embed it.
///
/// In release builds the output of `ow-tauri sign` ([`SIGNED_DIR`] next to
/// `package.json`) is applied when it was signed for the same version: its
/// `overwolf.uid` becomes the manifest uid and the signed `package.json`
/// becomes `raw` (CONTRACT G.4 a). For a Windows target the
/// `OWEINTEGRITY/OWE` resource is then compiled and linked with the
/// `embed-resource` feature (G.4 c); without the feature a warning says so.
///
/// # Errors
///
/// [`BuildError`] when a file cannot be read, fails validation (the error
/// names the field path), `OUT_DIR` is not set, or the resource cannot be
/// compiled.
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
    let out = embed_manifest_to(
        &package_json,
        Some(&tauri_conf),
        &out_dir,
        BuildTarget::from_env(),
    )?;
    for file in &out.rerun_if_changed {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    for warning in &out.warnings {
        println!("cargo:warning={warning}");
    }
    if let Some(owe) = &out.owe_resource {
        link_owe_resource(owe)?;
    }
    Ok(())
}

#[cfg(feature = "embed-resource")]
fn link_owe_resource(owe: &OweResource) -> Result<(), BuildError> {
    embed_resource::compile(&owe.rc_path, embed_resource::NONE)
        .manifest_required()
        .map_err(|e| BuildError::Resource(e.to_string()))
}

#[cfg(not(feature = "embed-resource"))]
#[expect(
    clippy::print_stdout,
    reason = "cargo reads build-script directives from stdout"
)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the same signature as the embed-resource variant"
)]
fn link_owe_resource(owe: &OweResource) -> Result<(), BuildError> {
    println!(
        "cargo:warning=the OWEINTEGRITY/OWE resource ({}) is not linked: enable the `embed-resource` feature of the tauri-plugin-overwolf build-dependency",
        owe.rc_path.display()
    );
    Ok(())
}

/// The testable core of [`embed_manifest`]: no environment, no printing,
/// no resource compiler.
///
/// # Errors
///
/// As [`embed_manifest`].
///
/// ```
/// use tauri_plugin_overwolf::build::{embed_manifest_to, BuildTarget};
/// let dir = std::env::temp_dir().join(format!("ow-tauri-doc-embed-{}", std::process::id()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let package_json = dir.join("package.json");
/// std::fs::write(&package_json, r#"{"name":"demo","productName":"Demo","version":"1.0.0","author":"Example Studio"}"#).unwrap();
/// let out = embed_manifest_to(&package_json, None, &dir, BuildTarget::default()).unwrap();
/// let text = std::fs::read_to_string(&out.manifest_path).unwrap();
/// assert!(text.contains("Example Studio"));
/// assert!(!out.signed);
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn embed_manifest_to(
    package_json: &Path,
    tauri_conf: Option<&Path>,
    out_dir: &Path,
    target: BuildTarget,
) -> Result<EmbedOutput, BuildError> {
    let text = std::fs::read_to_string(package_json).map_err(|source| BuildError::Io {
        action: "reading",
        path: package_json.to_path_buf(),
        source,
    })?;
    let parsed = parse_package_json(&text)?;
    let mut manifest = parsed.manifest;
    let mut warnings = parsed.warnings;
    let mut rerun = vec![package_json.to_path_buf()];
    let package_dir = package_json.parent().unwrap_or_else(|| Path::new(""));

    let signed = if target.release {
        let signed_dir = package_dir.join(SIGNED_DIR);
        // Watched even while missing: a release build must pick up a
        // later `ow-tauri sign` (a missing file re-runs the script).
        rerun.push(signed_dir.join("package.json"));
        apply_signed(&mut manifest, &signed_dir, package_dir, &mut warnings)?
    } else {
        false
    };
    let signing_target = target.release && target.windows;
    warnings.extend(signing_warnings(
        &manifest.build_overwolf,
        signing_target,
        signed,
    ));

    if let Some(conf_path) = tauri_conf
        && let Ok(conf_text) = std::fs::read_to_string(conf_path)
    {
        rerun.push(conf_path.to_path_buf());
        if let Ok(conf) = serde_json::from_str::<Value>(&conf_text) {
            warnings.extend(tauri_conf_warnings(&manifest, &conf));
            if signing_target {
                warnings.extend(sign_command_warnings(&manifest.build_overwolf, &conf));
            }
            if signing_target && signed && !ships_integrity_dll(&conf) {
                warnings.push(ManifestWarning {
                    path: "bundle.resources".into(),
                    message: format!(
                        "tauri.conf.json does not ship {SIGNED_DIR}/integrity.dll and _metadata.json next to the exe; map both in bundle.resources"
                    ),
                });
            }
        }
    }

    let json = serde_json::to_string(&manifest).map_err(|e| {
        BuildError::Manifest(ManifestError {
            path: "$".into(),
            message: e.to_string(),
        })
    })?;
    let manifest_path = out_dir.join(MANIFEST_FILE);
    write(&manifest_path, json.as_bytes())?;

    let owe = match manifest.overwolf.uid.as_deref() {
        Some(uid) if signed && target.windows => Some(owe_resource(uid, out_dir)?),
        _ => {
            for stale in [OWE_JSON_FILE, OWE_RC_FILE] {
                let _ = std::fs::remove_file(out_dir.join(stale));
            }
            None
        }
    };

    let dev_update = if target.release {
        None
    } else {
        let source = package_dir.join("dev-app-update.yml");
        let bytes = std::fs::read(&source).ok();
        if bytes.is_some() {
            rerun.push(source);
        }
        bytes
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
        signed,
        owe_resource: owe,
        warnings,
        rerun_if_changed: rerun,
    })
}

/// Applies `<signed_dir>/package.json` when it exists and was signed for
/// the manifest's version. Returns whether it was applied.
fn apply_signed(
    manifest: &mut EmbeddedManifest,
    signed_dir: &Path,
    package_dir: &Path,
    warnings: &mut Vec<ManifestWarning>,
) -> Result<bool, BuildError> {
    let path = signed_dir.join("package.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(BuildError::Io {
                action: "reading",
                path,
                source,
            });
        }
    };
    let invalid = |message: &str| {
        BuildError::Manifest(ManifestError {
            path: format!("{SIGNED_DIR}/package.json"),
            message: message.into(),
        })
    };
    let Ok(Value::Object(signed)) = serde_json::from_str::<Value>(&text) else {
        return Err(invalid(
            "is not a JSON object; run `npx ow-tauri sign` again",
        ));
    };
    let version = signed
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if version != manifest.version {
        warnings.push(ManifestWarning {
            path: "version".into(),
            message: format!(
                "{SIGNED_DIR}/package.json was signed for version \"{version}\", not \"{}\"; it is ignored, run `npx ow-tauri sign` again",
                manifest.version
            ),
        });
        return Ok(false);
    }
    let uid = signed
        .get("overwolf")
        .and_then(|ow| ow.get("uid"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|uid| is_valid_uid(uid))
        .ok_or_else(|| invalid("has no valid overwolf.uid; run `npx ow-tauri sign` again"))?
        .to_owned();
    if let Some(message) = stale_entry_warning(signed_dir, package_dir) {
        warnings.push(ManifestWarning {
            path: "main".into(),
            message,
        });
    }
    manifest.overwolf.uid = Some(uid);
    manifest.raw = without_dev_keys(signed);
    Ok(true)
}

/// The signed `package.json` is already the packaged form; this only guards
/// against a hand-edited copy.
fn without_dev_keys(mut signed: Map<String, Value>) -> Map<String, Value> {
    signed.remove("devDependencies");
    signed.remove("scripts");
    signed
}

/// A warning when the entry file `ow-tauri sign` hashed changed since.
fn stale_entry_warning(signed_dir: &Path, package_dir: &Path) -> Option<String> {
    let result: Value =
        serde_json::from_slice(&std::fs::read(signed_dir.join("sign-result.json")).ok()?).ok()?;
    let main = result.get("mainFile")?.as_str()?;
    let expected = result.get("mainSha256")?.as_str()?;
    let bytes = std::fs::read(package_dir.join(main)).ok()?;
    let actual = Sha256::digest(&bytes)
        .iter()
        .fold(String::new(), |mut hex, b| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{b:02x}");
            hex
        });
    (!actual.eq_ignore_ascii_case(expected)).then(|| {
        format!("{main} changed since `ow-tauri sign` hashed it; run `npx ow-tauri sign` again")
    })
}

/// Whether `bundle.resources` names `integrity.dll`.
fn ships_integrity_dll(conf: &Value) -> bool {
    match conf.pointer("/bundle/resources") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .any(|item| item.contains("integrity.dll") || item.contains(SIGNED_DIR)),
        Some(Value::Object(map)) => map.iter().any(|(from, to)| {
            from.contains("integrity.dll") || to.as_str() == Some("integrity.dll")
        }),
        _ => false,
    }
}

/// Writes the `OWEINTEGRITY/OWE` PE resource of CONTRACT G.4 c into
/// `out_dir`: the data `{"appUid":"<uid>"}`, as Overwolf's builder writes
/// it, and a resource script naming it, for `embed-resource` or any `.rc`
/// compiler. The resource uses the neutral language, which is the version
/// information language tauri-build writes, and is its own type, so no
/// second `VERSIONINFO` is added.
///
/// # Errors
///
/// [`BuildError::Manifest`] for an invalid uid, [`BuildError::Io`] when a
/// file cannot be written.
///
/// ```
/// use tauri_plugin_overwolf::build::owe_resource;
/// let dir = std::env::temp_dir().join(format!("ow-tauri-doc-owe-{}", std::process::id()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let owe = owe_resource("djpddhibpjddgdpcfkbooljealnjnamkhlihgbab", &dir).unwrap();
/// assert_eq!(
///     std::fs::read_to_string(&owe.json_path).unwrap(),
///     r#"{"appUid":"djpddhibpjddgdpcfkbooljealnjnamkhlihgbab"}"#
/// );
/// assert!(std::fs::read_to_string(&owe.rc_path).unwrap().contains("OWE OWEINTEGRITY"));
/// assert!(owe_resource("../x", &dir).is_err());
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn owe_resource(uid: &str, out_dir: &Path) -> Result<OweResource, BuildError> {
    if !is_valid_uid(uid) {
        return Err(BuildError::Manifest(ManifestError {
            path: "overwolf.uid".into(),
            message: "must be 1 to 64 ASCII letters or digits".into(),
        }));
    }
    let json_path =
        std::path::absolute(out_dir.join(OWE_JSON_FILE)).map_err(|source| BuildError::Io {
            action: "resolving",
            path: out_dir.join(OWE_JSON_FILE),
            source,
        })?;
    // The uid is ASCII letters and digits, so it needs no JSON escaping.
    write(&json_path, format!(r#"{{"appUid":"{uid}"}}"#).as_bytes())?;
    let rc = format!(
        "// Generated by tauri-plugin-overwolf (CONTRACT G.4 c).\nLANGUAGE 0, 0\nOWE OWEINTEGRITY \"{}\"\n",
        rc_escape(&json_path.to_string_lossy())
    );
    let rc_path = out_dir.join(OWE_RC_FILE);
    write(&rc_path, rc.as_bytes())?;
    Ok(OweResource { json_path, rc_path })
}

/// Escapes a value for a quoted `.rc` string.
fn rc_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' => out.push_str("\"\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out
}

/// The uninstall Counter host of the installer (I.6), which differs from
/// the runtime's Counter host.
pub const UNINSTALL_COUNTER_URL: &str = "https://analyticssec.overwolf.com/analytics/Counter";

const NSIS_HOOKS_TEMPLATE: &str = include_str!("build/installer-hooks.nsh");

/// Renders the NSIS installer hooks of CONTRACT I.6 for `manifest`: the
/// `SHCTX\Software\OverwolfElectron\<uid>` install record after install and,
/// on a real uninstall only, the removal of `%APPDATA%\ow-electron\<uid>`,
/// that registry key and the `ow_<label>_app_uninstall` Counter.
///
/// `config_uid` is the plugin's `uid` override, if the app sets one (G.2);
/// `host_label` is `plugins.overwolf.analytics.hostLabel` (default `tauri`).
///
/// ```
/// use tauri_plugin_overwolf::build::nsis_installer_hooks;
/// use tauri_plugin_overwolf::manifest::EmbeddedManifest;
/// let m = EmbeddedManifest::minimal("Example App", "Example Studio", "1.0.0");
/// let nsh = nsis_installer_hooks(&m, None, "tauri");
/// assert!(nsh.contains("!macro NSIS_HOOK_POSTUNINSTALL"));
/// assert!(nsh.contains("Name=ow_tauri_app_uninstall"));
/// assert!(nsh.contains("%22app_name%22%3A%22Example+App%22"));
/// ```
#[must_use]
pub fn nsis_installer_hooks(
    manifest: &EmbeddedManifest,
    config_uid: Option<&str>,
    host_label: &str,
) -> String {
    let uid = resolve_uid(config_uid, manifest).uid;
    let counter_name = format!("ow_{host_label}_app_uninstall");
    let extra = serde_json::json!({
        "app_id": uid,
        "app_version": manifest.version,
        "app_name": manifest.product_name,
    })
    .to_string();
    let prefix = format!(
        "{UNINSTALL_COUNTER_URL}?Name={}",
        form_encode(&counter_name)
    );
    // Every substituted value is either a validated uid or form-encoded, so
    // none can carry NSIS syntax (`$`, quotes, newlines).
    let comment: String = manifest
        .product_name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    NSIS_HOOKS_TEMPLATE
        .replace("@UID@", &uid)
        .replace("@PRODUCT_NAME_COMMENT@", &comment)
        .replace("@VERSION@", &nsis_escape(&manifest.version))
        .replace("@COUNTER_NAME@", &counter_name)
        .replace("@COUNTER_URL_PREFIX@", &prefix)
        .replace("@EXTRA@", &form_encode(&extra))
}

/// Reads `package_json` and writes [`nsis_installer_hooks`] to `out`, only
/// when the content changed. Call it from the app's build script and point
/// `bundle.windows.nsis.installerHooks` at `out`.
///
/// # Errors
///
/// [`BuildError::Io`] when a file cannot be read or written, and
/// [`BuildError::Manifest`] when `package.json` fails validation.
///
/// ```
/// use tauri_plugin_overwolf::build::write_nsis_installer_hooks;
/// let dir = std::env::temp_dir().join(format!("ow-tauri-doc-nsis-{}", std::process::id()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let package_json = dir.join("package.json");
/// std::fs::write(&package_json, r#"{"name":"demo","version":"1.0.0","author":"Example Studio"}"#).unwrap();
/// write_nsis_installer_hooks(&package_json, None, "tauri", &dir.join("hooks.nsh")).unwrap();
/// assert!(std::fs::read_to_string(dir.join("hooks.nsh")).unwrap().contains("$UpdateMode"));
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub fn write_nsis_installer_hooks(
    package_json: &Path,
    config_uid: Option<&str>,
    host_label: &str,
    out: &Path,
) -> Result<(), BuildError> {
    let text = std::fs::read_to_string(package_json).map_err(|source| BuildError::Io {
        action: "reading",
        path: package_json.to_path_buf(),
        source,
    })?;
    let parsed = parse_package_json(&text)?;
    write(
        out,
        nsis_installer_hooks(&parsed.manifest, config_uid, host_label).as_bytes(),
    )
}

/// `URLSearchParams` encoding (space as `+`).
fn form_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Escapes a value for an NSIS double-quoted string.
fn nsis_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '$' => out.push_str("$$"),
            '"' => out.push_str("$\\\""),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
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
            BuildTarget::default(),
        )
        .unwrap();
        let json = std::fs::read_to_string(&out.manifest_path).unwrap();
        let m = EmbeddedManifest::from_embedded_json(&json).unwrap();
        assert_eq!(m.product_name, "demo");
        assert!(out.dev_app_update_embedded);
        assert_eq!(
            std::fs::read_to_string(&out.dev_app_update_path).unwrap(),
            "provider: generic\n"
        );
        let paths: Vec<&str> = out.warnings.iter().map(|w| w.path.as_str()).collect();
        assert_eq!(paths, ["overwolf.packages", "productName"]);
        assert_eq!(out.rerun_if_changed.len(), 3);

        // Release builds embed no dev update config and, for Windows, warn
        // about signing.
        let release = BuildTarget {
            release: true,
            windows: true,
        };
        let out = embed_manifest_to(&dir.join("package.json"), None, &dir, release).unwrap();
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
        let missing = embed_manifest_to(&dir.join("nope.json"), None, &dir, BuildTarget::default())
            .unwrap_err();
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
        let invalid = embed_manifest_to(
            &dir.join("package.json"),
            None,
            &dir,
            BuildTarget::default(),
        )
        .unwrap_err();
        assert!(
            invalid.to_string().contains("overwolf.packages[0]"),
            "{invalid}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    const SIGNED_UID: &str = "djpddhibpjddgdpcfkbooljealnjnamkhlihgbab";

    fn signed_fixture(name: &str, signed_version: &str) -> PathBuf {
        let dir = temp_dir(name);
        std::fs::write(
            dir.join("package.json"),
            r#"{"name":"demo","version":"1.0.0","author":"Studio","main":"main.js","scripts":{"a":"b"},"build":{"overwolf":{"enableOWCertSigning":true}}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("main.js"), "test").unwrap();
        let signed = dir.join(SIGNED_DIR);
        std::fs::create_dir_all(&signed).unwrap();
        std::fs::write(
            signed.join("package.json"),
            format!(
                r#"{{"name":"demo","version":"{signed_version}","author":"Studio","main":"main.js","overwolf":{{"uid":"{SIGNED_UID}"}}}}"#
            ),
        )
        .unwrap();
        // SHA-256 of "test".
        std::fs::write(
            signed.join("sign-result.json"),
            r#"{"mainFile":"main.js","mainSha256":"9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"}"#,
        )
        .unwrap();
        dir
    }

    const WINDOWS_RELEASE: BuildTarget = BuildTarget {
        release: true,
        windows: true,
    };

    #[test]
    fn release_applies_the_signed_output() {
        let dir = signed_fixture("signed", "1.0.0");
        std::fs::write(
            dir.join("tauri.conf.json"),
            r#"{"bundle":{"resources":{"../ow-tauri-signed/integrity.dll":"integrity.dll"},"windows":{"signCommand":"npx ow-tauri sign-exe %1"}}}"#,
        )
        .unwrap();
        let out = embed_manifest_to(
            &dir.join("package.json"),
            Some(&dir.join("tauri.conf.json")),
            &dir,
            WINDOWS_RELEASE,
        )
        .unwrap();
        assert!(out.signed);
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        let m = EmbeddedManifest::from_embedded_json(
            &std::fs::read_to_string(&out.manifest_path).unwrap(),
        )
        .unwrap();
        assert_eq!(m.overwolf.uid.as_deref(), Some(SIGNED_UID));
        assert_eq!(
            resolve_uid(None, &m).source,
            crate::identity::UidSource::Manifest
        );
        assert_eq!(m.raw["overwolf"]["uid"], SIGNED_UID);
        assert!(
            m.build_overwolf.enable_ow_cert_signing,
            "kept from package.json"
        );
        assert!(
            out.rerun_if_changed
                .contains(&dir.join(SIGNED_DIR).join("package.json"))
        );
        let owe = out.owe_resource.expect("OWE resource on Windows");
        assert_eq!(
            std::fs::read_to_string(&owe.json_path).unwrap(),
            format!(r#"{{"appUid":"{SIGNED_UID}"}}"#)
        );
        let rc = std::fs::read_to_string(&owe.rc_path).unwrap();
        assert!(rc.contains("LANGUAGE 0, 0\nOWE OWEINTEGRITY \""), "{rc}");
        assert!(owe.json_path.is_absolute());

        // Other targets apply the uid but compile no resource, and a stale
        // resource from an earlier build is removed.
        let mac = BuildTarget {
            release: true,
            windows: false,
        };
        let out = embed_manifest_to(&dir.join("package.json"), None, &dir, mac).unwrap();
        assert!(out.signed && out.owe_resource.is_none());
        assert!(!owe.rc_path.exists());

        // Debug builds ignore the signed output, as an unpackaged
        // ow-electron run reads the source package.json.
        let out = embed_manifest_to(
            &dir.join("package.json"),
            None,
            &dir,
            BuildTarget::default(),
        )
        .unwrap();
        assert!(!out.signed);
        let m = EmbeddedManifest::from_embedded_json(
            &std::fs::read_to_string(&out.manifest_path).unwrap(),
        )
        .unwrap();
        assert_eq!(m.overwolf.uid, None);
        // No dev-app-update.yml: nothing missing is watched.
        assert_eq!(out.rerun_if_changed, [dir.join("package.json")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn signed_output_warnings_and_errors() {
        // Another version: ignored with a warning, and the unsigned warning.
        let dir = signed_fixture("stale", "0.9.0");
        let out =
            embed_manifest_to(&dir.join("package.json"), None, &dir, WINDOWS_RELEASE).unwrap();
        assert!(!out.signed && out.owe_resource.is_none());
        let paths: Vec<&str> = out.warnings.iter().map(|w| w.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "version",
                "build.overwolf.requireSigning",
                "build.overwolf.enableOWCertSigning"
            ]
        );

        // Same version, changed entry file, missing signCommand and resources.
        let dir = signed_fixture("changed", "1.0.0");
        std::fs::write(dir.join("main.js"), "changed").unwrap();
        std::fs::write(dir.join("tauri.conf.json"), "{}").unwrap();
        let out = embed_manifest_to(
            &dir.join("package.json"),
            Some(&dir.join("tauri.conf.json")),
            &dir,
            WINDOWS_RELEASE,
        )
        .unwrap();
        assert!(out.signed);
        let paths: Vec<&str> = out.warnings.iter().map(|w| w.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "main",
                "build.overwolf.enableOWCertSigning",
                "bundle.resources"
            ]
        );

        // A signed copy without a valid uid, or not JSON, fails the build.
        std::fs::write(
            dir.join(SIGNED_DIR).join("package.json"),
            r#"{"version":"1.0.0","overwolf":{"uid":"../x"}}"#,
        )
        .unwrap();
        let err =
            embed_manifest_to(&dir.join("package.json"), None, &dir, WINDOWS_RELEASE).unwrap_err();
        assert!(err.to_string().contains("overwolf.uid"), "{err}");
        std::fs::write(dir.join(SIGNED_DIR).join("package.json"), "[").unwrap();
        assert!(matches!(
            embed_manifest_to(&dir.join("package.json"), None, &dir, WINDOWS_RELEASE),
            Err(BuildError::Manifest(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rc_paths_are_escaped() {
        assert_eq!(rc_escape(r#"C:\a "b"\c"#), r#"C:\\a ""b""\\c"#);
        let conf = serde_json::json!({"bundle":{"resources":["../ow-tauri-signed/*"]}});
        assert!(ships_integrity_dll(&conf));
        assert!(!ships_integrity_dll(
            &serde_json::json!({"bundle":{"resources":["icons/*"]}})
        ));
    }

    #[test]
    fn nsis_hooks() {
        let mut m = EmbeddedManifest::minimal("Example App", "Example Studio", "1.2.3");
        let computed = crate::identity::computed_uid("Example Studio", "Example App");
        let nsh = nsis_installer_hooks(&m, None, "tauri");
        assert!(!nsh.contains('@'), "unfilled placeholder");
        assert!(nsh.contains(&format!(
            r#"WriteRegStr SHCTX "Software\OverwolfElectron\{computed}" "version" "1.2.3""#
        )));
        // The per-user state folder, in the per-user context also for a
        // per-machine install; the install record in the install mode's.
        let rmdir = format!(r#"RMDir /r "$APPDATA\ow-electron\{computed}""#);
        let at = nsh.find(&rmdir).expect("state folder removal");
        let before = &nsh[..at];
        assert!(
            before.trim_end().ends_with("SetShellVarContext current"),
            "{nsh}"
        );
        let record = before
            .find(&format!(
                r#"DeleteRegKey SHCTX "Software\OverwolfElectron\{computed}""#
            ))
            .expect("record removal");
        assert!(
            before[..record].contains("!insertmacro OW_TAURI_INSTALL_CONTEXT"),
            "{nsh}"
        );
        assert!(nsh.contains(r#"!if "${INSTALLMODE}" == "perMachine""#));
        assert!(nsh.contains("${If} $UpdateMode <> 1"));
        let url = format!(
            "https://analyticssec.overwolf.com/analytics/Counter?Name=ow_tauri_app_uninstall&MUID=$R0&MUIDV2=$R1&Extra=%7B%22app_id%22%3A%22{computed}%22%2C%22app_version%22%3A%221.2.3%22%2C%22app_name%22%3A%22Example+App%22%7D"
        );
        assert!(nsh.contains(&url), "{nsh}");

        m.overwolf.uid = Some("djpddhibpjddgdpcfkbooljealnjnamkhlihgbab".into());
        m.version = "1.0.0-\"$x".into();
        m.product_name = "Line\nBreak".into();
        let nsh = nsis_installer_hooks(&m, Some("../bad"), "electron");
        assert!(nsh.contains(r#"OverwolfElectron\djpddhibpjddgdpcfkbooljealnjnamkhlihgbab""#));
        assert!(nsh.contains(r#""version" "1.0.0-$\"$$x""#), "{nsh}");
        assert!(nsh.contains("; app name: Line Break"));
        assert!(nsh.contains("Name=ow_electron_app_uninstall"));
    }
}
