//! Windows lab probes of a window's native child windows, in process: the
//! lab checks for transparency (L1-W), stacking (L2), input pass-through
//! (L3-W) and mute state (L5) of the ad guests (AD-FORMATS-SPEC §7).
//!
//! Each webview (`WebView2`, windowed hosting) lives in its own container
//! window: the controller's `ParentWindow`. [`webview_facts`] reads, on the
//! webview's thread, that container and the controller's state (bounds,
//! visibility, default background colour, mute). [`inspect`] then reads the
//! container windows' z-order (`GetWindow(GW_CHILD / GW_HWNDNEXT)`, top to
//! bottom), each container's window region (`GetWindowRgn`: the
//! pass-through switch) and, for each probe point, the window a click there
//! would reach (`WindowFromPoint`, which skips a window whose region is
//! empty, as the system's own hit test does). [`capture`] copies the
//! composed window with `PrintWindow(PW_RENDERFULLCONTENT)` (the window's
//! own content, `WebView2` children included; no screen capture) and samples
//! it at the points. [`click`] is the only input it ever creates: one
//! `SendInput` left click at a point, sent only when the hit test names the
//! app's own webview, so an ad guest never receives it.
//!
//! Used by the Windows CI lab only (`run.mjs --ci-visible`), where the lab
//! windows are shown on the runner's desktop.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_COLOR, ICoreWebView2_8, ICoreWebView2Controller, ICoreWebView2Controller2,
};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, ClientToScreen, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateRectRgn, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits,
    GetWindowRgn, GetWindowRgnBox, HDC, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GW_CHILD, GW_HWNDNEXT, GetClassNameW, GetParent, GetSystemMetrics, GetWindow, GetWindowRect,
    HWND_TOPMOST, IsWindowVisible, PW_RENDERFULLCONTENT, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
};
use windows::core::{BOOL, Interface};

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

/// What the webview's own thread reports about one webview.
#[derive(Clone, Debug)]
pub struct Facts {
    /// The webview label.
    pub label: String,
    /// The controller's parent window (the webview's container), as an
    /// address.
    pub container: isize,
    /// The controller's bounds in the container's client coordinates
    /// (`[left, top, right, bottom]`, device pixels).
    pub bounds: [i32; 4],
    /// `ICoreWebView2Controller::IsVisible`.
    pub visible: Option<bool>,
    /// `ICoreWebView2Controller2::DefaultBackgroundColor` (`[a, r, g, b]`).
    pub background: Option<[u8; 4]>,
    /// `ICoreWebView2_8::IsMuted`.
    pub muted: Option<bool>,
    /// `ICoreWebView2_8::IsDocumentPlayingAudio`.
    pub playing_audio: Option<bool>,
}

fn hwnd(address: isize) -> HWND {
    HWND(address as *mut std::ffi::c_void)
}

fn address(window: HWND) -> isize {
    window.0 as isize
}

/// Reads [`Facts`] from a webview's controller. Call it on the webview's
/// own thread (inside `Webview::with_webview`).
#[must_use]
pub fn webview_facts(label: &str, controller: &ICoreWebView2Controller) -> Facts {
    let mut container = HWND::default();
    let mut bounds = RECT::default();
    let mut visible = BOOL::default();
    // SAFETY: COM getters on the webview's own thread with valid out
    // pointers.
    let (has_parent, has_bounds, has_visible) = unsafe {
        (
            controller.ParentWindow(&raw mut container).is_ok(),
            controller.Bounds(&raw mut bounds).is_ok(),
            controller.IsVisible(&raw mut visible).is_ok(),
        )
    };
    let background = controller
        .cast::<ICoreWebView2Controller2>()
        .ok()
        .and_then(|c2| {
            let mut colour = COREWEBVIEW2_COLOR::default();
            // SAFETY: as above.
            unsafe { c2.DefaultBackgroundColor(&raw mut colour) }
                .ok()
                .map(|()| [colour.A, colour.R, colour.G, colour.B])
        });
    // SAFETY: as above.
    let core8 = unsafe { controller.CoreWebView2() }
        .ok()
        .and_then(|core| core.cast::<ICoreWebView2_8>().ok());
    let read = |f: &dyn Fn(&ICoreWebView2_8, *mut BOOL) -> windows::core::Result<()>| {
        core8.as_ref().and_then(|c| {
            let mut value = BOOL::default();
            f(c, &raw mut value).ok().map(|()| value.as_bool())
        })
    };
    // SAFETY: as above.
    let muted = read(&|c, out| unsafe { c.IsMuted(out) });
    // SAFETY: as above.
    let playing_audio = read(&|c, out| unsafe { c.IsDocumentPlayingAudio(out) });
    Facts {
        label: label.to_owned(),
        container: if has_parent { address(container) } else { 0 },
        bounds: if has_bounds {
            [bounds.left, bounds.top, bounds.right, bounds.bottom]
        } else {
            [0; 4]
        },
        visible: has_visible.then(|| visible.as_bool()),
        background,
        muted,
        playing_audio,
    }
}

