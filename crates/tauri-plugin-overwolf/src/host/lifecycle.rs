//! Start and exit (DESIGN §4.2, D1, D2).
//!
//! - [`on_ready`] runs at `RunEvent::Ready`: the first state writes, the
//!   persisted analytics preference, `started`, `<UA>` discovery and the
//!   launch burst.
//! - `RunEvent::ExitRequested` is never prevented and the plugin never exits
//!   the app: a tray app that keeps running after its last window closes
//!   keeps running (D2).
//! - [`on_exit`] runs at `RunEvent::Exit` and, through the restart sentinel
//!   ([`ExitSentinel`]), when Tauri clears the app's resources before an exit
//!   or a restart that skips `RunEvent::Exit` (`AppHandle::restart`). It is
//!   synchronous, bounded and idempotent: the queued analytics requests drain
//!   on the request lane for at most [`DRAIN_LIMIT`] while this thread only
//!   waits on a channel, so it never needs the main thread.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use tauri::{Manager, Resource, Runtime};

use super::{Core, LOG_TARGET, lock};
use crate::analytics::DRAIN_LIMIT;

/// Start and exit state.
#[derive(Debug)]
pub(crate) struct Lifecycle {
    /// When setup started (the session clock's zero).
    pub(crate) setup_at: Instant,
    /// When `RunEvent::Ready` arrived.
    ready_at: Mutex<Option<Instant>>,
    /// `true` once [`on_ready`] ran; `adview_mount` waits for it.
    started: tokio::sync::watch::Sender<bool>,
    /// [`on_exit`] ran.
    exited: AtomicBool,
    /// How many times [`on_exit`] did its work (at most once; tests read it).
    exits: AtomicUsize,
}

impl Lifecycle {
    /// Before Ready.
    pub(crate) fn new(setup_at: Instant) -> Self {
        Lifecycle {
            setup_at,
            ready_at: Mutex::new(None),
            started: tokio::sync::watch::Sender::new(false),
            exited: AtomicBool::new(false),
            exits: AtomicUsize::new(0),
        }
    }

    /// Whether [`on_ready`] ran.
    pub(crate) fn is_started(&self) -> bool {
        *self.started.borrow()
    }

    /// When `RunEvent::Ready` arrived.
    pub(crate) fn ready_at(&self) -> Option<Instant> {
        *lock(&self.ready_at)
    }

    /// Waits until [`on_ready`] ran. Cannot deadlock: `RunEvent::Ready`
    /// always follows setup.
    pub(crate) async fn wait_started(&self) {
        let mut rx = self.started.subscribe();
        let _ = rx.wait_for(|started| *started).await;
    }

    /// Whether [`on_exit`] ran.
    #[allow(
        dead_code,
        reason = "the ads host (W2) stops recreating guests after it"
    )]
    pub(crate) fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// How many times [`on_exit`] did its work.
    #[allow(dead_code, reason = "read by the lifecycle tests")]
    pub(crate) fn exit_count(&self) -> usize {
        self.exits.load(Ordering::SeqCst)
    }
}

/// `RunEvent::Ready`: the start of the plugin's work (DESIGN §4.2).
pub(crate) fn on_ready<R: Runtime>(core: &Arc<Core<R>>) {
    if core.lifecycle.is_started() {
        return;
    }
    *lock(&core.lifecycle.ready_at) = Some(Instant::now());
    first_writes(core);
    apply_persisted_preference(core);
    core.lifecycle.started.send_replace(true);
    super::analytics::start_user_agent_discovery(core);
    let queued = core.analytics.start(core.now(), core.identity.first_launch);
    if core.identity.first_launch
        && let Err(err) = core.state.ow_electron.set_first_launch()
    {
        log::warn!(target: LOG_TARGET, "could not record the first launch: {err}");
    }
    super::consent::start(core);
    super::windows::start_ticker(core);
    log::debug!(target: LOG_TARGET, "started ({queued} launch requests)");
}

/// The first writes of the launch, all at Ready: the machine ids missing
/// from the registry (Windows), the corrupt `ow-tauri.json` moved aside, the
/// new per-install muid.
fn first_writes<R: Runtime>(core: &Arc<Core<R>>) {
    crate::platform::machine::persist_machine_ids(&core.identity.machine);
    if let Some(backup) = core.state.ow_tauri.repair() {
        log::warn!(
            target: LOG_TARGET,
            "ow-tauri.json was not valid JSON; moved to {}",
            backup
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
    }
    if core.identity.new_install_muid {
        let muid = core.identity.machine.muid.clone();
        if let Err(err) = core.state.ow_tauri.update(|s| s.muid = Some(muid)) {
            log::warn!(target: LOG_TARGET, "could not write ow-tauri.json ({})", err.kind());
        }
    }
}

/// `setAnonymousAnalyticsPreference(false)` of an earlier launch applies
/// before this launch's burst (R10).
fn apply_persisted_preference<R: Runtime>(core: &Arc<Core<R>>) {
    if core.state.ow_tauri.get().anonymous_analytics == Some(false) {
        core.analytics.disable_anonymous();
    }
}

/// `RunEvent::Exit`, or the restart sentinel: ends the visible periods,
/// drains the analytics requests for at most [`DRAIN_LIMIT`], once.
pub(crate) fn on_exit<R: Runtime>(core: &Core<R>) {
    if core.lifecycle.exited.swap(true, Ordering::SeqCst) {
        return;
    }
    core.lifecycle.exits.fetch_add(1, Ordering::SeqCst);
    super::windows::end_all_periods(core);
    let drained = core.analytics.dispatcher.drain_blocking(DRAIN_LIMIT);
    if !drained {
        log::debug!(target: LOG_TARGET, "exit: analytics requests still in flight after the drain limit");
    }
}

/// The restart sentinel (PAR-M7): a resource in the app's resource table.
/// Tauri clears that table in `cleanup_before_exit`, which runs on every
/// exit and on `AppHandle::restart()`, which skips `RunEvent::Exit`; the
/// sentinel's `Drop` then runs [`on_exit`].
pub(crate) struct ExitSentinel<R: Runtime>(pub(crate) Weak<Core<R>>);

impl<R: Runtime> Resource for ExitSentinel<R> {}

impl<R: Runtime> Drop for ExitSentinel<R> {
    fn drop(&mut self) {
        if let Some(core) = self.0.upgrade() {
            on_exit(&core);
        }
    }
}

/// Adds the restart sentinel to the app's resource table.
pub(crate) fn install_sentinel<R: Runtime>(core: &Arc<Core<R>>) {
    let sentinel = ExitSentinel(Arc::downgrade(core));
    core.app.resources_table().add(sentinel);
}

#[cfg(test)]
mod tests;
