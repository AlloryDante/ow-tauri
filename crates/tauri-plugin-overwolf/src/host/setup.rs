//! Building the [`Core`] in the plugin's setup hook (DESIGN §4.2).
//!
//! Setup reads and never writes: no state file, no directory and no registry
//! value is created before `RunEvent::Ready`, so a second instance that
//! `tauri-plugin-single-instance` ends right after setup leaves no trace
//! (PAR-minor-8). The first writes happen in
//! [`lifecycle::on_ready`](super::lifecycle::on_ready).

use std::sync::Arc;

use tauri::{AppHandle, Manager, Runtime};
use url::Url;

use super::{Core, Flags, Identity, StateFiles, ads, analytics, consent, lifecycle, windows};
use crate::app_identity::AppIdentity;
use crate::config::{Config, MuidStrategy, Validation};
use crate::identity::{muid_from_bytes, phase_percent};
use crate::paths::{TargetOs, ads_data_dir};
use crate::platform::machine::{MachineIds, machine_ids};
use crate::state::StateDir;
use crate::state::ow_electron::{FileStatus, OwElectronFile};
use crate::state::ow_tauri::OwTauriFile;
use crate::types::HostInfo;

/// The environment variable that turns test ads on (`1`).
pub(crate) const TEST_AD_ENV: &str = "OW_TAURI_TEST_AD";
/// The command-line switch that turns test ads on.
pub(crate) const TEST_AD_SWITCH: &str = "--test-ad";

/// What the [`Builder`](crate::Builder) passes to setup.
#[derive(Clone, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent builder switches"
)]
pub(crate) struct SetupOptions {
    /// `Builder::test_ad(true)`.
    pub(crate) test_ad: bool,
    /// `Builder::disable_anonymous_analytics()`.
    pub(crate) disable_anonymous_analytics: bool,
    /// `Builder::disable_ads_optimization()`.
    pub(crate) disable_ads_optimization: bool,
    /// `Builder::disable_ads_fpd()`.
    pub(crate) disable_ads_fpd: bool,
    /// `Builder::host_label`.
    pub(crate) host_label: Option<(String, Option<String>)>,
    /// `Builder::exclude_windows`, added to `analytics.excludeWindows`.
    pub(crate) exclude_windows: Vec<String>,
    /// macOS: the app forwards the web content terminate hook itself.
    pub(crate) forwards_terminate: bool,
    /// macOS: `Builder::macos_key_fix` (on by default, DESIGN §4.6a [R1]).
    #[cfg(target_os = "macos")]
    pub(crate) macos_key_fix: bool,
    /// The embedded `dev-app-update.yml` (CONTRACT I.1).
    #[allow(dead_code, reason = "the update client (W3) reads it")]
    pub(crate) dev_update_config: Option<&'static str>,
    /// A transport that replaces the HTTP client (tests).
    pub(crate) transport: Option<Arc<dyn crate::analytics::Transport>>,
    /// Endpoint overrides for failure injection (tests).
    pub(crate) endpoints: Option<crate::analytics::TestEndpoints>,
    /// The process arguments (tests replace them).
    pub(crate) argv: Option<Vec<String>>,
    /// Query the machine ids (off in unit tests, which must not depend on
    /// the machine).
    pub(crate) os_queries: bool,
    /// Register the guest and consent runtime capabilities (off in unit
    /// tests: a mock app carries no ACL manifest for the plugin; the
    /// acl-app tests cover the registration).
    pub(crate) runtime_capabilities: bool,
}

impl std::fmt::Debug for SetupOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("SetupOptions");
        s.field("test_ad", &self.test_ad)
            .field(
                "disable_anonymous_analytics",
                &self.disable_anonymous_analytics,
            )
            .field("disable_ads_optimization", &self.disable_ads_optimization)
            .field("disable_ads_fpd", &self.disable_ads_fpd)
            .field("host_label", &self.host_label)
            .field("exclude_windows", &self.exclude_windows)
            .field("forwards_terminate", &self.forwards_terminate);
        #[cfg(target_os = "macos")]
        s.field("macos_key_fix", &self.macos_key_fix);
        s.field("transport", &self.transport.is_some())
            .field("os_queries", &self.os_queries)
            .finish_non_exhaustive()
    }
}

