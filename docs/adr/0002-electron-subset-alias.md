# ADR 0002: Provide an Electron-compatible subset behind a bundler alias

- Status: Accepted
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
  exposed API from being replaced, but page scripts can see everything the
  preload defines in its own scope if it leaks it onto `window`.
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
