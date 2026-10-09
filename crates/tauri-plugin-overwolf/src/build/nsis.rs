//! The NSIS installer hooks of CONTRACT I.6 (DESIGN §4.15): rendered from
//! the merged-config identity, so the install record, the uninstall
//! Counter and the state folder the uninstaller removes name the uid the
//! app uses at run time.

use std::path::Path;

use super::{BuildError, BuildIdentity, file_error};

/// The uninstall Counter host of the installer (I.6), which differs from
/// the runtime's Counter host.
pub(crate) const UNINSTALL_COUNTER_URL: &str =
    "https://analyticssec.overwolf.com/analytics/Counter";

/// The macros file template (`OW_TAURI_HOOK_*`).
const HOOKS_TEMPLATE: &str = include_str!("overwolf-hooks.nsh");

/// The thin `NSIS_HOOK_*` file that includes the macros file.
pub(crate) const INSTALLER_HOOKS: &str = include_str!("installer-hooks.nsh");

/// The macros file name, next to [`INSTALLER_HOOKS_FILE`].
pub(crate) const OVERWOLF_HOOKS_FILE: &str = "overwolf-hooks.nsh";
/// The file `bundle.windows.nsis.installerHooks` points at.
pub(crate) const INSTALLER_HOOKS_FILE: &str = "installer-hooks.nsh";
/// The output folder, relative to the Tauri folder.
pub(crate) const GEN_DIR: &str = "gen/overwolf";

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

fn comment(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// The `overwolf-hooks.nsh` text for `id` with the Counter label
/// `host_label` (`analytics.hostLabel`, CONTRACT I.6).
pub(crate) fn overwolf_hooks(id: &BuildIdentity, host_label: &str) -> String {
    let version = id.version.clone().unwrap_or_default();
    let counter_name = format!("ow_{host_label}_app_uninstall");
    let extra = serde_json::json!({
        "app_id": id.uid,
        "app_version": version,
        "app_name": id.name,
    })
    .to_string();
    let prefix = format!(
        "{UNINSTALL_COUNTER_URL}?Name={}",
        form_encode(&counter_name)
    );
    // Every substituted value is a validated uid, form-encoded or escaped,
    // so none can carry NSIS syntax (`$`, quotes, newlines).
    HOOKS_TEMPLATE
        .replace("@UID@", &id.uid)
        .replace("@PRODUCT_NAME_COMMENT@", &comment(&id.name))
        .replace("@VERSION_COMMENT@", &comment(&version))
        .replace("@VERSION@", &nsis_escape(&version))
        .replace("@COUNTER_NAME@", &counter_name)
        .replace("@COUNTER_URL_PREFIX@", &prefix)
        .replace("@EXTRA@", &form_encode(&extra))
}

/// Writes `bytes` to `path` only when the content changed (so cargo does
/// not rebuild needlessly), creating the folder.
pub(crate) fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<(), BuildError> {
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| file_error(parent, e))?;
    }
    std::fs::write(path, bytes).map_err(|e| file_error(path, e))
}

/// Writes both hook files into `<tauri dir>/gen/overwolf`; returns their
/// paths (macros file, `NSIS_HOOK_*` file).
pub(crate) fn write_hooks(
    tauri_dir: &Path,
    id: &BuildIdentity,
    host_label: &str,
) -> Result<[std::path::PathBuf; 2], BuildError> {
    let dir = tauri_dir.join(GEN_DIR);
    let macros = dir.join(OVERWOLF_HOOKS_FILE);
    let wrapper = dir.join(INSTALLER_HOOKS_FILE);
    write_if_changed(&macros, overwolf_hooks(id, host_label).as_bytes())?;
    write_if_changed(&wrapper, INSTALLER_HOOKS.as_bytes())?;
    Ok([macros, wrapper])
}

