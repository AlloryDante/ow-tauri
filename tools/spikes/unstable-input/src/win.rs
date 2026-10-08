//! Windows driver (CI runner only: windows are visible there, on a desktop
//! nobody uses). The app brings its own window to the foreground, then
//! clicks and types with `SendInput`, so the input travels the same path a
//! real keyboard's does (the foreground thread's queue → the focused
//! `WebView2` window).

use std::time::Duration;

use serde_json::{Value, json};
use tauri::{AppHandle, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, KLF_ACTIVATE, LoadKeyboardLayoutW,
    MAPVK_VK_TO_VSC, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, MapVirtualKeyW,
    SendInput, VIRTUAL_KEY, VK_6, VK_A, VK_BACK, VK_DOWN, VK_E, VK_ESCAPE, VK_F2, VK_LEFT, VK_N,
    VK_O, VK_OEM_3, VK_OEM_7, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_U, VK_UP, VkKeyScanW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GUITHREADINFO, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo,
    GetSystemMetrics, GetWindowThreadProcessId, PostMessageW, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SetForegroundWindow,
    WM_INPUTLANGCHANGEREQUEST,
};
use windows::core::w;

use crate::Key;

pub struct Driver {
    main: WebviewWindow,
    hwnd: isize,
    layout_switched: std::sync::atomic::AtomicBool,
}

fn h(v: isize) -> HWND {
    HWND(v as *mut core::ffi::c_void)
}