fn class_name(window: HWND) -> String {
    let mut buf = [0_u16; 128];
    // SAFETY: a valid buffer; any window handle (a stale one returns 0).
    let len = unsafe { GetClassNameW(window, &mut buf) };
    String::from_utf16_lossy(&buf[..usize::try_from(len).unwrap_or(0)])
}

fn window_rect(window: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    // SAFETY: valid out pointer.
    unsafe { GetWindowRect(window, &raw mut rect) }
        .ok()
        .map(|()| rect)
}

/// A container's window region: `none` (no region, the whole window takes
/// input), `empty` (input passes through), `rect` or `complex`, with its
/// bounding box.
fn region(window: HWND) -> Value {
    // SAFETY: a scratch region the call fills, deleted right after.
    unsafe {
        let rgn = CreateRectRgn(0, 0, 0, 0);
        let kind = GetWindowRgn(window, rgn);
        let mut bbox = RECT::default();
        let _ = GetWindowRgnBox(window, &raw mut bbox);
        let _ = DeleteObject(rgn.into());
        let name = match kind.0 {
            1 => "empty",
            2 => "rect",
            3 => "complex",
            _ => "none",
        };
        json!({ "kind": name, "box": [bbox.left, bbox.top, bbox.right, bbox.bottom] })
    }
}

/// The label of the webview whose container is `window` or one of its
/// ancestors, else `None`.
fn owner_label(window: HWND, containers: &BTreeMap<isize, String>) -> Option<String> {
    let mut cur = window;
    for _ in 0..32 {
        if cur.is_invalid() {
            return None;
        }
        if let Some(label) = containers.get(&address(cur)) {
            return Some(label.clone());
        }
        // SAFETY: any window handle; an error ends the walk.
        cur = unsafe { GetParent(cur) }.unwrap_or_default();
    }
    None
}

/// The screen position of page point `p` of the webview `f` (its bounds in
/// its container's client area, at the window's DPI).
fn screen_point(f: &Facts, p: &Point, scale: f64) -> POINT {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "screen pixels of a window on a desktop fit in i32"
    )]
    let mut pt = POINT {
        x: f.bounds[0] + (p.x * scale).round() as i32,
        y: f.bounds[1] + (p.y * scale).round() as i32,
    };
    // SAFETY: valid in/out pointer; the container is a live window.
    let _ = unsafe { ClientToScreen(hwnd(f.container), &raw mut pt) };
    pt
}

fn target(at: POINT, containers: &BTreeMap<isize, String>) -> Value {
    // SAFETY: a plain hit test.
    let hit = unsafe { windows::Win32::UI::WindowsAndMessaging::WindowFromPoint(at) };
    match owner_label(hit, containers) {
        Some(label) => json!({ "label": label }),
        None => json!({ "label": null, "class": class_name(hit) }),
    }
}

/// The DPI scale of `window` (1.0 at 96 DPI).
fn scale_of(window: HWND) -> f64 {
    // SAFETY: any window handle; 0 for an invalid one.
    let dpi = unsafe { GetDpiForWindow(window) };
    if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 }
}

