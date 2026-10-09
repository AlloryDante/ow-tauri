//! The update client commands (DESIGN §3.5, D18; Windows with the
//! `updater` feature (R6)). Without the feature every command answers
//! `unsupported`; with it, the engine answers `unsupported` off Windows.
//!
//! `updater_check` stores the found update as a resource of the calling
//! webview; the other commands take its `rid`. JavaScript cannot send
//! headers (SEC-M10), and its `channel` / `allowDowngrade` never allow a
//! downgrade unless `updater.allowJsDowngrade` is set (SEC-m4).
#![cfg_attr(
    not(feature = "updater"),
    allow(
        dead_code,
        reason = "without the updater feature the commands answer unsupported"
    )
)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{ResourceId, Runtime, State, Webview};

use super::{core, require_app_webview};
#[cfg(not(feature = "updater"))]
use crate::error::Error;
use crate::error::Result;
use crate::ext::Overwolf;

/// `CheckOptions` of `check()`. No headers from JavaScript (SEC-M10).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CheckOptions {
    /// The channel (`latest`, `beta`, ...).
    pub(crate) channel: Option<String>,
    /// Allow a lower version (only with `updater.allowJsDowngrade`, SEC-m4).
    pub(crate) allow_downgrade: Option<bool>,
    /// Allow pre-release versions.
    pub(crate) allow_prerelease: Option<bool>,
    /// Request timeout in milliseconds.
    pub(crate) timeout: Option<u64>,
}

/// What `updater_check` returns for an available update.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateMetadata {
    /// The `Update` resource.
    pub(crate) rid: ResourceId,
    /// The new version.
    pub(crate) version: String,
    /// The running version.
    pub(crate) current_version: String,
    /// The release date.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) date: Option<String>,
    /// The release notes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) body: Option<String>,
    /// The feed entry as received.
    pub(crate) raw: Value,
}

/// A download progress event (`tauri-plugin-updater`'s shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", content = "data")]
pub(crate) enum DownloadEvent {
    /// The download started.
    #[serde(rename_all = "camelCase")]
    Started {
        /// The body length, when known.
        content_length: Option<u64>,
    },
    /// A chunk arrived.
    #[serde(rename_all = "camelCase")]
    Progress {
        /// Its length.
        chunk_length: usize,
    },
    /// The download finished.
    Finished,
}

/// The longest `timeout` JavaScript may ask for (10 minutes).
const MAX_TIMEOUT_MS: u64 = 600_000;

#[cfg(not(feature = "updater"))]
fn unavailable() -> Error {
    Error::unsupported("the update client needs the `updater` cargo feature (Windows)")
}

