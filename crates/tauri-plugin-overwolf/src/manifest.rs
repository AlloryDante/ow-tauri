//! The app manifest: the `package.json` fields ow-electron reads (CONTRACT
//! section G).
//!
//! `package.json` stays the single manifest of a ported app. The app's build
//! script parses and validates it with [`crate::build::embed_manifest`], which
//! writes an [`EmbeddedManifest`] as JSON into `OUT_DIR`; the
//! [`embedded_manifest!`](crate::embedded_manifest) macro includes that JSON
//! in the binary and the plugin reads it at setup. Nothing reads
//! `package.json` from disk at runtime.
//!
//! ```
//! use tauri_plugin_overwolf::manifest::parse_package_json;
//!
//! let parsed = parse_package_json(r#"{
//!   "name": "my-app",
//!   "version": "1.2.3",
//!   "author": { "name": "Example Studio" },
//!   "overwolf": { "packages": ["gep", "utility"] },
//!   "build": { "productName": "My App", "overwolf": { "disableAdOptimization": true } }
//! }"#).unwrap();
//! // `build.productName` is ignored at runtime, as ow-electron ignores it (G.1).
//! assert_eq!(parsed.manifest.product_name, "my-app");
//! assert_eq!(parsed.manifest.author, "Example Studio");
//! assert!(parsed.manifest.build_overwolf.disable_ad_optimization);
//! ```

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The package names ow-electron documents for `overwolf.packages`.
pub const KNOWN_PACKAGES: [&str; 5] = ["gep", "overlay", "recorder", "utility", "crn"];

/// The validated manifest the plugin embeds (CONTRACT G.3, `EmbeddedManifest`).
///
/// It is also `HostSnapshot.manifest` on the wire, so it serialises with
/// `camelCase` keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedManifest {
    /// `package.json` `name`, or the app name when `name` is absent.
    pub name: String,
    /// The app name `<PN>` (G.1): `productName` if it is a non-empty string,
    /// else `name`. `build.productName` is ignored, as ow-electron ignores it.
    pub product_name: String,
    /// `package.json` `version`.
    pub version: String,
    /// `author.name`, or the `author` string as written (OQ-01).
    pub author: String,
    /// The `overwolf` block.
    pub overwolf: OverwolfBlock,
    /// The `build.overwolf` block, with the builder's defaults applied.
    pub build_overwolf: BuildOverwolf,
    /// The packaged form of `package.json` ([`packaged_form`]); in a signed
    /// release build, the signed `package.json` from `ow-tauri sign`.
    pub raw: Map<String, Value>,
}

/// `package.json` `overwolf`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverwolfBlock {
    /// Packages to load, in manifest order (G.1; `utility` is not added
    /// implicitly, OQ-34).
    #[serde(default)]
    pub packages: Vec<String>,
    /// A console-assigned uid written by Overwolf signing (G.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
}

/// `package.json` `build.overwolf` (the five builder fields).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the five independent builder flags of the manifest"
)]
pub struct BuildOverwolf {
    /// Default of the runtime ad-optimisation switch (`settings.disableOptimization`).
    pub disable_ad_optimization: bool,
    /// Bundle packages into the installer (native runtime build hook only).
    pub enable_package_bundling: bool,
    /// Package list URL override (native runtime only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_packages_url: Option<String>,
    /// Overwolf signing required. The builder treats an absent flag as `true`.
    pub require_signing: bool,
    /// Sign with Overwolf's certificate.
    #[serde(rename = "enableOWCertSigning")]
    pub enable_ow_cert_signing: bool,
}

impl Default for BuildOverwolf {
    fn default() -> Self {
        BuildOverwolf {
            disable_ad_optimization: false,
            enable_package_bundling: false,
            override_packages_url: None,
            require_signing: true,
            enable_ow_cert_signing: false,
        }
    }
}

