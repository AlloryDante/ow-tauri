//! The showcase's native side: the window, the restart in TEST or LIVE, the
//! timeline exports and the window actions of the controls page. Everything
//! Overwolf-specific (ads, consent, identity) is `tauri-plugin-overwolf`;
//! the page calls it directly through `tauri-plugin-overwolf-api`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::webview::PageLoadEvent;
use tauri::window::Color;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, RunEvent, Runtime, WebviewUrl,
    WebviewWindowBuilder, Window, WindowEvent,
};
use tauri_plugin_overwolf::OverwolfExt;

/// The label of the showcase window and of its webview.
pub const MAIN: &str = "main";
/// The event the window's page listens to for window state changes
/// (`WindowEvent` in `src/shared/ipc.ts`).
pub const WINDOW_EVENT: &str = "showcase://window-event";
/// The switch that selects test ads (the plugin reads it too).
const TEST_AD: &str = "--test-ad";
/// The switch that opens a page first (`--showcase-page=<route>`).
const PAGE_SWITCH: &str = "--showcase-page";
/// The window size (it meets every ad format's documented minimum).
const SIZE: (f64, f64) = (1280.0, 860.0);
/// The window's minimum size.
const MIN_SIZE: (f64, f64) = (1100.0, 700.0);
/// How long the controls page's hide and minimize last.
const ACTION_SPAN: Duration = Duration::from_secs(3);
/// Why a restart is refused under `tauri dev` (LEAD-RULINGS R11).
pub const DEV_RESTART_NOTICE: &str = "Restart needs a built app: run `npm run start:tauri` (or `start:tauri:test`). Under `tauri dev` the new process would lose the Tauri CLI's dev server and show no page.";

/// The arguments of a pending restart: set by [`showcase_restart`], used at
/// `RunEvent::Exit` once the plugins have finished (analytics drained,
/// single-instance released).
#[derive(Default)]
pub struct Relaunch(Mutex<Option<Vec<OsString>>>);

/// What [`showcase_info`] answers (`HostInfo` in `src/shared/ipc.ts`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostInfo {
    /// Always `ow-tauri`.
    host: &'static str,
    /// The plugin version.
    host_version: &'static str,
    /// `Tauri <version>`.
    engine: String,
    /// `darwin`, `win32` or `linux` (Node's `process.platform` names).
    platform: &'static str,
    /// `test` or `live`.
    mode: &'static str,
    /// The app uid (the page masks it).
    uid: String,
    /// The formula uid.
    cuid: String,
    /// The machine id (the page masks it).
    muid: String,
    /// The rollout bucket.
    phase_percent: u8,
    /// The product name.
    product_name: String,
    /// The app version.
    app_version: String,
    /// Where exports go, with the home folder as `~`.
    exports_dir: String,
}

/// The paths the page needs to keep the home folder out of what it shows.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Paths {
    /// The home folder.
    home: String,
}

/// A parity report read from the app data folder.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParityText {
    /// The file, with the home folder as `~`.
    path: String,
    /// Its text, or `None` when it does not exist.
    text: Option<String>,
}

