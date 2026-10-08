# End-to-end lab run

`e2e/` drives the whole sample, every page and every button, with test ads,
in an invisible lab on macOS, and records what each action did: the page's
console output, errors, unhandled rejections and `alert()` calls, the main
process's log lines and console, the app's windows, and the ad events. The
same steps (`steps.js`) also drive the **upstream** sample on ow-electron, so
the two runs can be compared action by action (`compare.mjs`).

It is a development tool. Nothing in it is part of the app: the normal
`npm run build` never bundles the driver, and the Tauri shell has the lab
only with its `lab` Cargo feature, which is off by default and must never
ship.

## Run it

```sh
# From examples/packages-sample, after `npm install` at the repository root.
node e2e/run.mjs --host tauri --run-id tauri-1          # builds, then runs (about 8 minutes)
node e2e/run.mjs --host tauri --run-id tauri-2 --no-build

# Baseline: the upstream sample (built with its own webpack configs, with
# electron-updater in its node_modules) on tools/parity-harness's ow-electron.
node e2e/run.mjs --host electron --upstream-dir /path/to/upstream-sample --run-id electron-1

node e2e/compare.mjs e2e/out/tauri-1 e2e/out/electron-1   # writes compare.md into the first run

# Idle run: start every slot of one Ads Tester layout, leave the ads running
# for 5 minutes, then close the app (memory, queues and leaks over time).
node e2e/run.mjs --host tauri --run-id idle-1 --no-build --idle-ms 300000 --idle-layout tower-right

# The same, reloading every <owadview> each minute (element.reload(); the ad
# SDK reloads on its own about every 20 minutes): memory across reloads.
node e2e/run.mjs --host tauri --run-id reload-1 --no-build --idle-ms 600000 --idle-layout tower-right --idle-reload-ms 60000
```

Output goes to `e2e/out/<run-id>/` (git-ignored): `e2e.jsonl` (one record
per action), `summary.json`, `window-monitor.jsonl`, `blocked.jsonl`, the
app's `stdout.log` / `stderr.log`, the local update feed's requests,
`proc-samples.jsonl` (the memory of the app and of every process it owns,
RSS and physical footprint, every `--sample-ms`, 10 s by default; compare
growth by the footprint, since RSS drops whenever macOS compresses pages) and, on ow-tauri, the plugin's lab
trace (`host-requests.jsonl`, `ipc.jsonl`, `windows.jsonl`, ...), including
`plugin-log.jsonl` (every plugin log line, whatever the logging setting) and
`core-stats.jsonl` (the sizes of the plugin's IPC queues and tables every
10 s: over an idle run none of them may keep growing).

## What it does

1. **Build** (`--host tauri`): `ow-tauri` and its injected scripts, then `webpack` as `npm run build`
   does, then the main bundle again with `e2e/tauri-driver.js` in front of
   `src/browser/index.ts` into `e2e/out/.build/web/`, then a debug build with
   `--features lab` into `src-tauri/target/e2e`, embedding that folder
   (`TAURI_CONFIG` sets `build.frontendDist`).
2. **Launch** with `--test-ad`, an isolated home (`HOME`,
   `CFFIXED_USER_HOME`), the lab identity (below) and:
   - `OW_TAURI_LAB_INVISIBLE=1`: the app is an accessory app (no Dock icon,
     no app switcher entry); every window is built hidden, then gets alpha 0,
     click-through and an on-screen position (an ad must be on screen to
     fill), and only then is shown; the main webview is never shown; no
     window is ever focused or made full screen. Native dialogs, the file
     manager and the browser do not open: the plugin answers as if they were
     dismissed at once and records the request in `blocked.jsonl`.
   - `OW_TAURI_LAB_DIR`: the plugin's trace, and the driver's `e2e.jsonl`.
   - `OW_SAMPLE_E2E_CONFIG`: the driver's configuration. Without it (or
     without the `lab` feature) the driver does nothing.
3. **Drive** (`steps.js`, in the main webview, through the
   `ow-tauri/electron` facade's `webContents.executeJavaScript`):
   - Logger: every top button (`setRequiredFeatures`, `getInfo`, Create
     OSR, Create DPI OSR, Show all OSR, Track classId, `disableAdsFPD`,
     `disableAdsOptimization`, `hasPendingUpdates`, Scan Games,
     `utmParams`), the log search, auto scroll and Clear.
   - Ads Tester: all 15 layouts (each slot started, left to run, removed),
     including Tower Plus + High Impact, then the performance ad.
   - Channels, App Settings and Recording settings: every button, select
     and field, in page order (Manage CMP, email hashes, Check for Updates,
     display, screenshot format, overlay and hotkey settings, the
     high-elevation helper, every capture control, open-folder buttons,
     choose-folder buttons).
   - The updater against a local feed (`newer`: 1.0.1, `same`: 1.0.0), with
     the sample's updater settings.
   - The window buttons: minimize, maximize, unmaximize, and Close App last.
   It never clicks an ad and never presses Restart App.
4. **Watch** the app's windows with the window monitor
   (`CGWindowListCopyWindowInfo` through `tools/parity-harness/lib/window-monitor.swift`;
   no screen capture): the app is killed the moment one of its windows is
   visible, and `summary.json` records `everVisible`. The runner also
   checks every 200 ms that the app is never the frontmost app (it would
   take the keyboard from the app the user is typing in; `everFront`), and
   kills it at once if it is (verdict `safety-kill`).
5. **Clean up**: the app's whole process group is killed on every exit
   path (done, timeout, safety kill, Ctrl-C). WebKit's web content,
   networking and GPU processes are XPC services outside that group: the
   runner finds them by their responsible process (`proc-owner.swift`) and
   records any still running 10 s after the app quit in `summary.json`
   (`leftProcesses`). A run passes only when it is `done`, never visible,
   never in front and leaves no process behind.

## Comparing runs

`compare.mjs` lists, per action, the output lines (with their counts), the
ad events per slot (with their counts) and the window list that differ
between the two runs, then every state record that differs (the page's
`<owadview>` elements, the window state after the header buttons, the window
list after Close App, ...). It folds differences that are the JavaScript
engine's (V8 and JavaScriptCore word errors differently) and the port's
documented changes (CHANGES-FROM-UPSTREAM.md #9: each `<owadview>` has its
own DOM id).

## Lab identity

The run uses `tools/parity-harness/local.identity.json` (git-ignored; see
that folder's README), or `--identity <file>`: its `name`, `productName`,
`author`, `version` (and an optional `uid`, written as `overwolf.uid`)
replace the sample's in a `package.json` written to the run folder, which
the lab build reads at run time (`OW_TAURI_LAB_PACKAGE_JSON`). Without the
file the sample's own identity is used. No identity ever enters the build or
the repository.

## The baseline

`--host electron` needs the upstream sample
([overwolf/ow-electron-packages-sample](https://github.com/overwolf/ow-electron-packages-sample),
the first commit of this example) built with its webpack configs and with
`electron-updater` 5.3.0 in its `node_modules`. The runner makes a
throwaway app folder whose main entry is `electron-main.cjs`: it makes the
app an accessory app (no Dock icon), keeps every window at opacity 0,
click-through and unfocusable (as `tools/parity-harness` does), answers
dialogs as dismissed, keeps the file manager closed, then loads the upstream
main bundle unchanged and runs `steps.js`. The upstream main window is built
with `show: true`, and Electron's constructor then activates the app on
macOS, so the bundle's `require('electron')` gets a `BrowserWindow` that is
built hidden and then shown inactive.
