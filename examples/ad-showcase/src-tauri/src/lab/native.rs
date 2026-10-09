//! macOS lab probes of the showcase window's native view tree, in process
//! (feature `lab` only; adapted from `tools/parity-harness`'s native probes).
//!
//! Nothing here captures the screen: there is no `CGWindowListCreateImage`
//! and no screen-recording prompt. [`still`] asks each `WKWebView` of the
//! window to render its own content (`takeSnapshotWithConfiguration:`), then
//! draws the snapshots bottom to top at their frames into one bitmap and
//! writes it as a PNG. [`inspect`] asks `AppKit` which view a point would hit
//! (`hitTest:`), which is how the window routes a click. [`click`] is the
//! only input it ever creates, only into the app's own webview, and it
//! refuses when the hit test names any other view, so an ad guest never
//! receives it.
//!
//! In the invisible lab, [`hold_app_back`], [`prepare_invisible`] and
//! [`order_front`] keep the app in the background: never activated, the
//! window on screen at alpha 0 without becoming key. The lab window sits
//! above every normal window on every Space ([`LAB_WINDOW_LEVEL`]), so no
//! window of another app can cover it: `WebKit` treats the page of a
//! covered (occluded) window as hidden and throttles its timers until they
//! stop, which stalled a tour mid-way.
//!
//! Every function that touches a view runs on the main thread.

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::sync::{Once, OnceLock};

use block2::RcBlock;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::{NSEdgeInsets, NSPoint, NSRect, NSSize};
use serde_json::{Value, json};

/// The lab window's level: `NSStatusWindowLevel`, above every normal app
/// window, so no other app's window can cover (occlude) it. It stays at
/// alpha 0 and ignores the mouse, so nobody sees or clicks it.
const LAB_WINDOW_LEVEL: isize = 25;
/// The lab window's collection behavior: on every Space
/// (`canJoinAllSpaces`, 1 << 0), not moved by Mission Control (`stationary`,
/// 1 << 4), out of the window cycle (`ignoresCycle`, 1 << 6) and allowed
/// next to a full-screen app (`fullScreenAuxiliary`, 1 << 8).
const LAB_COLLECTION_BEHAVIOR: usize = 1 | (1 << 4) | (1 << 6) | (1 << 8);

/// The webviews of a window, by address (`*mut NSView` as `usize`) to label.
pub type Views = BTreeMap<usize, String>;

/// A probe point in the embedder page's CSS pixels (top-left origin).
#[derive(Clone, Debug)]
pub struct Point {
    /// Name for the report.
    pub name: String,
    /// Horizontal page coordinate.
    pub x: f64,
    /// Vertical page coordinate.
    pub y: f64,
}

