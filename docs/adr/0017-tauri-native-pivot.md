# ADR 0017: Adapt Overwolf's SDK to Tauri instead of emulating Electron

- Status: Accepted
- Date: 2026-10-08
- Supersedes: [ADR 0001](0001-hidden-main-webview.md),
  [ADR 0002](0002-electron-subset-alias.md),
  [ADR 0009](0009-main-webview-liveness-and-lifecycle.md),
  [ADR 0010](0010-per-webview-ipc-channels.md),
  [ADR 0012](0012-js-runtime-singleton.md)

## Context

The first design ran an ow-electron app nearly unchanged. It put the app's
main-process code in a hidden webview (`ow-main`), offered an
Electron-compatible subset behind a bundler alias, and routed `ipcMain` and
`webContents.send` traffic over one channel per webview. It worked in the
lab, but it had real costs:

- A Tauri app had to be written as if it were an Electron app. Rust, Tauri
  windows and Tauri plugins sat outside the model.
- The hidden webview needed its own liveness rules, startup queue, state
  cache and crash handling. None of that had anything to do with Overwolf.
- The surface to secure, document and keep compatible was large: an IPC
  router, a JavaScript runtime per webview and every emulated Electron
  module.

What Overwolf needs from a host is narrower. It needs its ad and consent
pages hosted correctly and the same analytics, identity, state and installer
data that ow-electron sends.

## Decision

ow-tauri is Overwolf's ow-electron SDK adapted to Tauri:

- **The app is an ordinary Tauri app.** Its windows, commands, menus and
  plugins use Tauri's own APIs. ow-tauri adds no Electron API and no hidden
  main webview.
- **The plugin is the host.** `tauri-plugin-overwolf` reacts to Tauri's own
  events: `RunEvent::Ready` and `RunEvent::Exit` (ADR 0018), window and page
  events (ADR 0019). It hosts the ad and consent pages as native webviews.
- **The app talks to it through Tauri idioms.** These are Tauri commands
  guarded by capabilities, the `tauri-plugin-overwolf-api` npm package, the
  `<owadview>` element, the Rust `OverwolfExt` trait and the
  `plugins.overwolf` block in `tauri.conf.json` (ADR 0021).
- **The wire stays identical.** Every request, header, id, cookie, state-file
  byte and guest message that Overwolf receives matches what ow-electron
  sends ([CONTRACT](../CONTRACT.md)). The parity harness proves it.

## Consequences

- An ow-electron app moves its window and main-process logic to Tauri. It
  keeps its `<owadview>` markup, its uid and its per-app state
  ([MIGRATION](../MIGRATION.md)).
- The IPC router, the Electron subset, the injected runtime, the state
  snapshot and the main-webview liveness work are deleted.
- ow-electron's `packages` API (GEP, overlay, recorder and the rest) has no
  counterpart in 1.0 ([ADR 0004](0004-packages-backend-selection.md)).
- Child webviews need Tauri's `unstable` feature on Windows and macOS
  ([ADR 0023](0023-unstable-and-macos-input.md)).
- App webviews can now call ow-tauri directly. Commands that ran in
  ow-electron's trusted main process now run behind capabilities and an
  app-webview gate ([SECURITY model](../SECURITY.md)).

## Alternatives considered

- **Keep the Electron emulation and harden it.** Every Electron module would
  stay a compatibility promise and every hidden-webview quirk an open bug,
  all for an API Tauri apps do not want. Rejected.
- **Offer both models.** That means two hosts to test against the same wire
  contract. Rejected.
