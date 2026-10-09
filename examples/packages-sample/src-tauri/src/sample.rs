//! The sample's native side: the plugins, the window and the restart in
//! TEST or LIVE mode. Everything Overwolf-specific (ads, consent, analytics,
//! identity, the Windows updater) is `tauri-plugin-overwolf`; the page calls
//! it directly through `tauri-plugin-overwolf-api`.

use std::ffi::OsString;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::webview::PageLoadEvent;
use tauri::window::Color;
use tauri::{
    AppHandle, Manager, RunEvent, Runtime, Webview, WebviewUrl, WebviewWindowBuilder, Window,
};

/// The label of the sample window and of its webview (the capability in
/// `capabilities/default.json` names this webview).
pub const MAIN: &str = "main";
/// The window size (it fits the ads tester's widest layout).
const SIZE: (f64, f64) = (1280.0, 860.0);
/// The window's minimum size.
const MIN_SIZE: (f64, f64) = (1000.0, 680.0);
/// The window background while the page loads (the page's dark theme).
const BACKGROUND: Color = Color(0x10, 0x10, 0x10, 0xff);
/// The switch that selects test ads (the plugin reads it).
const TEST_AD: &str = "--test-ad";
/// The switch that opens a page first (`--sample-page=<page>`).
const PAGE_SWITCH: &str = "--sample-page";
/// Why a restart is refused under `tauri dev` (the page comes from the
/// Tauri CLI's dev server, which exits with this process).
pub const DEV_RESTART_NOTICE: &str = "Restart needs a built app: run `npm start` (or `npm run start:test`). Under `tauri dev` the new process would lose the Tauri CLI's dev server and show no page.";

/// The arguments of a pending restart: set by [`sample_restart`], used at
/// `RunEvent::Exit` once the plugins have finished (analytics drained,
/// single-instance released).
#[derive(Default)]
pub struct Relaunch(Mutex<Option<Vec<OsString>>>);

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
        // Overwolf plugin writes nothing in the second process.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_window(MAIN) {
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
        });

    // The sample's one command, plus the lab driver's.
    #[cfg(not(feature = "lab"))]
    let builder = builder.invoke_handler(tauri::generate_handler![sample_restart]);
    #[cfg(feature = "lab")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        sample_restart,
        crate::lab::e2e_config,
        crate::lab::e2e_record,
        crate::lab::e2e_window,
        crate::lab::e2e_quit,
        crate::lab::e2e_still,
        crate::lab::e2e_reveal,
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

/// Builds the sample window, hidden until its page has loaded (no white
/// flash before the dark page). `--sample-page=<page>` (a restart keeps the
/// page this way) opens that page first.
fn create_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let title = app.package_info().name.clone();
    #[cfg(feature = "lab")]
    let size = crate::lab::window_size().unwrap_or(SIZE);
    #[cfg(not(feature = "lab"))]
    let size = SIZE;
    let mut builder = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(size.0, size.1)
        .min_inner_size(MIN_SIZE.0.min(size.0), MIN_SIZE.1.min(size.1))
        .background_color(BACKGROUND)
        .visible(false)
        .on_page_load(|webview, payload| {
            static SHOWN: AtomicBool = AtomicBool::new(false);
            if payload.event() == PageLoadEvent::Finished && !SHOWN.swap(true, Ordering::SeqCst) {
                show(&webview.as_ref().window());
                #[cfg(feature = "lab")]
                crate::lab::page_shown(webview.app_handle());
            }
        });
    if let Some(page) = start_page(std::env::args()) {
        builder = builder.initialization_script(start_page_script(&page));
    }
    let window = builder.build()?;
    #[cfg(feature = "lab")]
    crate::lab::prepare_window(&window.as_ref().window());
    #[cfg(not(feature = "lab"))]
    let _ = window;
    Ok(())
}

/// The page of `--sample-page=<page>` among `args`, if valid (the last one
/// wins).
fn start_page(args: impl IntoIterator<Item = String>) -> Option<String> {
    let prefix = format!("{PAGE_SWITCH}=");
    args.into_iter()
        .filter_map(|a| a.strip_prefix(&prefix).map(str::to_owned))
        .filter(|p| is_page(p))
        .last()
}

/// A page id is 1 to 20 of `a-z` (`PageId` in `src/nav.ts`; an unknown id
/// opens the start page).
fn is_page(text: &str) -> bool {
    (1..=20).contains(&text.len()) && text.chars().all(|c| c.is_ascii_lowercase())
}

/// The script that opens `page` (a valid page id) before the sample's page
/// runs (the page reads its route from the URL hash), only when the URL has
/// no route yet. It runs in the sample webview's main frame only.
fn start_page_script(page: &str) -> String {
    format!("if (!window.location.hash) {{ history.replaceState(null, '', '#{page}'); }}")
}

