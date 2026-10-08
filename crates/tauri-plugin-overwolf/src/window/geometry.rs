//! Electron's window geometry for `BrowserWindow` (CONTRACT B.2.2).
//!
//! Electron sizes a framed window by its outer frame: `width` and `height`
//! include the title bar and borders unless `useContentSize` is `true`, and
//! `getBounds()` / `getSize()` report the frame while `getContentBounds()` /
//! `getContentSize()` report the area inside it. A new window is placed as
//! Electron places it:
//!
//! - Windows and Linux ([`Placement::FitWorkArea`]): centred in the primary
//!   display's work area with its size clamped to that work area, then moved
//!   to `x` / `y` when both are given (the clamped size stays). [OBS: Windows
//!   lab, a 1000 x 760 window at `(0, 0)` on a 1024 x 720 work area is
//!   `{x: 12, y: 0, width: 1000, height: 720}` in `browser-window-created`
//!   and `{x: 0, y: 0, width: 1000, height: 720}` after the constructor.]
//! - macOS ([`Placement::ScreenCenter`]): centred on the primary display's
//!   full frame, size unchanged, and moved up or left into the work area
//!   when it would reach past its bottom or right edge (above the Dock),
//!   but never above or left of the work area; then moved to `x` / `y` when
//!   both are given. `AppKit` keeps the title bar below the menu bar when
//!   the window is shown. [OBS: macOS lab, 1470 x 956 display with the work
//!   area `{0, 33, 1470, 837}`: frames 1000 x 274, 324, 624 and 760 are
//!   created at y 341, 316, 166 and 98 (centred), 1200 x 800 at y 70 and
//!   1000 x 837 at y 33 (moved up to the work area's bottom edge), all
//!   centred horizontally.]
//!
//! ```
//! use tauri_plugin_overwolf::window::geometry::{initial_frame, Bounds, Insets, Placement};
//! let work = Bounds { x: 0.0, y: 0.0, width: 1024.0, height: 720.0 };
//! let frame = initial_frame((1000.0, 760.0), Some((0.0, 0.0)), Placement::FitWorkArea, work, work);
//! assert_eq!(frame.created, Bounds { x: 12.0, y: 0.0, width: 1000.0, height: 720.0 });
//! assert_eq!(frame.outer, Bounds { x: 0.0, y: 0.0, width: 1000.0, height: 720.0 });
//! let insets = Insets { left: 8.0, top: 31.0, right: 8.0, bottom: 8.0 };
//! assert_eq!(
//!     insets.content_of(frame.outer),
//!     Bounds { x: 8.0, y: 31.0, width: 984.0, height: 681.0 }
//! );
//! ```

use serde::Serialize;

use crate::screen::Rect;

/// A rectangle in logical pixels (DIP), as Electron's `Rectangle`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Bounds {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

impl From<Rect> for Bounds {
    fn from(r: Rect) -> Self {
        Self {
            x: f64::from(r.x),
            y: f64::from(r.y),
            width: f64::from(r.width),
            height: f64::from(r.height),
        }
    }
}

/// The window frame around the content area, in logical pixels.
///
/// ```
/// use tauri_plugin_overwolf::window::geometry::{Bounds, Insets};
/// let outer = Bounds { x: 0.0, y: 33.0, width: 1200.0, height: 800.0 };
/// let content = Bounds { x: 0.0, y: 65.0, width: 1200.0, height: 768.0 };
/// let insets = Insets::between(outer, content);
/// assert_eq!(insets.top, 32.0);
/// assert_eq!(insets.outer_of(content), outer);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Insets {
    /// Frame width on the left.
    pub left: f64,
    /// Frame height above the content (title bar and border).
    pub top: f64,
    /// Frame width on the right.
    pub right: f64,
    /// Frame height below the content.
    pub bottom: f64,
}

impl Insets {
    /// The insets of `content` inside `outer`; negative values (a platform
    /// that reports a frame smaller than its content) count as zero.
    #[must_use]
    pub fn between(outer: Bounds, content: Bounds) -> Self {
        let left = (content.x - outer.x).max(0.0);
        let top = (content.y - outer.y).max(0.0);
        Self {
            left,
            top,
            right: (outer.width - content.width - left).max(0.0),
            bottom: (outer.height - content.height - top).max(0.0),
        }
    }

