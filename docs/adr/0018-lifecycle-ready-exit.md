# ADR 0018: Start at `RunEvent::Ready`, drain at `RunEvent::Exit`, never hold the exit

- Status: Accepted
- Date: 2026-10-08

## Context

ow-electron sends its launch burst when Electron's app is ready. The burst
includes the first-launch and start counters, the first heartbeat, the
`cmp-eu-only` request and, after it, the hidden startup consent window. At
quit, it ends each window's visible period and sends the queued analytics.

The first ow-tauri design waited for a `ready()` call from the hidden main
webview and held the exit with `prevent_exit` until the queue had drained.
Holding the exit broke tray apps that quit on purpose, and the app had to
remember to call `ready()`.

Tauri gives a plugin three relevant moments:

- `RunEvent::Ready`, after the app's own `setup` closure has run;
- `RunEvent::ExitRequested`, which can be prevented;
- `RunEvent::Exit`, which runs before `cleanup_before_exit`.

`AppHandle::restart()` skips both exit events.

## Decision

- **Start.** At `RunEvent::Ready` the plugin does the following
  (`host/lifecycle.rs` `on_ready`):
  1. its first state writes (machine ids when absent, `ow-tauri.json`);
  2. applies the persisted anonymous-analytics preference;
  3. marks itself started;
  4. reads the user agent ([ADR 0019](0019-window-tracking-and-naming.md),
     bounded by 2.5 s);
  5. sends the launch burst and starts the consent round.

  Opt-outs made in `tauri.conf.json`, in the `Builder` or in the app's
  `setup` closure land before the burst, as ow-electron's pre-ready
  main-process calls do.
- **No writes before Ready.** Plugin setup reads state but writes nothing. A
  second instance that `tauri-plugin-single-instance` ends during setup
  leaves no trace.
- **Exit.** The plugin never calls `prevent_exit` and never calls `exit`. At
  `RunEvent::Exit`, `on_exit` ends every open visible period and waits for
  the analytics request thread for at most 1.5 s (`DRAIN_LIMIT`). The drain
  runs on that thread, so `on_exit` never needs the main thread.
- **Restart sentinel.** Setup adds a resource to the app's resource table.
  `cleanup_before_exit` drops it on every exit and restart path, and its
  `Drop` runs the same idempotent `on_exit`. A `relaunch()` therefore drains
  like a quit, with no app code.

## Consequences

- The app needs no `ready()` call, and `setExternalPaymentUserId` never fails
  with "not ready".
- Mounting an ad before the burst waits for it, so 400025 never precedes the
  launch counters.
- The exit takes at most 1.5 s longer when analytics are still queued. A
  quit is never blocked or cancelled.
- A Windows logoff or shutdown delivers no `Exit` event. The drain is lost,
  as it is after a crash (TROUBLESHOOTING).
- A late `disableAnonymousAnalytics()` from JavaScript cannot stop this
  launch's burst. It logs a warning;
  `setAnonymousAnalyticsPreference(false)` stores the preference in
  `ow-tauri.json` for the next launch.

## Alternatives considered

- **An explicit `ready()` call.** It is easy to forget, and the burst timing
  would differ between apps. Rejected.
- **Prevent the exit until the drain finishes.** This broke tray apps and
  `tauri-plugin-process` relaunches. Rejected.
- **Drain at `ExitRequested`.** Another handler may still prevent that exit,
  and a restart never emits it. Rejected.
