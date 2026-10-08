//! The plugin's runtime state (DESIGN §4.1): one [`Core`] per app, managed
//! as Tauri state behind [`Overwolf`](crate::Overwolf).
//!
//! `Core`'s fields are a frozen interface (DESIGN §10.0): each field belongs
//! to one module, which owns its locking. Locking rule: a module holds its
//! own lock only for pure state changes; every Tauri call (creating,
//! evaluating in or closing a webview, sending on a channel) happens after
//! the lock is released, because Tauri may run a window event handler,
//! which takes the lock again, on the calling thread.
//!
//! Tauri hooks reach the modules through [`dispatch`] only; commands and
//! the Rust API call the modules directly.

pub(crate) mod ads;
pub(crate) mod analytics;
pub(crate) mod consent;
pub(crate) mod cookies;
pub(crate) mod dispatch;
pub(crate) mod lifecycle;
pub(crate) mod setup;
pub(crate) mod windows;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use serde_json::Value;
use tauri::{AppHandle, Runtime};
use url::Url;

use crate::app_identity::AppIdentity;
use crate::config::Config;
use crate::platform::machine::MachineIds;
use crate::state::StateDir;
use crate::state::ow_electron::OwElectronFile;
use crate::state::ow_tauri::OwTauriFile;
use crate::types::HostInfo;

pub(crate) use setup::SetupOptions;

/// The log target of the plugin's own messages.
pub(crate) const LOG_TARGET: &str = "tauri_plugin_overwolf";

/// The plugin's runtime: every service of one app.
pub(crate) struct Core<R: Runtime> {
    /// The app.
    pub(crate) app: AppHandle<R>,
    /// Facts fixed at setup.
    pub(crate) identity: Identity,
    /// Run-time switches.
    pub(crate) flags: Flags,
    /// Ad guests (`<owadview>`, DESIGN §4.4).
    pub(crate) ads: ads::AdsCore,
    /// Consent (DESIGN §4.7).
    pub(crate) consent: consent::ConsentCore,
    /// The analytics session and the host request lane (DESIGN §4.8).
    pub(crate) analytics: analytics::AnalyticsHost,
    /// App windows tracked from native events (DESIGN §4.3).
    pub(crate) windows: windows::AppWindows,
    /// `<UA>` (DESIGN §4.10).
    pub(crate) ua: analytics::UserAgent,
    /// The visibility ticker (DESIGN §4.11).
    #[allow(dead_code, reason = "the ticker (W2) keeps its state here")]
    pub(crate) ticker: windows::Ticker,
    /// Start and exit (DESIGN §4.2).
    pub(crate) lifecycle: lifecycle::Lifecycle,
    /// The state files (DESIGN §4.12).
    pub(crate) state: StateFiles,
    /// What the builder asked for.
    #[cfg_attr(
        not(target_os = "macos"),
        allow(
            dead_code,
            reason = "read by the macOS terminate check; the W2 hosts read the rest"
        )
    )]
    pub(crate) options: SetupOptions,
}

impl<R: Runtime> std::fmt::Debug for Core<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core")
            .field("uid", &self.identity.app.uid)
            .field("started", &self.lifecycle.is_started())
            .finish_non_exhaustive()
    }
}

/// Facts fixed at setup.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "the ads and consent hosts (W2) read the remaining facts"
)]
pub(crate) struct Identity {
    /// The parsed and validated `plugins.overwolf`.
    pub(crate) config: Config,
    /// `<PN>`, version, author, uid and cuid.
    pub(crate) app: AppIdentity,
    /// The machine ids (CONTRACT E.4); `unsaved` ones are written at Ready.
    pub(crate) machine: MachineIds,
    /// A per-install muid created at setup, written to `ow-tauri.json` at
    /// Ready.
    pub(crate) new_install_muid: bool,
    /// The phase bucket of this machine, 0 to 99.
    pub(crate) phase_percent: u8,
    /// `utmParams` of `ow-electron.json` at launch.
    pub(crate) utm_params: Option<Value>,
    /// `firstLaunch` was absent from `ow-electron.json` at launch.
    pub(crate) first_launch: bool,
    /// `cmp.unifiedConsentString` of `ow-electron.json` at launch: the
    /// `consent` of every ad guest (D.2).
    pub(crate) launch_consent: String,
    /// Test ads (config, `--test-ad`, `OW_TAURI_TEST_AD=1` or the builder).
    pub(crate) test_ad: bool,
    /// The host the analytics and the guests report.
    pub(crate) host: HostInfo,
    /// The OS configuration directory (or `state.appDataDir`).
    pub(crate) app_data_dir: PathBuf,
    /// `<appData>/ow-electron/<uid>`.
    pub(crate) state_dir: StateDir,
    /// `<appData>/<PN>/EBWebView-ow`.
    pub(crate) ads_data_dir: PathBuf,
    /// The origin app pages are served from (DESIGN §4.5).
    pub(crate) app_origin: Url,
    /// `build.devUrl`'s origin, accepted as an app origin in debug builds.
    pub(crate) dev_origin: Option<String>,
}

/// Switches that change at run time.
#[derive(Debug, Default)]
pub(crate) struct Flags {
    /// `disableAdsOptimization()` (or config, builder).
    pub(crate) ads_optimization_disabled: AtomicBool,
    /// `disableAdsFPD()` (or config, builder).
    pub(crate) ads_fpd_disabled: AtomicBool,
    /// The late `disableAnonymousAnalytics()` warning was logged (R10).
    pub(crate) late_disable_warned: AtomicBool,
}

impl Flags {
    /// Reads a flag.
    #[allow(dead_code, reason = "the ads host (W2) reads the flags")]
    pub(crate) fn get(flag: &AtomicBool) -> bool {
        flag.load(Ordering::SeqCst)
    }
}

/// The two state files of the state directory.
#[derive(Debug)]
pub(crate) struct StateFiles {
    /// `ow-tauri.json`.
    pub(crate) ow_tauri: OwTauriFile,
    /// `ow-electron.json`, shared with ow-electron.
    pub(crate) ow_electron: OwElectronFile,
}

impl<R: Runtime> Core<R> {
    /// Milliseconds since setup (the clock of every session timer).
    pub(crate) fn now(&self) -> u64 {
        u64::try_from(self.lifecycle.setup_at.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// A `Mutex` lock that ignores poisoning: the plugin's state stays usable
/// after a panic in another thread.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The instant setup started.
pub(crate) fn setup_instant() -> Instant {
    Instant::now()
}

/// The [`Core`] of `app`, once setup has run.
pub(crate) fn core_of<R: Runtime, M: tauri::Manager<R>>(manager: &M) -> Option<Arc<Core<R>>> {
    manager
        .try_state::<crate::Overwolf<R>>()
        .map(|s| Arc::clone(&s.inner().0))
}
