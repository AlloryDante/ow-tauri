//! macOS lab probes of a window's native view tree, in process: the lab
//! checks for transparency (L1), stacking (L2) and input pass-through (L3)
//! of the ad guests (AD-FORMATS-SPEC §7).
//!
//! Nothing here captures the screen (no `CGWindowListCreateImage`, no
//! screen-recording prompt): [`snapshot`] asks each `WKWebView` to render
//! its own content (`takeSnapshotWithConfiguration:completionHandler:`), which
//! keeps the page's alpha. [`inspect`] reads the subview order and asks
//! `AppKit` which view a point would hit (`hitTest:`), which is how the window
//! routes a click. [`click`] is the only input it ever creates, and only into
//! the app's own webview: it refuses when the hit test names any other view,
//! so an ad guest never receives it.
//!
//! Every function runs on the main thread (Tauri's `run_on_main_thread`, or a
//! snapshot completion handler, which `WebKit` calls there).

use std::collections::BTreeMap;
use std::ffi::c_void;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde_json::{Value, json};

/// A view of the window, by its address (`*mut NSView` as `usize`).
pub type Views = BTreeMap<usize, String>;

/// A probe point in the embedder page's CSS pixels (top-left origin).
#[derive(Clone, Debug)]
pub struct Point {
    /// Name for the report (`app-control`, `slot-center`, ...).
    pub name: String,
    /// Horizontal page coordinate.
    pub x: f64,
    /// Vertical page coordinate.
    pub y: f64,
}

/// `[obj respondsToSelector:sel]`.
fn responds(obj: &AnyObject, sel: Sel) -> bool {
    // SAFETY: NSObject protocol method on a live object.
    let yes: Bool = unsafe { msg_send![obj, respondsToSelector: sel] };
    yes.as_bool()
}

/// The object at `address`, or `None` for null.
///
/// # Safety
///
/// `address` must be null or a live Objective-C object for the whole call
/// that uses the returned reference.
unsafe fn object<'a>(address: usize) -> Option<&'a AnyObject> {
    // SAFETY: the caller guarantees a live object (or null).
    unsafe { (address as *const AnyObject).as_ref() }
}

/// The class name of `obj`, for views that are not webviews.
fn class_name(obj: &AnyObject) -> String {
    obj.class().name().to_string_lossy().into_owned()
}

/// The webview label of `view` or one of its ancestors, else the class
/// name of `view`.
fn owner(view: *mut AnyObject, views: &Views) -> Value {
    let mut cur = view;
    while !cur.is_null() {
        if let Some(label) = views.get(&(cur as usize)) {
            return json!({ "label": label });
        }
        // SAFETY: `cur` is a live NSView (a hit test result or its superview).
        cur = unsafe { msg_send![&*cur, superview] };
    }
    if view.is_null() {
        return json!({ "label": null, "class": null });
    }
    // SAFETY: non-null live NSView.
    json!({ "label": null, "class": class_name(unsafe { &*view }) })
}

/// `point` (embedder page coordinates) in window coordinates.
fn window_point(embedder: &AnyObject, point: &Point) -> NSPoint {
    // SAFETY: NSView coordinate conversion; nil means the window.
    unsafe {
        msg_send![embedder, convertPoint: NSPoint::new(point.x, point.y), toView: None::<&AnyObject>]
    }
}

/// The view a click at `point` would reach: `-[NSView hitTest:]` on the
/// content view, as `-[NSWindow sendEvent:]` routes a mouse-down.
fn hit(content: &AnyObject, wp: NSPoint) -> *mut AnyObject {
    // SAFETY: NSView methods on the live content view; the point is in the
    // superview's coordinates, as hitTest: takes it.
    unsafe {
        let superview: *mut AnyObject = msg_send![content, superview];
        let local: NSPoint = if superview.is_null() {
            wp
        } else {
            msg_send![&*superview, convertPoint: wp, fromView: None::<&AnyObject>]
        };
        msg_send![content, hitTest: local]
    }
}

