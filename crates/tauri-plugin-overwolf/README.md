# tauri-plugin-overwolf

The Rust half of [ow-tauri](../../README.md): a Tauri 2 plugin that gives an
app ported from ow-electron the same Overwolf runtime services it had before.

- app identity (uid, muid, phase percent) and the per-app state file
- `<owadview>` ads hosted in native child webviews, with the guest page shim
- consent (CMP) windows and storage, email hashes, ad-optimisation switches
- anonymous app analytics
- the `app.overwolf.packages` manager with a pluggable package runtime
- an electron-updater-compatible update client
- the IPC router behind the `ow-tauri/electron` subset

Status: scaffold. The public surface is specified in
[docs/CONTRACT.md](../../docs/CONTRACT.md) and the design in
[docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md).

## Requirements

- Rust 1.90 or newer (edition 2024)
- tauri 2.12.1 or newer with the `unstable` feature (child webviews,
  see [ADR 0003](../../docs/adr/0003-owadview-native-child-webviews.md))

## License

MIT, see [LICENSE](../../LICENSE).
