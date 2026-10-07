//! Displays in Electron's `Display` shape (CONTRACT B.2.5).
//!
//! `bounds` and `workArea` are in DIP (physical pixels divided by the
//! monitor's scale factor, as Electron reports them), and `id` is a stable
//! 32-bit FNV-1a hash of the monitor's OS name and physical position.
//!
//! ```
//! use tauri_plugin_overwolf::screen::{MonitorInfo, to_display};
//! let m = MonitorInfo {
//!     name: "DISPLAY1".into(),
//!     x: 0, y: 0, width: 3840, height: 2160,
//!     work_x: 0, work_y: 0, work_width: 3840, work_height: 2100,
//!     scale_factor: 2.0,
//! };
//! let d = to_display(&m);
//! assert_eq!(d.bounds.width, 1920);
//! assert_eq!(d.work_area.height, 1050);
//! assert_eq!(d.label, "DISPLAY1");
//! ```

use serde::{Deserialize, Serialize};

/// A rectangle in DIP.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub width: i32,
    /// Height.
    pub height: i32,
}

/// A size in DIP.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    /// Width.
    pub width: i32,
    /// Height.
    pub height: i32,
}

/// A point.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: i32,
    /// Vertical coordinate.
    pub y: i32,
}

/// One display, as `screen.getAllDisplays()` returns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElectronDisplay {
    /// Stable hash of the OS monitor name and position.
    pub id: u32,
    /// The OS monitor name.
    pub label: String,
    /// Bounds in DIP.
    pub bounds: Rect,
    /// Work area in DIP.
    pub work_area: Rect,
    /// Device pixels per DIP.
    pub scale_factor: f64,
    /// `bounds` size.
    pub size: Size,
    /// `workArea` size.
    pub work_area_size: Size,
    /// Always 0 (Tauri does not report rotation).
    pub rotation: u32,
    /// Always `false` (unknown).
    pub internal: bool,
    /// Always `false`.
    pub monochrome: bool,
    /// Always `"unknown"`.
    pub accelerometer_support: &'static str,
    /// Always `"unknown"`.
    pub touch_support: &'static str,
    /// Always `true`.
    pub detected: bool,
    /// Origin in physical pixels.
    pub native_origin: Point,
}

/// What Tauri reports about a monitor, in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    /// OS monitor name (empty when unknown).
    pub name: String,
    /// Physical left edge.
    pub x: i32,
    /// Physical top edge.
    pub y: i32,
    /// Physical width.
    pub width: u32,
    /// Physical height.
    pub height: u32,
    /// Work area left edge.
    pub work_x: i32,
    /// Work area top edge.
    pub work_y: i32,
    /// Work area width.
    pub work_width: u32,
    /// Work area height.
    pub work_height: u32,
    /// Scale factor (1.0 when unknown).
    pub scale_factor: f64,
}