/// The content view of the window at `ns_window`.
fn content_view(ns_window: usize) -> Option<&'static AnyObject> {
    // SAFETY: `ns_window` is the live NSWindow Tauri handed out; the content
    // view lives as long as the window, which outlives this main-thread call.
    unsafe {
        let window = object(ns_window)?;
        let content: *mut AnyObject = msg_send![window, contentView];
        content.as_ref()
    }
}

/// Native facts of one `WKWebView`: hidden, frame, layer opacity, the
/// background state that makes it transparent (`_drawsBackground`, read
/// only where `WebKit` has it) and `underPageBackgroundColor`'s alpha.
fn webview_facts(view: &AnyObject) -> Value {
    // SAFETY: public NSView / WKWebView getters on a live view, and the
    // `_drawsBackground` getter only after `respondsToSelector:`.
    unsafe {
        let hidden: Bool = msg_send![view, isHidden];
        let opaque: Bool = msg_send![view, isOpaque];
        let frame: NSRect = msg_send![view, frame];
        let draws = if responds(view, sel!(_drawsBackground)) {
            let d: Bool = msg_send![view, _drawsBackground];
            Value::Bool(d.as_bool())
        } else {
            Value::Null
        };
        let under = if responds(view, sel!(underPageBackgroundColor)) {
            let c: *mut AnyObject = msg_send![view, underPageBackgroundColor];
            c.as_ref().map_or(Value::Null, |c| {
                let a: f64 = msg_send![c, alphaComponent];
                json!(a)
            })
        } else {
            Value::Null
        };
        let layer: *mut AnyObject = msg_send![view, layer];
        let layer_opaque = layer.as_ref().map_or(Value::Null, |l| {
            let o: Bool = msg_send![l, isOpaque];
            Value::Bool(o.as_bool())
        });
        json!({
            "hidden": hidden.as_bool(),
            "opaque": opaque.as_bool(),
            "layerOpaque": layer_opaque,
            "drawsBackground": draws,
            "underPageAlpha": under,
            "frame": [frame.origin.x, frame.origin.y, frame.size.width, frame.size.height],
        })
    }
}

/// The window's view tree and hit tests: content-view subviews bottom to
/// top (webview label or class name, hidden, frame), the native facts of
/// every webview, and for each point the view a click there would reach.
pub fn inspect(ns_window: usize, embedder: &str, views: &Views, points: &[Point]) -> Value {
    let Some(content) = content_view(ns_window) else {
        return json!({ "error": "no content view" });
    };
    let mut order = Vec::new();
    // SAFETY: NSView `subviews` (an NSArray of live views) on the main thread.
    unsafe {
        let subviews: *mut AnyObject = msg_send![content, subviews];
        if let Some(subviews) = subviews.as_ref() {
            let count: usize = msg_send![subviews, count];
            for i in 0..count {
                let v: *mut AnyObject = msg_send![subviews, objectAtIndex: i];
                let Some(v) = v.as_ref() else { continue };
                let hidden: Bool = msg_send![v, isHidden];
                let mut entry = owner(std::ptr::from_ref(v).cast_mut(), views);
                entry["hidden"] = Value::Bool(hidden.as_bool());
                order.push(entry);
            }
        }
    }
    let mut facts = serde_json::Map::new();
    for (address, label) in views {
        // SAFETY: the addresses are the window's live webviews, read just
        // before this main-thread call.
        if let Some(v) = unsafe { object(*address) } {
            facts.insert(label.clone(), webview_facts(v));
        }
    }
    let embedder_view = views
        .iter()
        .find(|(_, l)| l.as_str() == embedder)
        // SAFETY: as above.
        .and_then(|(a, _)| unsafe { object(*a) });
    let hits: Vec<Value> = points
        .iter()
        .map(|p| {
            let Some(e) = embedder_view else {
                return json!({ "name": p.name, "error": "no embedder view" });
            };
            let target = owner(hit(content, window_point(e, p)), views);
            json!({ "name": p.name, "x": p.x, "y": p.y, "target": target })
        })
        .collect();
    json!({ "order": order, "webviews": facts, "hits": hits })
}

