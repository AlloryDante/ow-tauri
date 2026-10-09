//! The packages sample's Tauri app: Overwolf's ow-electron packages sample
//! rebuilt on `tauri-plugin-overwolf`.
//!
//! One window shows the React page (`src/` of the example): the logger, the
//! ads tester, consent and settings, the updater and the packages page. The
//! page reaches the plugin through `tauri-plugin-overwolf-api`; the app
//! itself has no commands. With the `lab` feature it also carries the
//! invisible lab of `e2e/README.md`.

// No console window next to the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "lab")]
mod lab;
mod sample;

fn main() -> tauri::Result<()> {
    sample::run()
}