#[cfg(feature = "updater")]
mod engine {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, PoisonError};
    use std::time::Duration;

    use tauri::ipc::Channel;
    use tauri::{Manager as _, Resource, ResourceId, Runtime, Webview};

    use super::{CheckOptions, DownloadEvent, MAX_TIMEOUT_MS, UpdateMetadata};
    use crate::config::UpdaterConfig;
    use crate::error::{Error, Result};
    use crate::updater::{DownloadedUpdate, Update, UpdaterBuilder};

    /// A found update, owned by the webview that checked.
    pub(super) struct UpdateResource {
        update: Update,
        downloaded: Mutex<Option<DownloadedUpdate>>,
    }

    impl Resource for UpdateResource {}

    /// JavaScript's `allowDowngrade` / `channel` over the configuration
    /// (SEC-m4): they may turn a downgrade off, and on only with
    /// `updater.allowJsDowngrade`.
    pub(super) fn js_downgrade(config: &UpdaterConfig, options: &CheckOptions) -> bool {
        let asked = options
            .allow_downgrade
            .or(options.channel.as_ref().map(|_| true));
        match asked {
            Some(false) => false,
            Some(true) if config.allow_js_downgrade => true,
            _ => config.allow_downgrade,
        }
    }

    pub(super) async fn check<R: Runtime>(
        webview: &Webview<R>,
        config: &UpdaterConfig,
        options: CheckOptions,
    ) -> Result<Option<UpdateMetadata>> {
        let mut builder = UpdaterBuilder::new(webview.app_handle().clone());
        if let Some(channel) = options.channel.clone() {
            builder = builder.channel(channel);
        }
        builder = builder.allow_downgrade(js_downgrade(config, &options));
        if let Some(pre) = options.allow_prerelease {
            builder = builder.allow_prerelease(pre);
        }
        if let Some(ms) = options.timeout {
            if ms == 0 || ms > MAX_TIMEOUT_MS {
                return Err(Error::invalid_argument(format!(
                    "timeout must be 1 to {MAX_TIMEOUT_MS} ms"
                )));
            }
            let t = Duration::from_millis(ms);
            builder = builder.connect_timeout(t).read_timeout(t);
        }
        let Some(update) = builder.build()?.check().await? else {
            return Ok(None);
        };
        let meta = UpdateMetadata {
            rid: 0,
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            date: update.date.clone(),
            body: update.body.clone(),
            raw: update.raw.clone(),
        };
        let rid = webview.resources_table().add(UpdateResource {
            update,
            downloaded: Mutex::new(None),
        });
        Ok(Some(UpdateMetadata { rid, ..meta }))
    }

    fn resource<R: Runtime>(
        webview: &Webview<R>,
        rid: ResourceId,
    ) -> Result<std::sync::Arc<UpdateResource>> {
        webview
            .resources_table()
            .get::<UpdateResource>(rid)
            .map_err(|_| Error::not_found(format!("no update resource {rid}")))
    }

    pub(super) async fn download<R: Runtime>(
        webview: &Webview<R>,
        rid: ResourceId,
        on_event: &Channel<DownloadEvent>,
    ) -> Result<()> {
        let res = resource(webview, rid)?;
        let started = AtomicBool::new(false);
        let downloaded = res
            .update
            .download(
                |chunk_length, content_length| {
                    if !started.swap(true, Ordering::SeqCst) {
                        let _ = on_event.send(DownloadEvent::Started { content_length });
                    }
                    let _ = on_event.send(DownloadEvent::Progress { chunk_length });
                },
                || {
                    if !started.swap(true, Ordering::SeqCst) {
                        let _ = on_event.send(DownloadEvent::Started {
                            content_length: Some(0),
                        });
                    }
                    let _ = on_event.send(DownloadEvent::Finished);
                },
            )
            .await?;
        *res.downloaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(downloaded);
        Ok(())
    }

    pub(super) fn install<R: Runtime>(webview: &Webview<R>, rid: ResourceId) -> Result<()> {
        let res = resource(webview, rid)?;
        let downloaded = res
            .downloaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(|| Error::not_found("download the update before installing it"))?;
        res.update.install(downloaded)?;
        let _ = webview.resources_table().close(rid);
        Ok(())
    }
}

/// `check(options)`.
#[tauri::command]
pub(crate) async fn updater_check<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    options: Option<CheckOptions>,
) -> Result<Option<UpdateMetadata>> {
    let core = core(&state);
    require_app_webview(core, &webview)?;
    #[cfg(feature = "updater")]
    {
        engine::check(
            &webview,
            &core.identity.config.updater,
            options.unwrap_or_default(),
        )
        .await
    }
    #[cfg(not(feature = "updater"))]
    {
        let _ = options;
        Err(unavailable())
    }
}

/// `Update.download(onEvent)`.
#[tauri::command]
pub(crate) async fn updater_download<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    rid: ResourceId,
    on_event: Channel<DownloadEvent>,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    #[cfg(feature = "updater")]
    {
        engine::download(&webview, rid, &on_event).await
    }
    #[cfg(not(feature = "updater"))]
    {
        let _ = (rid, on_event);
        Err(unavailable())
    }
}

/// `Update.install()`.
#[tauri::command]
pub(crate) async fn updater_install<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    rid: ResourceId,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    #[cfg(feature = "updater")]
    {
        engine::install(&webview, rid)
    }
    #[cfg(not(feature = "updater"))]
    {
        let _ = rid;
        Err(unavailable())
    }
}