/// Starts the app.
///
/// # Errors
///
/// Tauri could not build or run the app.
pub fn run() -> tauri::Result<()> {
    #[cfg(feature = "lab")]
    crate::lab::hold_app_back();
    let builder = tauri::Builder::default()
        // Single instance first: a second launch focuses this app and the
        // plugin writes nothing in the second process (docs/INTEROP.md).
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = find_window(app, MAIN) {
                let _ = window.unminimize();
                show(&window);
                let _ = window.set_focus();
            }
        }))
        // Log plugin before the Overwolf plugin, so its setup messages
        // reach it.
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(overwolf())
        .manage(Relaunch::default())
        .setup(|app| {
            create_window(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == MAIN
                && let WindowEvent::Resized(_) = event
            {
                report(window, "resize", Some(bounds(window)));
            }
        });

    #[cfg(not(feature = "lab"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        showcase_info,
        showcase_paths,
        showcase_write_export,
        showcase_read_parity,
        showcase_restart,
        showcase_window_action,
    ]);
    #[cfg(feature = "lab")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        showcase_info,
        showcase_paths,
        showcase_write_export,
        showcase_read_parity,
        showcase_restart,
        showcase_window_action,
        crate::lab::e2e_config,
        crate::lab::e2e_record,
        crate::lab::e2e_windows,
        crate::lab::e2e_quit,
        crate::lab::e2e_native_probe,
        crate::lab::e2e_still,
    ]);
    // An invisible lab app is never activated (it would take the keyboard
    // from the app the user is typing in).
    #[cfg(all(feature = "lab", target_os = "macos"))]
    let builder = builder.activate_ignoring_other_apps(!crate::lab::invisible());

    // macOS: lets the plugin recover crashed ad guests and report them.
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(
        tauri_plugin_overwolf::web_content_process_terminate_hook(),
    );

    #[allow(unused_mut, reason = "mutated by the macOS lab only")]
    let mut app = builder.build(tauri::generate_context!())?;
    // The lab keeps the app out of the Dock and the app switcher.
    #[cfg(all(feature = "lab", target_os = "macos"))]
    if crate::lab::invisible() {
        app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
    app.run(|app, event| {
        if let RunEvent::Exit = event {
            relaunch_if_pending(app);
        }
    });
    Ok(())
}

/// The Overwolf plugin: its configuration is `plugins.overwolf` in
/// `tauri.conf.json`; the lab adds loopback endpoints.
fn overwolf<R: Runtime>() -> tauri::plugin::TauriPlugin<R, Option<tauri_plugin_overwolf::Config>> {
    let builder = tauri_plugin_overwolf::Builder::new();
    #[cfg(feature = "lab")]
    let builder = crate::lab::overwolf_builder(builder);
    builder.build()
}

/// Builds the showcase window, hidden until its page has loaded.
fn create_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let title = app.package_info().name.clone();
    let mut builder =
        WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("renderer/index.html".into()))
            .title(title)
            .inner_size(SIZE.0, SIZE.1)
            .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
            .background_color(Color(0x0e, 0x10, 0x14, 0xff))
            .visible(false)
            .on_page_load(|webview, payload| {
                static SHOWN: AtomicBool = AtomicBool::new(false);
                if payload.event() == PageLoadEvent::Finished && !SHOWN.swap(true, Ordering::SeqCst)
                {
                    show(&webview.as_ref().window());
                }
            });
    // `--showcase-page=<route>` opens that page first, without mounting
    // page 1 (the page reads its route from the URL hash).
    if let Some(route) = start_route(std::env::args()) {
        builder = builder.initialization_script(format!(
            "if (window.location.pathname.endsWith('/renderer/index.html') && !window.location.hash) {{ history.replaceState(null, '', '#{route}'); }}"
        ));
    }
    let window = builder.build()?;
    #[cfg(feature = "lab")]
    crate::lab::prepare_window(&window.as_ref().window());
    #[cfg(not(feature = "lab"))]
    let _ = window;
    Ok(())
}

/// Shows a window (in the invisible lab: on screen at alpha 0, without
/// becoming key or activating the app).
fn show<R: Runtime>(window: &Window<R>) {
    #[cfg(feature = "lab")]
    if crate::lab::invisible() {
        crate::lab::order_front(window);
        return;
    }
    let _ = window.show();
}

/// The page route of `--showcase-page=<route>` among `args`, if valid.
fn start_route(args: impl IntoIterator<Item = String>) -> Option<String> {
    let prefix = format!("{PAGE_SWITCH}=");
    args.into_iter()
        .filter_map(|a| a.strip_prefix(&prefix).map(str::to_owned))
        .filter(|r| is_route(r))
        .last()
}

/// A route is `page` or `page/arg`, each 1 to 40 of `a-z 0-9 _ -`,
/// starting with a letter or digit (`parseRoute` in `src/shared/route.ts`).
fn is_route(text: &str) -> bool {
    let part = |p: &str| {
        let mut chars = p.chars();
        p.len() <= 40
            && chars
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    };
    let mut parts = text.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(page), None, None) => part(page),
        (Some(page), Some(arg), None) => part(page) && part(arg),
        _ => false,
    }
}