/// A warning when `bundle.windows.nsis.installerHooks` (of the merged
/// configuration) neither is the generated file nor includes the macros
/// file, so the installer would miss the Overwolf work.
pub(crate) fn hooks_config_warning(tauri_dir: &Path, merged: &serde_json::Value) -> Option<String> {
    let wanted = format!("{GEN_DIR}/{INSTALLER_HOOKS_FILE}");
    let configured = merged
        .pointer("/bundle/windows/nsis/installerHooks")
        .and_then(serde_json::Value::as_str);
    let Some(configured) = configured else {
        return Some(format!(
            "set bundle.windows.nsis.installerHooks to \"{wanted}\": the Windows installer otherwise skips the Overwolf install record and uninstall work (CONTRACT I.6)"
        ));
    };
    let normal = configured.replace('\\', "/");
    if normal.trim_start_matches("./") == wanted {
        return None;
    }
    let text = std::fs::read_to_string(tauri_dir.join(configured)).unwrap_or_default();
    if text.contains(OVERWOLF_HOOKS_FILE) || text.contains(INSTALLER_HOOKS_FILE) {
        return None;
    }
    Some(format!(
        "bundle.windows.nsis.installerHooks ({configured}) does not include {GEN_DIR}/{OVERWOLF_HOOKS_FILE}; !include it and insert OW_TAURI_HOOK_POSTINSTALL and OW_TAURI_HOOK_POSTUNINSTALL (CONTRACT I.6)"
    ))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    fn id(uid: &str, name: &str, version: &str) -> BuildIdentity {
        BuildIdentity {
            uid: uid.into(),
            cuid: uid.into(),
            name: name.into(),
            author: "Example Studio".into(),
            version: Some(version.into()),
        }
    }

    /// The body of macro `name` in `text`.
    pub(crate) fn macro_body<'a>(text: &'a str, name: &str) -> &'a str {
        let start = text
            .find(&format!("!macro {name}\n"))
            .unwrap_or_else(|| panic!("no macro {name}"));
        let rest = &text[start..];
        &rest[..rest.find("!macroend").unwrap()]
    }

    #[test]
    fn hooks_render_the_identity() {
        let nsh = overwolf_hooks(
            &id(
                "abcdefghijklmnopabcdefghijklmnop",
                "Example \"App\" $X",
                "1.2.3",
            ),
            "tauri",
        );
        assert!(!nsh.contains('@'), "every placeholder is replaced");
        let post = macro_body(&nsh, "OW_TAURI_HOOK_POSTINSTALL");
        assert!(post.contains(
            r#""Software\OverwolfElectron\abcdefghijklmnopabcdefghijklmnop" "version" "1.2.3""#
        ));
        let un = macro_body(&nsh, "OW_TAURI_HOOK_POSTUNINSTALL");
        assert!(un.contains("Name=ow_tauri_app_uninstall"));
        // The Extra object, form-encoded: the name's quotes and `$` cannot
        // break out of the NSIS string.
        assert!(un.contains(
            "%7B%22app_id%22%3A%22abcdefghijklmnopabcdefghijklmnop%22%2C%22app_version%22%3A%221.2.3%22%2C%22app_name%22%3A%22Example+%5C%22App%5C%22+%24X%22%7D"
        ));
        assert_eq!(nsis_escape("a$\"b\n"), "a$$$\\\"b ");
        let custom = overwolf_hooks(&id("u", "A", "1"), "studio");
        assert!(custom.contains("Name=ow_studio_app_uninstall"));
        assert!(INSTALLER_HOOKS.contains(r#"!include "${__FILEDIR__}\overwolf-hooks.nsh""#));
    }

    #[test]
    fn installer_hooks_setting() {
        let dir = std::env::temp_dir().join(format!("ow-tauri-nsis-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = |hooks: Option<&str>| match hooks {
            Some(h) => {
                serde_json::json!({ "bundle": { "windows": { "nsis": { "installerHooks": h } } } })
            }
            None => serde_json::json!({}),
        };
        assert!(hooks_config_warning(&dir, &cfg(None)).is_some());
        assert!(
            hooks_config_warning(&dir, &cfg(Some("gen/overwolf/installer-hooks.nsh"))).is_none()
        );
        assert!(
            hooks_config_warning(&dir, &cfg(Some(r".\gen\overwolf\installer-hooks.nsh"))).is_none()
        );
        std::fs::write(
            dir.join("mine.nsh"),
            "!include \"gen\\overwolf\\overwolf-hooks.nsh\"\n",
        )
        .unwrap();
        assert!(hooks_config_warning(&dir, &cfg(Some("mine.nsh"))).is_none());
        std::fs::write(dir.join("other.nsh"), "; nothing\n").unwrap();
        assert!(hooks_config_warning(&dir, &cfg(Some("other.nsh"))).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
