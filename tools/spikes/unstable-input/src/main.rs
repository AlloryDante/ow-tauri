//! Spike: does Tauri's `unstable` feature (required for `Window::add_child`,
//! which hosts ad guests as child webviews) break text input in the app's
//! own pages? See README.md.
//!
//! One window `main` (a `WebviewWindow`, as an app builds it) with six text
//! fields; with `SPIKE_MODE=unstable-child` also one child webview (a blank
//! local page, not an ad) beside them. A driver thread clicks each field and
//! types into it with OS-level input events (macOS: `NSEvent`s delivered to
//! this process only; Windows: `SendInput`), then reads the field's value
//! and the DOM event log, and writes everything to `SPIKE_OUT` as JSON.
//!
//! macOS always runs invisible: the window is alpha 0, click-through and
//! never key at the window-server level, and the app never activates (see
//! `mac.rs`).

#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod win;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// One key the driver presses. `Dead(c)` types `c` with a dead-key
/// sequence of the active layout (macOS ABC: Option+accent, then the base
/// letter; Windows US-International: accent key, then the base letter).
/// `Uni(c)` injects `c` as a Unicode packet (Windows `KEYEVENTF_UNICODE`).
#[derive(Clone, Copy, Debug)]
pub enum Key {
    Ch(char),
    Left,
    Right,
    Up,
    Down,
    Back,
    Enter,
    Escape,
    FKey,
    Dead(char),
    #[cfg_attr(not(windows), allow(dead_code))]
    Uni(char),
}

impl Key {
    fn label(self) -> String {
        match self {
            Key::Ch(c) => c.to_string(),
            Key::Left => "<Left>".into(),
            Key::Right => "<Right>".into(),
            Key::Up => "<Up>".into(),
            Key::Down => "<Down>".into(),
            Key::Back => "<Backspace>".into(),
            Key::Enter => "<Enter>".into(),
            Key::Escape => "<Escape>".into(),
            Key::FKey => "<F-key: F5 macOS, F2 Windows (F5 reloads WebView2)>".into(),
            Key::Dead(c) => format!("<dead:{c}>"),
            Key::Uni(c) => format!("<unicode:{c}>"),
        }
    }
}

fn chars(s: &str) -> Vec<Key> {
    s.chars().map(Key::Ch).collect()
}

struct Case {
    name: &'static str,
    keys: Vec<Key>,
    expect: String,
}

fn cases(target: &str) -> Vec<Case> {
    let multiline = matches!(target, "textarea" | "editable" | "rich" | "remount");
    if matches!(target, "refocus" | "remount" | "mv" | "ta") {
        let mut enter = chars("a");
        enter.push(Key::Enter);
        enter.push(Key::Ch('b'));
        let pre = if target == "mv" { "|" } else { "" };
        return vec![
            Case { name: "ascii", keys: chars("Hello, World! @home 123"), expect: format!("{pre}Hello, World! @home 123") },
            Case { name: "enter", keys: enter, expect: format!("{pre}{}", if multiline { "a\nb" } else { "ab" }) },
        ];
    }
    let mut arrows = chars("abc");
    arrows.extend([Key::Left; 4]);
    arrows.push(Key::Ch('X'));
    arrows.extend([Key::Right; 5]);
    arrows.push(Key::Ch('Y'));
    arrows.extend([Key::Up, Key::Down, Key::Left, Key::Right]);
    let mut back = chars("abcd");
    back.extend([Key::Back; 5]);
    back.push(Key::Ch('z'));
    let mut enter = chars("a");
    enter.push(Key::Enter);
    enter.push(Key::Ch('b'));
    let mut list = vec![
        Case {
            name: "ascii",
            keys: chars("Hello, World! @home 123"),
            expect: "Hello, World! @home 123".into(),
        },
        Case {
            name: "arrows",
            keys: arrows,
            expect: "XabcY".into(),
        },
        Case {
            name: "backspace",
            keys: back,
            expect: "z".into(),
        },
        Case {
            name: "boundary-arrows-empty",
            keys: vec![Key::Left, Key::Right, Key::Up, Key::Down, Key::Left, Key::Ch('q'), Key::Right, Key::Right],
            expect: "q".into(),
        },
        Case {
            name: "unhandled-keys",
            keys: vec![Key::Ch('a'), Key::Escape, Key::FKey, Key::Ch('b')],
            expect: "ab".into(),
        },
        Case {
            name: "dead-keys",
            keys: "éüñàô".chars().map(Key::Dead).collect(),
            expect: "éüñàô".into(),
        },
        Case {
            name: "enter",
            keys: enter,
            expect: if multiline { "a\nb".into() } else { "ab".into() },
        },
    ];
    if cfg!(windows) {
        list.push(Case {
            name: "unicode-packets",
            keys: "éüñàô€".chars().map(Key::Uni).collect(),
            expect: "éüñàô€".into(),
        });
    }
    list
}

