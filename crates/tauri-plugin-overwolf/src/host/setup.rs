//! Building the [`Host`] in the plugin's setup hook (ARCHITECTURE 3.2).

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::Value;
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
use crate::packages::{PackageRuntime, PackagesBackend, PackagesSnapshot};
use crate::paths::{BaseDirs, TargetOs, electron_paths, node_arch, user_data_dir};
use crate::screen::ElectronDisplay;
use crate::snapshot::{Flags, HostSnapshot, IdentityInfo, StateHub, SwitchesInfo, Versions};
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
    pub(crate) package_runtime: Option<Arc<dyn PackageRuntime>>,
    pub(crate) test_ad: Option<bool>,
    pub(crate) uid: Option<String>,
    pub(crate) companion_plugins: bool,
    pub(crate) runtime_capabilities: bool,
    pub(crate) main_webview: bool,
    pub(crate) argv: Option<Vec<String>>,
    /// Query monitors and the cursor (off on a mock runtime, which has none).
    pub(crate) os_queries: bool,
}

impl std::fmt::Debug for SetupOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetupOptions")
            .field("packages_backend", &self.packages_backend)
            .field("package_runtime", &self.package_runtime.is_some())
            .field("test_ad", &self.test_ad)
            .field("companion_plugins", &self.companion_plugins)
            .field("runtime_capabilities", &self.runtime_capabilities)
            .field("main_webview", &self.main_webview)
            .field("os_queries", &self.os_queries)
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
    let logger = Logger::open(state_dir.log_file());
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
        warnings.push("ow-tauri.json was not valid JSON; starting from defaults".into());
    }
    if config.analytics.muid_strategy == MuidStrategy::MachineId {
        warnings.push(
            "analytics.muidStrategy \"machine-id\" is not available yet (OQ-02); using per-install"
                .into(),
        );
    }
    let stored = ow_tauri.get();
    let muid = if let Some(m) = stored.muid.filter(|m| crate::identity::is_valid_muid(m)) {
        m
    } else {
        let m = random_muid();
        let value = m.clone();
        if let Err(err) = ow_tauri.update(|s| s.muid = Some(value)) {
            warnings.push(format!("could not write ow-tauri.json: {}", err.kind()));
        }
        m
    };
    let phase = phase_percent(&muid);

    let ow_electron = OwElectronFile::new(state_dir.ow_electron_json());
    let shared = ow_electron.read();
    if shared.status == FileStatus::Invalid {
        warnings.push("ow-electron.json is not valid JSON; it is left untouched".into());
    }
    let utm_params = shared.state.utm_params.clone().unwrap_or(Value::Null);

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
        user_data,
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

    let backend = config
        .packages_backend
        .resolve(options.package_runtime.is_some());
    let packages = PackagesSnapshot::initial(
        backend,
        &manifest.overwolf.packages,
        state_dir.logs_dir().to_string_lossy().into_owned(),
        phase,
        stored.package_channels.clone(),
    );

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
            muid_v2: muid.clone(),
            phase_percent: phase,
        },
        utm_params: utm_params.clone(),
        switches: SwitchesInfo {
            argv: argv.clone(),
            test_ad: config.ads.test_ad,
        },
        paths,
        is_packaged: !debug,
        locale: sys_locale::get_locale().unwrap_or_else(|| "en-US".to_owned()),
        displays,
        primary_display_id: primary,
        packages,
        flags: Flags::default(),
        platform: os.node_platform().to_owned(),
        arch: node_arch().to_owned(),
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
        ticks: 0,
    };

    let host = Arc::new(Host {
        app: app.clone(),
        info: Info {
            app_origin: app_origin(app),
            browser_args: webview_args,
            argv,
            switches,
            debug,
            os,
            muid,
            phase_percent: phase,
            utm_params,
            state_dir,
            fs_scope,
            identity,
            manifest,
            config,
        },
        logger,
        ow_tauri,
        ow_electron,
        options,
        core: Mutex::new(core),
        started: Instant::now(),
        flush_scheduled: AtomicBool::new(false),
    });
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