/// The arguments of the restarted app: the current ones without `--test-ad`
/// and `--showcase-page`, `--test-ad` first in test mode, and the page last
/// (`relaunchArgs` + `withPageSwitch` of the ow-electron twin).
fn relaunch_args(
    current: impl IntoIterator<Item = OsString>,
    test: bool,
    route: Option<&str>,
) -> Vec<OsString> {
    let page_prefix = format!("{PAGE_SWITCH}=");
    let mut args: Vec<OsString> = current
        .into_iter()
        .filter(|a| {
            a.to_str().is_none_or(|s| {
                s != TEST_AD && s != PAGE_SWITCH && !s.starts_with(page_prefix.as_str())
            })
        })
        .collect();
    if test {
        args.insert(0, TEST_AD.into());
    }
    if let Some(route) = route {
        args.push(format!("{page_prefix}{route}").into());
    }
    args
}

/// Whether the page comes from a dev server (`tauri dev`, which also sets a
/// `devUrl` for its built-in server): a restarted process would lose it. A
/// debug build without a dev URL embeds its page and restarts fine.
fn under_dev_server(is_dev: bool, has_dev_url: bool) -> bool {
    is_dev && has_dev_url
}

/// Starts the new process of a pending restart (at `RunEvent::Exit`).
fn relaunch_if_pending<R: Runtime>(app: &AppHandle<R>) {
    let pending = app
        .try_state::<Relaunch>()
        .and_then(|r| r.0.lock().ok().and_then(|mut p| p.take()));
    let Some(args) = pending else { return };
    // Release the single-instance lock first, or the new process would hand
    // its arguments to this one and quit.
    tauri_plugin_single_instance::destroy(app);
    match tauri::process::current_binary(&app.env()) {
        Ok(binary) => match std::process::Command::new(binary).args(args).spawn() {
            Ok(_) => log::info!("relaunching"),
            Err(error) => log::error!("relaunch failed: {error}"),
        },
        Err(error) => log::error!("relaunch failed: {error}"),
    }
}

/// The home folder written as `~` at the start of `path`.
fn shown(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// Node's `process.platform` name of this OS.
fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// The exports folder: `<app data>/exports`.
fn exports_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("exports"))
}

/// Host, mode and identity for the top bar and the identity page.
///
/// # Errors
///
/// The app data folder cannot be resolved.
#[tauri::command]
pub fn showcase_info<R: Runtime>(app: AppHandle<R>) -> Result<HostInfo, String> {
    let overwolf = app.overwolf();
    let home = app.path().home_dir().ok();
    Ok(HostInfo {
        host: "ow-tauri",
        host_version: tauri_plugin_overwolf::VERSION,
        engine: format!("Tauri {}", tauri::VERSION),
        platform: platform(),
        mode: if overwolf.is_test_ad() {
            "test"
        } else {
            "live"
        },
        uid: overwolf.uid().to_owned(),
        cuid: overwolf.cuid().to_owned(),
        muid: overwolf.muid().to_owned(),
        phase_percent: overwolf.phase_percent(),
        product_name: app.package_info().name.clone(),
        app_version: app.package_info().version.to_string(),
        exports_dir: shown(&exports_dir(&app)?, home.as_deref()),
    })
}

/// The home folder, so the page can keep it out of what it shows.
///
/// # Errors
///
/// The home folder cannot be resolved.
#[tauri::command]
pub fn showcase_paths<R: Runtime>(app: AppHandle<R>) -> Result<Paths, String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    Ok(Paths {
        home: home.display().to_string(),
    })
}