const TARGETS: [&str; 10] = ["input", "textarea", "editable", "rich", "kp", "kd", "refocus", "remount", "mv", "ta"];

/// Runs `js` in `webview` and returns its JSON result (or `null` after 5 s).
fn eval(w: &WebviewWindow, js: &str) -> Value {
    let (tx, rx) = mpsc::channel();
    let js = format!("(() => {{ try {{ return {js}; }} catch (e) {{ return {{ error: String(e) }}; }} }})()");
    if w
        .eval_with_callback(js, move |s| {
            let _ = tx.send(serde_json::from_str::<Value>(&s).unwrap_or(Value::String(s)));
        })
        .is_err()
    {
        return Value::Null;
    }
    rx.recv_timeout(Duration::from_secs(5)).unwrap_or(Value::Null)
}

fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn focus_state(app: &AppHandle, main: &WebviewWindow) -> Value {
    #[cfg(feature = "unstable")]
    let focused = {
        let windows: Vec<Value> = app
            .windows()
            .values()
            .map(|w| json!({ "label": w.label(), "isFocused": w.is_focused().ok() }))
            .collect();
        let webviews: Vec<Value> = app
            .webviews()
            .values()
            .map(|w| json!({ "label": w.label() }))
            .collect();
        json!({
            "getFocusedWindow": app.get_focused_window().map(|w| w.label().to_owned()),
            "windows": windows,
            "webviews": webviews,
        })
    };
    #[cfg(not(feature = "unstable"))]
    let focused = {
        let _ = app;
        json!({ "getFocusedWindow": "n/a (API needs the unstable feature)" })
    };
    json!({
        "mainIsFocused": main.is_focused().ok(),
        "unstableApis": focused,
        "page": eval(main, "({ hasFocus: document.hasFocus(), active: document.activeElement && document.activeElement.id })"),
    })
}

