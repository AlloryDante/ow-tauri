# ow-tauri Packages Sample

A Tauri 2 app on [`tauri-plugin-overwolf`](../../crates/tauri-plugin-overwolf)
that does what Overwolf's ow-electron packages sample does, where Tauri can:
a log of every Overwolf call and ad event, an ads tester, consent and
settings, the Overwolf updater, and a plain account of the packages that do
not exist on Tauri. It is a Tauri-native app: one window, React 19 + Vite,
the Rust side is a few lines of plugin setup, and the page talks to the
plugin through `tauri-plugin-overwolf-api`. There is no Electron API, no
hidden main process and no compatibility layer.

## Prerequisites

- Node 22.12 or newer and the [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/)
  for your OS (Rust, WebView2 on Windows, WebKitGTK on Linux).
- From the repository root, once:

```shell
npm install
npm run build --workspace tauri-plugin-overwolf-api
```

The second line builds the API package's `dist/`, which the Vite build
imports (the unit tests read its TypeScript sources directly).

## Scripts

Run from `examples/packages-sample`.

| Script                | What it does                                                                          |
| --------------------- | ------------------------------------------------------------------------------------- |
| `npm start`           | Debug build with the page embedded, then runs it ([scripts/run.mjs](scripts/run.mjs)) |
| `npm run start:test`  | The same with `--test-ad`: test ad inventory                                          |
| `npm run dev`         | `tauri dev` with hot reload                                                           |
| `npm run dev:test`    | `tauri dev` with `--test-ad`                                                          |
| `npm run build`       | Vite production build of the page (`dist/`)                                           |
| `npm run tauri build` | Release bundle (NSIS on Windows)                                                      |
| `npm run typecheck`   | `tsc` in strict mode                                                                  |
| `npm run lint`        | Typed ESLint (React hooks rules included)                                             |
| `npm test`            | Vitest with happy-dom and the API's `mockOverwolf`                                    |
| `npm run check:rust`  | `cargo fmt --check` and `clippy -D warnings`, with and without the `lab` feature      |
| `npm run lab:smoke`   | The invisible lab smoke run (macOS, see [e2e/README.md](e2e/README.md))               |
| `npm run doctor`      | `ow-tauri doctor`: checks the project's Overwolf setup                                |

Use `start:test` or `dev:test` while developing: real ad inventory is for
the published app.

## Pages

**Logger** — the start page. At launch the app logs `getInfo()` (uid, host,
ads and analytics state, UTM parameters) and `isCMPRequired()`. Every API
call of the other pages appears here with its arguments and its result or
error code, and so does every ad event. Values are expandable JSON trees;
the search box filters by message and value.

**Ads Tester** — fifteen layouts of display slots (160x600, 300x250,
400x600, 400x60, 728x90) inside a mock game window, the 400x300 video slot,
and a high-impact layout that grows its slot when the ad asks.
Each slot can be started, removed, recreated (a new `<owadview>`) and muted
(`setAudioMuted`). **Performance ad** shows an interstitial
(`<owadview performance>`) and removes it when it ends. The ad events of
the page are listed under the slots. The `<owadview>` runtime is installed
once by `import 'tauri-plugin-overwolf-api/adview'` in
[src/main.tsx](src/main.tsx); [src/pages/ads/AdSlot.tsx](src/pages/ads/AdSlot.tsx)
attaches the listeners with a `ref` and `addEventListener`, since the ad
events are DOM events with underscore names (`display_ad_loaded`) that
React's `on*` props do not map.

**CMP & Settings** — whether consent rules apply, the ad privacy settings
window (`openAdPrivacySettingsWindow`, with a tab) and the CMP window,
e-mail hashes (the address itself is never logged), the app identity and
UTM parameters, the machine ids (masked unless "Show in full" is checked),
and the analytics and ads switches.

**Updater** — `check()` against Overwolf's update feed, then
`downloadAndInstall()` with its `Started`, `Progress` and `Finished` events.
The Overwolf updater is Windows only: the Rust crate turns on the plugin's
`updater` feature for Windows targets, and on macOS and Linux the page shows
the `unsupported` error as it is. A check only finds an update once a
version of the app is published in the Overwolf console.

**Packages** — GEP, overlay, recorder and utility, what each does on
ow-electron, and why it is not available on Tauri: they run inside
ow-electron's package runtime and patched Electron, which Tauri does not
have. The page does not fake them.

## Configuration

- [src-tauri/tauri.conf.json](src-tauri/tauri.conf.json): the app identity
  (`plugins.overwolf`: author and name as in the Overwolf console; this
  sample uses the neutral "Example Studio"), the content security policy,
  and `analytics.userSwitch`, which allows `setAnalyticsUserEnabled`.
- [src-tauri/tauri.windows.conf.json](src-tauri/tauri.windows.conf.json):
  NSIS with the plugin's installer hooks (`gen/overwolf/installer-hooks.nsh`,
  written by `build.rs`) and the updater's trusted publisher name.
- [src-tauri/capabilities/default.json](src-tauri/capabilities/default.json):
  the `main` webview gets `overwolf:default` plus the opt-in machine id,
  e-mail hash, analytics and updater permissions.
- [src-tauri/src/sample.rs](src-tauri/src/sample.rs): single instance first,
  logging, the Overwolf plugin, one window shown once its page has loaded,
  and on macOS the hook that lets the plugin recover a crashed ad page.

## Based on

This app is based on Overwolf's
[ow-electron-packages-sample](https://github.com/overwolf/ow-electron-packages-sample)
(commit `8a27053`, MIT, Copyright Overwolf Ltd.; [LICENSE](LICENSE)). From
it come the page structure (logger, ads tester, settings, packages), the
ads tester's layouts and slot sizes, the log view (search, expandable JSON
values) and the dark theme. It differs from upstream in these ways:

- It is a Tauri app, not an Electron app: no main process, no preload, no
  IPC channels. The page calls `tauri-plugin-overwolf-api` directly, and the
  plugin's permissions in the capability decide what it may call.
- The GEP, overlay, recorder and utility packages are shown as not
  available instead of being driven, since Tauri has no package runtime.
- The updater uses the plugin's updater (Overwolf's feed, Windows) instead
  of `electron-updater`.
- Webpack, Electron Builder and their configuration are replaced by Vite and
  the Tauri CLI; the TypeScript is strict and the ESLint rules are typed.
- New: the CMP & Settings page's identity card and switches, the updater
  page, unit tests for every module, and the invisible lab smoke run.
