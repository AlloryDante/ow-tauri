//! The ad showcase's Tauri app: one window that shows every `<owadview>` ad
//! format, built on `tauri-plugin-overwolf`.
//!
//! The window's page is the showcase renderer (`src/renderer`, the same
//! TypeScript the ow-electron twin runs). It reaches the plugin through
//! `tauri-plugin-overwolf-api` and this app through the commands in
//! [`showcase`]. With the `lab` feature the app also carries the invisible
//! lab of `e2e/README.md`.

// No console window next to the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "lab")]
mod lab;
mod showcase;

fn main() -> tauri::Result<()> {
    showcase::run()
}
