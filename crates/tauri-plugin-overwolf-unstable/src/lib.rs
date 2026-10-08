//! Helper crate of `tauri-plugin-overwolf`: its only content is a dependency
//! on `tauri` with the `unstable` feature.
//!
//! `tauri-plugin-overwolf` depends on this crate on Windows and macOS when its
//! `ads` feature is on, so Cargo enables Tauri's `unstable` feature (child
//! webviews, which host the ads) in the app's `tauri` on those targets only.
//! Apps never depend on it directly.
#![no_std]