fn class_of(hwnd: HWND) -> String {
    if hwnd.0.is_null() {
        return String::new();
    }
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// The foreground thread's focus window and its keyboard layout.
fn focus_info() -> Value {
    unsafe {
        let fg = GetForegroundWindow();
        let tid = GetWindowThreadProcessId(fg, None);
        let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        let _ = GetGUIThreadInfo(tid, &mut info);
        let focus_tid = GetWindowThreadProcessId(info.hwndFocus, None);
        json!({
            "foreground": fg.0 as isize,
            "foregroundClass": class_of(fg),
            "focus": info.hwndFocus.0 as isize,
            "focusClass": class_of(info.hwndFocus),
            "focusThreadLayout": format!("{:#x}", GetKeyboardLayout(focus_tid).0 as usize),
        })
    }
}

impl Driver {
    pub fn new(_app: &AppHandle, main: &WebviewWindow) -> Self {
        let hwnd = main.hwnd().map(|h| h.0 as isize).unwrap_or(0);
        let d = Self { main: main.clone(), hwnd, layout_switched: false.into() };
        d.to_foreground();
        d
    }

    fn to_foreground(&self) -> Value {
        self.focus_window(&self.main)
    }

    /// Brings `window` to the foreground the way a user's Alt-Tab does (no
    /// click into it). Returns whether it is the foreground window now.
    pub fn focus_window(&self, window: &WebviewWindow) -> Value {
        let target = window.hwnd().map(|h| h.0 as isize).unwrap_or(0);
        let _ = window.set_focus();
        std::thread::sleep(Duration::from_millis(300));
        unsafe {
            if GetForegroundWindow().0 as isize != target {
                // The usual trick: share the foreground thread's input state.
                let fg = GetForegroundWindow();
                let fg_tid = GetWindowThreadProcessId(fg, None);
                let me = GetCurrentThreadId();
                let _ = AttachThreadInput(me, fg_tid, true);
                let _ = SetForegroundWindow(h(target));
                let _ = BringWindowToTop(h(target));
                let _ = AttachThreadInput(me, fg_tid, false);
                std::thread::sleep(Duration::from_millis(300));
            }
            json!({ "label": window.label(), "isForeground": GetForegroundWindow().0 as isize == target })
        }
    }

    pub fn environment(&self) -> Value {
        let scale = self.main.scale_factor().unwrap_or(1.0);
        let pos = self.main.inner_position().map(|p| [p.x, p.y]).ok();
        unsafe {
            json!({
                "hwnd": self.hwnd,
                "isForeground": GetForegroundWindow().0 as isize == self.hwnd,
                "scale": scale,
                "innerPosition": pos,
                "ownThreadLayout": format!("{:#x}", GetKeyboardLayout(0).0 as usize),
                "focus": focus_info(),
            })
        }
    }

    pub fn after_keys(&self) -> Value {
        json!({ "focus": focus_info() })
    }

    /// One left click at the page point `prep.{x,y}` (CSS px of the main
    /// webview, which fills the window's client area from its top-left).
    pub fn click(&self, prep: &Value) -> Value {
        let fg = self.to_foreground();
        let x = prep["x"].as_f64().unwrap_or(10.0);
        let y = prep["y"].as_f64().unwrap_or(10.0);
        let scale = self.main.scale_factor().unwrap_or(1.0);
        let Ok(origin) = self.main.inner_position() else {
            return json!({ "error": "no inner position" });
        };
        let sx = origin.x + (x * scale).round() as i32;
        let sy = origin.y + (y * scale).round() as i32;
        let (vx, vy, vw, vh) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2),
                GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2),
            )
        };
        let norm = |v: i32, o: i32, s: i32| ((v - o) * 65535) / (s - 1);
        let mouse = |flags: MOUSE_EVENT_FLAGS| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: norm(sx, vx, vw),
                    dy: norm(sy, vy, vh),
                    mouseData: 0,
                    dwFlags: flags | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [mouse(MOUSEEVENTF_MOVE), mouse(MOUSEEVENTF_LEFTDOWN), mouse(MOUSEEVENTF_LEFTUP)];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        std::thread::sleep(Duration::from_millis(150));
        let mut switched = Value::Null;
        if !self.layout_switched.swap(true, std::sync::atomic::Ordering::SeqCst) {
            // US-International (dead keys) for the focused window's thread,
            // in case the runner's default layout is not already it.
            unsafe {
                let before = focus_info();
                if let Ok(hkl) = LoadKeyboardLayoutW(w!("00020409"), KLF_ACTIVATE) {
                    let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
                    let tid = GetWindowThreadProcessId(GetForegroundWindow(), None);
                    let _ = GetGUIThreadInfo(tid, &mut info);
                    let _ = PostMessageW(Some(info.hwndFocus), WM_INPUTLANGCHANGEREQUEST, WPARAM(0), LPARAM(hkl.0 as isize));
                    let _ = PostMessageW(Some(GetForegroundWindow()), WM_INPUTLANGCHANGEREQUEST, WPARAM(0), LPARAM(hkl.0 as isize));
                }
                std::thread::sleep(Duration::from_millis(300));
                switched = json!({ "before": before, "after": focus_info() });
            }
        }
        json!({ "at": [x, y], "screen": [sx, sy], "sent": sent, "foreground": fg, "focus": focus_info(), "layoutSwitch": switched })
    }

    pub fn key(&self, key: Key) {
        let mut inputs: Vec<INPUT> = Vec::new();
        let vk_input = |vk: VIRTUAL_KEY, up: bool, ext: bool| {
            let mut flags = KEYBD_EVENT_FLAGS(0);
            if up {
                flags |= KEYEVENTF_KEYUP;
            }
            if ext {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        wScan: unsafe { MapVirtualKeyW(u32::from(vk.0), MAPVK_VK_TO_VSC) } as u16,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            }
        };
        let tap = |vk: VIRTUAL_KEY, shift: bool, ext: bool, inputs: &mut Vec<INPUT>| {
            if shift {
                inputs.push(vk_input(VK_SHIFT, false, false));
            }
            inputs.push(vk_input(vk, false, ext));
            inputs.push(vk_input(vk, true, ext));
            if shift {
                inputs.push(vk_input(VK_SHIFT, true, false));
            }
        };
        match key {
            Key::Ch(c) => {
                let scan = unsafe { VkKeyScanW(c as u16) };
                let vk = VIRTUAL_KEY((scan as u16) & 0xff);
                let shift = (scan as u16 >> 8) & 1 == 1;
                tap(vk, shift, false, &mut inputs);
            }
            Key::Left => tap(VK_LEFT, false, true, &mut inputs),
            Key::Right => tap(VK_RIGHT, false, true, &mut inputs),
            Key::Up => tap(VK_UP, false, true, &mut inputs),
            Key::Down => tap(VK_DOWN, false, true, &mut inputs),
            Key::Back => tap(VK_BACK, false, false, &mut inputs),
            Key::Enter => tap(VK_RETURN, false, false, &mut inputs),
            Key::Escape => tap(VK_ESCAPE, false, false, &mut inputs),
            Key::FKey => tap(VK_F2, false, false, &mut inputs),
            Key::Dead(c) => {
                // US-International: ' acute, " diaeresis, ~ tilde, ` grave,
                // ^ circumflex; then the base letter.
                let (accent, shift, base) = match c {
                    'é' => (VK_OEM_7, false, VK_E),
                    'ü' => (VK_OEM_7, true, VK_U),
                    'ñ' => (VK_OEM_3, true, VK_N),
                    'à' => (VK_OEM_3, false, VK_A),
                    'ô' => (VK_6, true, VK_O),
                    _ => panic!("no dead-key recipe for {c}"),
                };
                tap(accent, shift, false, &mut inputs);
                tap(base, false, false, &mut inputs);
            }
            Key::Uni(c) => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    for up in [false, true] {
                        inputs.push(INPUT {
                            r#type: INPUT_KEYBOARD,
                            Anonymous: INPUT_0 {
                                ki: KEYBDINPUT {
                                    wVk: VIRTUAL_KEY(0),
                                    wScan: *unit,
                                    dwFlags: if up { KEYEVENTF_UNICODE | KEYEVENTF_KEYUP } else { KEYEVENTF_UNICODE },
                                    time: 0,
                                    dwExtraInfo: 0,
                                },
                            },
                        });
                    }
                }
            }
        }
        for input in inputs {
            unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
            std::thread::sleep(Duration::from_millis(8));
        }
    }
}