/// One webview's snapshot: its label, its rectangle in content-view
/// coordinates (bottom-left origin, points), its stacking index among the
/// content view's subviews, and the PNG bytes (empty when it is hidden or
/// the snapshot failed).
pub struct Shot {
    /// Webview label.
    pub label: String,
    /// `[x, y, width, height]` in the content view (points).
    pub rect: [f64; 4],
    /// Index among the content view's subviews (bottom first).
    pub z: usize,
    /// Whether the view is hidden.
    pub hidden: bool,
    /// The view's top safe-area inset (points): the title bar band of a
    /// full-size content view. The web engine lays the page out below it, but the
    /// snapshot draws the page from the view's top edge.
    pub inset_top: f64,
    /// The snapshot as PNG, or empty.
    pub png: Vec<u8>,
    /// Why there is no snapshot, if there is none.
    pub error: Option<String>,
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

/// The address of a raw view pointer, for [`Views`].
#[must_use]
pub fn address(view: *mut c_void) -> usize {
    view as usize
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

/// The webview label of `view` or one of its ancestors, else its class name.
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
    let class = unsafe { &*view }
        .class()
        .name()
        .to_string_lossy()
        .into_owned();
    json!({ "label": null, "class": class })
}

/// The top safe-area inset of `view` (points), 0 when it has none.
fn inset_top(view: &AnyObject) -> f64 {
    // SAFETY: NSView getter (macOS 11+) on a live view, main thread.
    let insets: NSEdgeInsets = unsafe { msg_send![view, safeAreaInsets] };
    if insets.top.is_finite() && insets.top > 0.0 {
        insets.top
    } else {
        0.0
    }
}

/// `point` (embedder page coordinates) in window coordinates. The page
/// starts below the view's top safe-area inset (the title bar band), so a
/// page point is that much lower in the (flipped) view.
fn window_point(embedder: &AnyObject, point: &Point) -> NSPoint {
    let view_point = NSPoint::new(point.x, point.y + inset_top(embedder));
    // SAFETY: NSView coordinate conversion; nil means the window.
    unsafe { msg_send![embedder, convertPoint: view_point, toView: None::<&AnyObject>] }
}

/// The view a click at window point `wp` would reach: `-[NSView hitTest:]`
/// on the content view, as `-[NSWindow sendEvent:]` routes a mouse-down.
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

/// For each point, the view a click there would reach (webview label, or
/// the class name of a view that is not a webview).
pub fn inspect(ns_window: usize, embedder: &str, views: &Views, points: &[Point]) -> Value {
    let Some(content) = content_view(ns_window) else {
        return json!({ "error": "no content view" });
    };
    let embedder_view = views
        .iter()
        .find(|(_, l)| l.as_str() == embedder)
        // SAFETY: the addresses are the window's live webviews, read just
        // before this main-thread call.
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
    json!({ "hits": hits })
}

/// Sends one left click at `point` to the embedder webview (`mouseDown:` /
/// `mouseUp:`, so the window is not made key and the app is not activated),
/// but only when the window's hit test at that point names the embedder.
/// Otherwise nothing is sent and the view the click would have reached is
/// reported.
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
        return json!({
            "name": point.name,
            "sent": false,
            "refused": "the click would not reach the app webview",
            "target": target,
        });
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
        for (kind, down) in [(1_usize, true), (2_usize, false)] {
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
            if down {
                let () = msg_send![view, mouseDown: &*event];
            } else {
                let () = msg_send![view, mouseUp: &*event];
            }
        }
    }
    json!({ "name": point.name, "sent": true, "target": target })
}

/// The index of `view` (or of its ancestor that is a direct subview of
/// `content`) among `content`'s subviews.
fn stack_index(content: &AnyObject, view: &AnyObject) -> usize {
    // SAFETY: NSView `superview` / `subviews` on live views, main thread.
    unsafe {
        let mut cur: *const AnyObject = view;
        loop {
            let parent: *mut AnyObject = msg_send![&*cur, superview];
            if parent.is_null() {
                return 0;
            }
            if std::ptr::eq(parent, content) {
                let subviews: *mut AnyObject = msg_send![content, subviews];
                let Some(subviews) = subviews.as_ref() else {
                    return 0;
                };
                let i: usize = msg_send![subviews, indexOfObject: &*cur];
                return if i == usize::MAX { 0 } else { i };
            }
            cur = parent;
        }
    }
}

/// The `NSImage` `image` encoded as PNG.
fn png_of(image: &AnyObject) -> Result<Vec<u8>, String> {
    // SAFETY: AppKit image and bitmap methods on live objects; the NSData
    // bytes are copied before it is released.
    unsafe {
        let tiff: *mut AnyObject = msg_send![image, TIFFRepresentation];
        let tiff = tiff.as_ref().ok_or("no TIFF data")?;
        let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), imageRepWithData: tiff];
        let rep = rep.as_ref().ok_or("no bitmap")?;
        data_of_png(rep)
    }
}