    /// Left plus right.
    #[must_use]
    pub fn horizontal(self) -> f64 {
        self.left + self.right
    }

    /// Top plus bottom.
    #[must_use]
    pub fn vertical(self) -> f64 {
        self.top + self.bottom
    }

    /// The content area of a window whose frame is `outer` (never negative).
    #[must_use]
    pub fn content_of(self, outer: Bounds) -> Bounds {
        Bounds {
            x: outer.x + self.left,
            y: outer.y + self.top,
            width: (outer.width - self.horizontal()).max(0.0),
            height: (outer.height - self.vertical()).max(0.0),
        }
    }

    /// The frame of a window whose content area is `content`.
    #[must_use]
    pub fn outer_of(self, content: Bounds) -> Bounds {
        Bounds {
            x: content.x - self.left,
            y: content.y - self.top,
            width: content.width + self.horizontal(),
            height: content.height + self.vertical(),
        }
    }
}

/// How the platform's Electron places a new window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Windows and Linux: centred in the work area, size clamped to it.
    FitWorkArea,
    /// macOS: centred on the display's full frame, size unchanged.
    ScreenCenter,
}

impl Placement {
    /// The placement of the platform this build targets.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::ScreenCenter
        } else {
            Self::FitWorkArea
        }
    }
}

/// Where a new window's frame is, before and after its `x` / `y` options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InitialFrame {
    /// The frame when `browser-window-created` is emitted (centred).
    pub created: Bounds,
    /// The frame once the constructor returns.
    pub outer: Bounds,
}

/// Centres `size` in `area` with integer offsets, as Chromium's
/// `CenterWindow` does (`(area - size) / 2`, truncated).
#[expect(
    clippy::cast_possible_truncation,
    reason = "window coordinates are far inside i64"
)]
#[expect(
    clippy::cast_precision_loss,
    reason = "window coordinates are far inside f64's exact integers"
)]
fn centre(origin: f64, area: f64, size: f64) -> f64 {
    origin + (((area - size) as i64) / 2) as f64
}

/// Moves an edge at `start` of a `size` long span back into the area
/// `[origin, origin + extent]`: back from the far edge first, then never
/// before `origin` (a span longer than the area starts at `origin`).
fn into_area(start: f64, size: f64, origin: f64, extent: f64) -> f64 {
    start.min(origin + extent - size).max(origin)
}

