# tauri-plugin-overwolf-unstable

Helper crate of [`tauri-plugin-overwolf`](../tauri-plugin-overwolf). It has no code: it depends on `tauri` with the
`unstable` feature, and `tauri-plugin-overwolf` depends on it on Windows and macOS when its `ads` feature is on. Cargo
then enables `unstable` (child webviews, which host the ads) in the app's `tauri` on those targets only.

Apps never add this crate themselves.

Licensed under either of MIT or Apache-2.0, at your option.
