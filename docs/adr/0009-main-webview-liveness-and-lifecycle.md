# ADR 0009: Keep the main webview alive and give it one lifecycle

- Status: Superseded by ADR 0017 and ADR 0018 (2026-10-08)
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

1. **One browser-argument set per webview environment** (CONTRACT A.1.1),
   computed at setup; the ads environment adds its own switches to the app
   set. On Windows the app set adds `--disable-background-timer-throttling
   --disable-renderer-backgrounding --disable-backgrounding-occluded-windows`
   to wry's default `--disable-features=...` value. Runtime calls that would
   change arguments (`appendSwitch`, `disableHardwareAcceleration`) apply from
   the next launch.
2. **macOS 14+**: `ow-main` is hidden with
   `BackgroundThrottlingPolicy::Disabled`.
3. **macOS 12 and 13, Linux**: `ow-main` is a technically visible window:
   1 x 1, fully transparent, ignores the cursor, skips the taskbar and the
   window switcher, never focused. On macOS 12 and 13 transparency needs
   Tauri's private-API switch (`macOSPrivateApi`); without it the window is
   an opaque 1 x 1 pixel, and timers still run.
4. **A measurable requirement and a soak test.** A 1 s interval in `ow-main`
   keeps a median period under 1.5 s for 10 minutes with every app window
   hidden. A scheduled CI job checks it on all three platforms.
5. **No navigation in release builds.** After its first load `ow-main` cannot
   navigate. In debug builds a reload (a navigation to the current document
   URL, nothing else) is a soft restart: every other window is closed, the
   router and registries are cleared, and the app's main code runs again
   from a fresh snapshot. An exit request during a soft restart waits until
   the new document has loaded.
6. **A crash relaunches the app**, up to `main.crashRestartLimit` times per
   60 s, then the app exits with an error. The crash times are stored in
   `ow-tauri.json`, so the limit holds across relaunches. There is no
   rehydration protocol. Crash signals: WebView2 `ProcessFailed`, WebKitGTK
   `web-process-terminated`, and on macOS the web-content termination hook,
   which Tauri offers only on the app's builder, so the app forwards it to
   the plugin; the `ow-main` window's `Destroyed` event outside an exit
   counts too.
7. **Relaunch at `RunEvent::Exit`.** `app.relaunch()` and the crash path
   start the new process when Tauri delivers `RunEvent::Exit`, which reaches
   plugins in registration order. `tauri-plugin-single-instance` is
   registered first (CONTRACT A.5), so its lock is already released and the
   new process is not turned away as a second instance.
8. **One quit sequence** for every exit path, including OS-initiated ones
   (`RunEvent::ExitRequested` with `prevent_exit`): `before-quit`, window
   `close` events, `will-quit`, each answerable within 5 s, then analytics
   drain and exit (CONTRACT A.6). Without a main webview nobody can answer
   `before-quit`, so an exit request then goes straight to the drain and
   exits with code 0.

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

## Amendments

- 2026-10-06, first implementation: one argument set per webview
  environment (decision 1); macOS 12 and 13 transparency needs Tauri's
  private-API switch (decision 3); only a navigation to the current URL is a
  reload, and an exit request waits for a soft restart (decision 5); crash
  signals per platform and the crash history in `ow-tauri.json`
  (decision 6); relaunches start at `RunEvent::Exit` (decision 7, new); an
  exit request without a main webview skips the sequence (decision 8).
- 2026-10-08, Tauri-native pivot: superseded by [ADR 0017](0017-tauri-native-pivot.md), because there is no main webview to keep alive, and by [ADR 0018](0018-lifecycle-ready-exit.md): the plugin starts at `RunEvent::Ready`, drains at `RunEvent::Exit` and never holds the exit.
