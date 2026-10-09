# Lab runs

`e2e/` drives the showcase on ow-tauri and on ow-electron in an invisible lab
on macOS, with test ads only. It is a development tool: the normal build
never bundles the driver, and the Tauri shell has the lab only with its
`lab` Cargo feature, which is off by default and must never ship.

```sh
# From examples/ad-showcase, after `npm install` at the repository root.
node e2e/run.mjs --host tauri                 # stage --lab, debug build with `lab`, smoke run
node e2e/run.mjs --host tauri --no-build
node e2e/run.mjs --host electron              # stage, then tools/parity-harness's ow-electron
node e2e/run.mjs --host electron --steps tour # every page and its buttons
node e2e/run.mjs --host tauri --steps tour --stills /tmp/stills   # plus PNG stills (macOS)
node e2e/compare.mjs e2e/out/<tauri run> e2e/out/<electron run>   # compare.md per step
node e2e/run.mjs --host tauri --mode live --steps live-layout     # LIVE, budgeted (below)
```

- `--steps smoke` (default): the window starts, page 1 renders, and the run
  waits (`--ad-wait`, 60 s) until its slots have an ad.
- `--steps restart` (ow-tauri): starts on the parity page (no ad guest),
  restarts in TEST through `window.showcase.restart` onto
  `parity/restarted`, and follows the new process under its own window
  monitor and front check until it reports that page and quits
  (`summary.json` `restart`). It proves the restart of a built app and that
  the lab environment carries over, so the new process stays invisible.