fn run(app: &AppHandle, main: WebviewWindow, mode: &str) -> Value {
    // Kept from setup: once a child webview is attached, the window is no
    // longer a "webview window" and `get_webview_window("main")` is None.
    let lookup = json!({ "getWebviewWindowMain": app.get_webview_window("main").is_some() });
    // Wait for the page.
    let start = Instant::now();
    while eval(&main, "!!(window.__spike && window.__spike.ready)") != Value::Bool(true) {
        if start.elapsed() > Duration::from_secs(20) {
            return json!({ "error": "page never became ready" });
        }
        sleep(100);
    }
    sleep(500);

    #[cfg(target_os = "macos")]
    let platform = mac::Driver::new(app, &main);
    #[cfg(windows)]
    let platform = win::Driver::new(app, &main);

    let mut env = platform.environment();
    env["focusAfterSetup"] = focus_state(app, &main);
    env["lookupWithChild"] = lookup;

    let mut results = Vec::new();

    // A page-level shortcut right after the window opened: nothing focused,
    // nobody clicked; does the page see the key at all?
    {
        let before = eval(&main, "window.__docKeys");
        let state = focus_state(app, &main);
        platform.key(Key::Ch('k'));
        sleep(400);
        let after = platform.after_keys();
        let count = eval(&main, "window.__docKeys");
        let seen = count.as_i64().unwrap_or(0) - before.as_i64().unwrap_or(0);
        results.push(json!({
            "target": "document", "case": "shortcut-at-open-no-focus", "keys": ["k"], "expect": "1 keydown",
            "value": format!("{seen} keydown"), "ok": seen == 1, "focusBefore": state, "afterKeys": after,
        }));
    }

    // Type right after the window opened: the page focused its input by
    // script (as `autofocus` does), nobody clicked into the webview yet.
    {
        let prep = eval(&main, "__spike.prep('input')");
        for k in chars("hi") {
            platform.key(k);
            sleep(35);
        }
        sleep(400);
        let after = platform.after_keys();
        let read = eval(&main, "__spike.read('input')");
        let value = read["value"].as_str().unwrap_or_default().to_owned();
        results.push(json!({
            "target": "input", "case": "type-at-open-no-click", "keys": ["h", "i"], "expect": "hi",
            "value": value, "ok": value == "hi", "prep": prep, "afterKeys": after, "log": read["log"],
        }));
    }
    for target in TARGETS {
        for case in cases(target) {
            let prep = eval(&main, &format!("__spike.prep({target:?})"));
            let click = platform.click(&prep);
            sleep(150);
            for key in &case.keys {
                platform.key(*key);
                sleep(35);
            }
            sleep(400);
            let after = platform.after_keys();
            let read = eval(&main, &format!("__spike.read({target:?})"));
            let value = read["value"].as_str().unwrap_or_default().to_owned();
            let keydowns = read["log"]
                .as_array()
                .map(|l| l.iter().filter(|e| e["type"] == "keydown").count())
                .unwrap_or(0);
            let inputs = read["log"]
                .as_array()
                .map(|l| l.iter().filter(|e| e["type"] == "input").count())
                .unwrap_or(0);
            results.push(json!({
                "target": target,
                "case": case.name,
                "keys": case.keys.iter().map(|k| k.label()).collect::<Vec<_>>(),
                "expect": case.expect,
                "value": value,
                "ok": value == case.expect,
                "cps": read["cps"],
                "keydownEvents": keydowns,
                "inputEvents": inputs,
                "click": click,
                "afterKeys": after,
                "prep": prep,
                "log": read["log"],
            }));
        }
    }

    // Focus with a child webview: click the child (if any), then the app page.
    #[allow(unused_mut)]
    let mut focus = vec![json!({ "step": "after typing", "state": focus_state(app, &main) })];
    if mode == "unstable-child" {
        let child_click = platform.click(&json!({ "x": 750.0, "y": 300.0 }));
        sleep(400);
        focus.push(json!({ "step": "after clicking the child webview", "click": child_click, "state": focus_state(app, &main) }));
        let prep = eval(&main, "__spike.prep('input')");
        let back = platform.click(&prep);
        sleep(400);
        focus.push(json!({ "step": "after clicking the app input again", "click": back, "state": focus_state(app, &main) }));
    }

    // A guest webview created while the user types (an ad mounting or being
    // recreated): does the keyboard focus stay in the app's input?
    #[cfg(feature = "unstable")]
    if mode == "unstable-child" {
        let prep = eval(&main, "__spike.prep('input')");
        let click = platform.click(&prep);
        sleep(150);
        for k in chars("ab") {
            platform.key(k);
            sleep(35);
        }
        let created = main
            .as_ref()
            .window()
            .add_child(
                tauri::webview::WebviewBuilder::new("guest2", WebviewUrl::App("child.html".into()))
                    .focused(child_focused()),
                tauri::LogicalPosition::new(600.0, 300.0),
                tauri::LogicalSize::new(300.0, 300.0),
            )
            .is_ok();
        sleep(800);
        for k in chars("cd") {
            platform.key(k);
            sleep(35);
        }
        sleep(400);
        let after = platform.after_keys();
        let read = eval(&main, "__spike.read('input')");
        let value = read["value"].as_str().unwrap_or_default().to_owned();
        results.push(json!({
            "target": "input", "case": "guest-created-while-typing", "expect": "abcd", "value": value,
            "ok": value == "abcd", "created": created, "childFocused": child_focused(), "click": click,
            "afterKeys": after, "log": read["log"],
        }));
    }

    // Windows: Alt-Tab away to a second app window and back (no click),
    // then keep typing into the input that had focus.
    #[cfg(windows)]
    if let Some(other) = app.get_webview_window("other") {
        let prep = eval(&main, "__spike.prep('input')");
        let click = platform.click(&prep);
        sleep(150);
        for k in chars("ab") {
            platform.key(k);
            sleep(35);
        }
        let away = platform.focus_window(&other);
        sleep(400);
        let state_away = focus_state(app, &main);
        let back = platform.focus_window(&main);
        sleep(400);
        let state_back = focus_state(app, &main);
        for k in chars("cd") {
            platform.key(k);
            sleep(35);
        }
        sleep(400);
        let read = eval(&main, "__spike.read('input')");
        let value = read["value"].as_str().unwrap_or_default().to_owned();
        if value != "abcd" {
            results.push(json!({ "target": "input", "case": "alt-tab-and-back", "ok": false, "value": value, "expect": "abcd" }));
        }
        focus.push(json!({
            "step": "alt-tab to a second window and back, no click, keep typing",
            "click": click, "away": away, "stateAway": state_away, "back": back, "stateBack": state_back,
            "value": value, "expect": "abcd", "ok": value == "abcd", "log": read["log"],
        }));
    }

    let failed: Vec<String> = results
        .iter()
        .filter(|r| r["ok"] != Value::Bool(true))
        .map(|r| format!("{}/{}: {:?}", r["target"].as_str().unwrap_or(""), r["case"].as_str().unwrap_or(""), r["value"].as_str().unwrap_or("")))
        .collect();
    json!({
        "mode": mode,
        "unstableFeature": cfg!(feature = "unstable"),
        "os": std::env::consts::OS,
        "tauri": tauri::VERSION,
        "mitigation": std::env::var("SPIKE_MITIGATE").unwrap_or_default(),
        "focusAtOpen": std::env::var("SPIKE_FOCUS_AT_OPEN").unwrap_or_default(),
        "childFocused": child_focused(),
        "environment": env,
        "summary": { "cases": results.len(), "failed": failed.len(), "failures": failed },
        "focus": focus,
        "results": results,
    })
}