impl EmbeddedManifest {
    /// Parses the JSON that [`crate::build::embed_manifest`] wrote.
    ///
    /// # Errors
    ///
    /// [`ManifestError`] when the JSON is not an embedded manifest.
    ///
    /// ```
    /// use tauri_plugin_overwolf::manifest::EmbeddedManifest;
    /// let json = serde_json::to_string(&EmbeddedManifest::minimal("Demo", "Example Studio", "1.0.0")).unwrap();
    /// let m = EmbeddedManifest::from_embedded_json(&json).unwrap();
    /// assert_eq!(m.version, "1.0.0");
    /// assert_eq!(EmbeddedManifest::from_embedded_json("[]").unwrap_err().path, "$");
    /// ```
    pub fn from_embedded_json(json: &str) -> Result<Self, ManifestError> {
        serde_json::from_str(json).map_err(|e| ManifestError {
            path: "$".into(),
            message: format!("not an embedded ow-tauri manifest: {e}"),
        })
    }

    /// A minimal manifest for tests and examples.
    ///
    /// ```
    /// let m = tauri_plugin_overwolf::manifest::EmbeddedManifest::minimal("Demo", "Example Studio", "1.0.0");
    /// assert_eq!(m.product_name, "Demo");
    /// ```
    #[must_use]
    pub fn minimal(product_name: &str, author: &str, version: &str) -> Self {
        let mut raw = Map::new();
        raw.insert("name".into(), Value::String(product_name.into()));
        raw.insert("version".into(), Value::String(version.into()));
        raw.insert("author".into(), Value::String(author.into()));
        EmbeddedManifest {
            name: product_name.into(),
            product_name: product_name.into(),
            version: version.into(),
            author: author.into(),
            overwolf: OverwolfBlock::default(),
            build_overwolf: BuildOverwolf::default(),
            raw,
        }
    }

    /// The `package.json` text served at `<appPath>/package.json` (CONTRACT
    /// A.2.3): the raw manifest, pretty-printed.
    ///
    /// ```
    /// use tauri_plugin_overwolf::manifest::EmbeddedManifest;
    /// let text = EmbeddedManifest::minimal("Demo", "Example Studio", "1.0.0").package_json_text();
    /// let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    /// assert_eq!(v["author"], "Example Studio");
    /// ```
    #[must_use]
    pub fn package_json_text(&self) -> String {
        serde_json::to_string_pretty(&self.raw).unwrap_or_default()
    }
}

/// A validation failure, naming the offending field.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("package.json {path}: {message}")]
pub struct ManifestError {
    /// JSON path of the field, for example `overwolf.packages[2]`.
    pub path: String,
    /// What is wrong.
    pub message: String,
}

/// A condition the build reports as `cargo:warning` (CONTRACT G.1, G.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestWarning {
    /// JSON path of the field the warning is about.
    pub path: String,
    /// The warning text.
    pub message: String,
}

impl std::fmt::Display for ManifestWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "package.json {}: {}", self.path, self.message)
    }
}

/// The result of [`parse_package_json`].
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedManifest {
    /// The validated manifest.
    pub manifest: EmbeddedManifest,
    /// Non-fatal findings, in field order.
    pub warnings: Vec<ManifestWarning>,
}

fn err(path: &str, message: impl Into<String>) -> ManifestError {
    ManifestError {
        path: path.into(),
        message: message.into(),
    }
}

fn warn(path: &str, message: impl Into<String>) -> ManifestWarning {
    ManifestWarning {
        path: path.into(),
        message: message.into(),
    }
}

fn optional_string(
    obj: &Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<String>, ManifestError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(err(path, "must be a string")),
    }
}

fn optional_bool(
    obj: &Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<bool>, ManifestError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(err(path, "must be a boolean")),
    }
}