- `--steps tour`: then every page in order, waiting for each format's events
  instead of fixed times: the size groups and the below-the-fold box
  (scrolled by the page's own scroll box), all eight layouts, the high-impact
  takeover and restore, the interstitial (pass-through probe while it loads;
  modal probe 800 ms after its first `display_ad_loaded`, once
  `performance_ad_loaded` has ended the pass-through; a second one dropped;
  the 900x500 error path, the red-dim and blur variants, removal by the app,
  an unknown `unit`), the reward flow (ready, Watch, play, hide during play,
  complete, granted once, the next preload), house, every control, consent
  and identity, parity, and finally **Export JSON**. Restart is never pressed
  and no ad is ever clicked.
- The interstitial probe reads the page's hit test at the "Click me" button
  (and on ow-tauri the window's native hit test, `hitTest:`). In test mode
  only, and only when the hit test names that button, it sends one click
  there: on ow-electron `webContents.sendInputEvent` into the app page, on
  ow-tauri `mouseDown:`/`mouseUp:` sent to the app's own webview. It never
  sends input where the hit test names the ad. On both hosts the counter
  then goes from 0 to 1 while the ad loads (pass-through); once the ad is
  shown the hit test names the ad guest and nothing is sent.
- `--stills DIR` (ow-tauri, macOS): after each key step every webview of the
  window renders itself (`WKWebView takeSnapshotWithConfiguration:`), and the
  snapshots are drawn bottom to top at their frames into
  `DIR/<page>-<state>.png`. Each snapshot keeps its own size, anchored at
  the bottom of its frame: the app webview's snapshot is its viewport, below
  the title bar band, which the still leaves out. Nothing captures the
  screen, so no screen-recording permission is involved. With
  `OW_SHOWCASE_STILL_PARTS=1` each webview's own snapshot is kept too
  (`<name>.<label>.png`). `--theme light` records the light theme (dark
  is the default).
- `--stills DIR` (ow-electron): each ad guest renders itself in process
  (`webContents.capturePage()`) into
  `DIR/<page>-<state>.electron-guest-<id>-<w>x<h>.png`, and the `still` row
  of `e2e.jsonl` gives each guest's painted share and colour count, so a
  blank test creative is told apart from a host that shows nothing (the
  970x90 test creative is blank on both hosts).
- The ow-electron wrapper records what the ad guests log to
  `guest-console.jsonl` (`<owadview> is not visible. waiting...`, `not
valid slot size`), as the ow-tauri trace does for its guests.
- `compare.mjs` compares two runs step by step: event names and counts per
  slot (slots with a per-creation counter are matched by order), slot
  statuses, the page's state, each step's checks and the probes, plus each
  slot's event order over the run. `count` rows are timing (refreshes and
  reloads); `differs` rows need a look.

## Live runs

`--mode live` takes one of four scenarios, each started on its page with
`--showcase-page` so that only the planned ad guests mount: `live-layout`
(Combo Classic, 2 loads), `live-300x250` (page 1, the 300x250 alone, 1),
`live-reward` (the two reward slots, 2; Watch is never pressed, which would
script a play) and `live-perf` (one interstitial, 1). Each mounted guest is
one line in `e2e/out/live-loads.jsonl` (run id, host, slot, running count);
a run whose planned loads would take the count over `--live-cap` (default 10) is refused before launch. A live run reports the guests it mounted, the
fill events per slot and the impression pings the ad pages sent (ow-electron:
`--log-net-log`; ow-tauri reports none: the plugin no longer probes guest
pages).

Output goes to `e2e/out/<run-id>/` (git-ignored): `e2e.jsonl` (one record
per step, with `window.__showcase.snapshot()` and the new timeline rows),
`summary.json`, `window-monitor.jsonl`, `blocked.jsonl`, the app's
`stdout.log` / `stderr.log` and, on ow-tauri, the plugin's lab trace.

## Invisibility

- **ow-tauri**: `OW_TAURI_LAB_INVISIBLE=1`. The app is an accessory app (no
  Dock icon, no app switcher entry) and is never activated
  ([src-tauri/src/lab.rs](../src-tauri/src/lab.rs)): before the app is
  built, the lab turns every app activation into a no-op and makes
  `makeKeyAndOrderFront:` order a window front without making it key
  (`WebKit` activates the app for every webview it creates). The showcase
  window is built hidden, gets alpha 0, click-through and not focusable,
  and is then ordered front (`orderFrontRegardless`): on screen, as an ad
  needs to be to fill, and invisible. Dialogs and the file manager do not
  open (the plugin's lab).
- **ow-electron**: [electron-main.cjs](electron-main.cjs) is the main entry
  of a throwaway app folder around `.stage/electron`. It hides the Dock icon,
  pins every window at opacity 0, click-through and not focusable, turns
  `show()` into `showInactive()`, drops focus calls, and answers dialogs as
  dismissed (ow-electron itself cannot be changed, so its lab works from
  the outside). Then it loads the
  showcase's main process unchanged.
- Both: an isolated home (`HOME`, `CFFIXED_USER_HOME`), the window monitor
  (`tools/parity-harness/lib/window-monitor.swift`; the app is killed the
  moment one of its windows becomes visible), a check that the app never
  becomes the frontmost app (`lsappinfo front`), and a kill of the whole
  process group on every exit path. `summary.json` records `everVisible`,
  `everFront` and any process the app left behind.

`run.mjs` exits 0 only when the driver finished, `everVisible` is `false`,
the app was never frontmost, nothing was left running, the driver reported
no fatal error and (test mode) a `display_ad_loaded` arrived.

## Configuration

- `OW_SHOWCASE_E2E_CONFIG`: the driver's configuration (`runDir`, `steps`,
  `mode`, `adWaitMs`, `dwellMs`, `liveObserveMs`, `theme`, `stillsDir`). Without it
  the driver stays inert.
- `OW_TAURI_LAB_DIR` (ow-tauri): the plugin's trace folder; the driver's
  `e2e.jsonl` goes there too. The staged identity is built in
  (`TAURI_CONFIG` from `.stage/tauri.conf.json`), under the lab bundle id
  `dev.ow-tauri.ad-showcase.lab`, so a lab process never reaches a running
  normal build through the single-instance lock.
- `OW_SHOWCASE_LAB_SINK` (ow-tauri): the loopback sink the runner starts for
  the plugin's analytics (`Counter`, `InsertStats`) and the consent
  experiment (`cmp-eu-only`); each request is a line of `sink.jsonl`.
  Without it a lab build sends them to a closed loopback port: a lab build
  never reports to Overwolf. The consent page itself (the CMP window, opened
  by the consent page and the privacy settings) still loads from Overwolf:
  the plugin accepts only an `https` `consent.cmpUrl`.
