# tauri-plugin-overwolf-unstable

A helper crate of [`tauri-plugin-overwolf`](https://github.com/AlloryDante/ow-tauri/tree/main/crates/tauri-plugin-overwolf). Apps never add it themselves: `tauri-plugin-overwolf` pulls it in on Windows and macOS when its `ads` feature is on.

The crate has no code. Its only content is a dependency on `tauri` with the `unstable` feature, so Cargo turns
`unstable` (child webviews, which host the ads) on in the app's `tauri` on those two targets only. On Linux, where ads
are unsupported, the app keeps stable Tauri. It is a separate crate because a crate cannot depend on `tauri` twice
under two names.

To use the plugin, follow the [`tauri-plugin-overwolf` README](https://github.com/AlloryDante/ow-tauri/tree/main/crates/tauri-plugin-overwolf#readme).

## License

MIT or Apache-2.0, at your option.
