# Lab runs

`e2e/` drives the showcase on ow-tauri and on ow-electron in an invisible lab
on macOS, with test ads only. It is a development tool: the normal build
never bundles the driver, and the Tauri shell has the lab only with its
`lab` Cargo feature, which is off by default and must never ship.

```sh
# From examples/ad-showcase, after `npm install` at the repository root.
node e2e/run.mjs --host tauri                 # stage --lab, debug build with `lab`, smoke run
node e2e/run.mjs --host tauri --no-build
node e2e/run.mjs --host electron              # stage, then the workspace's ow-electron
node e2e/run.mjs --host electron --steps tour # every page and its buttons
```

- `--steps smoke` (default): the window starts, page 1 renders, and the run
  waits (`--ad-wait`, 60 s) for a `display_ad_loaded`.
- `--steps tour`: then every page in order, pressing the showcase's own
  buttons by their `data-action` (`--dwell` ms per page). Restart is never
  pressed and no ad is ever clicked.

Output goes to `e2e/out/<run-id>/` (git-ignored): `e2e.jsonl` (one record
per step, with `window.__showcase.snapshot()` and the new timeline rows),
`summary.json`, `window-monitor.jsonl`, `blocked.jsonl`, the app's
`stdout.log` / `stderr.log` and, on ow-tauri, the plugin's lab trace.

## Invisibility

- **ow-tauri**: `OW_TAURI_LAB_INVISIBLE=1`. The app is an accessory app (no
  Dock icon, no app switcher entry) and is never activated; every window is
  built hidden, then gets alpha 0, click-through and an on-screen position
  (an ad must be on screen to fill). Dialogs and the file manager do not
  open.
- **ow-electron**: [electron-main.cjs](electron-main.cjs) is the main entry
  of a throwaway app folder around `.stage/electron`. It hides the Dock icon,
  pins every window at opacity 0, click-through and not focusable, drops
  focus calls, and answers dialogs as dismissed. Then it loads the
  showcase's main process unchanged.
- Both: an isolated home (`HOME`, `CFFIXED_USER_HOME`), the window monitor
  (`tools/parity-harness/lib/window-monitor.swift`; the app is killed the
  moment one of its windows becomes visible), a check that the app never
  becomes the frontmost app (`lsappinfo front`), and a kill of the whole
  process group on every exit path. `summary.json` records `everVisible`,
  `everFront` and any process the app left behind.

`run.mjs` exits 0 only when the driver finished, `everVisible` is `false`,
the app was never frontmost, nothing was left running and a
`display_ad_loaded` arrived.

## Configuration

- `OW_SHOWCASE_E2E_CONFIG`: the driver's configuration (`runDir`, `steps`,
  `adWaitMs`, `dwellMs`). Without it the driver stays inert.
- `OW_TAURI_LAB_DIR`, `OW_TAURI_LAB_PACKAGE_JSON` (ow-tauri): the trace
  folder and the manifest to run with (`.stage/package.json`, so the staged
  identity applies; see the main README).