/// Keeps the lab window above other windows of the runner's desktop, so the
/// hit test and the capture see it (no activation).
pub fn keep_on_top(top: isize) {
    // SAFETY: a live top-level window of this process.
    let _ = unsafe {
        SetWindowPos(
            hwnd(top),
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
}

/// The window's container z-order and hit tests: the child windows of the
/// top-level window bottom to top (webview label or class name, hidden),
/// the facts and window region of every webview, and for each point the
/// webview a click there would reach.
#[must_use]
pub fn inspect(top: isize, embedder: &str, facts: &[Facts], points: &[Point]) -> Value {
    let containers: BTreeMap<isize, String> = facts
        .iter()
        .filter(|f| f.container != 0)
        .map(|f| (f.container, f.label.clone()))
        .collect();
    let mut order = Vec::new();
    // SAFETY: walking the child list of a live window.
    let mut child = unsafe { GetWindow(hwnd(top), GW_CHILD) }.unwrap_or_default();
    while !child.is_invalid() {
        let mut entry = match containers.get(&address(child)) {
            Some(label) => json!({ "label": label }),
            None => json!({ "label": null, "class": class_name(child) }),
        };
        // SAFETY: as above.
        entry["hidden"] = Value::Bool(!unsafe { IsWindowVisible(child) }.as_bool());
        order.push(entry);
        // SAFETY: as above.
        child = unsafe { GetWindow(child, GW_HWNDNEXT) }.unwrap_or_default();
    }
    // GetWindow lists top to bottom; the report is bottom to top (as AppKit's
    // subviews). A webview hosted in the top-level window itself is the
    // bottom layer.
    order.reverse();
    if let Some(label) = containers.get(&top) {
        order.insert(
            0,
            json!({ "label": label, "hidden": false, "topLevel": true }),
        );
    }
    let mut webviews = serde_json::Map::new();
    for f in facts {
        let window = hwnd(f.container);
        webviews.insert(
            f.label.clone(),
            json!({
                // SAFETY: a live window handle.
                "hidden": f.visible == Some(false) || !unsafe { IsWindowVisible(window) }.as_bool(),
                "controllerVisible": f.visible,
                "containerClass": class_name(window),
                "containerIsTopLevel": f.container == top,
                "bounds": f.bounds,
                "region": region(window),
                "backgroundArgb": f.background,
                "muted": f.muted,
                "playingAudio": f.playing_audio,
            }),
        );
    }
    let scale = scale_of(hwnd(top));
    let host = facts.iter().find(|f| f.label == embedder);
    let hits: Vec<Value> = points
        .iter()
        .map(|p| {
            let Some(e) = host else {
                return json!({ "name": p.name, "error": "no embedder webview" });
            };
            let at = screen_point(e, p, scale);
            json!({ "name": p.name, "x": p.x, "y": p.y, "screen": [at.x, at.y], "target": target(at, &containers) })
        })
        .collect();
    json!({
        "platform": "windows",
        "scale": scale,
        "order": order,
        "webviews": webviews,
        "hits": hits,
    })
}

/// One left click at `point` with `SendInput`, only when the system hit
/// test there names the app's webview `embedder`; otherwise nothing is sent
/// and the result says which window the click would have reached.
#[must_use]
pub fn click(top: isize, embedder: &str, facts: &[Facts], point: &Point) -> Value {
    let containers: BTreeMap<isize, String> = facts
        .iter()
        .filter(|f| f.container != 0)
        .map(|f| (f.container, f.label.clone()))
        .collect();
    let Some(e) = facts.iter().find(|f| f.label == embedder) else {
        return json!({ "error": "no embedder webview" });
    };
    let at = screen_point(e, point, scale_of(hwnd(top)));
    let hit = target(at, &containers);
    if hit["label"].as_str() != Some(embedder) {
        return json!({ "name": point.name, "sent": false, "refused": "the click would not reach the app webview", "target": hit });
    }
    // Absolute coordinates over the virtual desktop, 0..65535.
    // SAFETY: plain metric reads.
    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2),
        )
    };
    let norm = |v: i32, origin: i32, size: i32| ((v - origin) * 65535) / (size - 1);
    let mouse = |flags: MOUSE_EVENT_FLAGS| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: norm(at.x, vx, vw),
                dy: norm(at.y, vy, vh),
                mouseData: 0,
                dwFlags: flags | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [
        mouse(MOUSEEVENTF_MOVE),
        mouse(MOUSEEVENTF_LEFTDOWN),
        mouse(MOUSEEVENTF_LEFTUP),
    ];
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "size_of::<INPUT>() is a small constant"
    )]
    // SAFETY: well-formed mouse inputs.
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    json!({ "name": point.name, "sent": sent == 3, "inputs": sent, "screen": [at.x, at.y], "target": hit })
}

/// Pixels of a window copy: BGRA rows, top-down.
struct Image {
    width: i32,
    height: i32,
    bgra: Vec<u8>,
}

