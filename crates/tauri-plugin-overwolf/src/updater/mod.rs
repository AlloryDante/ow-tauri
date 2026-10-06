//! The electron-updater compatible update client (CONTRACT A.2.8, I;
//! ADR 0008, `docs/adr/0008-updater-client.md`).
//!
//! Implemented in a later milestone: generic-feed parsing, staged rollout,
//! download with size, SHA-512 and publisher-signature checks, and the
//! per-OS install. The host lifecycle already calls `install_pending` at
//! the end of the quit sequence (A.6 step 5), where a downloaded update with
//! `autoInstallOnAppQuit` will run.

/// Runs a downloaded update on exit. Nothing is ever downloaded until the
/// updater milestone lands, so this does nothing.
#[cfg(feature = "plugin")]
pub(crate) fn install_pending() {}
