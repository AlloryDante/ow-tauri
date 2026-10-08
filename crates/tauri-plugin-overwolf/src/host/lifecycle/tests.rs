//! Lifecycle on Tauri's mock runtime (DESIGN §4.2, W1 acceptance): the
//! burst starts at `RunEvent::Ready`, setup writes nothing, `ExitRequested`
//! is never prevented by the plugin, the exit drain runs once across
//! `RunEvent::Exit` and the restart sentinel, and is bounded.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::{App, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

use crate::analytics::{BoxFuture, HostRequest, HostResponse, Transport};
use crate::host::{Core, core_of};

/// Records every request and answers at once.
#[derive(Default)]
struct Capture(Mutex<Vec<HostRequest>>);

impl Transport for Capture {
    fn send(&self, request: HostRequest) -> BoxFuture<Result<HostResponse, String>> {
        self.0.lock().unwrap().push(request);
        Box::pin(async {
            Ok(HostResponse {
                status: 200,
                ..HostResponse::default()
            })
        })
    }
}

impl Capture {
    fn urls(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.url.clone())
            .collect()
    }
}

/// Never answers.
struct Hang;

impl Transport for Hang {
    fn send(&self, _request: HostRequest) -> BoxFuture<Result<HostResponse, String>> {
        Box::pin(std::future::pending())
    }
}

fn app_with(dir: &Path, transport: Arc<dyn Transport>) -> App<MockRuntime> {
    let mut context = mock_context(noop_assets());
    context.config_mut().plugins.0.insert(
        "overwolf".into(),
        serde_json::json!({
            "author": "Example Studio",
            "name": "Example App",
            "state": { "appDataDir": dir },
            "analytics": { "muidStrategy": "per-install" }
        }),
    );
    let mut builder = crate::Builder::new();
    let options = builder.options_mut();
    options.transport = Some(transport);
    options.os_queries = false;
    options.runtime_capabilities = false;
    options.argv = Some(vec!["app".into()]);
    let app = mock_builder()
        .plugin(builder.build())
        .build(context)
        .unwrap();
    WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
        .build()
        .unwrap();
    app
}

fn core(app: &App<MockRuntime>) -> Arc<Core<MockRuntime>> {
    core_of(app.handle()).expect("the plugin ran its setup")
}

fn is_empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir).unwrap().next().is_none()
}

#[test]
fn setup_writes_nothing_and_the_burst_starts_at_ready() {
    let dir = crate::state::test_dir("lifecycle-burst");
    let capture = Arc::new(Capture::default());
    let app = app_with(&dir, capture.clone());
    let core = core(&app);

    // After setup: nothing on disk, nothing sent, not started.
    assert!(is_empty_dir(&dir), "setup wrote to {}", dir.display());
    assert!(!core.lifecycle.is_started());
    assert!(!core.analytics.is_started());
    std::thread::sleep(Duration::from_millis(50));
    assert!(capture.urls().is_empty());

    let seen_at_ready = Arc::new(AtomicBool::new(false));
    let seen = seen_at_ready.clone();
    let c = core.clone();
    app.run(move |handle, event| {
        if let RunEvent::Ready = event {
            // Plugin hooks run before the app's callback.
            seen.store(
                c.lifecycle.is_started() && c.analytics.is_started(),
                Ordering::SeqCst,
            );
            handle
                .get_webview_window("main")
                .unwrap()
                .destroy()
                .unwrap();
        }
    });

    assert!(
        seen_at_ready.load(Ordering::SeqCst),
        "the burst was queued at Ready"
    );
    let urls = capture.urls();
    assert_eq!(urls.len(), 5, "first launch burst: {urls:?}");
    assert!(
        urls.iter()
            .all(|u| u.starts_with(crate::analytics::COUNTER_URL)
                || u.starts_with(crate::analytics::INSERT_STATS_URL))
    );
    // The first writes happened at Ready.
    assert!(core.identity.state_dir.ow_tauri_json().is_file());
    // Exit, then the sentinel at cleanup: one drain.
    assert_eq!(core.lifecycle.exit_count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exit_requested_is_never_prevented_and_a_tray_app_keeps_running() {
    let dir = crate::state::test_dir("lifecycle-tray");
    let app = app_with(&dir, Arc::new(Capture::default()));
    let core = core(&app);
    let requests = Arc::new(AtomicUsize::new(0));
    let after_prevented = Arc::new(AtomicUsize::new(0));
    let (r, a, c) = (requests.clone(), after_prevented.clone(), core.clone());
    app.run(move |handle, event| match event {
        RunEvent::Ready => {
            handle
                .get_webview_window("main")
                .unwrap()
                .destroy()
                .unwrap();
        }
        // A tray app: the last window closed, the app stays.
        RunEvent::ExitRequested { api, .. } if r.fetch_add(1, Ordering::SeqCst) == 0 => {
            api.prevent_exit();
            assert_eq!(c.lifecycle.exit_count(), 0);
            WebviewWindowBuilder::new(handle, "later", WebviewUrl::default())
                .build()
                .unwrap();
        }
        RunEvent::MainEventsCleared if r.load(Ordering::SeqCst) == 1 => {
            assert_eq!(
                c.lifecycle.exit_count(),
                0,
                "the plugin did not exit the app"
            );
            if a.fetch_add(1, Ordering::SeqCst) == 0 {
                handle
                    .get_webview_window("later")
                    .unwrap()
                    .destroy()
                    .unwrap();
            }
        }
        _ => {}
    });
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert!(
        after_prevented.load(Ordering::SeqCst) >= 1,
        "the app kept running"
    );
    assert_eq!(core.lifecycle.exit_count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_restart_sentinel_drains_once() {
    let dir = crate::state::test_dir("lifecycle-sentinel");
    let app = app_with(&dir, Arc::new(Capture::default()));
    let core = core(&app);
    assert_eq!(core.lifecycle.exit_count(), 0);
    // `AppHandle::restart()` skips `RunEvent::Exit` but clears the
    // resources before it restarts.
    app.handle().cleanup_before_exit();
    assert_eq!(core.lifecycle.exit_count(), 1);
    app.handle().cleanup_before_exit();
    crate::host::lifecycle::on_exit(&core);
    assert_eq!(core.lifecycle.exit_count(), 1);
    assert!(core.lifecycle.has_exited());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_exit_drain_is_bounded_with_a_hanging_transport() {
    let dir = crate::state::test_dir("lifecycle-hang");
    let app = app_with(&dir, Arc::new(Hang));
    let core = core(&app);
    let exit_requested_at = Arc::new(Mutex::new(None));
    let at = exit_requested_at.clone();
    app.run(move |handle, event| match event {
        RunEvent::Ready => handle
            .get_webview_window("main")
            .unwrap()
            .destroy()
            .unwrap(),
        RunEvent::ExitRequested { .. } => *at.lock().unwrap() = Some(Instant::now()),
        _ => {}
    });
    let took = exit_requested_at.lock().unwrap().unwrap().elapsed();
    let limit = crate::analytics::DRAIN_LIMIT;
    assert!(took >= limit, "waited for the drain: {took:?}");
    assert!(took < limit + Duration::from_secs(1), "bounded: {took:?}");
    assert_eq!(core.lifecycle.exit_count(), 1);
    assert!(
        core.analytics.dispatcher.in_flight() > 0,
        "requests were still hanging"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