impl Image {
    fn rgba(&self, x: i32, y: i32) -> Option<[f64; 4]> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        let i = usize::try_from((y * self.width + x) * 4).ok()?;
        let px = self.bgra.get(i..i + 4)?;
        let v = |c: u8| (f64::from(c) / 255.0 * 1000.0).round() / 1000.0;
        // GDI leaves the alpha byte undefined: a window copy is opaque.
        Some([v(px[2]), v(px[1]), v(px[0]), 1.0])
    }

    /// A 32-bit top-down BMP of the image.
    fn bmp(&self) -> Vec<u8> {
        let data = u32::try_from(self.bgra.len()).unwrap_or(0);
        let mut out = Vec::with_capacity(self.bgra.len() + 54);
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&(54 + data).to_le_bytes());
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(&54_u32.to_le_bytes());
        out.extend_from_slice(&40_u32.to_le_bytes());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&(-self.height).to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&32_u16.to_le_bytes());
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(&data.to_le_bytes());
        out.extend_from_slice(&[0; 16]);
        out.extend_from_slice(&self.bgra);
        out
    }
}

/// Copies `width` x `height` pixels with `fill` (which draws into the
/// memory DC) and reads them back.
fn copy(width: i32, height: i32, fill: impl FnOnce(HDC) -> bool) -> Option<Image> {
    if width <= 0 || height <= 0 {
        return None;
    }
    // SAFETY: GDI objects created, used and released in this block.
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let old = SelectObject(mem, bitmap.into());
        let ok = fill(mem);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).unwrap_or(40),
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..BITMAPINFOHEADER::default()
            },
            ..BITMAPINFO::default()
        };
        let mut bgra = vec![0_u8; usize::try_from(width * height * 4).unwrap_or(0)];
        SelectObject(mem, old);
        let lines = GetDIBits(
            mem,
            bitmap,
            0,
            u32::try_from(height).unwrap_or(0),
            Some(bgra.as_mut_ptr().cast()),
            &raw mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        (ok && lines == height).then_some(Image {
            width,
            height,
            bgra,
        })
    }
}

/// The composed window (`PrintWindow` with `PW_RENDERFULLCONTENT`, the
/// window's own content) and, for comparison, the same rectangle of the
/// desktop (`BitBlt` from the screen DC), each sampled at the points: RGBA
/// (0..1) in window pixels. When `save` names a file stem, both copies are
/// written next to it as BMP files (`<stem>-print.bmp`, `<stem>-screen.bmp`).
#[must_use]
pub fn capture(
    top: isize,
    embedder: Option<&Facts>,
    points: &[Point],
    save: Option<&Path>,
) -> Value {
    let Some(rect) = window_rect(hwnd(top)) else {
        return json!({ "error": "no window rect" });
    };
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    let print = copy(w, h, |dc| {
        // SAFETY: a live window of this process and a memory DC of its size.
        unsafe { PrintWindow(hwnd(top), dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)) }.as_bool()
    });
    let screen = copy(w, h, |dc| {
        // SAFETY: the screen DC, released right after the copy.
        unsafe {
            let src = GetDC(None);
            let ok = BitBlt(dc, 0, 0, w, h, Some(src), rect.left, rect.top, SRCCOPY).is_ok();
            ReleaseDC(None, src);
            ok
        }
    });
    let scale = scale_of(hwnd(top));
    let sample = |image: &Option<Image>, kind: &str| -> Value {
        let Some(image) = image else {
            return json!({ "error": format!("{kind} copy failed") });
        };
        if let Some(stem) = save {
            let file = stem.with_file_name(format!(
                "{}-{kind}.bmp",
                stem.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("capture")
            ));
            let _ = std::fs::write(file, image.bmp());
        }
        let samples: Vec<Value> = points
            .iter()
            .map(|p| {
                let rgba = embedder.and_then(|e| {
                    let at = screen_point(e, p, scale);
                    image.rgba(at.x - rect.left, at.y - rect.top)
                });
                json!({ "name": p.name, "rgba": rgba })
            })
            .collect();
        json!({ "pixels": [image.width, image.height], "samples": samples })
    };
    json!({
        "source": "PrintWindow(PW_RENDERFULLCONTENT)",
        "window": [rect.left, rect.top, w, h],
        "print": sample(&print, "print"),
        "screen": sample(&screen, "screen"),
    })
}