/// Whether the command line or the environment turns test ads on.
///
/// The environment value counts when it is `1` or `true` (any case).
pub(crate) fn test_ad_switch(argv: &[String], env: Option<&str>) -> bool {
    argv.iter().skip(1).any(|a| a == TEST_AD_SWITCH)
        || env.is_some_and(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))
}

/// The origin app assets are served from: `build.devUrl`'s origin in a
/// development run, else Tauri's custom protocol origin for this OS.
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
    Url::parse(text).unwrap_or_else(|_| unreachable!("a constant URL parses"))
}

fn random_muid() -> String {
    muid_from_bytes(*uuid::Uuid::new_v4().as_bytes())
}

/// The machine ids: the OS ids (`machine-id`), else the per-install muid of
/// `ow-tauri.json`, else a new per-install muid (written at Ready). Returns
/// the ids and whether a new per-install muid was made.
fn resolve_machine_ids(
    config: &Config,
    stored: Option<String>,
    os_queries: bool,
) -> (MachineIds, bool) {
    if config.analytics.muid_strategy == MuidStrategy::MachineId && os_queries {
        match machine_ids(|| uuid::Uuid::new_v4().to_string()) {
            Ok(ids) => return (ids, false),
            Err(reason) => log::warn!(
                target: super::LOG_TARGET,
                "machine id unavailable ({reason}); using a per-install muid"
            ),
        }
    }
    if let Some(m) = stored.filter(|m| crate::identity::is_valid_muid(m)) {
        return (MachineIds::per_install(m), false);
    }
    (MachineIds::per_install(random_muid()), true)
}