/// Whether child webviews are built with `focused(true)` (Tauri's default;
/// `SPIKE_CHILD_FOCUSED=1`) or `focused(false)` (default here, as
/// tauri-plugin-overwolf builds its ad guests).
#[cfg_attr(not(feature = "unstable"), allow(dead_code))]
fn child_focused() -> bool {
    std::env::var("SPIKE_CHILD_FOCUSED").is_ok_and(|v| v == "1")
}

fn main() {
    let mode = std::env::var("SPIKE_MODE").unwrap_or_else(|_| "stable".into());
    let out = std::env::var("SPIKE_OUT").unwrap_or_else(|_| "spike-result.json".into());
    assert!(
        matches!(mode.as_str(), "stable" | "unstable-nochild" | "unstable-child"),
        "SPIKE_MODE must be stable, unstable-nochild or unstable-child"
    );
    assert!(
        cfg!(feature = "unstable") == (mode != "stable"),
        "SPIKE_MODE {mode} needs a build {} the unstable feature",
        if mode == "stable" { "without" } else { "with" }
    );

    #[cfg(target_os = "macos")]
    mac::hold_app_back();

    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.activate_ignoring_other_apps(false);
    let mode_setup = mode.clone();
    let builder = builder.setup(move |app| {
        let invisible = cfg!(target_os = "macos");
        let page = if std::env::var("SPIKE_MITIGATE").is_ok_and(|v| v == "js") { "index.html#mitigate-js" } else { "index.html" };
        let main = WebviewWindowBuilder::new(app, "main", WebviewUrl::App(page.into()))
            .title("unstable input spike")
            .inner_size(900.0, 600.0)
            .position(40.0, 40.0)
            .resizable(false)
            .visible(!invisible)
            .build()?;
        #[cfg(target_os = "macos")]
        mac::make_invisible(&main);
        #[cfg(feature = "unstable")]
        if mode_setup == "unstable-child" {
            main.as_ref().window().add_child(
                tauri::webview::WebviewBuilder::new("guest", WebviewUrl::App("child.html".into()))
                    .focused(child_focused()),
                tauri::LogicalPosition::new(600.0, 0.0),
                tauri::LogicalSize::new(300.0, 600.0),
            )?;
        }
        if cfg!(windows) {
            WebviewWindowBuilder::new(app, "other", WebviewUrl::App("child.html".into()))
                .title("spike other window")
                .inner_size(300.0, 200.0)
                .position(620.0, 420.0)
                .build()?;
        }
        if std::env::var("SPIKE_FOCUS_AT_OPEN").is_ok_and(|v| v == "1") {
            // Mitigation under test: give the app's webview the keyboard
            // focus inside its window (macOS: makeFirstResponder), as a
            // stable build does when it creates the window.
            main.as_ref().set_focus()?;
        }
        let handle = app.handle().clone();
        let main_for_driver = main.clone();
        let mode = mode_setup.clone();
        let out = out.clone();
        std::thread::spawn(move || {
            let result = run(&handle, main_for_driver, &mode);
            let text = serde_json::to_string_pretty(&result).unwrap_or_default();
            let _ = std::fs::write(&out, text);
            eprintln!("spike: wrote {out}: {}", result["summary"]);
            handle.exit(0);
        });
        Ok(())
    });
    #[allow(unused_mut)]
    let mut app = builder
        .build(tauri::generate_context!())
        .expect("build the spike app");
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    app.run(|_, _| {});
}
