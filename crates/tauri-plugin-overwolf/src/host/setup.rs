//! Building the [`Host`] in the plugin's setup hook (ARCHITECTURE 3.2).

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::{AppHandle, Manager, Runtime};
use url::Url;

use super::{Core, Host, Info};
use crate::config::{
    Config, EnvOverrides, MuidStrategy, Switches, apply_overrides, browser_args,
    filter_pending_browser_args,
};
use crate::fs_scope::{FsScope, TemplateDirs, expand_template};
use crate::identity::{muid_from_bytes, phase_percent, resolve_uid};
use crate::ipc::router::Router;
use crate::lifecycle::QuitSequence;
use crate::manifest::EmbeddedManifest;
use crate::packages::{PackagesBackend, PackagesSnapshot, logs_folder_path};
use crate::paths::{BaseDirs, TargetOs, electron_paths, node_arch, user_data_dir};
use crate::platform::machine::{MachineIds, machine_ids};
use crate::screen::ElectronDisplay;
use crate::snapshot::{
    Flags, HostSnapshot, IdentityInfo, IpcLimits, StateHub, SwitchesInfo, Versions,
};
use crate::state::StateDir;
use crate::state::log::{LogLevel, Logger};
use crate::state::ow_electron::{FileStatus, OwElectronFile};
use crate::state::ow_tauri::OwTauriFile;
use crate::window::WindowRegistry;

/// What the [`Builder`](crate::Builder) passes to setup.
#[derive(Clone, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent builder switches"
)]
pub(crate) struct SetupOptions {
    pub(crate) manifest_json: Option<&'static str>,
    pub(crate) packages_backend: Option<PackagesBackend>,
    pub(crate) host_label: Option<(String, Option<String>)>,
    pub(crate) transport: Option<Arc<dyn crate::analytics::Transport>>,
    pub(crate) test_ad: Option<bool>,
    pub(crate) uid: Option<String>,
    pub(crate) companion_plugins: bool,
    pub(crate) runtime_capabilities: bool,
    pub(crate) main_webview: bool,
    pub(crate) argv: Option<Vec<String>>,
    /// Query monitors and the cursor (off on a mock runtime, which has none).
    pub(crate) os_queries: bool,
    /// Run the updater's OS steps: the Authenticode and code-signature
    /// checks, the macOS unpack and the installer. Only
    /// `Builder::skip_updater_os_steps` (feature `test-util`) turns it off.
    pub(crate) updater_os_steps: bool,
    /// The embedded `dev-app-update.yml` (I.1 `forceDevUpdateConfig`).
    pub(crate) dev_app_update: Option<&'static str>,
}

impl std::fmt::Debug for SetupOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetupOptions")
            .field("packages_backend", &self.packages_backend)
            .field("host_label", &self.host_label)
            .field("transport", &self.transport.is_some())
            .field("test_ad", &self.test_ad)
            .field("companion_plugins", &self.companion_plugins)
            .field("runtime_capabilities", &self.runtime_capabilities)
            .field("main_webview", &self.main_webview)
            .field("os_queries", &self.os_queries)
            .field("updater_os_steps", &self.updater_os_steps)
            .finish_non_exhaustive()
    }
}

/// The origin app assets are served from in the webviews the plugin creates.
pub(crate) fn app_origin<R: Runtime>(app: &AppHandle<R>) -> Url {
    if tauri::is_dev()
        && let Some(dev) = &app.config().build.dev_url
    {
        let mut origin = dev.clone();
        origin.set_path("/");
        origin.set_query(None);
        origin.set_fragment(None);
        return origin;
    }
    let text = if cfg!(any(windows, target_os = "android")) {
        "http://tauri.localhost/"
    } else {
        "tauri://localhost/"
    };
    Url::parse(text)
        .unwrap_or_else(|_| Url::parse("tauri://localhost/").unwrap_or_else(|_| unreachable!()))
}

