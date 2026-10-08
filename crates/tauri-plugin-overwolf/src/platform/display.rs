//! The displays an ad guest reports in `systemInfo.displays` (CONTRACT D.2):
//! Tauri's monitors with the names the OS shows to the user.
#![allow(dead_code, reason = "the ads host (W2) reports the displays (D.2)")]

use tauri::{AppHandle, Runtime};

/// What Tauri reports about a monitor, in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MonitorInfo {
    /// OS monitor name (empty when unknown).
    pub(crate) name: String,
    /// Physical left edge.
    pub(crate) x: i32,
    /// Physical top edge.
    pub(crate) y: i32,
    /// Physical width.
    pub(crate) width: u32,
    /// Physical height.
    pub(crate) height: u32,
    /// Scale factor (1.0 when unknown).
    pub(crate) scale_factor: f64,
}

/// A display as the OS names it: its frame in DIP (top-left origin) and its
/// user-facing name (macOS `NSScreen.localizedName`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OsScreenName {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) name: String,
}

fn scale_of(m: &MonitorInfo) -> f64 {
    if m.scale_factor.is_finite() && m.scale_factor > 0.0 {
        m.scale_factor
    } else {
        1.0
    }
}

/// Replaces each monitor's name with the OS name of the screen at the same
/// DIP frame, where one matches. Tauri names macOS monitors after their
/// model number (`Monitor #41058`); the guests' `systemInfo` reports the
/// localized name (`Built-in Retina Display`, D.2), as ow-electron does.
pub(crate) fn apply_os_names(monitors: &mut [MonitorInfo], screens: &[OsScreenName]) {
    for m in monitors {
        let s = scale_of(m);
        let near = |a: f64, b: f64| (a - b).abs() < 1.0;
        if let Some(screen) = screens.iter().find(|o| {
            near(f64::from(m.x) / s, o.x)
                && near(f64::from(m.y) / s, o.y)
                && near(f64::from(m.width) / s, o.width)
                && near(f64::from(m.height) / s, o.height)
        }) && !screen.name.is_empty()
        {
            m.name.clone_from(&screen.name);
        }
    }
}

fn monitor_info(m: &tauri::Monitor) -> MonitorInfo {
    MonitorInfo {
        name: m.name().cloned().unwrap_or_default(),
        x: m.position().x,
        y: m.position().y,
        width: m.size().width,
        height: m.size().height,
        scale_factor: m.scale_factor(),
    }
}

/// Current monitors as Tauri reports them, with the OS display names, and
/// the primary one. Empty when `os_queries` is off (Tauri's mock runtime
/// has no monitors).
pub(crate) fn monitors<R: Runtime>(
    app: &AppHandle<R>,
    os_queries: bool,
) -> (Vec<MonitorInfo>, Option<MonitorInfo>) {
    if !os_queries {
        return (Vec::new(), None);
    }
    let mut all: Vec<MonitorInfo> = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(monitor_info)
        .collect();
    let mut primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .as_ref()
        .map(monitor_info);
    // The OS display names (macOS: Tauri reports `Monitor #<model>`).
    let names = super::webview::screen_names(app);
    apply_os_names(&mut all, &names);
    // Windows: the monitor's friendly name, not its GDI device name.
    for m in all.iter_mut().chain(primary.as_mut()) {
        if let Some(name) = super::graphics::display_friendly_name(&m.name) {
            m.name = name;
        }
    }
    if let Some(p) = primary.as_mut() {
        apply_os_names(std::slice::from_mut(p), &names);
    }
    (all, primary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(name: &str, x: i32, scale: f64) -> MonitorInfo {
        MonitorInfo {
            name: name.into(),
            x,
            y: 0,
            width: 2560,
            height: 1440,
            scale_factor: scale,
        }
    }

    /// Regression (lab diff): `systemInfo.displays[].name` was Tauri's
    /// `Monitor #<model>`; ow-electron reports the localized screen name.
    #[test]
    fn os_names_replace_model_names_by_frame() {
        let mut monitors = vec![mon("Monitor #41058", 0, 2.0), mon("Monitor #7", 2560, 1.0)];
        let screens = vec![
            OsScreenName {
                x: 0.0,
                y: 0.0,
                width: 1280.0,
                height: 720.0,
                name: "Built-in Retina Display".into(),
            },
            OsScreenName {
                x: 9999.0,
                y: 0.0,
                width: 2560.0,
                height: 1440.0,
                name: "Elsewhere".into(),
            },
        ];
        apply_os_names(&mut monitors, &screens);
        assert_eq!(monitors[0].name, "Built-in Retina Display");
        // No screen at that frame: the name stays.
        assert_eq!(monitors[1].name, "Monitor #7");
    }

    #[test]
    fn a_bad_scale_factor_counts_as_one() {
        let mut monitors = vec![mon("A", 0, 0.0)];
        let screens = vec![OsScreenName {
            x: 0.0,
            y: 0.0,
            width: 2560.0,
            height: 1440.0,
            name: "Wide".into(),
        }];
        apply_os_names(&mut monitors, &screens);
        assert_eq!(monitors[0].name, "Wide");
    }

    #[test]
    fn monitors_are_empty_without_os_queries() {
        let app = tauri::test::mock_app();
        let (all, primary) = monitors(app.handle(), false);
        assert!(all.is_empty() && primary.is_none());
    }
}