/// Whether `name` is a timeline export file name
/// (`timeline-<host>-<mode>-<time>.json`, no folders).
fn is_export_name(name: &str) -> bool {
    name.len() <= 120
        && name.starts_with("timeline-")
        && name
            .strip_suffix(".json")
            .is_some_and(|stem| stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

/// Writes a timeline export (`<app data>/exports/<name>`); the page has
/// already replaced the home folder in `json`. Answers the written path
/// with the home folder as `~`.
///
/// # Errors
///
/// `name` is not an export file name, or the file cannot be written.
#[tauri::command]
pub fn showcase_write_export<R: Runtime>(
    app: AppHandle<R>,
    name: String,
    json: String,
) -> Result<String, String> {
    if !is_export_name(&name) {
        return Err("not an export file name".to_owned());
    }
    let dir = exports_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(shown(&path, app.path().home_dir().ok().as_deref()))
}

/// Reads `<app data>/parity-report.json` (a `tools/parity-harness`
/// `parity-diff.json` placed there by hand), if it exists.
///
/// # Errors
///
/// The file exists but cannot be read.
#[tauri::command]
pub fn showcase_read_parity<R: Runtime>(app: AppHandle<R>) -> Result<ParityText, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("parity-report.json");
    let shown_path = shown(&path, app.path().home_dir().ok().as_deref());
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(ParityText {
            path: shown_path,
            text: Some(text),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ParityText {
            path: shown_path,
            text: None,
        }),
        Err(error) => Err(error.to_string()),
    }
}

/// Restarts the app in `mode` (`test` or `live`), on `route` when given.
/// The new process starts once this one has exited. Refused when the page
/// comes from `tauri dev`'s dev server ([`DEV_RESTART_NOTICE`]).
///
/// # Errors
///
/// An unknown mode or route, or the app runs under `tauri dev`.
#[tauri::command]
pub fn showcase_restart<R: Runtime>(
    app: AppHandle<R>,
    mode: String,
    route: Option<String>,
) -> Result<(), String> {
    let test = match mode.as_str() {
        "test" => true,
        "live" => false,
        _ => return Err("mode must be test or live".to_owned()),
    };
    if route.as_deref().is_some_and(|r| !is_route(r)) {
        return Err("not a page route".to_owned());
    }
    if under_dev_server(tauri::is_dev(), app.config().build.dev_url.is_some()) {
        return Err(DEV_RESTART_NOTICE.to_owned());
    }
    let args = relaunch_args(std::env::args_os().skip(1), test, route.as_deref());
    if let Ok(mut pending) = app.state::<Relaunch>().0.lock() {
        *pending = Some(args);
    }
    app.exit(0);
    Ok(())
}

/// A window state change for the page.
fn report<R: Runtime>(window: &Window<R>, name: &str, detail: Option<serde_json::Value>) {
    let mut event = serde_json::json!({ "name": name });
    if let Some(detail) = detail {
        event["detail"] = detail;
    }
    let _ = window.emit_to(MAIN, WINDOW_EVENT, event);
}

/// The window's outer bounds in logical pixels.
fn bounds<R: Runtime>(window: &Window<R>) -> serde_json::Value {
    let scale = window.scale_factor().unwrap_or(1.0);
    let position = window
        .outer_position()
        .map_or(LogicalPosition::new(0.0, 0.0), |p| {
            p.to_logical::<f64>(scale)
        });
    let size = window
        .outer_size()
        .map_or(LogicalSize::new(0.0, 0.0), |s| s.to_logical::<f64>(scale));
    serde_json::json!({
        "x": position.x.round(), "y": position.y.round(),
        "width": size.width.round(), "height": size.height.round(),
    })
}

/// The window `label`. `get_window`, not `get_webview_window`: a window
/// that hosts an ad has several webviews (docs/GETTING-STARTED.md, "Living
/// with `unstable`"). Linux has no ads and no Tauri `unstable` API; there
/// every window keeps its one webview.
fn find_window<R: Runtime>(app: &AppHandle<R>, label: &str) -> Option<Window<R>> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        app.get_window(label)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        app.get_webview_window(label).map(|w| w.as_ref().window())
    }
}

/// Runs `then` on the window after [`ACTION_SPAN`], if it still exists.
fn later<R: Runtime>(window: &Window<R>, then: impl FnOnce(&Window<R>) + Send + 'static) {
    let app = window.app_handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(ACTION_SPAN);
        let main = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = find_window(&main, MAIN) {
                then(&window);
            }
        });
    });
}

