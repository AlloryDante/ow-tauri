# ADR 0002: Provide an Electron-compatible subset behind a bundler alias

- Status: Superseded by ADR 0017 (2026-10-08)
- Date: 2026-10-06

## Context

Main-process code imports `app`, `BrowserWindow`, `ipcMain`, `screen`,
`dialog`, `shell` and `crashReporter` from `electron`. Preload code imports
`contextBridge` and `ipcRenderer`. In the official sample, overlay pages also
use `require('electron').ipcRenderer` directly. Rewriting all of these call
sites would make every migration a rewrite.

## Decision

`ow-tauri/electron` exports an Electron-compatible subset. Apps alias the
module in their bundler (`resolve.alias: { electron: 'ow-tauri/electron' }`).
Each member is classified in CONTRACT B.2 as supported, partial or
unsupported. Unsupported members throw `OwTauriUnsupportedError` with the
member name, so a missing feature fails loudly at the call site instead of
misbehaving later. Unsupported modules still exist as objects so imports
compile.

Preload scripts referenced by `webPreferences.preload` are bundled assets; the
window manager injects them as Tauri initialization scripts. `contextBridge`
defines frozen globals.

## Consequences

- Most of the sample's main-process and renderer code compiles and runs
  unchanged.
- The subset grows only on demand and every addition updates CONTRACT B.2.
- Preload code shares the page's JavaScript world (Tauri has no isolated
  world for initialization scripts). Frozen, non-configurable globals keep the
  exposed API from being replaced, but that is not isolation:
  `__TAURI_INTERNALS__.invoke` is a page global, so **any script in a UI
  window, including injected or XSS code, can call
  `plugin:overwolf|ipc_invoke` on any channel and reach every `ipcMain`
  handler**, whatever the preload chose to expose. `ipcMain` handlers must
  validate their arguments as they would for untrusted input, UI windows need
  a strict CSP (ARCHITECTURE section 5.5), and SECURITY.md says so.
- Electron types still describe the API; `OverlayWindowOptions extends BrowserWindowConstructorOptions`
  keeps compiling.

## Alternatives considered

- **A new ow-tauri-specific API.** Cleaner, but every app rewrites its main
  process. Rejected.
- **Full Electron emulation.** Unbounded scope (sessions, protocols, menus,
  offscreen rendering) and much of it has no WebView2/WKWebView equivalent.
  Rejected.
- **Silently ignoring unsupported members.** Hides bugs. Rejected in favour of
  typed errors; options and properties that are safe to ignore are documented
  as partial.

## Amendments

- 2026-10-06, contract review: the preload-isolation consequence now states
  plainly that every script in a UI window can reach every `ipcMain` handler.
- 2026-10-08, Tauri-native pivot: superseded by [ADR 0017](0017-tauri-native-pivot.md). ow-tauri ships no Electron API and no bundler alias. Apps use Tauri's own APIs and `tauri-plugin-overwolf-api` ([ADR 0021](0021-package-split.md)).