/// Injects one left click at `point` into the embedder webview, the way
/// `-[NSWindow sendEvent:]` would deliver it after its hit test, but only
/// when that hit test names the embedder itself. Otherwise it sends nothing
/// and reports which view the click would have reached. The event goes
/// straight to the view (`mouseDown:` / `mouseUp:`), so the window is not
/// made key and the app is not activated. Observed: `WebKit` does not turn
/// these synthesized events into DOM events in the invisible lab window (nor
/// through `-[NSWindow sendEvent:]`), so the page's `app-pointer` / `app-click`
/// records say whether a click arrived; the hit test is the routing proof.
pub fn click(ns_window: usize, embedder: &str, views: &Views, point: &Point) -> Value {
    let Some(content) = content_view(ns_window) else {
        return json!({ "error": "no content view" });
    };
    let Some((address, _)) = views.iter().find(|(_, l)| l.as_str() == embedder) else {
        return json!({ "error": "no embedder view" });
    };
    // SAFETY: a live webview of the window (read just before this call).
    let Some(view) = (unsafe { object(*address) }) else {
        return json!({ "error": "no embedder view" });
    };
    let wp = window_point(view, point);
    let target = owner(hit(content, wp), views);
    if target["label"].as_str() != Some(embedder) {
        return json!({ "name": point.name, "sent": false, "refused": "the click would not reach the app webview", "target": target });
    }
    // SAFETY: NSEvent factory and NSResponder mouse methods with the
    // documented argument types (NSEventTypeLeftMouseDown = 1, Up = 2).
    unsafe {
        let Some(window) = object(ns_window) else {
            return json!({ "error": "no window" });
        };
        let number: isize = msg_send![window, windowNumber];
        let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
        let uptime: f64 = msg_send![&*info, systemUptime];
        for (kind, sel_down) in [(1_usize, true), (2_usize, false)] {
            let event: Option<Retained<AnyObject>> = msg_send![
                class!(NSEvent),
                mouseEventWithType: kind,
                location: wp,
                modifierFlags: 0_usize,
                timestamp: uptime,
                windowNumber: number,
                context: None::<&AnyObject>,
                eventNumber: 0_isize,
                clickCount: 1_isize,
                pressure: 1.0_f32
            ];
            let Some(event) = event else {
                return json!({ "error": "NSEvent was not created" });
            };
            if sel_down {
                let () = msg_send![view, mouseDown: &*event];
            } else {
                let () = msg_send![view, mouseUp: &*event];
            }
        }
    }
    json!({ "name": point.name, "sent": true, "target": target })
}