fn base_dirs<R: Runtime>(app: &AppHandle<R>, app_data: std::path::PathBuf) -> BaseDirs {
    let p = app.path();
    let exe = std::env::current_exe().unwrap_or_default();
    // Tauri cannot name a resource directory in some layouts (tests, some
    // Linux installs); the executable's directory stands in for it.
    let resources = p
        .resource_dir()
        .ok()
        .or_else(|| exe.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_default();
    BaseDirs {
        app_data,
        home: p.home_dir().unwrap_or_default(),
        temp: std::env::temp_dir(),
        desktop: p.desktop_dir().unwrap_or_default(),
        documents: p.document_dir().unwrap_or_default(),
        downloads: p.download_dir().unwrap_or_default(),
        music: p.audio_dir().unwrap_or_default(),
        pictures: p.picture_dir().unwrap_or_default(),
        videos: p.video_dir().unwrap_or_default(),
        exe,
        resources,
    }
}

fn random_muid() -> String {
    muid_from_bytes(*uuid::Uuid::new_v4().as_bytes())
}

/// Builds and registers the host; called from the plugin's setup hook.
#[expect(
    clippy::too_many_lines,
    reason = "one linear setup sequence, easier to audit in one place"
)]
pub(crate) fn setup<R: Runtime>(
    app: &AppHandle<R>,
    mut config: Config,
    options: SetupOptions,
) -> Result<Arc<Host<R>>, Box<dyn std::error::Error>> {
    let debug = cfg!(debug_assertions);
    let argv: Vec<String> = options
        .argv
        .clone()
        .unwrap_or_else(|| std::env::args().collect());
    let switches = Switches::parse(&argv);
    let mut warnings = Vec::new();
    if let Some(uid) = &options.uid {
        config.uid = Some(uid.clone());
    }
    if let Some(b) = options.packages_backend {
        config.packages_backend = b;
    }
    if let Some(t) = options.test_ad {
        config.ads.test_ad = t;
    }
    if let Some((label, version)) = &options.host_label {
        config.analytics.host_label.clone_from(label);
        config.analytics.host_version.clone_from(version);
    }
    let (env, env_warnings) = EnvOverrides::read(|k| std::env::var(k).ok(), debug);
    warnings.extend(env_warnings);
    warnings.extend(apply_overrides(&mut config, &env, &switches));
    config.validate()?;

    let manifest_json = options
        .manifest_json
        .ok_or("tauri-plugin-overwolf: Builder::manifest_json(embedded_manifest!()) is required")?;
    let manifest = EmbeddedManifest::from_embedded_json(manifest_json)?;
    let identity = resolve_uid(config.uid.as_deref(), &manifest);

    let app_data = match &config.state.app_data_dir {
        Some(dir) => dir.clone(),
        None => app.path().config_dir()?,
    };
    let state_dir = StateDir::new(&app_data, &identity.uid);
    let ads_data_dir = app_data.join(&manifest.product_name).join("EBWebView-ow");
    let logger = if config.logging.enabled {
        Logger::open(state_dir.log_file())
    } else {
        Logger::disabled(state_dir.log_file())
    };
    logger.write(
        LogLevel::Info,
        &format!(
            "ow-tauri {} session start - app '{}' {} - uid {} - pid {}",
            crate::VERSION,
            manifest.product_name,
            manifest.version,
            identity.uid,
            std::process::id()
        ),
    );

    let ow_tauri = OwTauriFile::load(state_dir.ow_tauri_json());
    if ow_tauri.corrupt_at_load {
        warnings.push(match &ow_tauri.corrupt_backup {
            Some(backup) => format!(
                "ow-tauri.json was not valid JSON; starting from defaults (the old file is {})",
                backup
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
            None => "ow-tauri.json could not be read; starting from defaults".into(),
        });
    }
    let stored = ow_tauri.get();
    let machine = match config.analytics.muid_strategy {
        MuidStrategy::MachineId if options.os_queries => match machine_ids() {
            Ok(ids) => Some(ids),
            Err(reason) => {
                warnings.push(format!(
                    "machine id unavailable ({reason}); using a per-install muid"
                ));
                None
            }
        },
        _ => None,
    };
    let MachineIds { muid, muid_v2 } = if let Some(ids) = machine {
        ids
    } else if let Some(m) = stored
        .muid
        .clone()
        .filter(|m| crate::identity::is_valid_muid(m))
    {
        MachineIds {
            muid_v2: m.clone(),
            muid: m,
        }
    } else {
        let m = random_muid();
        let value = m.clone();
        if let Err(err) = ow_tauri.update(|s| s.muid = Some(value)) {
            warnings.push(format!("could not write ow-tauri.json: {}", err.kind()));
        }
        MachineIds {
            muid_v2: m.clone(),
            muid: m,
        }
    };
    let phase = phase_percent(&muid);

    let ow_electron = OwElectronFile::new(state_dir.ow_electron_json());
    let shared = ow_electron.read();
    if shared.status == FileStatus::Invalid {
        warnings.push("ow-electron.json is not valid JSON; it is left untouched".into());
    }
    let utm_params = shared.state.utm_params.clone();
    // Ad guests carry the consent stored at launch (D.2).
    let launch_consent = shared
        .state
        .cmp
        .and_then(|c| c.unified_consent_string)
        .unwrap_or_default();

    let os = TargetOs::current();
    let base = base_dirs(app, app_data.clone());
    let paths = electron_paths(&base, &manifest.product_name, os);
    let user_data = user_data_dir(&app_data, &manifest.product_name);
    let template_dirs = TemplateDirs {
        user_data: user_data.clone(),
        pictures: base.pictures.clone(),
        videos: base.videos.clone(),
        documents: base.documents.clone(),
        downloads: base.downloads.clone(),
        temp: base.temp.clone(),
        app_name: manifest.product_name.clone(),
    };
    let extra: Vec<_> = config
        .fs
        .scope
        .iter()
        .filter_map(|t| {
            let expanded = expand_template(t, &template_dirs);
            if expanded.is_none() {
                warnings.push(format!(
                    "fs.scope entry {t:?} has no directory on this system; ignored"
                ));
            }
            expanded
        })
        .collect();
    let fs_scope = FsScope::new(
        user_data.clone(),
        state_dir.root().to_path_buf(),
        &crate::paths::app_path(&base.resources),
        extra,
    );

    let pending = filter_pending_browser_args(&stored.pending_browser_args);
    let webview_args = browser_args(&config.webview, &pending, debug);
    if os != TargetOs::Windows
        && (config.webview.disable_gpu
            || config.webview.remote_debugging_port.is_some()
            || !config.webview.additional_browser_args.is_empty()
            || !pending.is_empty())
    {
        warnings.push(
            "webview browser arguments only apply on Windows; ignored on this platform".into(),
        );
    }

    if config.packages_backend == PackagesBackend::Native {
        warnings.push(
            "packagesBackend \"native\" is reserved: no package runtime exists, so it behaves as \"none\""
                .into(),
        );
    }
    if env.package_runtime.is_some() {
        warnings.push("OW_TAURI_PACKAGE_RUNTIME is reserved (Appendix P); ignored".into());
    }
    let packages = PackagesSnapshot::new(
        config.packages_backend,
        &manifest.overwolf.packages,
        logs_folder_path(&user_data.to_string_lossy(), &identity.uid),
        phase,
    );

    let locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".to_owned());
    let (displays, primary) = super::main_webview::read_displays(app, options.os_queries);
    let snapshot = HostSnapshot {
        seq: 0,
        versions: Versions {
            ow_tauri: crate::VERSION.to_owned(),
            tauri: tauri::VERSION.to_owned(),
            app: manifest.version.clone(),
            webview: tauri::webview_version().unwrap_or_default(),
            os: crate::platform::os_release(),
        },
        manifest: manifest.clone(),
        identity: IdentityInfo {
            uid: identity.uid.clone(),
            cuid: identity.cuid.clone(),
            muid: muid.clone(),
            muid_v2: muid_v2.clone(),
            phase_percent: phase,
        },
        utm_params: utm_params.clone(),
        switches: SwitchesInfo {
            argv: argv.clone(),
            test_ad: config.ads.test_ad,
        },
        paths,
        is_packaged: !debug,
        locale: locale.clone(),
        displays,
        primary_display_id: primary,
        packages,
        flags: Flags::default(),
        platform: os.node_platform().to_owned(),
        arch: node_arch().to_owned(),
        first_launch: !shared.state.first_launch,
        cursor: super::main_webview::read_cursor(app, options.os_queries),
        ipc_limits: IpcLimits {
            max_message_bytes: config.ipc.max_message_bytes,
        },
    };
    let snapshot_value = serde_json::to_value(&snapshot)?;

    let core = Core {
        router: Router::new(&config.ipc),
        sinks: HashMap::new(),
        state: StateHub::new(snapshot_value),
        windows: WindowRegistry::new(),
        quit: QuitSequence::default(),
        request_ids: 0,
        close_requests: HashMap::new(),
        evals: HashMap::new(),
        next_eval: 0,
        shortcuts: std::collections::BTreeMap::new(),
        flags: Flags::default(),
        main_ready: false,
        main_ready_warned: false,
        main_loaded: false,
        relaunch_args: None,
        exiting: false,
        soft_restart: None,
        restart_recreating: false,
        quit_after_restart: false,
        restart_stale_windows: std::collections::BTreeSet::new(),
        urls: HashMap::new(),
        in_page_urls: HashMap::new(),
        ticks: 0,
        ads: super::ads::AdsCore::default(),
        consent: super::consent::ConsentCore::default(),
        browser_opens: Vec::new(),
        updater: crate::updater::UpdaterCore::default(),
    };

    let label = super::analytics::host_label(&config.analytics);
    let reporter = crate::analytics::Reporter {
        user_agent: crate::analytics::compose_user_agent(
            &super::analytics::fallback_platform_ua(&tauri::webview_version().unwrap_or_default()),
            &manifest.product_name,
            &manifest.version,
            &label,
            crate::platform::safari_version(),
        ),
        label,
        app_version: manifest.version.clone(),
        uid: identity.uid.clone(),
        cuid: identity.cuid.clone(),
        os: os.node_platform().to_owned(),
        os_version: crate::platform::os_release(),
        app_name: manifest.product_name.clone(),
        muid: muid.clone(),
        muid_v2: muid_v2.clone(),
        locale: crate::analytics::accept_language(&locale),
    };
    let transport = options
        .transport
        .clone()
        .unwrap_or_else(|| Arc::new(crate::analytics::transport::HyperTransport::new()));
    let user_enabled =
        !config.analytics.user_switch || stored.analytics_user_enabled != Some(false);
    let analytics = super::analytics::AnalyticsHost::new(
        crate::analytics::transport::Dispatcher::new(transport),
        reporter,
        user_enabled,
        !shared.state.first_launch,
    );

    let host = Arc::new_cyclic(|weak| Host {
        updater_api: crate::updater::Updater(weak.clone()),
        app: app.clone(),
        info: Info {
            app_origin: app_origin(app),
            browser_args: webview_args,
            argv,
            switches,
            ads_data_dir,
            user_data_dir: user_data.clone(),
            debug,
            os,
            muid,
            muid_v2,
            phase_percent: phase,
            utm_params,
            launch_consent,
            state_dir,
            fs_scope,
            identity,
            manifest,
            config,
        },
        logger,
        ow_tauri,
        ow_electron,
        analytics,
        options,
        core: Mutex::new(core),
        started: Instant::now(),
        flush_scheduled: AtomicBool::new(false),
    });
    host.analytics
        .dispatcher
        .set_hooks(Arc::new(super::cookies::HostRequestHooks(Arc::downgrade(
            &host,
        ))));
    for w in warnings {
        host.log(LogLevel::Warn, &w);
    }
    Ok(host)
}

/// Displays as Electron reports them, and the primary display id.
pub(crate) fn displays_of(
    monitors: &[crate::screen::MonitorInfo],
    primary: Option<&crate::screen::MonitorInfo>,
) -> (Vec<ElectronDisplay>, u32) {
    let displays: Vec<ElectronDisplay> = monitors.iter().map(crate::screen::to_display).collect();
    let primary_id = primary
        .map(crate::screen::to_display)
        .map(|d| d.id)
        .or_else(|| displays.first().map(|d| d.id))
        .unwrap_or(0);
    (displays, primary_id)
}
