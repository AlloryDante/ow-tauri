//! Electron accelerator strings for `globalShortcut` (CONTRACT A.2.3,
//! B.2.5), translated to the syntax `tauri-plugin-global-shortcut` parses.
//!
//! ```
//! use tauri_plugin_overwolf::accelerator::normalize;
//! assert_eq!(normalize("CommandOrControl+Shift+Z").as_deref(), Some("CmdOrCtrl+Shift+Z"));
//! assert_eq!(normalize("Alt+Plus").as_deref(), Some("Alt+Shift+Equal"));
//! assert_eq!(normalize("Ctrl+Return").as_deref(), Some("Control+Enter"));
//! assert_eq!(normalize("Ctrl+").as_deref(), None);
//! ```

/// Translates an Electron accelerator. Returns `None` when it has no key,
/// an empty token, or a token with no equivalent (`AltGr`, shifted
/// punctuation such as `!`).
#[must_use]
pub fn normalize(accelerator: &str) -> Option<String> {
    let tokens: Vec<&str> = accelerator.split('+').map(str::trim).collect();
    if tokens.iter().any(|t| t.is_empty()) {
        // "Ctrl++" style accelerators are written "Ctrl+Plus" in Electron.
        return None;
    }
    let mut mods: Vec<&'static str> = Vec::new();
    let mut key: Option<String> = None;
    let add = |mods: &mut Vec<&'static str>, m: &'static str| {
        if !mods.contains(&m) {
            mods.push(m);
        }
    };
    for token in tokens {
        if key.is_some() {
            return None;
        }
        match token.to_ascii_lowercase().as_str() {
            "commandorcontrol" | "cmdorctrl" | "commandorctrl" | "cmdorcontrol" => {
                add(&mut mods, "CmdOrCtrl");
            }
            "command" | "cmd" | "super" | "meta" => add(&mut mods, "Super"),
            "control" | "ctrl" => add(&mut mods, "Control"),
            "alt" | "option" => add(&mut mods, "Alt"),
            "shift" => add(&mut mods, "Shift"),
            "altgr" => return None,
            "plus" => {
                add(&mut mods, "Shift");
                key = Some("Equal".into());
            }
            "return" | "enter" => key = Some("Enter".into()),
            "esc" | "escape" => key = Some("Escape".into()),
            "medianexttrack" => key = Some("MediaTrackNext".into()),
            "mediaprevioustrack" => key = Some("MediaTrackPrevious".into()),
            "numdec" => key = Some("NumDecimal".into()),
            "numsub" => key = Some("NumSubtract".into()),
            "nummult" => key = Some("NumMultiply".into()),
            "numdiv" => key = Some("NumDivide".into()),
            "volumeup" | "volumedown" | "volumemute" | "mediaplaypause" | "mediastop"
            | "printscreen" | "space" | "tab" | "backspace" | "delete" | "insert" | "home"
            | "end" | "pageup" | "pagedown" | "up" | "down" | "left" | "right" | "capslock"
            | "numlock" | "scrolllock" | "numadd" => key = Some(token.to_owned()),
            other => {
                let is_fn = other
                    .strip_prefix('f')
                    .and_then(|n| n.parse::<u8>().ok())
                    .is_some_and(|n| (1..=24).contains(&n));
                let is_num = other
                    .strip_prefix("num")
                    .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit());
                let single = other.len() == 1
                    && other
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"`\\[],=-.';/".contains(&b));
                if is_fn || is_num || single {
                    key = Some(token.to_ascii_uppercase());
                } else {
                    return None;
                }
            }
        }
    }
    let key = key?;
    let mut out: Vec<String> = mods.into_iter().map(str::to_owned).collect();
    out.push(key);
    Some(out.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translations() {
        for (input, want) in [
            ("F12", Some("F12")),
            ("Shift+F24", Some("Shift+F24")),
            ("F25", None),
            ("Cmd+Option+i", Some("Super+Alt+I")),
            ("Meta+Space", Some("Super+Space")),
            ("Ctrl+num5", Some("Control+NUM5")),
            ("Ctrl+numdec", Some("Control+NumDecimal")),
            ("MediaNextTrack", Some("MediaTrackNext")),
            ("Ctrl+Shift+Shift+A", Some("Control+Shift+A")),
            ("Ctrl+!", None),
            ("AltGr+A", None),
            ("Ctrl+A+B", None),
            ("Ctrl+Shift", None),
            ("", None),
            ("Ctrl+/", Some("Control+/")),
        ] {
            assert_eq!(normalize(input).as_deref(), want, "{input}");
        }
    }
}