/// Samples of one snapshot: RGBA (0..1, sRGB) at each point (embedder page
/// coordinates mapped into the view), and over a 16x16 grid of the view the
/// share of pixels that are fully transparent (alpha < 0.01) or opaque
/// (alpha > 0.99).
fn sample(
    image: &AnyObject,
    view: &AnyObject,
    embedder: Option<&AnyObject>,
    points: &[Point],
) -> Value {
    // SAFETY: AppKit image and bitmap getters on live objects returned by
    // WebKit; `colorAtX:y:` with in-range pixel coordinates.
    unsafe {
        let tiff: *mut AnyObject = msg_send![image, TIFFRepresentation];
        let Some(tiff) = tiff.as_ref() else {
            return json!({ "error": "no TIFF data" });
        };
        let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), imageRepWithData: tiff];
        let Some(rep) = rep.as_ref() else {
            return json!({ "error": "no bitmap" });
        };
        let wide: isize = msg_send![rep, pixelsWide];
        let high: isize = msg_send![rep, pixelsHigh];
        let size: NSSize = msg_send![image, size];
        if wide <= 0 || high <= 0 || size.width <= 0.0 {
            return json!({ "error": "empty snapshot" });
        }
        #[expect(clippy::cast_precision_loss, reason = "pixel counts are small")]
        let scale = wide as f64 / size.width;
        let srgb: *mut AnyObject = msg_send![class!(NSColorSpace), sRGBColorSpace];
        let rgba = |px: f64, py: f64| -> Option<[f64; 4]> {
            #[expect(clippy::cast_possible_truncation, reason = "clamped pixel index")]
            let (col, row) = (px.floor() as isize, py.floor() as isize);
            if col < 0 || row < 0 || col >= wide || row >= high {
                return None;
            }
            let color: *mut AnyObject = msg_send![rep, colorAtX: col, y: row];
            let color: *mut AnyObject = msg_send![color.as_ref()?, colorUsingColorSpace: &*srgb];
            let color = color.as_ref()?;
            let red: f64 = msg_send![color, redComponent];
            let green: f64 = msg_send![color, greenComponent];
            let blue: f64 = msg_send![color, blueComponent];
            let alpha: f64 = msg_send![color, alphaComponent];
            Some([red, green, blue, alpha].map(|v| (v * 1000.0).round() / 1000.0))
        };
        let samples: Vec<Value> = points
            .iter()
            .map(|p| {
                let local = embedder.map_or(NSPoint::new(p.x, p.y), |e| {
                    let wp = window_point(e, p);
                    msg_send![view, convertPoint: wp, fromView: None::<&AnyObject>]
                });
                json!({
                    "name": p.name,
                    "local": [local.x, local.y],
                    "rgba": rgba(local.x * scale, local.y * scale),
                })
            })
            .collect();
        let (mut clear, mut opaque, mut total) = (0_u32, 0_u32, 0_u32);
        for i in 0..16 {
            for j in 0..16 {
                #[expect(clippy::cast_precision_loss, reason = "pixel counts are small")]
                let (px, py) = (
                    (f64::from(i) + 0.5) * wide as f64 / 16.0,
                    (f64::from(j) + 0.5) * high as f64 / 16.0,
                );
                if let Some([_, _, _, a]) = rgba(px, py) {
                    total += 1;
                    if a < 0.01 {
                        clear += 1;
                    } else if a > 0.99 {
                        opaque += 1;
                    }
                }
            }
        }
        json!({
            "pixels": [wide, high],
            "points": [size.width, size.height],
            "samples": samples,
            "grid": { "total": total, "transparent": clear, "opaque": opaque },
        })
    }
}

/// Asks the `WKWebView` at `address` for a snapshot of its own content and
/// calls `done` (on the main thread) with the samples of [`sample`].
pub fn snapshot(
    address: usize,
    embedder: Option<usize>,
    points: Vec<Point>,
    done: impl Fn(Value) + 'static,
) {
    // SAFETY: a live webview of the window (read just before this call).
    let Some(view) = (unsafe { object(address) }) else {
        done(json!({ "error": "view gone" }));
        return;
    };
    let block = RcBlock::new(move |image: *mut AnyObject, error: *mut AnyObject| {
        // SAFETY: WebKit passes a live NSImage (or nil with an NSError); the
        // view and embedder are still in the window during the callback.
        let result = unsafe {
            match (image.as_ref(), object(address)) {
                (Some(image), Some(view)) => {
                    sample(image, view, embedder.and_then(|e| object(e)), &points)
                }
                _ => json!({
                    "error": if error.is_null() { "no image".to_owned() } else {
                        let d: *mut AnyObject = msg_send![&*error, localizedDescription];
                        d.as_ref().map_or_else(String::new, |d| {
                            let s: *const std::ffi::c_char = msg_send![d, UTF8String];
                            if s.is_null() { String::new() } else { std::ffi::CStr::from_ptr(s).to_string_lossy().into_owned() }
                        })
                    }
                }),
            }
        };
        done(result);
    });
    // SAFETY: public WKWebView API (macOS 10.13+); a nil configuration means
    // the whole visible bounds at the backing scale.
    unsafe {
        let () = msg_send![
            view,
            takeSnapshotWithConfiguration: None::<&AnyObject>,
            completionHandler: &*block
        ];
    }
}

/// The address of a raw view pointer, for [`Views`].
#[must_use]
pub fn address(view: *mut c_void) -> usize {
    view as usize
}