/// `-[NSBitmapImageRep representationUsingType:NSBitmapImageFileTypePNG]`.
fn data_of_png(rep: &AnyObject) -> Result<Vec<u8>, String> {
    // SAFETY: as above; NSBitmapImageFileTypePNG = 4; an empty properties
    // dictionary.
    unsafe {
        let props: Retained<AnyObject> = msg_send![class!(NSDictionary), dictionary];
        let data: *mut AnyObject =
            msg_send![rep, representationUsingType: 4_usize, properties: &*props];
        let data = data.as_ref().ok_or("no PNG data")?;
        let len: usize = msg_send![data, length];
        let bytes: *const u8 = msg_send![data, bytes];
        if bytes.is_null() || len == 0 {
            return Err("empty PNG data".to_owned());
        }
        Ok(std::slice::from_raw_parts(bytes, len).to_vec())
    }
}

/// Snapshots the `WKWebView` at `address` (main thread) and calls `done`
/// with its [`Shot`] from `WebKit`'s completion handler (also the main
/// thread).
pub fn shot(ns_window: usize, address: usize, label: String, done: impl Fn(Shot) + 'static) {
    let (Some(content), Some(view)) = (content_view(ns_window), unsafe {
        // SAFETY: a live webview of the window (read just before this call).
        object(address)
    }) else {
        done(Shot {
            label,
            rect: [0.0; 4],
            z: 0,
            hidden: true,
            inset_top: 0.0,
            png: Vec::new(),
            error: Some("view gone".to_owned()),
        });
        return;
    };
    // SAFETY: NSView geometry getters on live views, main thread.
    let (rect, hidden) = unsafe {
        let bounds: NSRect = msg_send![view, bounds];
        let r: NSRect = msg_send![view, convertRect: bounds, toView: content];
        let flipped: Bool = msg_send![content, isFlipped];
        let cb: NSRect = msg_send![content, bounds];
        // Content-view coordinates with a bottom-left origin.
        let y = if flipped.as_bool() {
            cb.size.height - r.origin.y - r.size.height
        } else {
            r.origin.y
        };
        let hidden: Bool = msg_send![view, isHiddenOrHasHiddenAncestor];
        (
            [r.origin.x, y, r.size.width, r.size.height],
            hidden.as_bool(),
        )
    };
    let z = stack_index(content, view);
    let inset = inset_top(view);
    if hidden {
        done(Shot {
            label,
            rect,
            z,
            hidden,
            inset_top: inset,
            png: Vec::new(),
            error: None,
        });
        return;
    }
    let block = RcBlock::new(move |image: *mut AnyObject, _error: *mut AnyObject| {
        // SAFETY: WebKit passes a live NSImage, or nil with an NSError.
        let result = unsafe { image.as_ref() }.map_or_else(|| Err("no image".to_owned()), png_of);
        let (png, error) = match result {
            Ok(png) => (png, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        done(Shot {
            label: label.clone(),
            rect,
            z,
            hidden,
            inset_top: inset,
            png,
            error,
        });
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

/// Draws the visible shots bottom to top into one bitmap (at `scale` pixels
/// per point) and returns it as PNG (main thread).
///
/// Each snapshot is drawn at its own size from the bottom-left corner of its
/// view's rectangle, moved down by the view's top inset: the web engine lays a page
/// out below the title bar band of a full-size content view (the app
/// webview's viewport is that much shorter than the view), and the ad guests
/// are placed against that layout, but the snapshot draws the page from the
/// view's top edge. The bitmap is the content view's width and ends at the
/// top of the highest drawn page, so the empty title bar band is left out.
pub fn composite(ns_window: usize, shots: &[Shot], scale: f64) -> Result<Vec<u8>, String> {
    let content = content_view(ns_window).ok_or("no content view")?;
    let mut order: Vec<&Shot> = shots
        .iter()
        .filter(|s| !s.hidden && !s.png.is_empty())
        .collect();
    order.sort_by_key(|s| s.z);
    // SAFETY: AppKit drawing on the main thread into a bitmap the function
    // owns; every object is live for the calls that use it.
    unsafe {
        let mut images: Vec<(Retained<AnyObject>, NSRect)> = Vec::new();
        for s in order {
            let data: Retained<AnyObject> = msg_send![
                class!(NSData),
                dataWithBytes: s.png.as_ptr().cast::<c_void>(),
                length: s.png.len()
            ];
            let image_alloc: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
            let image: Option<Retained<AnyObject>> = msg_send![image_alloc, initWithData: &*data];
            let Some(image) = image else { continue };
            let size: NSSize = msg_send![&*image, size];
            let size = if size.width > 0.0 && size.height > 0.0 {
                size
            } else {
                NSSize::new(s.rect[2], s.rect[3])
            };
            images.push((
                image,
                NSRect::new(NSPoint::new(s.rect[0], s.rect[1] - s.inset_top), size),
            ));
        }
        let bounds: NSRect = msg_send![content, bounds];
        let w = bounds.size.width;
        let h = images
            .iter()
            .map(|(_, r)| r.origin.y + r.size.height)
            .fold(0.0_f64, f64::max)
            .clamp(1.0, bounds.size.height.max(1.0));
        #[expect(
            clippy::cast_possible_truncation,
            reason = "window sizes are small positive numbers"
        )]
        let (pw, ph) = ((w * scale).round() as isize, (h * scale).round() as isize);
        let alloc: Allocated<AnyObject> = msg_send![class!(NSBitmapImageRep), alloc];
        let color_space: Retained<AnyObject> = msg_send![class!(NSString), stringWithUTF8String: c"NSCalibratedRGBColorSpace".as_ptr()];
        let rep: Option<Retained<AnyObject>> = msg_send![
            alloc,
            initWithBitmapDataPlanes: std::ptr::null_mut::<*mut u8>(),
            pixelsWide: pw,
            pixelsHigh: ph,
            bitsPerSample: 8_isize,
            samplesPerPixel: 4_isize,
            hasAlpha: Bool::YES,
            isPlanar: Bool::NO,
            colorSpaceName: &*color_space,
            bytesPerRow: 0_isize,
            bitsPerPixel: 0_isize
        ];
        let rep = rep.ok_or("no bitmap")?;
        let () = msg_send![&*rep, setSize: NSSize::new(w, h)];
        let ctx: *mut AnyObject =
            msg_send![class!(NSGraphicsContext), graphicsContextWithBitmapImageRep: &*rep];
        let ctx = ctx.as_ref().ok_or("no graphics context")?;
        let () = msg_send![class!(NSGraphicsContext), saveGraphicsState];
        let () = msg_send![class!(NSGraphicsContext), setCurrentContext: ctx];
        for (image, rect) in &images {
            // NSCompositingOperationSourceOver = 2.
            let () = msg_send![
                &**image,
                drawInRect: *rect,
                fromRect: NSRect::ZERO,
                operation: 2_usize,
                fraction: 1.0_f64
            ];
        }
        let () = msg_send![ctx, flushGraphics];
        let () = msg_send![class!(NSGraphicsContext), restoreGraphicsState];
        data_of_png(&rep)
    }
}

/// The backing scale factor of the window (2 on a Retina display).
pub fn scale(ns_window: usize) -> f64 {
    // SAFETY: NSWindow getter on the live window, main thread.
    unsafe { object(ns_window) }.map_or(1.0, |w| {
        // SAFETY: as above.
        let s: f64 = unsafe { msg_send![w, backingScaleFactor] };
        if s > 0.0 { s } else { 1.0 }
    })
}

/// The replaced implementations (kept so nothing else is lost).
static ACTIVATE: OnceLock<usize> = OnceLock::new();
/// See [`ACTIVATE`].
static ACTIVATE_IGNORING: OnceLock<usize> = OnceLock::new();
/// See [`ACTIVATE`].
static KEY_FRONT: OnceLock<usize> = OnceLock::new();

/// `-[NSApplication activate]` without effect.
extern "C-unwind" fn no_activate(_this: &AnyObject, _cmd: Sel) {}

/// `-[NSApplication activateIgnoringOtherApps:]` without effect.
extern "C-unwind" fn no_activate_ignoring(_this: &AnyObject, _cmd: Sel, _flag: Bool) {}

/// `-[NSWindow makeKeyAndOrderFront:]` that only orders the window front.
extern "C-unwind" fn order_front_only(this: &AnyObject, _cmd: Sel, _sender: *mut AnyObject) {
    // SAFETY: `this` is the NSWindow the message was sent to, on the main
    // thread; a public NSWindow method without arguments.
    let () = unsafe { msg_send![this, orderFrontRegardless] };
}

/// Replaces the instance method `selector` of `class` with `imp`, keeping
/// the original in `slot`.
fn replace_method(class: &AnyClass, selector: Sel, slot: &OnceLock<usize>, imp: Imp) {
    let Some(method) = class.instance_method(selector) else {
        return;
    };
    let _ = slot.set(method.implementation() as usize);
    // SAFETY: a method of a registered class; its encoding outlives the call.
    let types = unsafe { objc2::ffi::method_getTypeEncoding(method) };
    // SAFETY: `imp` has the selector's signature, as `types` says.
    unsafe {
        objc2::ffi::class_replaceMethod(std::ptr::from_ref(class).cast_mut(), selector, imp, types);
    }
}

/// For the rest of the process: the app never activates and no window
/// becomes key (`WebKit` asks `NSApplication` to activate for every webview
/// it creates; showing a window makes it key). Call before the app is built.
pub fn hold_app_back() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        type Activate = extern "C-unwind" fn(&AnyObject, Sel);
        type Ignoring = extern "C-unwind" fn(&AnyObject, Sel, Bool);
        type KeyFront = extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject);
        let (activate, ignoring, key_front): (Activate, Ignoring, KeyFront) =
            (no_activate, no_activate_ignoring, order_front_only);
        // SAFETY: function pointers cast to the runtime's untyped `Imp`; each
        // is installed only for a selector with its exact signature.
        let (activate, ignoring, key_front) = unsafe {
            (
                std::mem::transmute::<Activate, Imp>(activate),
                std::mem::transmute::<Ignoring, Imp>(ignoring),
                std::mem::transmute::<KeyFront, Imp>(key_front),
            )
        };
        let app = class!(NSApplication);
        replace_method(app, sel!(activate), &ACTIVATE, activate);
        replace_method(
            app,
            sel!(activateIgnoringOtherApps:),
            &ACTIVATE_IGNORING,
            ignoring,
        );
        replace_method(
            class!(NSWindow),
            sel!(makeKeyAndOrderFront:),
            &KEY_FRONT,
            key_front,
        );
    });
}

/// Alpha 0, click-through, above other apps' windows and on every Space,
/// for the window at `ns_window` (before it is shown, main thread).
pub fn prepare_invisible(ns_window: usize) {
    // SAFETY: Tauri's `ns_window()` of a live window, on the main thread
    // (the setup hook); public NSWindow setters (level and collection
    // behavior take `NSInteger` and `NSUInteger`).
    unsafe {
        if let Some(window) = object(ns_window) {
            let () = msg_send![window, setAlphaValue: 0.0_f64];
            let () = msg_send![window, setIgnoresMouseEvents: Bool::YES];
            let () = msg_send![window, setLevel: LAB_WINDOW_LEVEL];
            let () = msg_send![window, setCollectionBehavior: LAB_COLLECTION_BEHAVIOR];
        }
    }
}

/// Orders the window at `ns_window` front at alpha 0, without making it key
/// or activating the app (main thread).
pub fn order_front(ns_window: usize) {
    // SAFETY: the live window of `order_front`'s caller, on the main thread;
    // public NSWindow methods.
    unsafe {
        if let Some(window) = object(ns_window) {
            let () = msg_send![window, setAlphaValue: 0.0_f64];
            let () = msg_send![window, orderFrontRegardless];
        }
    }
}