/// Parses and validates `package.json` text (CONTRACT G.1, G.3).
///
/// Validation errors (the build fails): the file is not a JSON object; both
/// `name` and `productName` are missing;
/// `overwolf.packages` is not an array of strings; a `build.overwolf` flag is
/// not a boolean; a string field has another type.
///
/// Warnings: the app name contains `bot` (Overwolf refuses such names);
/// packages are listed without `utility` (OQ-34); a listed package name is
/// not one ow-electron documents; `enablePackageBundling` or
/// `overridePackagesUrl` have no effect without a native package runtime;
/// `version` or `author` is missing.
///
/// # Errors
///
/// [`ManifestError`] naming the first invalid field.
///
/// ```
/// use tauri_plugin_overwolf::manifest::parse_package_json;
/// let err = parse_package_json(r#"{ "overwolf": { "packages": "gep" } }"#).unwrap_err();
/// assert_eq!(err.path, "name");
/// let err = parse_package_json(r#"{ "name": "a", "overwolf": { "packages": "gep" } }"#).unwrap_err();
/// assert_eq!(err.path, "overwolf.packages");
/// ```
#[expect(
    clippy::too_many_lines,
    reason = "validates every G.1 field in manifest order"
)]
pub fn parse_package_json(text: &str) -> Result<ParsedManifest, ManifestError> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| err("$", format!("invalid JSON: {e}")))?;
    let Value::Object(root) = value else {
        return Err(err("$", "must be a JSON object"));
    };
    let mut warnings = Vec::new();

    let name = optional_string(&root, "name", "name")?;
    let product_name = optional_string(&root, "productName", "productName")?;
    let build = match root.get("build") {
        None | Some(Value::Null) => None,
        Some(Value::Object(b)) => Some(b),
        Some(_) => return Err(err("build", "must be an object")),
    };
    let build_product_name = match build {
        Some(b) => optional_string(b, "productName", "build.productName")?,
        None => None,
    };
    let app_name = product_name
        .filter(|s| !s.is_empty())
        .or_else(|| name.clone())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| err("name", "name or productName is required"))?;
    if build_product_name.as_deref().is_some_and(|b| b != app_name) {
        warnings.push(warn(
            "build.productName",
            "build.productName is ignored at runtime, as ow-electron ignores it; the app name and uid use productName or name",
        ));
    }

    let version = match optional_string(&root, "version", "version")? {
        Some(v) if !v.trim().is_empty() => v,
        _ => {
            warnings.push(warn(
                "version",
                "missing; app.getVersion() returns \"0.0.0\"",
            ));
            "0.0.0".to_owned()
        }
    };

    let author = match root.get("author") {
        None | Some(Value::Null) => {
            warnings.push(warn(
                "author",
                "missing; the computed uid uses the author \"unknown\", set it before the first release",
            ));
            String::new()
        }
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(a)) => optional_string(a, "name", "author.name")?.unwrap_or_default(),
        Some(_) => return Err(err("author", "must be a string or an object with a name")),
    };

    let overwolf = match root.get("overwolf") {
        None | Some(Value::Null) => OverwolfBlock::default(),
        Some(Value::Object(ow)) => {
            let packages = match ow.get("packages") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(items)) => items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| match item {
                        Value::String(s) if !s.is_empty() => Ok(s.clone()),
                        _ => Err(err(
                            &format!("overwolf.packages[{i}]"),
                            "must be a non-empty string",
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                Some(_) => return Err(err("overwolf.packages", "must be an array of strings")),
            };
            let uid = optional_string(ow, "uid", "overwolf.uid")?;
            // The uid names the state directory `<appData>/ow-electron/<uid>`
            // (F.1), so it must be a plain path segment.
            if let Some(u) = uid.as_deref().map(str::trim).filter(|u| !u.is_empty())
                && !crate::identity::is_valid_uid(u)
            {
                return Err(err(
                    "overwolf.uid",
                    "must be 1 to 64 ASCII letters or digits",
                ));
            }
            OverwolfBlock { packages, uid }
        }
        Some(_) => return Err(err("overwolf", "must be an object")),
    };

    let build_overwolf = match build.and_then(|b| b.get("overwolf")) {
        None | Some(Value::Null) => BuildOverwolf::default(),
        Some(Value::Object(bo)) => {
            let d = BuildOverwolf::default();
            BuildOverwolf {
                disable_ad_optimization: optional_bool(
                    bo,
                    "disableAdOptimization",
                    "build.overwolf.disableAdOptimization",
                )?
                .unwrap_or(d.disable_ad_optimization),
                enable_package_bundling: optional_bool(
                    bo,
                    "enablePackageBundling",
                    "build.overwolf.enablePackageBundling",
                )?
                .unwrap_or(d.enable_package_bundling),
                override_packages_url: optional_string(
                    bo,
                    "overridePackagesUrl",
                    "build.overwolf.overridePackagesUrl",
                )?,
                require_signing: optional_bool(
                    bo,
                    "requireSigning",
                    "build.overwolf.requireSigning",
                )?
                .unwrap_or(d.require_signing),
                enable_ow_cert_signing: optional_bool(
                    bo,
                    "enableOWCertSigning",
                    "build.overwolf.enableOWCertSigning",
                )?
                .unwrap_or(d.enable_ow_cert_signing),
            }
        }
        Some(_) => return Err(err("build.overwolf", "must be an object")),
    };

    if app_name.to_ascii_lowercase().contains("bot") {
        warnings.push(warn(
            "productName",
            "the app name contains \"bot\"; Overwolf refuses such names because ad partners see them",
        ));
    }
    for (i, package) in overwolf.packages.iter().enumerate() {
        if !KNOWN_PACKAGES.contains(&package.as_str()) {
            warnings.push(warn(
                &format!("overwolf.packages[{i}]"),
                format!(
                    "\"{package}\" is not a documented package; it is passed to the runtime as-is"
                ),
            ));
        }
    }
    if !overwolf.packages.is_empty() && !overwolf.packages.iter().any(|p| p == "utility") {
        warnings.push(warn(
            "overwolf.packages",
            "lists packages without \"utility\"; ow-electron's builder adds it implicitly, ow-tauri uses the list as written (OQ-34)",
        ));
    }
    if build_overwolf.enable_package_bundling {
        warnings.push(warn(
            "build.overwolf.enablePackageBundling",
            "has no effect unless a native package runtime provides a build hook",
        ));
    }
    if build_overwolf.override_packages_url.is_some() {
        warnings.push(warn(
            "build.overwolf.overridePackagesUrl",
            "is only passed to a native package runtime",
        ));
    }

    let raw = packaged_form(&root);

    Ok(ParsedManifest {
        manifest: EmbeddedManifest {
            name: name.unwrap_or_else(|| app_name.clone()),
            product_name: app_name,
            version,
            author,
            overwolf,
            build_overwolf,
            raw,
        },
        warnings,
    })
}