/// The frame Electron gives a new window whose outer size is `size`
/// (`width` / `height`, plus the frame when `useContentSize` is set),
/// with the optional `x` / `y` options, on the primary display (`work_area`
/// and full `screen` bounds, in DIP). `center: true` gives the same frame
/// as no position on Windows and Linux; on macOS the caller centres the
/// window with the platform's own `center`.
#[must_use]
pub fn initial_frame(
    size: (f64, f64),
    position: Option<(f64, f64)>,
    placement: Placement,
    work_area: Bounds,
    screen: Bounds,
) -> InitialFrame {
    let (width, height) = (size.0.max(0.0), size.1.max(0.0));
    let created = match placement {
        Placement::FitWorkArea => {
            let w = width.min(work_area.width);
            let h = height.min(work_area.height);
            Bounds {
                x: centre(work_area.x, work_area.width, w),
                y: centre(work_area.y, work_area.height, h),
                width: w,
                height: h,
            }
        }
        Placement::ScreenCenter => Bounds {
            x: into_area(
                screen.x + ((screen.width - width) / 2.0).round(),
                width,
                work_area.x,
                work_area.width,
            ),
            y: into_area(
                screen.y + ((screen.height - height) / 2.0).round(),
                height,
                work_area.y,
                work_area.height,
            ),
            width,
            height,
        },
    };
    let outer = match position {
        Some((x, y)) => Bounds { x, y, ..created },
        None => created,
    };
    InitialFrame { created, outer }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Bounds = Bounds {
        x: 0.0,
        y: 0.0,
        width: 1024.0,
        height: 720.0,
    };
    const SCREEN: Bounds = Bounds {
        x: 0.0,
        y: 0.0,
        width: 1024.0,
        height: 768.0,
    };

    fn b(x: f64, y: f64, width: f64, height: f64) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn windows_lab_observation_is_reproduced() {
        // [OBS] WE-high-impact: 1000 x 760 at (0, 0), work area 1024 x 720.
        let f = initial_frame(
            (1000.0, 760.0),
            Some((0.0, 0.0)),
            Placement::FitWorkArea,
            WORK,
            SCREEN,
        );
        assert_eq!(f.created, b(12.0, 0.0, 1000.0, 720.0));
        assert_eq!(f.outer, b(0.0, 0.0, 1000.0, 720.0));
        let insets = Insets {
            left: 8.0,
            top: 31.0,
            right: 8.0,
            bottom: 8.0,
        };
        assert_eq!(insets.content_of(f.outer), b(8.0, 31.0, 984.0, 681.0));
    }

    #[test]
    fn macos_lab_observations_are_reproduced() {
        // [OBS] macOS lab: 1470 x 956 display, menu bar 33, Dock 86.
        let screen = b(0.0, 0.0, 1470.0, 956.0);
        let work = b(0.0, 33.0, 1470.0, 837.0);
        let f = initial_frame(
            (1000.0, 324.0),
            Some((0.0, 0.0)),
            Placement::ScreenCenter,
            work,
            screen,
        );
        assert_eq!(f.created, b(235.0, 316.0, 1000.0, 324.0));
        assert_eq!(f.outer, b(0.0, 0.0, 1000.0, 324.0));
        for (width, height, x, y) in [
            (1000.0, 274.0, 235.0, 341.0),
            (1000.0, 624.0, 235.0, 166.0),
            (1000.0, 760.0, 235.0, 98.0),
            (1200.0, 800.0, 135.0, 70.0),
            (1000.0, 837.0, 235.0, 33.0),
            (800.0, 800.0, 335.0, 70.0),
        ] {
            let f = initial_frame((width, height), None, Placement::ScreenCenter, work, screen);
            assert_eq!(f.created, b(x, y, width, height), "{width} x {height}");
        }
    }

    #[test]
    fn a_window_larger_than_the_work_area_is_resized_only_on_fit_platforms() {
        let f = initial_frame((2000.0, 900.0), None, Placement::FitWorkArea, WORK, SCREEN);
        assert_eq!(f.outer, b(0.0, 0.0, 1024.0, 720.0));
        // macOS keeps the size and starts it at the work area's origin.
        let f = initial_frame((2000.0, 900.0), None, Placement::ScreenCenter, WORK, SCREEN);
        assert_eq!(f.outer, b(0.0, 0.0, 2000.0, 900.0));
        let work = b(0.0, 33.0, 1024.0, 700.0);
        let f = initial_frame((600.0, 900.0), None, Placement::ScreenCenter, work, SCREEN);
        assert_eq!(f.outer, b(212.0, 33.0, 600.0, 900.0));
    }

    #[test]
    fn centring_truncates_like_chromium_and_respects_the_work_area_origin() {
        let work = b(100.0, 40.0, 1001.0, 701.0);
        let f = initial_frame((800.0, 600.0), None, Placement::FitWorkArea, work, SCREEN);
        // (1001 - 800) / 2 = 100 (truncated), (701 - 600) / 2 = 50.
        assert_eq!(f.outer, b(200.0, 90.0, 800.0, 600.0));
        assert_eq!(f.created, f.outer);
    }

    #[test]
    fn insets_round_trip_and_never_go_negative() {
        let outer = b(10.0, 20.0, 300.0, 200.0);
        let content = b(12.0, 50.0, 296.0, 168.0);
        let i = Insets::between(outer, content);
        assert_eq!(
            i,
            Insets {
                left: 2.0,
                top: 30.0,
                right: 2.0,
                bottom: 2.0
            }
        );
        assert_eq!(i.outer_of(content), outer);
        assert_eq!(i.content_of(outer), content);
        // A 1 x 32 window with a 32 px title bar has a 1 x 0 content area
        // [OBS: R3-perf-test, the ow-electron calibration window].
        let tiny = Insets {
            left: 0.0,
            top: 32.0,
            right: 0.0,
            bottom: 0.0,
        };
        assert_eq!(
            tiny.content_of(b(735.0, 478.0, 1.0, 32.0)),
            b(735.0, 510.0, 1.0, 0.0)
        );
        assert_eq!(
            tiny.content_of(b(0.0, 0.0, 1.0, 10.0)),
            b(0.0, 32.0, 1.0, 0.0)
        );
        assert_eq!(Insets::between(content, outer), Insets::default());
    }
}