/// 32-bit FNV-1a.
#[must_use]
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for b in bytes {
        hash ^= u32::from(*b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// The display id: FNV-1a of `"<name>@<x>,<y>"` (physical origin).
#[must_use]
pub fn display_id(name: &str, x: i32, y: i32) -> u32 {
    fnv1a32(format!("{name}@{x},{y}").as_bytes())
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "screen coordinates are far inside i32"
)]
fn to_dip(v: f64, scale: f64) -> i32 {
    (v / scale).round() as i32
}

fn scale_of(m: &MonitorInfo) -> f64 {
    if m.scale_factor.is_finite() && m.scale_factor > 0.0 {
        m.scale_factor
    } else {
        1.0
    }
}

/// Converts a monitor to an Electron display.
#[must_use]
pub fn to_display(m: &MonitorInfo) -> ElectronDisplay {
    let s = scale_of(m);
    let bounds = Rect {
        x: to_dip(f64::from(m.x), s),
        y: to_dip(f64::from(m.y), s),
        width: to_dip(f64::from(m.width), s),
        height: to_dip(f64::from(m.height), s),
    };
    let work_area = Rect {
        x: to_dip(f64::from(m.work_x), s),
        y: to_dip(f64::from(m.work_y), s),
        width: to_dip(f64::from(m.work_width), s),
        height: to_dip(f64::from(m.work_height), s),
    };
    ElectronDisplay {
        id: display_id(&m.name, m.x, m.y),
        label: m.name.clone(),
        bounds,
        work_area,
        scale_factor: s,
        size: Size {
            width: bounds.width,
            height: bounds.height,
        },
        work_area_size: Size {
            width: work_area.width,
            height: work_area.height,
        },
        rotation: 0,
        internal: false,
        monochrome: false,
        accelerometer_support: "unknown",
        touch_support: "unknown",
        detected: true,
        native_origin: Point { x: m.x, y: m.y },
    }
}

/// Converts a physical point to DIP using the monitor that contains it (or
/// the first monitor, or scale 1).
#[must_use]
pub fn physical_to_dip(monitors: &[MonitorInfo], x: f64, y: f64) -> Point {
    let containing = monitors.iter().find(|m| {
        x >= f64::from(m.x)
            && y >= f64::from(m.y)
            && x < f64::from(m.x) + f64::from(m.width)
            && y < f64::from(m.y) + f64::from(m.height)
    });
    let s = containing.or(monitors.first()).map_or(1.0, scale_of);
    Point {
        x: to_dip(x, s),
        y: to_dip(y, s),
    }
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

/// Replaces each monitor's name with the OS name of the screen at the same
/// DIP frame, where one matches. Tauri names macOS monitors after their
/// model number (`Monitor #41058`); Electron and the guests' `systemInfo`
/// report the localized name (`Built-in Retina Display`, D.2).
#[cfg_attr(
    not(any(feature = "plugin", test)),
    expect(dead_code, reason = "used by the plugin's monitor queries")
)]
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
            work_x: x,
            work_y: 0,
            work_width: 2560,
            work_height: 1400,
            scale_factor: scale,
        }
    }

    #[test]
    fn fnv_vectors() {
        assert_eq!(fnv1a32(b""), 0x811c_9dc5);
        assert_eq!(fnv1a32(b"a"), 0xe40c_292c);
        assert_eq!(fnv1a32(b"foobar"), 0xbf9c_f968);
    }

    #[test]
    fn ids_are_stable_and_distinct() {
        assert_eq!(display_id("A", 0, 0), display_id("A", 0, 0));
        assert_ne!(display_id("A", 0, 0), display_id("A", 2560, 0));
        assert_ne!(display_id("A", 0, 0), display_id("B", 0, 0));
    }

    #[test]
    fn dip_conversion() {
        let d = to_display(&mon("A", 2560, 1.25));
        assert_eq!(
            d.bounds,
            Rect {
                x: 2048,
                y: 0,
                width: 2048,
                height: 1152
            }
        );
        assert_eq!(
            d.work_area_size,
            Size {
                width: 2048,
                height: 1120
            }
        );
        assert_eq!(d.native_origin, Point { x: 2560, y: 0 });
        let bad = to_display(&mon("B", 0, 0.0));
        assert!((bad.scale_factor - 1.0).abs() < f64::EPSILON);
        let json = serde_json::to_value(&d).unwrap();
        assert!(json.get("workArea").is_some());
        assert_eq!(json["touchSupport"], "unknown");
    }

    #[test]
    fn cursor_conversion_uses_containing_monitor() {
        let monitors = [mon("A", 0, 1.0), mon("B", 2560, 2.0)];
        assert_eq!(
            physical_to_dip(&monitors, 100.0, 100.0),
            Point { x: 100, y: 100 }
        );
        assert_eq!(
            physical_to_dip(&monitors, 3000.0, 100.0),
            Point { x: 1500, y: 50 }
        );
        assert_eq!(physical_to_dip(&[], 10.0, 10.0), Point { x: 10, y: 10 });
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
}