/// `Update.downloadAndInstall(onEvent)`.
#[tauri::command]
pub(crate) async fn updater_download_and_install<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Overwolf<R>>,
    rid: ResourceId,
    on_event: Channel<DownloadEvent>,
) -> Result<()> {
    require_app_webview(core(&state), &webview)?;
    #[cfg(feature = "updater")]
    {
        engine::download(&webview, rid, &on_event).await?;
        engine::install(&webview, rid)
    }
    #[cfg(not(feature = "updater"))]
    {
        let _ = (rid, on_event);
        Err(unavailable())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_events_match_the_official_updater() {
        let json = |e: DownloadEvent| serde_json::to_value(e).unwrap();
        assert_eq!(
            json(DownloadEvent::Started {
                content_length: Some(3)
            }),
            serde_json::json!({ "event": "Started", "data": { "contentLength": 3 } })
        );
        assert_eq!(
            json(DownloadEvent::Progress { chunk_length: 2 }),
            serde_json::json!({ "event": "Progress", "data": { "chunkLength": 2 } })
        );
        assert_eq!(
            json(DownloadEvent::Finished),
            serde_json::json!({ "event": "Finished" })
        );
        // No headers from JavaScript (SEC-M10).
        assert!(
            serde_json::from_value::<CheckOptions>(serde_json::json!({ "headers": {} })).is_err()
        );
    }

    #[cfg(feature = "updater")]
    mod with_engine {
        use std::sync::{Arc, Mutex};

        use serde_json::{Value, json};
        use tauri::ipc::InvokeResponseBody;
        use tauri::test::MockRuntime;
        use tauri::{App, Manager as _};

        use super::super::engine::js_downgrade;
        use super::super::*;
        use crate::config::UpdaterConfig;
        use crate::error::ErrorCode;
        use crate::host::windows::tests::window;
        use crate::updater::engine::tests::{FakeOs, Feed, Signer, mock, start_feed};

        fn block<T>(f: impl Future<Output = T>) -> T {
            tauri::async_runtime::block_on(f)
        }

        fn state(app: &App<MockRuntime>) -> State<'_, Overwolf<MockRuntime>> {
            app.state::<Overwolf<MockRuntime>>()
        }

        fn webview(app: &App<MockRuntime>, label: &str) -> Webview<MockRuntime> {
            crate::compat::webview(app, label).unwrap()
        }

        /// A Channel that records each event's JSON.
        fn recorder() -> (Channel<DownloadEvent>, Arc<Mutex<Vec<Value>>>) {
            let events = Arc::new(Mutex::new(Vec::new()));
            let log = Arc::clone(&events);
            let channel = Channel::new(move |body| {
                if let InvokeResponseBody::Json(text) = body {
                    log.lock().unwrap().push(serde_json::from_str(&text).unwrap());
                }
                Ok(())
            });
            (channel, events)
        }

        fn setup(name: &str, feed: &Feed, updater: &Value) -> (App<MockRuntime>, Arc<FakeOs>) {
            let os = FakeOs::new(Signer::Subject("CN=Example Studio"));
            let (app, _dir, _shared) = mock(name, feed, updater, Arc::clone(&os));
            window(&app, "main");
            (app, os)
        }

        fn check(
            app: &App<MockRuntime>,
            options: Value,
        ) -> Result<Option<UpdateMetadata>> {
            block(updater_check(
                webview(app, "main"),
                state(app),
                Some(serde_json::from_value(options).unwrap()),
            ))
        }

        /// check → download (Started, Progress, Finished) → install: the
        /// installer starts with `/UPDATE` and the app exits.
        #[test]
        fn check_download_install_from_javascript() {
            let feed = start_feed("2.0.0", b"test");
            let (app, os) = setup(
                "cmd-updater-flow",
                &feed,
                &json!({ "publisherNames": ["Example Studio"] }),
            );
            let meta = check(&app, json!({})).unwrap().unwrap();
            assert_eq!(meta.version, "2.0.0");
            let wire = serde_json::to_value(&meta).unwrap();
            assert_eq!(wire["currentVersion"], json!(meta.current_version));
            assert_eq!(wire["raw"]["version"], json!("2.0.0"));
            assert_eq!(wire["body"], json!("fixes"));
            let (channel, events) = recorder();
            block(updater_download(
                webview(&app, "main"),
                state(&app),
                meta.rid,
                channel,
            ))
            .unwrap();
            assert_eq!(
                *events.lock().unwrap(),
                vec![
                    json!({ "event": "Started", "data": { "contentLength": 4 } }),
                    json!({ "event": "Progress", "data": { "chunkLength": 4 } }),
                    json!({ "event": "Finished" }),
                ]
            );
            block(updater_install(webview(&app, "main"), state(&app), meta.rid)).unwrap();
            let launches = os.launches();
            assert_eq!(launches.len(), 1);
            assert!(launches[0].0.args.iter().any(|a| a == "/UPDATE"));
            // The resource is gone after the install.
            let again = block(updater_install(webview(&app, "main"), state(&app), meta.rid));
            assert_eq!(again.unwrap_err().code(), ErrorCode::NotFound);
        }

        #[test]
        fn download_and_install_and_errors() {
            let feed = start_feed("2.0.0", b"test");
            let (app, os) = setup(
                "cmd-updater-dai",
                &feed,
                &json!({ "publisherNames": ["Example Studio"] }),
            );
            // Install before download, an unknown rid.
            let meta = check(&app, json!({ "timeout": 5000 })).unwrap().unwrap();
            let early = block(updater_install(webview(&app, "main"), state(&app), meta.rid));
            assert_eq!(early.unwrap_err().code(), ErrorCode::NotFound);
            let (channel, _) = recorder();
            let unknown = block(updater_download(
                webview(&app, "main"),
                state(&app),
                9_999,
                channel,
            ));
            assert_eq!(unknown.unwrap_err().code(), ErrorCode::NotFound);
            let (channel, events) = recorder();
            block(updater_download_and_install(
                webview(&app, "main"),
                state(&app),
                meta.rid,
                channel,
            ))
            .unwrap();
            assert_eq!(events.lock().unwrap().len(), 3);
            assert_eq!(os.launches().len(), 1);
            // Timeouts are bounded.
            for bad in [0, MAX_TIMEOUT_MS + 1] {
                let err = check(&app, json!({ "timeout": bad })).unwrap_err();
                assert_eq!(err.code(), ErrorCode::InvalidArgument);
            }
            // A plugin webview may not call the updater (DESIGN §4.5).
            window(&app, "ow-cmp-default");
            let refused = block(updater_check(
                webview(&app, "ow-cmp-default"),
                state(&app),
                None,
            ));
            assert_eq!(refused.unwrap_err().code(), ErrorCode::Forbidden);
        }

        /// SEC-m4: JavaScript's `channel` / `allowDowngrade` never cause a
        /// downgrade unless `updater.allowJsDowngrade`.
        #[test]
        fn js_downgrade_gate() {
            let options = |v: Value| serde_json::from_value::<CheckOptions>(v).unwrap();
            let config = |v: Value| serde_json::from_value::<UpdaterConfig>(v).unwrap();
            let off = config(json!({}));
            let on = config(json!({ "allowJsDowngrade": true }));
            let rust_on = config(json!({ "allowDowngrade": true }));
            assert!(!js_downgrade(&off, &options(json!({ "allowDowngrade": true }))));
            assert!(!js_downgrade(&off, &options(json!({ "channel": "beta" }))));
            assert!(js_downgrade(&on, &options(json!({ "channel": "beta" }))));
            assert!(js_downgrade(&on, &options(json!({ "allowDowngrade": true }))));
            assert!(!js_downgrade(&on, &options(json!({ "channel": "beta", "allowDowngrade": false }))));
            assert!(js_downgrade(&rust_on, &options(json!({}))));
            assert!(!js_downgrade(&rust_on, &options(json!({ "allowDowngrade": false }))));

            let feed = start_feed("0.0.1", b"test");
            let (app, _os) = setup(
                "cmd-updater-downgrade",
                &feed,
                &json!({ "publisherNames": ["Example Studio"] }),
            );
            assert!(check(&app, json!({ "channel": "beta", "allowDowngrade": true })).unwrap().is_none());
            assert!(
                feed.server.seen().last().unwrap().line.contains("/feed/beta.yml"),
                "the channel still applies"
            );
            let (app, _os) = setup(
                "cmd-updater-downgrade-on",
                &feed,
                &json!({ "publisherNames": ["Example Studio"], "allowJsDowngrade": true }),
            );
            let meta = check(&app, json!({ "channel": "beta" })).unwrap().unwrap();
            assert_eq!(meta.version, "0.0.1");
        }
    }

    /// R6: off Windows the commands answer `unsupported`.
    #[cfg(all(feature = "updater", not(windows)))]
    #[test]
    fn unsupported_off_windows() {
        use tauri::Manager as _;
        let (app, dir, _core) = crate::host::windows::tests::mock_app(
            "cmd-updater-r6",
            &serde_json::json!({ "updater": { "publisherNames": ["Example Studio"] } }),
            &[],
            crate::host::windows::tests::Capture::answering("{}"),
        );
        crate::host::windows::tests::window(&app, "main");
        let err = tauri::async_runtime::block_on(updater_check(
            crate::compat::webview(&app, "main").unwrap(),
            app.state::<Overwolf<tauri::test::MockRuntime>>(),
            None,
        ))
        .unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::Unsupported);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
