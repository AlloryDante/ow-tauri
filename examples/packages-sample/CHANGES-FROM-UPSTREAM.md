# Changes from upstream

Upstream: [overwolf/ow-electron-packages-sample](https://github.com/overwolf/ow-electron-packages-sample)
at commit `8a27053` (MIT, Overwolf Ltd.), imported verbatim by this
repository's first example commit. Line numbers below refer to that upstream
version. The plan behind each change is in `docs/PORT-MAP.md`.

## Upstream defects fixed

The numbering matches `docs/PORT-MAP.md` section 5.

| # | Upstream location | Change | Why |
|---|---|---|---|
| 1 | `src/preload/preload.ts:50` invokes `disable-ads-optimization`; `src/browser/controllers/main-window.controller.ts` has no handler (next to `disable-ads-fpd`, `:261`) | New `ipcMain.handle('disable-ads-optimization')` calling `app.overwolf.disableAdsOptimization()` | The "Disable ads optimization" button rejected with "No handler registered". |
| 2 | `src/preload/preload.ts:229` (`enable-replays`) and `:199` (`recorder-ready`) | New handlers in `src/browser/controllers/recorder/recording.controller.ts`: `enable-replays (enabled)` turns replays on or off through the existing service calls; `recorder-ready` resolves whether the recorder service exists | Both invokes rejected: no handler. |
| 3 | `src/browser/services/recorder/recording.service.ts:61` (constructor) | Subscribes to `recorderApi.on('stats')` and re-emits `stats` | The controller forwards service `stats` (`recording.controller.ts:180`), but the service never emitted it, so `recording-stats` never fired. |
| 4 | `src/preload/preload.ts:37-42` | Removed `onElevationHelperPrompt` and `sendElevationHelperResponse` | Dead: nothing sends `elevation-helper-prompt` to the page and nothing handles `elevation-helper-response`; the UI never used them. The overlay controller's auto-install flow is unchanged. |
| 5 | `src/renderer/exclusive/exclusive.ts:27`, `:69`; `src/browser/controllers/overlay/exclusive-mode-window.controller.ts:62` | On every exclusive-mode change the controller sends `EXCLUSIVE_MODE` (boolean) to the custom exclusive window, and it hides that window on `HIDE_EXCLUSIVE`, through two new `ExclusiveModeWindowService` methods (`sendExclusiveModeChanged`, `hideExclusiveModeCustomWindow`), since the service owns the window | The page's channels had no main-side counterpart. |
| 6 | `src/browser/controllers/overlay/overlay.controller.ts:192` (`update-hotkey`) | Rejects a missing or nameless hotkey with `Error('update-hotkey: a hotkey with a name is required')` | `preload.ts` `osr.updateHotkey()` sends no hotkey; the handler then read `hotkey.name` of `undefined` (TypeError). |
| 7 | `src/browser/controllers/gep/game-events.controller.ts:94` (`gep-getInfo`) | Returns the fetched info | The handler returned `undefined`; the UI logged "undefined". |
| 8 | `src/common/channels/channels.ts:7`; `src/browser/controllers/overlay/overlay.controller.ts:321`, `:344` | `ZORDER_RESET` is `'zOrder-reset'`, the string the OSR page listens for; the controller sends `OverlayChannels.PASSTHROUGH_RESET` / `ZORDER_RESET`; `REGISTER_HOTKEY`, `UNREGISTER_HOTKEY` and `ELEVATION_HELPER_RESPONSE` are marked `@deprecated` (unused) | The constant disagreed with the channel actually used; several constants were unused. |
| 9 | `src/renderer/app/components/ads-tester/ad.tsx:39` | The `<owadview>` DOM id is `${id}-adview` (from the container id); `cid` stays `mainAd` | Every ad had `id="mainAd"`: duplicate DOM ids. |
| 10 | `README.md:21` (`npm run start`) | New `start` script (`tauri dev`) | The README documented a script that did not exist. |
| 11 | `tsconfig.json:15` | Deleted the top-level `typeRoots` | It sat outside `compilerOptions` and was ignored; moving it inside would break resolution (absolute paths, and it would drop `@types/*`). |
| 12 | `src/browser/controllers/overlay/overlay.controller.ts:204` (`toggle-osr-visibility`) | Hides all overlay windows when any is visible, else shows them | The "toggle" only ever showed windows. |
| 13 | `src/browser/controllers/main-window.controller.ts:394` (`get-utm-params`) | Returns `app.overwolf.utmParams ?? null` | It read ow-electron's private `ow-electron.json` although the API exists. |
| 14 | `src/browser/services/updater.service.ts:7`, `:20` | Removed `forceDevUpdateConfig = true` (default `false`); the guard skips the check when `!app.isPackaged` | `forceDevUpdateConfig = true` defeated the "not packaged" guard, so development builds updated too. Development builds now never check (`dev-app-update.yml` is unused, as the guard intended). Feed URL and channel unchanged. |

## Security fixes

| Upstream location | Change | Why |
|---|---|---|
| `src/browser/controllers/main-window.controller.ts:244` (`open-folder`) | `shell.openPath(String(path))` instead of `exec(\`explorer.exe ${path}\`)`; the handler awaits it and returns `false` (and logs the reason) when it fails | A renderer-supplied path reached a shell command line (injection). `shell.openPath` opens folders inside `plugins.overwolf.fs.scope` only, so a refusal must not report success. |
| `src/browser/services/overlay/ingame-windows.service.ts:156`, `:176-197` | The DPI test window, which loads a remote site, is created with `frame: true`; the injected drag header and its `require('electron').ipcRenderer.send('closeWindow')` button are removed | Remote pages get no IPC in ow-tauri (`bwr-*` windows, ADR 0011); native decorations give the same move and close actions. |

## Port to ow-tauri

| Upstream location | Change | Why |
|---|---|---|
| `package.json` `devDependencies`, `dependencies`, `scripts` | Removed `@overwolf/ow-electron`, `@overwolf/ow-electron-builder` and `electron-updater`; added `ow-tauri`, `@tauri-apps/api` and `@tauri-apps/cli` 2.12.1, `events`, `path-browserify`. Scripts: `build` is one webpack run (both configs), `start` / `build:start` / `start-ad` use `tauri dev`, `build:ow-tauri` replaces `build:ow-electron` and keeps its `dotenv --override --no-expand` wrapper (for `ow-tauri sign` and `tauri build`); `dotenv-cli` is now a devDependency (upstream relied on a global install); npm instead of yarn. `overwolf`, `build.overwolf`, `name`, `productName`, `author`, `version` and `copyright` unchanged | Tauri toolchain; the `overwolf` blocks and names keep the uid (CONTRACT G). |
| `package.json` `typescript` `^4.7.4` | `^5.9.3` | ow-tauri's declarations use TypeScript 5 syntax (`export type *`). |
| `tsconfig.json` | `types: ["node", "ow-tauri/types"]`, `paths` for `electron` and `@overwolf/ow-electron` (CONTRACT B.4; first entry for a standalone app, second for this repository's workspace), `skipLibCheck: true`, `src-tauri` excluded | ow-tauri's typings replace ow-electron's; `react-router-dom` 7's declarations import `react-router/dom`, which `moduleResolution: "node"` cannot resolve. |
| `webpack.base.config.js` | `resolve.alias` `electron` -> `ow-tauri/electron`; `plugins` no longer shared between configs | ADR 0002; the shared array leaked renderer plugins into the main config. |
| `webpack.main.config.js` | `target: 'web'`; `resolve.fallback` (`path-browserify`, `events`, `fs` and `child_process` off); `__dirname` defined as `'/browser'`; HtmlWebpackPlugin emits `dist/browser/main.html`; `electron-updater` external removed | The main process runs in the hidden main webview (ADR 0001); `main.html` is `plugins.overwolf.main.url`. |
| `webpack.renderer.config.js` | `target: 'web'`; own `plugins` array | Tauri webviews instead of Electron renderers. |
| `src/browser/controllers/main-window.controller.ts:12`, `:18`, `:50`, `:140`, `:149`, `:157`, `:165`, `:688` | `fs` -> `files` from `ow-tauri/main` (async; `package.json` and the display and screenshot prefs), `fs.promises.mkdir` -> `files.mkdir`; callers (`:171` `getTargetDisplay`, `:189` `createAndShow`, `:407`, `:415`, `:526`, `:627`) await them | Webviews have no Node `fs`; `files` is scoped (CONTRACT B.1.7). Timing: the main window is created, and the packages' `ready` listeners (`:526`) are attached, one file read later than upstream. A package that became ready inside that window would be missed; today none can, because the package objects are `undefined` (CONTRACT H). |
| `src/browser/controllers/recorder/recording.controller.ts:2`; `src/browser/controllers/overlay/overlay.controller.ts:2-3` | Removed unused `child_process`, `path` and `fs` imports | Not available in a webview, and unused. |
| `src/browser/services/updater.service.ts:1` | `autoUpdater` and `UpdateCheckResult` from `ow-tauri/main` | electron-updater replacement (CONTRACT I.5). |
| `src/preload/preload.ts:18` | `version` shows `ow-tauri v${process.versions.owTauri}` | There is no `process.versions.electron`. |
| `src/renderer/app/components/capture/constants.ts:145` | `'Lossless'` -> `'lossless'` | `@overwolf/ow-electron-packages-types` 1.1.12 (`latest`) spells the NVENC rate control in lower case; the upstream value no longer compiles. |
| `src/renderer/app/components/capture/capture-audio.tsx:68`, `:81` | Dropped `?? false` after `=== true` | TypeScript 5.6+ rejects an unreachable `??` operand (TS2869); behaviour unchanged. |
| `.vscode/launch.json` | Replaced: `npm run start-ad` with `OW_TAURI_REMOTE_DEBUGGING_PORT=9222`, a WebView2 attach, and CodeLLDB for the Rust shell | No ow-electron executable. |
| `.gitignore` | Adds `src-tauri/target/`, `src-tauri/gen/`, `src-tauri/windows/hooks.nsh` | Build output. |
| `README.md` | New "Running on ow-tauri" section at the top; upstream text kept below it | How to run, what works where, parity notes. |

## Added files

| Path | Purpose |
|---|---|
| `src-tauri/` | The Tauri app: `Cargo.toml` (tauri 2.12.1, single-instance, `serde_json` for `generate_context!`, the plugin as a path dependency), `build.rs` (`embed_manifest`, NSIS hooks), `src/main.rs`, `tauri.conf.json` (CSP, NSIS, `plugins.overwolf`; `app.security.freezePrototype` stays off because Tauri would inject it into every webview, ad guests and consent pages included, which ow-electron never does), `tauri.windows.conf.json`, `capabilities/ui.json` (`overwolf:renderer` only; window dragging comes from the plugin's `ow-tauri-ui-chrome` capability), neutral generated icons |
| `src-tauri/src/lab.rs`, the `lab` Cargo feature | The end-to-end lab (off by default, never shipped): a run-time manifest override for the lab identity, the driver's configuration and records |
| `e2e/` | End-to-end run of every page and button in an invisible lab, on ow-tauri and on the upstream sample on ow-electron, and an action-by-action comparison (`e2e/README.md`) |
| `CHANGES-FROM-UPSTREAM.md` | This file |
| `.env.example` | Names of the optional environment variables, no values |

## Unchanged on purpose

- Line endings: every file keeps upstream's (upstream mixes CRLF and LF), so
  `git diff` against the import shows only real edits. The repository's
  `.gitattributes` (`-text`), `.editorconfig` and `.prettierignore` leave this
  folder alone; editors must not normalise it.

- The overlay `registerGames` id list in `src/browser/application.ts` keeps
  every id Overwolf wrote (parity with the upstream sample).
- `crashReporter.start` stays; it is a documented no-op in ow-tauri.