/// The page id in the hash of the sample webview's URL (`#settings`), if
/// valid.
fn page_of(url: &tauri::Url) -> Option<String> {
    url.fragment()
        .map(|f| f.trim_start_matches('/'))
        .filter(|f| is_page(f))
        .map(str::to_owned)
}

/// The arguments of the restarted app: the current ones without `--test-ad`
/// and `--sample-page`, `--test-ad` first in test mode, and the page last.
fn relaunch_args(
    current: impl IntoIterator<Item = OsString>,
    test: bool,
    page: Option<&str>,
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
    if let Some(page) = page {
        args.push(format!("{page_prefix}{page}").into());
    }
    args
}

/// Whether the page comes from a dev server (`tauri dev`, which also sets a
/// `devUrl`): a restarted process would lose it. A debug build without a dev
/// URL embeds its page and restarts fine.
fn under_dev_server(is_dev: bool, has_dev_url: bool) -> bool {
    is_dev && has_dev_url
}

/// Restarts the app with test ads (`mode` `test`) or live ads (`live`), on
/// the page the sample webview shows. The new process starts once this one
/// has exited (as ow-electron's `app.relaunch()` + `app.exit(0)`). Refused
/// when the page comes from `tauri dev`'s dev server
/// ([`DEV_RESTART_NOTICE`]).
///
/// # Errors
///
/// Called from another webview, an unknown mode, or the app runs under
/// `tauri dev`.
#[tauri::command]
pub fn sample_restart<R: Runtime>(webview: Webview<R>, mode: String) -> Result<(), String> {
    if webview.label() != MAIN {
        return Err("sample_restart is for the sample webview only".to_owned());
    }
    let test = match mode.as_str() {
        "test" => true,
        "live" => false,
        _ => return Err("mode must be test or live".to_owned()),
    };
    let app = webview.app_handle();
    if under_dev_server(tauri::is_dev(), app.config().build.dev_url.is_some()) {
        return Err(DEV_RESTART_NOTICE.to_owned());
    }
    let page = webview.url().ok().as_ref().and_then(page_of);
    let args = relaunch_args(std::env::args_os().skip(1), test, page.as_deref());
    if let Ok(mut pending) = app.state::<Relaunch>().0.lock() {
        *pending = Some(args);
    }
    app.exit(0);
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn relaunch_args_toggle_test_ads_and_keep_the_rest() {
        let current = os(&["--test-ad", "--foo", "--sample-page=ads", "bar"]);
        assert_eq!(
            relaunch_args(current.clone(), false, None),
            os(&["--foo", "bar"])
        );
        assert_eq!(
            relaunch_args(current, true, Some("settings")),
            os(&["--test-ad", "--foo", "bar", "--sample-page=settings"])
        );
        assert_eq!(
            relaunch_args(os(&["--sample-page"]), true, None),
            os(&["--test-ad"])
        );
    }

    #[test]
    fn page_ids_are_lowercase_words() {
        for ok in ["ads", "settings", "a", &"p".repeat(20)] {
            assert!(is_page(ok), "{ok}");
        }
        for bad in [
            "",
            "Ads",
            "ads/x",
            "a-b",
            "a b",
            "x'y",
            "<s>",
            &"p".repeat(21),
        ] {
            assert!(!is_page(bad), "{bad}");
        }
    }

    #[test]
    fn start_page_takes_the_last_valid_switch() {
        let args = ["app", "--sample-page=ads", "--sample-page=settings"].map(String::from);
        assert_eq!(start_page(args).as_deref(), Some("settings"));
        let bad = ["app", "--sample-page=<script>"].map(String::from);
        assert_eq!(start_page(bad), None);
        assert_eq!(start_page(["app".to_owned()]), None);
    }

    #[test]
    fn the_start_page_script_sets_only_an_empty_hash() {
        let script = start_page_script("settings");
        assert!(script.contains("if (!window.location.hash)"));
        assert!(script.contains("'#settings'"));
    }

    #[test]
    fn the_kept_page_is_the_url_hash() {
        let url = |s: &str| tauri::Url::parse(s).ok();
        assert_eq!(
            url("tauri://localhost/index.html#settings")
                .as_ref()
                .and_then(page_of),
            Some("settings".to_owned())
        );
        assert_eq!(
            url("http://tauri.localhost/#/ads")
                .as_ref()
                .and_then(page_of),
            Some("ads".to_owned())
        );
        assert_eq!(
            url("tauri://localhost/index.html")
                .as_ref()
                .and_then(page_of),
            None
        );
        assert_eq!(
            url("tauri://localhost/#a'b").as_ref().and_then(page_of),
            None
        );
    }

    #[test]
    fn restart_is_refused_only_under_a_dev_server() {
        assert!(under_dev_server(true, true));
        assert!(!under_dev_server(true, false));
        assert!(!under_dev_server(false, true));
        assert!(!under_dev_server(false, false));
    }
}