/// The controls and interstitial pages' window actions: hide or minimize
/// for 3 s, shrink below the interstitial's minimum, restore the size.
///
/// # Errors
///
/// An unknown action, or no showcase window.
#[tauri::command]
pub fn showcase_window_action<R: Runtime>(app: AppHandle<R>, action: String) -> Result<(), String> {
    let window = find_window(&app, MAIN).ok_or("no showcase window")?;
    let failed = |e: tauri::Error| e.to_string();
    match action.as_str() {
        "hide-3s" => {
            window.hide().map_err(failed)?;
            report(&window, "hide", None);
            later(&window, |w| {
                show(w);
                report(w, "show", None);
            });
        }
        "minimize-3s" => {
            window.minimize().map_err(failed)?;
            report(&window, "minimize", None);
            later(&window, |w| {
                let _ = w.unminimize();
                report(w, "restore", None);
            });
        }
        "shrink-900x500" => {
            // Below the interstitial's minimum: the ad page answers with
            // performance_ad_error, then shutdown.
            window
                .set_min_size(Some(LogicalSize::new(800.0, 450.0)))
                .map_err(failed)?;
            window
                .set_size(LogicalSize::new(900.0, 500.0))
                .map_err(failed)?;
        }
        "restore-size" => {
            window
                .set_size(LogicalSize::new(SIZE.0, SIZE.1))
                .map_err(failed)?;
            window
                .set_min_size(Some(LogicalSize::new(MIN_SIZE.0, MIN_SIZE.1)))
                .map_err(failed)?;
        }
        _ => return Err("unknown window action".to_owned()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn relaunch_args_toggle_test_ads_and_keep_the_rest() {
        let current = os(&["--test-ad", "--foo", "--showcase-page=sizes", "bar"]);
        assert_eq!(
            relaunch_args(current.clone(), false, None),
            os(&["--foo", "bar"])
        );
        assert_eq!(
            relaunch_args(current, true, Some("layouts/tower")),
            os(&["--test-ad", "--foo", "bar", "--showcase-page=layouts/tower"])
        );
        assert_eq!(
            relaunch_args(os(&["--showcase-page"]), true, None),
            os(&["--test-ad"])
        );
    }

    #[test]
    fn routes_follow_the_page_rules() {
        for ok in ["sizes", "layouts/tower", "sizes/300x250", "a", "x_y-1/z"] {
            assert!(is_route(ok), "{ok}");
        }
        for bad in [
            "",
            "Sizes",
            "-a",
            "a/b/c",
            "a/",
            "a b",
            "a#b",
            "x'y",
            &"a".repeat(41),
        ] {
            assert!(!is_route(bad), "{bad}");
        }
    }

    #[test]
    fn start_route_takes_the_last_valid_switch() {
        let args = ["app", "--showcase-page=sizes", "--showcase-page=reward"].map(String::from);
        assert_eq!(start_route(args).as_deref(), Some("reward"));
        let bad = ["app", "--showcase-page=<script>"].map(String::from);
        assert_eq!(start_route(bad), None);
        assert_eq!(start_route(["app".to_owned()]), None);
    }

    #[test]
    fn export_names_are_plain_files() {
        assert!(is_export_name(
            "timeline-ow-tauri-test-2026-10-09T10-00-00-000Z.json"
        ));
        for bad in [
            "timeline-../x.json",
            "../timeline-a.json",
            "timeline-a.txt",
            "timeline-a/b.json",
            "x.json",
        ] {
            assert!(!is_export_name(bad), "{bad}");
        }
    }

    #[test]
    fn paths_inside_home_are_shown_with_a_tilde() {
        let home = Path::new("/home/me");
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            shown(Path::new("/home/me/a/b"), Some(home)),
            format!("~{sep}a{}b", '/')
        );
        assert_eq!(shown(Path::new("/home/me"), Some(home)), "~");
        assert_eq!(shown(Path::new("/opt/x"), Some(home)), "/opt/x");
        assert_eq!(shown(Path::new("/home/me/x"), None), "/home/me/x");
    }

    #[test]
    fn restart_is_refused_only_under_a_dev_server() {
        assert!(under_dev_server(true, true));
        assert!(!under_dev_server(true, false));
        assert!(!under_dev_server(false, true));
        assert!(!under_dev_server(false, false));
    }

    #[test]
    fn platform_uses_node_names() {
        let expected = if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(windows) {
            "win32"
        } else {
            std::env::consts::OS
        };
        assert_eq!(platform(), expected);
    }
}