/// Builds the [`Core`]; called from the plugin's setup hook. Reads only.
///
/// # Errors
///
/// An invalid configuration, or a directory Tauri cannot name.
#[allow(
    clippy::too_many_lines,
    reason = "one linear setup sequence (DESIGN §4.1); W2 splits the host parts out"
)]
pub(crate) fn setup<R: Runtime>(
    app: &AppHandle<R>,
    config: Option<Config>,
    mut options: SetupOptions,
) -> Result<Arc<Core<R>>, Box<dyn std::error::Error>> {
    let setup_at = super::setup_instant();
    // Tauri parsed `plugins.overwolf` through `Config::from_value`.
    let mut config = config.unwrap_or_default();
    for warning in config.normalize() {
        log::warn!(target: super::LOG_TARGET, "{warning}");
    }
    config.validate(Validation::runtime())?;
    if let Some((label, version)) = options.host_label.take() {
        config.analytics.host_label = label;
        config.analytics.host_version = version;
    }
    config
        .analytics
        .exclude_windows
        .extend(options.exclude_windows.iter().cloned());
    if options.disable_anonymous_analytics {
        config.analytics.disable_anonymous = true;
    }
    if options.disable_ads_optimization {
        config.ads.disable_optimization = true;
    }
    if options.disable_ads_fpd {
        config.ads.disable_fpd = true;
    }
    let argv = options
        .argv
        .clone()
        .unwrap_or_else(|| std::env::args().collect());
    let test_ad = config.ads.test_ad
        || options.test_ad
        || test_ad_switch(&argv, std::env::var(TEST_AD_ENV).ok().as_deref());

    let package = app.package_info();
    let app_identity = AppIdentity::resolve(&config, &package.name, &package.version.to_string());
    let app_data_dir = match &config.state.app_data_dir {
        Some(dir) => dir.clone(),
        None => app.path().config_dir()?,
    };
    let state_dir = StateDir::new(&app_data_dir, &app_identity.uid);

    let ow_tauri = OwTauriFile::load(state_dir.ow_tauri_json());
    if ow_tauri.corrupt_at_load {
        log::warn!(
            target: super::LOG_TARGET,
            "ow-tauri.json is not valid JSON; starting from defaults (it is moved aside at startup)"
        );
    }
    let stored = ow_tauri.get();
    let (machine, new_install_muid) =
        resolve_machine_ids(&config, stored.muid.clone(), options.os_queries);
    let phase = phase_percent(&machine.muid);

    let ow_electron = OwElectronFile::new(state_dir.ow_electron_json());
    let shared = ow_electron.read();
    if shared.status == FileStatus::Invalid {
        log::warn!(
            target: super::LOG_TARGET,
            "ow-electron.json is not valid JSON; it is left untouched"
        );
    }
    let launch_consent = shared
        .state
        .cmp
        .clone()
        .and_then(|c| c.unified_consent_string)
        .unwrap_or_default();

    let label = analytics::host_label(&config.analytics);
    let host = HostInfo::new(label.label(), label.version(), label.ow_version());
    let dev_origin = app
        .config()
        .build
        .dev_url
        .as_ref()
        .map(|u| u.origin().ascii_serialization());
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".to_owned());
    let user_enabled =
        !config.analytics.user_switch || stored.analytics_user_enabled != Some(false);
    let reporter = crate::analytics::Reporter {
        user_agent: analytics::template_user_agent(&app_identity, &label),
        label,
        app_version: app_identity.version.clone(),
        uid: app_identity.uid.clone(),
        cuid: app_identity.cuid.clone(),
        os: TargetOs::current().node_platform().to_owned(),
        os_version: crate::platform::os_release(),
        app_name: app_identity.name.clone(),
        muid: machine.muid.clone(),
        muid_v2: machine.muid_v2.clone(),
        locale: crate::analytics::accept_language(&locale),
    };
    let transport = options
        .transport
        .clone()
        .unwrap_or_else(|| Arc::new(crate::analytics::transport::HyperTransport::new()));
    let dispatcher = crate::analytics::transport::Dispatcher::new(transport);
    if let Some(endpoints) = options.endpoints.clone() {
        dispatcher.set_endpoints(endpoints);
    }
    let analytics = analytics::AnalyticsHost::new(
        dispatcher,
        reporter,
        user_enabled,
        config.analytics.disable_anonymous,
    );

    let flags = Flags::default();
    flags.ads_optimization_disabled.store(
        config.ads.disable_optimization,
        std::sync::atomic::Ordering::SeqCst,
    );
    flags
        .ads_fpd_disabled
        .store(config.ads.disable_fpd, std::sync::atomic::Ordering::SeqCst);

    let identity = Identity {
        ads_data_dir: ads_data_dir(&app_data_dir, &app_identity.name),
        app_origin: app_origin(app),
        dev_origin,
        first_launch: !shared.state.first_launch,
        utm_params: shared.state.utm_params.clone(),
        launch_consent,
        phase_percent: phase,
        machine,
        new_install_muid,
        test_ad,
        host,
        app_data_dir,
        state_dir,
        app: app_identity,
        config,
    };
    let windows = windows::AppWindows::new(&identity.config.analytics.exclude_windows);
    let core = Arc::new(Core {
        app: app.clone(),
        flags,
        ads: ads::AdsCore::default(),
        consent: consent::ConsentCore::default(),
        analytics,
        windows,
        ua: analytics::UserAgent::new(),
        ticker: windows::Ticker::default(),
        lifecycle: lifecycle::Lifecycle::new(setup_at),
        state: StateFiles {
            ow_tauri,
            ow_electron,
        },
        identity,
        options,
    });
    core.analytics
        .dispatcher
        .set_hooks(Arc::new(analytics::CoreRequestHooks(Arc::downgrade(&core))));
    Ok(core)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ads_from_the_command_line_or_the_environment() {
        let argv = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(test_ad_switch(&argv(&["app", "--test-ad"]), None));
        assert!(
            !test_ad_switch(&argv(&["--test-ad"]), None),
            "argv[0] is the program"
        );
        assert!(!test_ad_switch(&argv(&["app", "--test-ad=1"]), None));
        assert!(test_ad_switch(&argv(&["app"]), Some("1")));
        assert!(test_ad_switch(&argv(&["app"]), Some(" TRUE ")));
        assert!(!test_ad_switch(&argv(&["app"]), Some("0")));
        assert!(!test_ad_switch(&argv(&["app"]), None));
    }

    #[test]
    fn per_install_muids_come_from_the_state_file() {
        let config = Config::default();
        let stored = Some("8C7E4F2A-0000-4000-8000-00000000ABCD".to_owned());
        let (ids, new) = resolve_machine_ids(&config, stored, false);
        assert!(!new);
        assert_eq!(ids.muid, "8C7E4F2A-0000-4000-8000-00000000ABCD");
        let (ids, new) = resolve_machine_ids(&config, Some("not a muid".into()), false);
        assert!(new && crate::identity::is_valid_muid(&ids.muid));
        assert_eq!(ids.muid, ids.muid_v2);
    }
}
