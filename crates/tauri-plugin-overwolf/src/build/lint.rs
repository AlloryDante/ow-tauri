//! The capability lint of the build step (DESIGN §2.4, §4.15; SEC-B1),
//! with the rules of `ow-tauri doctor`:
//!
//! - **error**: a `remote.urls` pattern that covers Overwolf pages
//!   (`*.overwolf.com`) or every origin (`https://*`): an ad page could then
//!   call the app's commands;
//! - **warning** while the `ads` feature is on: a `windows` selector, which
//!   also reaches the ad guest webviews inside those windows (select
//!   `webviews` instead).

use std::path::{Path, PathBuf};

use serde_json::Value;

/// One lint result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    /// Fails the build.
    pub(crate) error: bool,
    /// What and where.
    pub(crate) message: String,
}

/// Whether a capability remote URL pattern covers Overwolf origins or every
/// http(s) origin (the CLI's `coversOverwolf`).
pub(crate) fn covers_overwolf(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let any_host = ["https://*", "http://*"].iter().any(|p| {
        lower
            .strip_prefix(p)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', ':']))
    });
    lower.contains("overwolf.com") || any_host || lower == "*"
}

/// The capabilities in a capability file's JSON: one capability, a list,
/// or `{ "capabilities": [...] }` (Tauri's three file shapes).
fn capabilities_of(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(map) => match map.get("capabilities") {
            Some(Value::Array(items)) => items.iter().collect(),
            _ => vec![value],
        },
        _ => Vec::new(),
    }
}

fn json_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            json_files(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            out.push(path);
        }
    }
}

fn check(capability: &Value, place: &str, ads: bool, out: &mut Vec<Finding>) {
    let id = capability
        .get("identifier")
        .and_then(Value::as_str)
        .unwrap_or(place);
    if ads && capability.get("windows").is_some() {
        out.push(Finding {
            error: false,
            message: format!(
                "capability \"{id}\" ({place}) selects \"windows\": it also reaches the ad guests inside those windows; select \"webviews\" instead"
            ),
        });
    }
    let urls = capability
        .pointer("/remote/urls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);
    for url in urls {
        if covers_overwolf(url) {
            out.push(Finding {
                error: true,
                message: format!(
                    "capability \"{id}\" ({place}) allows the remote URL \"{url}\", which covers Overwolf ad pages; remove it"
                ),
            });
        }
    }
}

/// Lints `<tauri dir>/capabilities/**/*.json` and the inline
/// `app.security.capabilities` of the merged configuration. A file that
/// does not parse is left to `tauri-build`, which reports it.
pub(crate) fn lint_capabilities(tauri_dir: &Path, merged: &Value, ads: bool) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut files = Vec::new();
    let root = tauri_dir.join("capabilities");
    json_files(&root, &mut files);
    for file in files {
        let Some(value) = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            continue;
        };
        let place = file
            .strip_prefix(tauri_dir)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        for capability in capabilities_of(&value) {
            check(capability, &place, ads, &mut out);
        }
    }
    let inline = merged
        .pointer("/app/security/capabilities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|c| c.is_object());
    for capability in inline {
        check(capability, "app.security.capabilities", ads, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn remote_url_rule_matches_the_cli() {
        for bad in [
            "https://*.overwolf.com/*",
            "https://www.overwolf.com/monsdk/*",
            "HTTPS://CONTENT.OVERWOLF.COM",
            "https://*",
            "https://*/*",
            "http://*:8080/*",
            "*",
        ] {
            assert!(covers_overwolf(bad), "{bad}");
        }
        for ok in [
            "https://example.com/*",
            "https://*.example.com/*",
            "http://localhost:1420",
        ] {
            assert!(!covers_overwolf(ok), "{ok}");
        }
    }

    #[test]
    fn files_and_inline_capabilities() {
        let dir = std::env::temp_dir().join(format!("ow-tauri-lint-{}", std::process::id()));
        let caps = dir.join("capabilities/nested");
        std::fs::create_dir_all(&caps).unwrap();
        std::fs::write(
            dir.join("capabilities/default.json"),
            json!({ "identifier": "main", "windows": ["*"], "permissions": ["overwolf:default"] })
                .to_string(),
        )
        .unwrap();
        std::fs::write(
            caps.join("remote.json"),
            json!({ "capabilities": [{ "identifier": "web", "webviews": ["main"], "remote": { "urls": ["https://*.overwolf.com/*"] } }] }).to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("capabilities/broken.json"), "{").unwrap();
        let merged = json!({ "app": { "security": { "capabilities": [
            "main",
            { "identifier": "inline", "webviews": ["main"], "remote": { "urls": ["https://*"] } }
        ] } } });
        let found = lint_capabilities(&dir, &merged, true);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(!found[0].error && found[0].message.contains("capabilities/default.json"));
        assert!(found[1].error && found[1].message.contains("capabilities/nested/remote.json"));
        assert!(found[2].error && found[2].message.contains("\"inline\""));
        // Without ads the windows selector is not reported.
        assert_eq!(lint_capabilities(&dir, &merged, false).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
