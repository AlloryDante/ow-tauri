//! The generic-provider feed file (CONTRACT I.1, I.2 #2): `latest.yml` and
//! its channel and per-OS variants.
//!
//! The YAML is read with a pure YAML 1.2 parser and mapped to JSON values;
//! the fields electron-updater reads are then picked out. Field names
//! inside `files[]` match case-insensitively, because Overwolf's feed
//! spells `IsAdminRightsRequired` with a capital `I` (observed).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use yaml_rust2::parser::{Event, Parser};
use yaml_rust2::{Yaml, YamlLoader};

use crate::error::Error;

/// The largest feed file the client reads (a feed is a few hundred bytes).
pub const MAX_FEED_BYTES: usize = 1024 * 1024;

/// One downloadable file of a release (`files[]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFileInfo {
    /// The file URL, absolute or relative to the feed URL.
    pub url: String,
    /// Base64 SHA-512 of the file.
    pub sha512: String,
    /// Size in bytes, when the feed states it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Size of the block map (ignored: updates are full downloads, I.2 #3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_map_size: Option<u64>,
    /// Whether the installer needs elevation (Overwolf's
    /// `IsAdminRightsRequired`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_admin_rights_required: Option<bool>,
}

/// `UpdateInfo` (I.5): a parsed feed file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// The release version, as written in the feed.
    pub version: String,
    /// The release files.
    pub files: Vec<UpdateFileInfo>,
    /// Legacy top-level file path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Legacy top-level SHA-512.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha512: Option<String>,
    /// Release date (ISO 8601 text).
    #[serde(default)]
    pub release_date: String,
    /// Release name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_name: Option<String>,
    /// Release notes: text, or a list of `{ version, note }`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<Value>,
    /// Staged rollout share, as written in the feed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staging_percentage: Option<Value>,
    /// The downloaded installer, on `update-downloaded` (electron-updater's
    /// `downloadedFile`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloaded_file: Option<String>,
}

