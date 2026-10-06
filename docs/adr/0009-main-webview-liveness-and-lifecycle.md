# ADR 0009: Keep the main webview alive and give it one lifecycle

- Status: Accepted
- Date: 2026-10-06
- Extends: [ADR 0001](0001-hidden-main-webview.md)

## Context

ADR 0001 runs the app's main-process code in a hidden webview, `ow-main`.
Engines treat hidden webviews as background pages:

- Tauri's own documentation for `background_throttling` says hidden views get
  their timers throttled and may be suspended after a few minutes. The setting
  that turns this off is supported only on macOS 14 and newer; it is marked
  unsupported on Windows and Linux.
- On Windows, WebView2 throttles timers in hidden and occluded renderers unless
  the browser is started with the matching Chromium switches, and WebView2
  requires every webview that shares a data directory to use the same browser
  arguments.

`ow-main` hosts package event handlers, the IPC router's main side, the
updater and the app's own timers. If it stalls, the app stalls. Electron's
main process is a Node process and is never throttled, so app code does not
expect it.

ADR 0001 also left open what happens when `ow-main` reloads, crashes, or the
OS asks the app to quit. The JS-side state in `ow-main` (the `BrowserWindow`
registry, `ipcMain` handlers, hotkeys, package listeners) exists only there.

## Decision

1. **One browser-argument set for all webviews** (CONTRACT A.1.1), computed at
   setup. On Windows it adds `--disable-background-timer-throttling
   --disable-renderer-backgrounding --disable-backgrounding-occluded-windows`
   to wry's default `--disable-features=...` value. Runtime calls that would
   change arguments (`appendSwitch`, `disableHardwareAcceleration`) apply from
   the next launch.
2. **macOS 14+**: `ow-main` is hidden with
   `BackgroundThrottlingPolicy::Disabled`.
3. **macOS 12 and 13, Linux**: `ow-main` is a technically visible window:
   1 x 1, fully transparent, ignores the cursor, skips the taskbar and the
   window switcher, never focused.
4. **A measurable requirement and a soak test.** A 1 s interval in `ow-main`
   keeps a median period under 1.5 s for 10 minutes with every app window
   hidden. A scheduled CI job checks it on all three platforms.
5. **No navigation in release builds.** After its first load `ow-main` cannot
   navigate. In debug builds a reload is a soft restart: every other window is
   closed, the router and registries are cleared, and the app's main code runs
   again from a fresh snapshot.
6. **A crash relaunches the app**, up to `main.crashRestartLimit` times per
   60 s, then the app exits with an error. There is no rehydration protocol.
7. **One quit sequence** for every exit path, including OS-initiated ones
   (`RunEvent::ExitRequested` with `prevent_exit`): `before-quit`, window
   `close` events, `will-quit`, each answerable within 5 s, then analytics
   drain and exit (CONTRACT A.6).

## Consequences

- App timers and package handlers behave as in Electron on every platform.
- Disabling throttling costs some power while the app is idle; that is the
  same trade Electron's main process makes.
- macOS 12 and 13 and Linux carry a 1 x 1 invisible window. It never takes
  focus or input, but window-listing tools can see it.
- A debug reload gives a clean, predictable state instead of orphaned windows.
- A release-build crash of `ow-main` costs the user a relaunch, which is
  visible but safe; a crash loop ends with a clear log line instead of a
  silent dead app.

## Alternatives considered

- **Raise the minimum macOS to 14.** Simpler, but drops supported systems for
  one mechanism. Rejected; the visible-window fallback covers them.
- **Rehydrate JS state after a crash or reload.** Rust would have to replay
  the window registry and every handler registration, which only the app's
  code can recreate. Rejected as unreliable.
- **Periodic keep-alive pings from Rust.** They wake the page but do not stop
  timer clamping. Rejected.