/// Top-level keys the builder never ships (G.3).
const UNSHIPPED_KEYS: [&str; 11] = [
    "dist",
    "gitHead",
    "build",
    "jspm",
    "ava",
    "xo",
    "nyc",
    "eslintConfig",
    "contributors",
    "bundleDependencies",
    "tags",
];

/// Merges `source` into `target` the way the builder's `deepAssign` does:
/// objects merge key by key, everything else (arrays included) replaces.
fn deep_assign(target: &mut Map<String, Value>, source: &Map<String, Value>) {
    for (key, value) in source {
        match (target.get_mut(key), value) {
            (Some(Value::Object(current)), Value::Object(update)) => deep_assign(current, update),
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

/// The packaged form of `package.json` (CONTRACT G.3, `raw`): what
/// Overwolf's builder ships and signs. `build.extraMetadata` is merged in,
/// then keys starting with `_`, the builder's build-only keys (`build`,
/// `dist`, `gitHead`, …), `scripts` and `keywords` (unless
/// `build.removePackageScripts` / `removePackageKeywords` is `false`),
/// `devDependencies`, and `babel` when no dependency is a Babel package,
/// are removed. `ow-tauri sign` sends the same object.
///
/// ```
/// use tauri_plugin_overwolf::manifest::packaged_form;
/// let pkg = serde_json::json!({
///     "name": "demo", "_id": "x", "scripts": {}, "keywords": ["a"],
///     "devDependencies": {}, "build": { "extraMetadata": { "main": "app.js" } },
/// });
/// let packaged = packaged_form(pkg.as_object().unwrap());
/// assert_eq!(serde_json::Value::Object(packaged), serde_json::json!({ "name": "demo", "main": "app.js" }));
/// ```
#[must_use]
pub fn packaged_form(root: &Map<String, Value>) -> Map<String, Value> {
    let build = root.get("build").and_then(Value::as_object);
    let mut out = root.clone();
    if let Some(extra) = build
        .and_then(|b| b.get("extraMetadata"))
        .and_then(Value::as_object)
    {
        deep_assign(&mut out, extra);
    }
    let keeps = |key: &str| build.and_then(|b| b.get(key)) == Some(&Value::Bool(false));
    let remove_scripts = !keeps("removePackageScripts");
    let remove_keywords = !keeps("removePackageKeywords");
    let remove_babel = out
        .get("dependencies")
        .and_then(Value::as_object)
        .is_some_and(|deps| !deps.keys().any(|name| name.starts_with("babel")));
    out.retain(|key, _| {
        !(key.starts_with('_')
            || UNSHIPPED_KEYS.contains(&key.as_str())
            || (remove_scripts && key == "scripts")
            || (remove_keywords && key == "keywords")
            || key == "devDependencies"
            || (remove_babel && key == "babel"))
    });
    out
}

/// Release-build warnings for the signing flags (CONTRACT G.1, G.4).
///
/// `release` is a release build for a target the builder signs (Windows);
/// `signed` is whether the output of `ow-tauri sign` was applied.
///
/// ```
/// use tauri_plugin_overwolf::manifest::{signing_warnings, BuildOverwolf};
/// let unsigned = signing_warnings(&BuildOverwolf::default(), true, false);
/// assert_eq!(unsigned[0].path, "build.overwolf.requireSigning");
/// assert!(signing_warnings(&BuildOverwolf::default(), true, true).is_empty());
/// assert!(signing_warnings(&BuildOverwolf::default(), false, false).is_empty());
/// ```
#[must_use]
pub fn signing_warnings(
    build: &BuildOverwolf,
    release: bool,
    signed: bool,
) -> Vec<ManifestWarning> {
    let mut out = Vec::new();
    if !release || signed {
        return out;
    }
    if build.require_signing {
        out.push(warn(
            "build.overwolf.requireSigning",
            "this release build is not Overwolf-signed: run `npx ow-tauri sign` before `tauri build` (it fails when signing is required and cannot complete), or set requireSigning to false",
        ));
    }
    if build.enable_ow_cert_signing {
        out.push(warn(
            "build.overwolf.enableOWCertSigning",
            "Overwolf certificate signing needs the output of `npx ow-tauri sign`",
        ));
    }
    out
}

/// Warns when `enableOWCertSigning` is set but `tauri.conf.json`
/// `bundle.windows.signCommand` does not run `ow-tauri sign-exe`, the step
/// that posts the app exe to the certificate service (CONTRACT G.4 d), and
/// when a string `signCommand` starts with `npx`: Tauri starts it without a
/// shell, so `npx` (`npx.cmd`) cannot start on Windows; the object form
/// `{"cmd": "npx.cmd", "args": [...]}` can.
///
/// ```
/// use tauri_plugin_overwolf::manifest::{sign_command_warnings, BuildOverwolf};
/// let b = BuildOverwolf { enable_ow_cert_signing: true, ..BuildOverwolf::default() };
/// let conf = serde_json::json!({ "bundle": { "windows": { "signCommand": {
///     "cmd": "npx.cmd", "args": ["ow-tauri", "sign-exe", "%1"]
/// } } } });
/// assert!(sign_command_warnings(&b, &conf).is_empty());
/// assert_eq!(sign_command_warnings(&b, &serde_json::json!({})).len(), 1);
/// let npx = serde_json::json!({ "bundle": { "windows": { "signCommand": "npx ow-tauri sign-exe %1" } } });
/// assert!(sign_command_warnings(&b, &npx)[0].message.contains("npx.cmd"));
/// ```
#[must_use]
pub fn sign_command_warnings(build: &BuildOverwolf, conf: &Value) -> Vec<ManifestWarning> {
    let mut out = Vec::new();
    let sign_command = conf.pointer("/bundle/windows/signCommand");
    if let Some(Value::String(line)) = sign_command
        && line
            .split_whitespace()
            .next()
            .is_some_and(|first| first.eq_ignore_ascii_case("npx"))
    {
        out.push(warn(
            "bundle.windows.signCommand",
            "Tauri starts a string signCommand without a shell, so `npx` cannot start on Windows; use the object form {\"cmd\": \"npx.cmd\", \"args\": [\"ow-tauri\", \"sign-exe\", \"%1\"]}",
        ));
    }
    if !build.enable_ow_cert_signing {
        return out;
    }
    let command = match sign_command {
        Some(Value::String(line)) => line.clone(),
        Some(Value::Object(spec)) => {
            let mut words = vec![spec.get("cmd").and_then(Value::as_str).unwrap_or_default()];
            if let Some(Value::Array(args)) = spec.get("args") {
                words.extend(args.iter().filter_map(Value::as_str));
            }
            words.join(" ")
        }
        _ => String::new(),
    };
    // `npx ow-tauri sign-exe`, `node …/ow-tauri/dist/cli/index.js sign-exe`, …
    let words: Vec<&str> = command.split_whitespace().collect();
    let runs_sign_exe = words
        .iter()
        .position(|w| *w == "sign-exe")
        .is_some_and(|at| words[..at].iter().any(|w| w.contains("ow-tauri")));
    if !runs_sign_exe {
        out.push(warn(
            "build.overwolf.enableOWCertSigning",
            "tauri.conf.json bundle.windows.signCommand does not run `ow-tauri sign-exe`, so the app exe is not signed with Overwolf's certificate; set it to {\"cmd\": \"npx.cmd\", \"args\": [\"ow-tauri\", \"sign-exe\", \"%1\"]}",
        ));
    }
    out
}

/// Warnings for `tauri.conf.json` values that disagree with the manifest
/// (CONTRACT G.1). `conf` is the parsed `tauri.conf.json`.
///
/// ```
/// use tauri_plugin_overwolf::manifest::{tauri_conf_warnings, EmbeddedManifest};
/// let m = EmbeddedManifest::minimal("Demo", "Studio", "1.0.0");
/// let conf = serde_json::json!({ "productName": "Other", "version": "1.0.0" });
/// assert_eq!(tauri_conf_warnings(&m, &conf).len(), 1);
/// ```
#[must_use]
pub fn tauri_conf_warnings(manifest: &EmbeddedManifest, conf: &Value) -> Vec<ManifestWarning> {
    let mut out = Vec::new();
    if let Some(product) = conf.get("productName").and_then(Value::as_str)
        && product != manifest.product_name
    {
        out.push(warn(
            "productName",
            format!(
                "tauri.conf.json productName \"{product}\" differs from the app name \"{}\"; the uid uses the package.json value",
                manifest.product_name
            ),
        ));
    }
    if let Some(version) = conf.get("version").and_then(Value::as_str)
        && !std::path::Path::new(version)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        && version != manifest.version
    {
        out.push(warn(
            "version",
            format!(
                "tauri.conf.json version \"{version}\" differs from package.json \"{}\"",
                manifest.version
            ),
        ));
    }
    if let Some(plugin) = conf.pointer("/plugins/overwolf") {
        for message in crate::config::removed_key_warnings(plugin) {
            out.push(warn("plugins.overwolf", message));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "name": "overwolf-official-sample-app",
      "version": "1.0.0",
      "author": "Overwolf Ltd.",
      "main": "dist/browser/index.js",
      "scripts": { "start": "x" },
      "devDependencies": { "typescript": "6" },
      "overwolf": { "packages": ["gep", "utility", "overlay", "recorder"] },
      "build": {
        "productName": "Overwolf Electron Official Sample App",
        "overwolf": { "disableAdOptimization": false },
        "win": { "target": "nsis" }
      }
    }"#;

    #[test]
    fn parses_the_upstream_sample_manifest() {
        let parsed = parse_package_json(SAMPLE).unwrap();
        let m = &parsed.manifest;
        assert_eq!(m.name, "overwolf-official-sample-app");
        // `build.productName` is electron-builder's; the runtime name is `name` (G.1).
        assert_eq!(m.product_name, "overwolf-official-sample-app");
        assert_eq!(m.author, "Overwolf Ltd.");
        assert_eq!(m.version, "1.0.0");
        assert_eq!(
            m.overwolf.packages,
            ["gep", "utility", "overlay", "recorder"]
        );
        assert!(m.overwolf.uid.is_none());
        assert!(!m.build_overwolf.disable_ad_optimization);
        assert!(
            m.build_overwolf.require_signing,
            "absent requireSigning means required"
        );
        assert!(!m.raw.contains_key("scripts"));
        assert!(!m.raw.contains_key("devDependencies"));
        assert!(m.raw.contains_key("main"));
        let paths: Vec<&str> = parsed.warnings.iter().map(|w| w.path.as_str()).collect();
        assert_eq!(paths, ["build.productName"]);
    }

    #[test]
    fn product_name_precedence() {
        let p = parse_package_json(r#"{"name":"n","productName":"P","build":{"productName":"B"}}"#)
            .unwrap();
        assert_eq!(p.manifest.product_name, "P");
        assert!(p.warnings.iter().any(|w| w.path == "build.productName"));
        let p = parse_package_json(r#"{"name":"n","productName":"P"}"#).unwrap();
        assert_eq!(p.manifest.product_name, "P");
        let p = parse_package_json(r#"{"name":"n"}"#).unwrap();
        assert_eq!(p.manifest.product_name, "n");
        let p = parse_package_json(r#"{"productName":"P"}"#).unwrap();
        assert_eq!(p.manifest.name, "P");
    }

    #[test]
    fn author_forms() {
        let p =
            parse_package_json(r#"{"name":"n","author":{"name":"Studio","email":"x"}}"#).unwrap();
        assert_eq!(p.manifest.author, "Studio");
        let p = parse_package_json(r#"{"name":"n","author":"Studio <a@b.c>"}"#).unwrap();
        assert_eq!(
            p.manifest.author, "Studio <a@b.c>",
            "string author is used as-is (OQ-01)"
        );
        let p = parse_package_json(r#"{"name":"n"}"#).unwrap();
        assert!(p.warnings.iter().any(|w| w.path == "author"));
        assert!(parse_package_json(r#"{"name":"n","author":5}"#).is_err());
    }

    #[test]
    fn validation_errors_name_the_field() {
        let cases = [
            ("[]", "$"),
            ("{", "$"),
            (r#"{"version":"1"}"#, "name"),
            (r#"{"name":"  "}"#, "name"),
            (
                r#"{"name":"n","overwolf":{"packages":[1]}}"#,
                "overwolf.packages[0]",
            ),
            (
                r#"{"name":"n","overwolf":{"packages":["gep",""]}}"#,
                "overwolf.packages[1]",
            ),
            (r#"{"name":"n","overwolf":[]}"#, "overwolf"),
            (
                r#"{"name":"n","build":{"overwolf":{"requireSigning":"yes"}}}"#,
                "build.overwolf.requireSigning",
            ),
            (
                r#"{"name":"n","build":{"overwolf":{"disableAdOptimization":1}}}"#,
                "build.overwolf.disableAdOptimization",
            ),
            (
                r#"{"name":"n","build":{"overwolf":{"enablePackageBundling":null,"enableOWCertSigning":0}}}"#,
                "build.overwolf.enableOWCertSigning",
            ),
            (
                r#"{"name":"n","build":{"overwolf":{"overridePackagesUrl":true}}}"#,
                "build.overwolf.overridePackagesUrl",
            ),
            (r#"{"name":"n","build":7}"#, "build"),
            (r#"{"name":"n","overwolf":{"uid":3}}"#, "overwolf.uid"),
            (
                r#"{"name":"n","overwolf":{"uid":"../../x"}}"#,
                "overwolf.uid",
            ),
            (r#"{"name":"n","overwolf":{"uid":"a/b"}}"#, "overwolf.uid"),
        ];
        for (text, path) in cases {
            let e = parse_package_json(text).unwrap_err();
            assert_eq!(e.path, path, "{text}");
        }
    }

    #[test]
    fn warnings() {
        let p = parse_package_json(
            r#"{"name":"Robotic Helper","version":"1","author":"a","overwolf":{"packages":["gep","custom"]},
                "build":{"overwolf":{"enablePackageBundling":true,"overridePackagesUrl":"https://x"}}}"#,
        )
        .unwrap();
        let paths: Vec<&str> = p.warnings.iter().map(|w| w.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "productName",
                "overwolf.packages[1]",
                "overwolf.packages",
                "build.overwolf.enablePackageBundling",
                "build.overwolf.overridePackagesUrl"
            ]
        );
        assert!(p.warnings[0].to_string().contains("bot"));
    }

    #[test]
    fn console_uid_is_kept() {
        let p = parse_package_json(
            r#"{"name":"n","author":"a","version":"1","overwolf":{"uid":"abc"}}"#,
        )
        .unwrap();
        assert_eq!(p.manifest.overwolf.uid.as_deref(), Some("abc"));
    }

    #[test]
    fn embedded_json_round_trips_with_contract_keys() {
        let m = parse_package_json(SAMPLE).unwrap().manifest;
        let json = serde_json::to_value(&m).unwrap();
        for key in [
            "name",
            "productName",
            "version",
            "author",
            "overwolf",
            "buildOverwolf",
            "raw",
        ] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
        let bo = &json["buildOverwolf"];
        for key in [
            "disableAdOptimization",
            "enablePackageBundling",
            "requireSigning",
            "enableOWCertSigning",
        ] {
            assert!(bo.get(key).is_some(), "missing buildOverwolf.{key}");
        }
        let back = EmbeddedManifest::from_embedded_json(&json.to_string()).unwrap();
        assert_eq!(back, m);
        assert!(EmbeddedManifest::from_embedded_json("{}").is_err());
    }

    #[test]
    fn signing_and_tauri_conf_warnings() {
        let mut b = BuildOverwolf {
            enable_ow_cert_signing: true,
            ..BuildOverwolf::default()
        };
        assert_eq!(signing_warnings(&b, true, false).len(), 2);
        assert!(signing_warnings(&b, true, true).is_empty());
        let object = serde_json::json!({"bundle":{"windows":{"signCommand":{"cmd":"node_modules/.bin/ow-tauri","args":["sign-exe","%1"]}}}});
        assert!(sign_command_warnings(&b, &object).is_empty());
        let other = serde_json::json!({"bundle":{"windows":{"signCommand":"signtool sign %1"}}});
        assert_eq!(sign_command_warnings(&b, &other).len(), 1);
        let npx =
            serde_json::json!({"bundle":{"windows":{"signCommand":"npx ow-tauri sign-exe %1"}}});
        let paths: Vec<String> = sign_command_warnings(&b, &npx)
            .into_iter()
            .map(|w| w.path)
            .collect();
        assert_eq!(paths, ["bundle.windows.signCommand"]);
        b.require_signing = false;
        b.enable_ow_cert_signing = false;
        assert!(signing_warnings(&b, true, false).is_empty());
        assert!(sign_command_warnings(&b, &other).is_empty());
        // The npx warning does not depend on Overwolf certificate signing.
        assert_eq!(sign_command_warnings(&b, &npx).len(), 1);

        let m = EmbeddedManifest::minimal("Demo", "S", "1.0.0");
        let ok = serde_json::json!({"productName":"Demo","version":"../package.json"});
        assert!(tauri_conf_warnings(&m, &ok).is_empty());
        let bad = serde_json::json!({"productName":"Demo","version":"2.0.0"});
        assert_eq!(tauri_conf_warnings(&m, &bad)[0].path, "version");
    }

    #[test]
    fn package_json_text_is_the_raw_manifest() {
        let m = parse_package_json(SAMPLE).unwrap().manifest;
        let text = m.package_json_text();
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["name"], "overwolf-official-sample-app");
        assert!(back.get("scripts").is_none());
        assert!(back.get("build").is_none());
        assert!(back.get("devDependencies").is_none());
    }

    #[test]
    fn packaged_form_follows_the_builder() {
        let pkg = serde_json::json!({
            "name": "demo",
            "babel": {},
            "keywords": ["k"],
            "scripts": {"a": "b"},
            "dependencies": {"left-pad": "1"},
            "overwolf": {"packages": ["gep"]},
            "build": {
                "removePackageScripts": false,
                "removePackageKeywords": false,
                "extraMetadata": {"overwolf": {"uid": "abc"}, "tags": ["x"]}
            }
        });
        let out = Value::Object(packaged_form(pkg.as_object().unwrap()));
        assert_eq!(
            out,
            serde_json::json!({
                "name": "demo",
                "keywords": ["k"],
                "scripts": {"a": "b"},
                "dependencies": {"left-pad": "1"},
                "overwolf": {"packages": ["gep"], "uid": "abc"}
            })
        );
        let babel =
            serde_json::json!({"name": "d", "babel": {}, "dependencies": {"babel-core": "6"}});
        assert!(packaged_form(babel.as_object().unwrap()).contains_key("babel"));
        let no_deps = serde_json::json!({"name": "d", "babel": {}});
        assert!(packaged_form(no_deps.as_object().unwrap()).contains_key("babel"));
    }
}