fn scalar_text(y: &Yaml) -> Option<String> {
    match y {
        Yaml::String(s) | Yaml::Real(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

fn to_json(y: &Yaml, depth: usize) -> Result<Value, Error> {
    if depth > 32 {
        return Err(Error::invalid_argument(
            "The update feed is nested too deeply.",
        ));
    }
    Ok(match y {
        Yaml::Null | Yaml::BadValue => Value::Null,
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Integer(i) => Value::from(*i),
        Yaml::Real(s) => s
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or_else(|| Value::String(s.clone()), Value::Number),
        Yaml::String(s) => Value::String(s.clone()),
        Yaml::Array(items) => Value::Array(
            items
                .iter()
                .map(|i| to_json(i, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        Yaml::Hash(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                let Some(key) = scalar_text(k) else {
                    return Err(Error::invalid_argument(
                        "The update feed has a key that is not text.",
                    ));
                };
                out.insert(key, to_json(v, depth + 1)?);
            }
            Value::Object(out)
        }
        Yaml::Alias(_) => {
            return Err(Error::invalid_argument(
                "The update feed uses YAML aliases, which are not supported.",
            ));
        }
    })
}

fn load(text: &str) -> Result<Yaml, Error> {
    if text.len() > MAX_FEED_BYTES {
        return Err(Error::invalid_argument("The update feed is too large."));
    }
    // Aliases are refused before loading: the loader copies an alias's
    // target, so nested aliases could expand a small file without bound.
    let mut parser = Parser::new_from_str(text);
    loop {
        match parser.next_token() {
            Ok((Event::StreamEnd, _)) => break,
            Ok((Event::Alias(_), _)) => {
                return Err(Error::invalid_argument(
                    "The update feed uses YAML aliases, which are not supported.",
                ));
            }
            Ok(_) => {}
            Err(_) => {
                return Err(Error::invalid_argument(
                    "The update feed is not valid YAML.",
                ));
            }
        }
    }
    let docs = YamlLoader::load_from_str(text)
        .map_err(|_| Error::invalid_argument("The update feed is not valid YAML."))?;
    match docs.into_iter().next() {
        Some(doc @ Yaml::Hash(_)) => Ok(doc),
        _ => Err(Error::invalid_argument(
            "The update feed is not a YAML mapping.",
        )),
    }
}

/// Reads a YAML mapping as a JSON object.
///
/// # Errors
///
/// `invalid-argument` when `text` is not one YAML mapping of plain values.
///
/// ```
/// use tauri_plugin_overwolf::updater::feed::yaml_to_json;
/// let v = yaml_to_json("a: 1\nb: [x, true]\n").unwrap();
/// assert_eq!(v, serde_json::json!({"a": 1, "b": ["x", true]}));
/// ```
pub fn yaml_to_json(text: &str) -> Result<Value, Error> {
    to_json(&load(text)?, 0)
}

fn non_negative(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn boolean(v: Option<&Value>) -> Option<bool> {
    match v? {
        Value::Bool(b) => Some(*b),
        Value::String(s) if s.eq_ignore_ascii_case("true") => Some(true),
        Value::String(s) if s.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

fn text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn file_entry(value: &Value) -> Result<UpdateFileInfo, Error> {
    let Value::Object(map) = value else {
        return Err(Error::invalid_argument(
            "An update feed file entry is not a mapping.",
        ));
    };
    let get = |name: &str| {
        map.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    };
    let url = text(get("url"))
        .filter(|u| !u.trim().is_empty())
        .ok_or_else(|| Error::invalid_argument("An update feed file entry has no url."))?;
    Ok(UpdateFileInfo {
        url,
        sha512: text(get("sha512")).unwrap_or_default(),
        size: non_negative(get("size")),
        block_map_size: non_negative(get("blockMapSize")),
        is_admin_rights_required: boolean(get("isAdminRightsRequired")),
    })
}

/// Parses a feed file (I.2 #2). A feed without `files` but with the legacy
/// top-level `path` gets one file entry from `path` and `sha512`, as
/// electron-updater does.
///
/// # Errors
///
/// `invalid-argument` when the text is not a YAML mapping, has no
/// `version`, or names no file.
///
/// ```
/// use tauri_plugin_overwolf::updater::feed::parse_feed;
/// let info = parse_feed("version: 2.0.0\npath: App-2.0.0.exe\nsha512: aGk=\n").unwrap();
/// assert_eq!(info.files[0].url, "App-2.0.0.exe");
/// assert_eq!(info.files[0].sha512, "aGk=");
/// ```
pub fn parse_feed(text_in: &str) -> Result<UpdateInfo, Error> {
    let doc = load(text_in)?;
    let Value::Object(map) = to_json(&doc, 0)? else {
        return Err(Error::invalid_argument(
            "The update feed is not a YAML mapping.",
        ));
    };
    // The version keeps its written form: `1.10` must not become `1.1`.
    let version = doc["version"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| scalar_text(&doc["version"]))
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| Error::invalid_argument("The update feed has no version."))?;
    let mut files = match map.get("files") {
        Some(Value::Array(items)) => items.iter().map(file_entry).collect::<Result<_, _>>()?,
        Some(Value::Null) | None => Vec::new(),
        Some(_) => {
            return Err(Error::invalid_argument(
                "The update feed's files is not a list.",
            ));
        }
    };
    let path = text(map.get("path"));
    let sha512 = text(map.get("sha512"));
    if files.is_empty() {
        let Some(path) = path.clone() else {
            return Err(Error::invalid_argument("The update feed lists no files."));
        };
        files.push(UpdateFileInfo {
            url: path,
            sha512: sha512.clone().unwrap_or_default(),
            ..UpdateFileInfo::default()
        });
    }
    Ok(UpdateInfo {
        version: version.trim().to_owned(),
        files,
        path,
        sha512,
        release_date: text(map.get("releaseDate")).unwrap_or_default(),
        release_name: text(map.get("releaseName")),
        release_notes: map.get("releaseNotes").filter(|v| !v.is_null()).cloned(),
        staging_percentage: map
            .get("stagingPercentage")
            .filter(|v| !v.is_null())
            .cloned(),
        downloaded_file: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape Overwolf's console serves (I.1 [OBS]); stand-in values.
    const OVERWOLF_SHAPE: &str = "version: 1.4.2
files:
  - url: https://downloads.example.com/prod/apps/abc/1.4.2/setup.exe
    sha512: 3q2+7w==
    size: 98765432
    blockMapSize: 102400
    IsAdminRightsRequired: false
releaseDate: '2026-09-30T10:00:00.000Z'
releaseName: Autumn
";

    #[test]
    fn overwolf_shape() {
        let info = parse_feed(OVERWOLF_SHAPE).unwrap();
        assert_eq!(info.version, "1.4.2");
        assert_eq!(info.release_date, "2026-09-30T10:00:00.000Z");
        assert_eq!(info.release_name.as_deref(), Some("Autumn"));
        assert!(info.path.is_none() && info.sha512.is_none());
        let f = &info.files[0];
        assert_eq!(f.size, Some(98_765_432));
        assert_eq!(f.block_map_size, Some(102_400));
        assert_eq!(f.is_admin_rights_required, Some(false));
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["files"][0]["isAdminRightsRequired"], false);
        assert!(json.get("downloadedFile").is_none());
    }

    #[test]
    fn electron_builder_shape_with_staging_and_notes() {
        let info = parse_feed(
            "version: 1.10\nfiles:\n  - url: App.zip\n    sha512: x\n    isAdminRightsRequired: 'true'\npath: App.zip\nsha512: x\nstagingPercentage: 40\nreleaseNotes:\n  - version: 1.10\n    note: Fixes\n",
        )
        .unwrap();
        assert_eq!(info.version, "1.10");
        assert_eq!(info.files[0].is_admin_rights_required, Some(true));
        assert_eq!(info.staging_percentage, Some(serde_json::json!(40)));
        assert!(info.release_notes.unwrap().is_array());
    }

    #[test]
    fn rejects_bad_feeds() {
        for bad in [
            "",
            "- a\n- b\n",
            "files: []\n",
            "version: 1.0.0\n",
            "version: 1.0.0\nfiles: x\n",
            "version: 1.0.0\nfiles:\n  - sha512: x\n",
            "version: 1.0.0\nfiles:\n  - 3\n",
            "version: [1]\nfiles:\n  - url: a\n",
            "version: 1.0.0\nfiles: &a\n  - url: x\nother: *a\n",
            ": : :\n\t",
        ] {
            assert!(parse_feed(bad).is_err(), "{bad:?}");
        }
        assert!(parse_feed(&"#".repeat(MAX_FEED_BYTES + 1)).is_err());
        let deep = format!("version: 1.0.0\nx: {}1{}\n", "[".repeat(40), "]".repeat(40));
        assert!(yaml_to_json(&deep).is_err());
    }
}
