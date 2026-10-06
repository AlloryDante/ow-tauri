//! Anonymous app analytics: `Counter` and `InsertStats` requests (CONTRACT E;
//! ADR 0006, `docs/adr/0006-analytics-labelling.md`).
//!
//! Implemented in a later milestone: the event catalogue of E.2, the
//! mandatory subset and opt-outs of E.3, the request builders of E.1, the
//! HTTP transport and the `Builder::analytics_transport` test hook. The host
//! lifecycle already calls `drain` before every exit (A.6), so the drain
//! budget is in place.

use std::time::Duration;

/// The longest an exit waits for queued analytics requests (A.6, E.1).
pub const DRAIN_LIMIT: Duration = Duration::from_millis(1500);

/// The `owver` value for this host: `tauri-<tauri crate version>` (E.1, OQ-03).
///
/// ```
/// assert!(tauri_plugin_overwolf::analytics::owver("2.12.1") == "tauri-2.12.1");
/// ```
#[must_use]
pub fn owver(tauri_version: &str) -> String {
    format!("tauri-{tauri_version}")
}

/// Waits for queued analytics requests, at most `limit`. Nothing is queued
/// until the analytics milestone lands, so this returns at once.
#[cfg(feature = "plugin")]
#[expect(
    clippy::unused_async,
    reason = "keeps the exit path's await point stable for the analytics milestone"
)]
pub(crate) async fn drain(limit: Duration) {
    let _ = limit;
}
