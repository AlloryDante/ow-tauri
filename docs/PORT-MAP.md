# Port map: `ow-electron-packages-sample` to ow-tauri

Source: [overwolf/ow-electron-packages-sample](https://github.com/overwolf/ow-electron-packages-sample)
at commit `8a27053` (2026-07-14, MIT, Overwolf Ltd.), imported verbatim into
`examples/packages-sample/` by the first commit of this repository. The port
happens in place; every change is also listed in
`examples/packages-sample/CHANGES-FROM-UPSTREAM.md`.

## 1. Approach

| Layer | Upstream | ow-tauri |
|---|---|---|
| Main process (`src/browser/**`) | Electron main process (Node) | bundled for the web and loaded into the hidden main webview (ADR 0001); `electron` aliased to `ow-tauri/electron` (ADR 0002) |
| Preload (`src/preload/preload.ts`) | Electron preload with `contextBridge` | the same bundle, injected as an initialization script by `webPreferences.preload` |
| Renderer (`src/renderer/**`) | React app, OSR page, exclusive page | unchanged except one defect fix; `electron` imports through the alias; `<owadview>` from `ow-tauri/renderer` |
| Shared (`src/common/**`) | channel names and recorder types | unchanged except one defect fix |
| Native shell | ow-electron + ow-electron-builder | new `src-tauri/` crate using `tauri-plugin-overwolf`; Overwolf signing through `ow-tauri sign` (CONTRACT G.4) |
| Packages | ow-electron package manager | `app.overwolf.packages` reporting packages as unavailable, as ow-electron does where they are not available: no events, observed results (CONTRACT H, ADR 0004) |
| Updater | `electron-updater` | `autoUpdater` from `ow-tauri/main` (ADR 0008) |

Legend for the file table: **Unchanged** (byte-identical), **Modified** (edited
in place; the note says what and why), **Replaced** (rewritten for Tauri).

## 2. New files

| Path (under `examples/packages-sample/`) | Purpose |
|---|---|
| `src-tauri/Cargo.toml` | thin app crate: `tauri` (with `unstable`), `tauri-build`, `tauri-plugin-overwolf`, all from `[workspace.dependencies]` |
| `src-tauri/build.rs` | `tauri_plugin_overwolf::build::embed_manifest("../package.json")`, then `tauri_build::build()` |
| `src-tauri/src/main.rs` | registers the plugin (`Builder::new().manifest_json(embedded_manifest!())`); no other logic |
| `src-tauri/tauri.conf.json` | `productName` and `version` matching `package.json`; `frontendDist: ../dist`; no windows (the plugin creates `ow-main`, the app creates the rest); `plugins.overwolf` with `main.url: "browser/main.html"` and `fs.scope: ["$PICTURES/Overwolf/$APPNAME", "$VIDEOS/$APPNAME"]`; `app.security.csp` from the ARCHITECTURE section 5.5 baseline plus `fonts.googleapis.com` (styles) and `fonts.gstatic.com` (fonts) for the renderer; `app.security.freezePrototype: true`; NSIS bundle mirroring `build.win` and `build.nsis` where Tauri has an equivalent, with `bundle.windows.nsis.installerHooks` pointing at the hooks that do Overwolf's install and uninstall work (CONTRACT I.6); `bundle.windows.signCommand` for the Authenticode step of `ow-tauri sign` (CONTRACT G.4) |
| `src-tauri/capabilities/ui.json` | `"webviews": ["bw-*"]` (no `windows` key, so ad guests and remote pages inside `bw-*` windows do not match), `"local": true`, permissions `overwolf:renderer` and `core:window:allow-start-dragging`; no `core:event:*` (ARCHITECTURE section 5.2) |
| `src-tauri/icons/*` | generated from a neutral placeholder icon |
| `src-tauri/windows/hooks.nsh` | the NSIS hooks of CONTRACT I.6: registry values at install; on a real uninstall only, the per-app data folder, the registry key and the `ow_<label>_app_uninstall` Counter |
| `CHANGES-FROM-UPSTREAM.md` | the change log against `8a27053` |
| `.env.example` | names of the optional variables (`OW_CLI_EMAIL`, `OW_CLI_API_KEY`, `OW_BUILD_KEY`, `OW_CLI_API_URL`, `OW_REQUIRE_SIGNING`, `OW_DEV_KEY`, `OW_TAURI_TEST_AD`, `OW_TAURI_REMOTE_DEBUGGING_PORT`), no values |

## 3. Node built-ins used by the main process

| Upstream use | Replacement |
|---|---|
| `events` (`EventEmitter`) | the `events` npm package through `resolve.fallback`; no source change |
| `path` | `path-browserify` through `resolve.fallback`; no source change |
| `__dirname` | webpack `DefinePlugin` `'/browser'`; `loadFile` resolves app-root paths to assets |
| `fs` | `files` from `ow-tauri/main` (scoped, async; CONTRACT B.1.7) |
| `child_process.exec('explorer.exe ...')` | `shell.openPath` (opener plugin) |
| global `process` (`process.argv`, `process.platform` in `index.ts`; `process.versions` in the preload), used without an import | the bootstrap installs a `globalThis.process` shim in `ow-main` and every UI webview before app scripts run (CONTRACT B.2.5); no source change |
| `process.env.NODE_ENV` | webpack 5 defines it from `mode` (`optimization.nodeEnv`); no source change |
| `electron-updater` | `autoUpdater` from `ow-tauri/main` |

## 4. File-by-file map

All paths are under `examples/packages-sample/`; the destination is the same
path unless the note says otherwise.

| Upstream file | Handling | Notes |
|---|---|---|
| `.eslintrc.json` | Unchanged | Upstream lint config; not part of ow-tauri's CI lint. |
| `.gitignore` | Modified | Adds `src-tauri/target/` and `src-tauri/gen/`. |
| `.prettierrc` | Unchanged |  |
| `.vscode/launch.json` | Replaced | `ow-electron` launch replaced by `npm run start-ad` (Tauri dev with `--test-ad`); WebView2 debugging through `OW_TAURI_REMOTE_DEBUGGING_PORT=9222`, which the plugin adds to the one browser-argument set every webview shares (CONTRACT A.1.1; setting `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` directly would replace that set and re-enable background throttling); Rust debugging with CodeLLDB. Dev-mode credentials stay in `.env`, never in the file. |
| `LICENSE` | Unchanged | Overwolf Ltd. MIT notice kept. |
| `README.md` | Modified | New "Running on ow-tauri" section at the top; upstream text kept below it; `npm run start` now exists (defect 10). |
| `dev-app-update.yml` | Unchanged | Read by the ow-tauri update client only when `forceDevUpdateConfig` is set in a debug build (defect 14). |
| `docs/general/index.md` | Unchanged | Upstream package documentation, kept for reference. |
| `docs/gep/game-events-provider.md` | Unchanged | Upstream package documentation, kept for reference. |
| `docs/high-elevation-helper.md` | Unchanged | Upstream package documentation, kept for reference. |
| `docs/index.md` | Unchanged | Upstream package documentation, kept for reference. |
| `docs/overlay/overlay.md` | Unchanged | Upstream package documentation, kept for reference. |
| `docs/packages.md` | Unchanged | Upstream package documentation, kept for reference. |
| `docs/recorder/api-specification.md` | Unchanged | Upstream package documentation, kept for reference. |
| `docs/recorder/recorder.md` | Unchanged | Upstream package documentation, kept for reference. |
| `docs/recorder/types.md` | Unchanged | Upstream package documentation, kept for reference. |
| `package.json` | Modified | `overwolf` and `build.overwolf` blocks, `name`, `productName`, `author`, `version` unchanged (uid continuity, CONTRACT G). Dev dependencies: `@overwolf/ow-electron` and `@overwolf/ow-electron-builder` removed; `ow-tauri`, `@tauri-apps/api`, `@tauri-apps/cli`, `events`, `path-browserify` added; `@overwolf/ow-electron-packages-types` stays on `latest`; `@types/node` stays (`NodeJS.EventEmitter` in the typings). `electron-updater` removed. Scripts: `start`, `start-ad` (`tauri dev -- -- --test-ad`: inside an npm script the first `--` ends the Tauri CLI options and the second passes `--test-ad` to the app), `build` (webpack then `tauri build`). |
| `src/browser/application.ts` | Modified | One entry of the overlay `registerGames` id list is removed, under the repository's naming policy (CONTRIBUTING.md, rule 4); the other ids are unchanged and the change is recorded in `CHANGES-FROM-UPSTREAM.md`. `crashReporter.start` is a documented no-op (partial). Recommended replacement: a Rust crash handler in `src-tauri`. |
| `src/browser/controllers/base.controller.ts` | Unchanged | `events` resolves to the `events` npm polyfill. |
| `src/browser/controllers/gep/game-events.controller.ts` | Modified | `gep-getInfo` returns the info it fetched (defect 7). |
| `src/browser/controllers/main-window.controller.ts` | Modified | `fs.readFileSync/writeFileSync` -> `files.readText/writeText` from `ow-tauri/main` (prefs reads become async); `package.json` read -> `files.readText(app.getAppPath() + '/package.json')`; `open-folder` uses `shell.openPath` instead of `exec('explorer.exe ...')` (security); `get-utm-params` returns `app.overwolf.utmParams` (defect 13); new `disable-ads-optimization` handler (defect 1); `fs.promises.mkdir` -> `files.mkdir` with `$PICTURES/Overwolf/$APPNAME` in `fs.scope`. |
| `src/browser/controllers/overlay/exclusive-mode-window.controller.ts` | Modified | Sends `EXCLUSIVE_MODE` (boolean) to the custom exclusive window on enter/exit and hides it on `HIDE_EXCLUSIVE` (defect 5). |
| `src/browser/controllers/overlay/ingame-windows.controller.ts` | Unchanged |  |
| `src/browser/controllers/overlay/overlay.controller.ts` | Modified | `toggle-osr-visibility` hides when any window is visible, else shows (defect 12); `zOrder-reset` / `passthrough-reset` sent through the `OverlayChannels` constants (defect 8); unused `fs` and `path` imports removed; `update-hotkey` rejects a missing hotkey with a clear error (defect 6). |
| `src/browser/controllers/recorder/recording.controller.ts` | Modified | New `enable-replays` and `recorder-ready` handlers (defect 2); unused `child_process` import removed. |
| `src/browser/index.ts` | Unchanged | `process.argv` and `process.platform` come from the `ow-tauri/electron` process shim; `disableHardwareAcceleration()` is partial (CONTRACT B.2.1). |
| `src/browser/interfaces/recording.service.interface.ts` | Unchanged |  |
| `src/browser/services/base.service.ts` | Unchanged |  |
| `src/browser/services/gep/game-events.service.ts` | Unchanged |  |
| `src/browser/services/overlay/exclusive-mode-window.service.ts` | Unchanged | `path.join(__dirname, '../exclusive/exclusive.html')` resolves through the `__dirname` define. |
| `src/browser/services/overlay/ingame-windows.service.ts` | Modified | The `dpiOsrWindow` loads a remote page, so it is a `remote` window without IPC (security): the injected drag header and its `require('electron').ipcRenderer.send('closeWindow')` button are replaced by native window decorations (`frame: true`). Local overlay windows unchanged. |
| `src/browser/services/overlay/overlay-hotkeys.service.ts` | Unchanged |  |
| `src/browser/services/recorder/recording.service.ts` | Modified | Subscribes to `recorderApi.on('stats')` and forwards it, so `recording-stats` fires (defect 3). |
| `src/browser/services/recorder/replay-game-events-listener.ts` | Unchanged |  |
| `src/browser/services/updater.service.ts` | Modified | `import { autoUpdater } from 'ow-tauri/main'` instead of `electron-updater`; `forceDevUpdateConfig` only when `!app.isPackaged` (defect 14). Feed URL and channel unchanged. |
| `src/browser/services/utility.service.ts` | Unchanged |  |
| `src/common/channels/channels.ts` | Modified | `ZORDER_RESET` corrected to `'zOrder-reset'`, the string actually sent and listened for; unused constants marked deprecated (defect 8). |
| `src/common/recorder/audio-trackes-enum.ts` | Unchanged |  |
| `src/common/recorder/recorder-information.ts` | Unchanged |  |
| `src/common/recorder/recording-status.ts` | Unchanged |  |
| `src/common/utils/promise-resolver.ts` | Unchanged |  |
| `src/global.d.ts` | Unchanged | `owadview` JSX typing and window API types compile as before. |
| `src/preload/preload.ts` | Modified | `version` reads `ow-tauri v${process.versions.owTauri}`; dead `onElevationHelperPrompt` / `sendElevationHelperResponse` removed (defect 4). `require('electron')` resolves through the alias; channel strings unchanged. Injected as an initialization script through `webPreferences.preload`. |
| `src/renderer/app/api-actions/app-actions.ts` | Unchanged |  |
| `src/renderer/app/api-actions/gep-actions.ts` | Unchanged |  |
| `src/renderer/app/api-actions/osr-actions.ts` | Unchanged |  |
| `src/renderer/app/api-actions/overlay-actions.ts` | Unchanged |  |
| `src/renderer/app/api-actions/package-channels-actions.ts` | Unchanged |  |
| `src/renderer/app/api-actions/recording-actions.ts` | Unchanged |  |
| `src/renderer/app/app.tsx` | Unchanged |  |
| `src/renderer/app/components/ad-view.tsx` | Unchanged |  |
| `src/renderer/app/components/ads-tester/ad.tsx` | Modified | Each `<owadview>` gets a unique `id` derived from its container id instead of the shared `mainAd` (defect 9). `cid`, attributes and listeners unchanged. |
| `src/renderer/app/components/ads-tester/ads-tester-logs-display.tsx` | Unchanged |  |
| `src/renderer/app/components/ads-tester/performance-ad.tsx` | Unchanged |  |
| `src/renderer/app/components/app-settings/app-settings-item.tsx` | Unchanged |  |
| `src/renderer/app/components/app-settings/app-settings.tsx` | Unchanged |  |
| `src/renderer/app/components/app-settings/input-elements/any-type-input.tsx` | Unchanged |  |
| `src/renderer/app/components/app-settings/input-elements/dual-option-radio-button.tsx` | Unchanged |  |
| `src/renderer/app/components/app-settings/input-elements/number-input.tsx` | Unchanged |  |
| `src/renderer/app/components/app-settings/input-elements/on-off-toggle-switch.tsx` | Unchanged |  |
| `src/renderer/app/components/app-settings/input-elements/range-slider.tsx` | Unchanged |  |
| `src/renderer/app/components/app-settings/input-elements/toggle-switch.tsx` | Unchanged |  |
| `src/renderer/app/components/app-settings/recording-settings.tsx` | Unchanged |  |
| `src/renderer/app/components/app-settings/show-more-options.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/audio-devices.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/audio-tracks.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/capture-actions.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/capture-audio.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/capture-diagnostics.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/capture-general.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/capture-output-general.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/capture-replay-capture.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/capture-replay.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/capture-splitting.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/capture-status.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/capture-video-encoder-advanced-settings.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/capture-video-encoder-settings.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/capture-video.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/capture.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/constants.ts` | Unchanged |  |
| `src/renderer/app/components/capture/encoder-settings/av1.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/encoder-settings/nvenc-av1.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/encoder-settings/nvenc-hevc.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/encoder-settings/nvenc.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/encoder-settings/qs-hevc.tsx` | Unchanged |  |
| `src/renderer/app/components/capture/encoder-settings/x264.tsx` | Unchanged |  |
| `src/renderer/app/components/check-for-updates/CheckForUpdates.tsx` | Unchanged |  |
| `src/renderer/app/components/cmp/CmpSettings.tsx` | Unchanged |  |
| `src/renderer/app/components/cmp/EHashes.tsx` | Unchanged |  |
| `src/renderer/app/components/cmp/SetEHashes.tsx` | Unchanged |  |
| `src/renderer/app/components/display/DisplaySettings.tsx` | Unchanged |  |
| `src/renderer/app/components/hotkeys/hotkeys-settings.tsx` | Unchanged |  |
| `src/renderer/app/components/json-node.tsx` | Unchanged |  |
| `src/renderer/app/components/layout/Layout.tsx` | Unchanged |  |
| `src/renderer/app/components/layout/header.tsx` | Unchanged | `WebkitAppRegion: 'drag'` works through the renderer's `app-region` emulation on Windows and the overlay title bar on macOS (CONTRACT B.2.2 `frame`). |
| `src/renderer/app/components/layout/icon.tsx` | Unchanged |  |
| `src/renderer/app/components/layout/section-header.tsx` | Unchanged |  |
| `src/renderer/app/components/layout/side-navigation.tsx` | Unchanged |  |
| `src/renderer/app/components/log-view.tsx` | Unchanged |  |
| `src/renderer/app/components/overlay/OverlaySettings.tsx` | Unchanged |  |
| `src/renderer/app/components/overlay/high-elevation-helper.tsx` | Unchanged |  |
| `src/renderer/app/components/overlay/input-exclusive.tsx` | Unchanged |  |
| `src/renderer/app/components/package-channels/package-channels.tsx` | Unchanged |  |
| `src/renderer/app/components/router/routing.tsx` | Unchanged |  |
| `src/renderer/app/components/screenshot/ScreenshotSettings.tsx` | Unchanged |  |
| `src/renderer/app/components/top-buttons.tsx` | Unchanged |  |
| `src/renderer/app/context/app-context.provider.tsx` | Unchanged |  |
| `src/renderer/app/context/app-context.ts` | Unchanged |  |
| `src/renderer/app/context/context-types.ts` | Unchanged |  |
| `src/renderer/app/context/gep-logs-context.tsx` | Unchanged | `ipcRenderer` through the alias. |
| `src/renderer/app/hooks/useLogs.ts` | Unchanged |  |
| `src/renderer/app/hooks/useOverlaySettings.ts` | Unchanged |  |
| `src/renderer/app/hooks/usePackages.ts` | Unchanged |  |
| `src/renderer/app/hooks/useRecording.ts` | Unchanged |  |
| `src/renderer/app/index.tsx` | Unchanged |  |
| `src/renderer/app/styles/ad-container-styles.ts` | Unchanged |  |
| `src/renderer/app/styles/ads-tester-page.ts` | Unchanged |  |
| `src/renderer/app/styles/header.ts` | Unchanged |  |
| `src/renderer/app/styles/icons.tsx` | Unchanged |  |
| `src/renderer/app/styles/logger-page.ts` | Unchanged |  |
| `src/renderer/app/styles/overlay-window-header.tsx` | Unchanged |  |
| `src/renderer/app/styles/overlay-window-styles.tsx` | Unchanged |  |
| `src/renderer/app/styles/package-channels-page.ts` | Unchanged |  |
| `src/renderer/app/styles/reset.ts` | Unchanged |  |
| `src/renderer/app/styles/settings-page.ts` | Unchanged |  |
| `src/renderer/app/styles/sidebar.ts` | Unchanged |  |
| `src/renderer/app/styles/styles.tsx` | Unchanged |  |
| `src/renderer/app/styles/theme-tokens.ts` | Unchanged |  |
| `src/renderer/exclusive/exclusive.html` | Unchanged |  |
| `src/renderer/exclusive/exclusive.ts` | Unchanged | `import { ipcRenderer } from 'electron'` resolves through the alias; `EXCLUSIVE_MODE` / `HIDE_EXCLUSIVE` now have a main-side counterpart (defect 5). |
| `src/renderer/index.html` | Unchanged | Loads Google Fonts: the app's CSP in `tauri.conf.json` allows `fonts.googleapis.com` and `fonts.gstatic.com`. |
| `src/renderer/osr/components/gepLogger.tsx` | Unchanged |  |
| `src/renderer/osr/components/more-actions-buttons.tsx` | Unchanged | `ipcRenderer` through the alias; `devtools` needs a debug build. |
| `src/renderer/osr/components/osr-window-header.tsx` | Unchanged | `ipcRenderer` through the alias; header dragging via `app-region` emulation. |
| `src/renderer/osr/components/overlay-window-settings.tsx` | Unchanged | `ipcRenderer` through the alias. |
| `src/renderer/osr/osr.html` | Unchanged |  |
| `src/renderer/osr/osr.tsx` | Unchanged |  |
| `tsconfig.json` | Modified | The misplaced top-level `typeRoots` is deleted (defect 11); `compilerOptions.paths` maps `electron` and `@overwolf/ow-electron` to ow-tauri's declarations and `types` gains `ow-tauri/types` (CONTRACT B.4). `moduleResolution` stays `node` (ow-tauri ships `typesVersions` for it). |
| `webpack.base.config.js` | Modified | `plugins` no longer shared by reference between configs. |
| `webpack.config.js` | Unchanged |  |
| `webpack.main.config.js` | Modified | `target: 'web'`; `resolve.alias` `electron -> ow-tauri/electron`; `resolve.fallback` `path -> path-browserify`, `events -> events`, `fs`/`child_process` -> `false`; `DefinePlugin` `__dirname = '/browser'` so `path.join(__dirname, '../renderer/index.html')` resolves to the app asset; HtmlWebpackPlugin emits `dist/browser/main.html` (the main webview page, `plugins.overwolf.main.url`); the `electron-updater` external is removed. |
| `webpack.renderer.config.js` | Modified | `target: 'web'`; `resolve.alias` `electron -> ow-tauri/electron` for the `renderer`, `preload`, `osr` and `exclusive` entries (the preload's `require('electron')` resolves through the package's `default` export condition); outputs unchanged. |

## 5. Upstream defects and planned fixes

The numbering matches the issue list we intend to share with Overwolf.

| # | Where | Defect | Planned fix |
|---|---|---|---|
| 1 | `preload.ts` `app.disableAdsOptimization` -> `disable-ads-optimization` | No `ipcMain.handle`; the invoke rejects and the "Disable ads optimization" button logs an error. | Add the handler in `main-window.controller.ts`: `app.overwolf.disableAdsOptimization()`. |
| 2 | `preload.ts` `recorder.enableReplays` -> `enable-replays`, `recorder.recorderReady` -> `recorder-ready` | No handlers. | `enable-replays (enabled)` starts or stops replays through the existing service calls; `recorder-ready` resolves whether the recorder package is ready. |
| 3 | `recording.controller.ts` forwards service `stats` | `RecordingService` never subscribes to `recorderApi.on('stats')`, so `recording-stats` never fires. | Subscribe in `RecordingService` and emit `stats`. |
| 4 | `elevation-helper-prompt` / `elevation-helper-response` | Never emitted / never handled; the overlay controller installs the helper itself. | Remove the two dead preload members (the UI never uses them); keep the controller's documented auto-install flow. |
| 5 | `exclusive.ts` `HIDE_EXCLUSIVE` / `EXCLUSIVE_MODE` | No main-side counterpart. | `ExclusiveModeWindowController` sends `EXCLUSIVE_MODE` on enter/exit and hides the window on `HIDE_EXCLUSIVE`. |
| 6 | `preload.ts` `osr.updateHotkey()` | Invokes `update-hotkey` without a hotkey; the handler would throw a `TypeError` on `hotkey.name`. | The `update-hotkey` handler rejects a missing or nameless hotkey with a clear error; preload unchanged. |
| 7 | `game-events.controller.ts` `gep-getInfo` | Returns `undefined`; the UI logs "undefined". | Return the result of `getInfo`. |
| 8 | `channels.ts` | `ZORDER_RESET = 'zorder-reset'` differs from the `'zOrder-reset'` actually sent; several constants unused. | Correct the constant, send through the constants, mark unused ones deprecated. |
| 9 | `ad.tsx` | Every `<owadview>` gets `id="mainAd"`: duplicate DOM ids. | Derive a unique id from the container id. |
| 10 | `README.md` | Documents `npm run start`, which does not exist. | Add a `start` script and update the README. |
| 11 | `tsconfig.json` | `typeRoots` is outside `compilerOptions` and ignored. | Delete it. Moving it inside would break resolution: its entries are absolute (`/node_modules/...`), it would replace the default `./node_modules/@types` (losing `@types/node` and `@types/react`), and `@overwolf/ow-electron-packages-types` is a package, not a type root. The types are reached through explicit imports and the `types` field. |
| 12 | `overlay.controller.ts` `toggle-osr-visibility` | Only shows windows, never hides them. | Hide when any window is visible, else show. |
| 13 | `main-window.controller.ts` `get-utm-params` | Reads ow-electron's private JSON although `app.overwolf.utmParams` exists. | Return `app.overwolf.utmParams`. |
| 14 | `updater.service.ts` | `forceDevUpdateConfig = true` defeats the "not packaged" guard, so development builds update too. | Set it only for unpackaged builds and keep the guard. |

Security fixes made during the port (beyond the list above):

- `open-folder` passed a renderer-supplied path to `exec("explorer.exe ...")`
  (shell injection). It now uses `shell.openPath`.
- Overlay windows ran with `nodeIntegration: true, contextIsolation: false`,
  and one loaded a remote site with an injected IPC button. The ported code
  creates them as ordinary windows with only the renderer permission set, and
  the remote window gets no IPC at all. They are created only once the overlay
  package is ready, which does not happen while packages are deferred.

## 6. What does not port, and why

| Feature | Status | Reference |
|---|---|---|
| GEP, overlay injection, in-game hotkeys, exclusive mode, recorder, utility, CRN | Deferred. Packages are reported as unavailable exactly as ow-electron reports them where they are not available: no `ready` or `failed-to-initialize` event, `getChannel()` resolves `{}`, `getAvailableChannels()` rejects. The sample's package screens stay in their waiting state. The runtime interface is a design appendix | CONTRACT H, Appendix P, ADR 0004, OQ-21 |
| Offscreen-rendered overlay windows and shared textures | Deferred with the packages; no WebView2 / WKWebView equivalent | OQ-33 |
| `owutility.dll` ad optimisation | Not shipped | OQ-14 |
| Overwolf signing (`requireSigning`, `enableOWCertSigning`) | `ow-tauri sign` runs the published builder's flow: `/sign/electron`, `integrity.dll`, the `OWEINTEGRITY/OWE` resource, optional Overwolf Authenticode, and the same build gating. `/sign/asar` is not done and not faked (Tauri has no asar) | CONTRACT G.4, ADR 0016, OQ-09 |
| `crashReporter` | No-op; use a Rust crash handler | CONTRACT B.2.5 |
| Display friendly names from the recorder's monitor list | Available only with a recorder runtime; otherwise the OS monitor name or `Display N` | CONTRACT B.2.5 |
