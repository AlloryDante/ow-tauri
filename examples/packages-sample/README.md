# ow-tauri packages sample

Overwolf's ow-electron packages sample, ported to Tauri 2 on
[`tauri-plugin-overwolf`](../../crates/tauri-plugin-overwolf). It has the same pages as the
upstream sample where Tauri can support them: a log of every Overwolf call and ad event, an ads
tester, consent and settings, the Overwolf updater, and a page that explains why the Overwolf
packages are not available on Tauri.

Use it if you know the ow-electron sample and want to see the same calls on Tauri, or if you want
to try each plugin API by hand and read its result. For the smallest app with one ad, start with
[quickstart-vanilla](../quickstart-vanilla) instead. To move your own ow-electron app, read
[MIGRATION.md](../../docs/MIGRATION.md).

The app has one window with a React 19 and Vite page, and the page calls the plugin directly
through `tauri-plugin-overwolf-api`. The Rust side is a few lines of plugin setup. No Electron code
runs.

To run it with test ads, from this folder after the setup below:

```sh
npm run start:test
```

## Set up

You need Node.js 22.12 or newer, Rust 1.90 or newer and the
[Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for your OS (WebView2 on Windows,
WebKitGTK on Linux). Ads show on Windows and macOS. On Linux the app builds and runs, but
ads report `unsupported` ([COMPATIBILITY.md](../../docs/COMPATIBILITY.md)).

The sample uses the repository's crate by path and the repository's npm packages as workspaces,
so it runs from a clone. Run this once:

```sh
git clone https://github.com/AlloryDante/ow-tauri
cd ow-tauri
npm install
npm run build --workspace tauri-plugin-overwolf-api --workspace tauri-plugin-overwolf-cli
```

The last line builds the `dist/` folders of the two packages. The Vite build imports the API
package's `dist/`, and `npm run doctor` runs the CLI from its `dist/`. The unit tests read the API's
TypeScript sources directly, so `npm test` does not need the build.

## Run

From `examples/packages-sample`:

| Script               | What it does                                                                                        |
| -------------------- | --------------------------------------------------------------------------------------------------- |
| `npm run start:test` | Builds a debug app with the page embedded and runs it with `--test-ad` ([scripts/run.mjs](scripts/run.mjs)) |
| `npm start`          | The same without `--test-ad`                                                                        |
| `npm run dev:test`   | `tauri dev` with hot reload and `--test-ad`                                                         |
| `npm run dev`        | `tauri dev` with hot reload                                                                         |

The sample's configuration does not turn test ads on, so `npm start` and `npm run dev` request
live ads. Use `start:test` or `dev:test` while you develop. Live ad inventory is for the published
app.

`npm start` and `npm run start:test` run `tauri build --debug --no-bundle` and then start the binary
from `src-tauri/target/debug` (`CARGO_TARGET_DIR` is honoured). `node scripts/run.mjs -- <args>`
passes the arguments after `--` to the app, and `node scripts/run.mjs --no-build` runs the last
build again. Under `tauri dev` the page comes from Vite's dev server on port 1420.

## Pages

The navigation on the left selects one of five pages. The URL hash (`#ads`, `#settings` and so on)
keeps the page across a reload.

### Logger

This is the start page. At launch the app logs `getInfo()` (uid, host, ads and analytics state, UTM
parameters) and `isCMPRequired()`. Every API call made on the other pages appears here with its
arguments and its result or error code, and so does every ad event. Values are expandable JSON
trees, and the search box filters by message and value.

### Ads tester

Fifteen layouts, each with two ad slots, inside a mock game window. The display sizes are 160x600,
300x250, 400x600, 400x60 and 728x90, and four layouts use the 400x300 video slot. The high impact
layout grows its 400x600 slot when the ad asks.

Each slot can be started, removed, recreated (a new `<owadview>`) and muted (`setAudioMuted`). The
Performance ad button shows an interstitial (`<owadview performance>`) and removes it when it ends.
The page's ad events are listed under the slots.

The `<owadview>` runtime is installed once by `import 'tauri-plugin-overwolf-api/adview'` in
[src/main.tsx](src/main.tsx). [src/pages/ads/AdSlot.tsx](src/pages/ads/AdSlot.tsx) attaches the
listeners with a `ref` and `addEventListener`, because the ad events are DOM events with underscore
names (`display_ad_loaded`) that React's `on*` props do not map.

### CMP and settings

The "CMP & Settings" page shows whether consent rules apply and opens the ad privacy settings window
(`openAdPrivacySettingsWindow`, with a tab) and the CMP window. It also has these cards:

- e-mail hashes (the address itself is never logged);
- the app identity and UTM parameters;
- the machine ids, masked unless you check "Show in full";
- the analytics and ads switches.

### Updater

The page runs `check()` against Overwolf's update feed, then `downloadAndInstall()` with its
`Started`, `Progress` and `Finished` events. The Overwolf updater is Windows only. The Rust crate turns on the
plugin's `updater` feature for Windows targets, and on macOS and Linux the page shows the
`unsupported` error as it is. A check only finds an update once a version of the app is published in
the Overwolf console.

### Packages

The page lists GEP, overlay, recorder and utility, says what each does on ow-electron, and
explains why it is not available on Tauri. These packages run inside ow-electron's package runtime and patched Electron, which Tauri does not
have. The page does not fake them.

## Where the plugin is wired in

- [src-tauri/Cargo.toml](src-tauri/Cargo.toml): `tauri-plugin-overwolf` as a dependency, with the
  `updater` feature on Windows targets, and as a build dependency with the `build` feature. In your
  own app it is a git dependency on this repository; it is not on crates.io or npm yet
  ([GETTING-STARTED](../../docs/GETTING-STARTED.md#before-you-start)).
- [src-tauri/build.rs](src-tauri/build.rs): the plugin's build step before `tauri-build`.
- [src-tauri/src/sample.rs](src-tauri/src/sample.rs): single instance first, logging, the Overwolf
  plugin, one window shown once its page has loaded, and on macOS the hook that lets the plugin
  recover a crashed ad page.
- [src-tauri/tauri.conf.json](src-tauri/tauri.conf.json): the app identity in `plugins.overwolf`
  (author and name as in the Overwolf console; this sample uses the neutral "Example Studio"), the
  content security policy, and `analytics.userSwitch`, which allows `setAnalyticsUserEnabled`.
- [src-tauri/tauri.windows.conf.json](src-tauri/tauri.windows.conf.json): NSIS with the plugin's
  installer hooks (`gen/overwolf/installer-hooks.nsh`, written by `build.rs`) and the updater's
  trusted publisher name.
- [src-tauri/capabilities/default.json](src-tauri/capabilities/default.json): the `main` webview
  gets `overwolf:default` plus the opt-in permissions `overwolf:machine-id`,
  `overwolf:email-hashes`, `overwolf:analytics` and `overwolf:updater`.
- [.env.example](.env.example): the signing credentials `ow-tauri sign` reads from the environment
  for a Windows release build. Copy it to `.env` (git-ignored) and load it into your shell, or set
  the variables as CI secrets. `ow-tauri` does not read `.env` itself. Never commit values.

## Coming from ow-electron

The sample has no main process, no preload script and no IPC channels. The page calls the plugin,
and the plugin's permissions in the capability decide which calls it may make.
[CHANGES-FROM-UPSTREAM.md](CHANGES-FROM-UPSTREAM.md) lists every difference, and
[MIGRATION.md](../../docs/MIGRATION.md) covers moving your own app.

## Other scripts

These are for working on the sample itself. Run them from `examples/packages-sample`.

| Script                | What it does                                                                          |
| --------------------- | ------------------------------------------------------------------------------------- |
| `npm run build`       | Vite production build of the page (`dist/`)                                           |
| `npm run tauri build` | Release bundle (NSIS on Windows)                                                      |
| `npm run typecheck`   | `tsc` in strict mode                                                                  |
| `npm run lint`        | Typed ESLint (React hooks rules included)                                             |
| `npm test`            | Vitest with happy-dom and the API's `mockOverwolf`                                    |
| `npm run check:rust`  | `cargo fmt --check` and `clippy -D warnings`, with and without the `lab` feature      |
| `npm run lab:smoke`   | The invisible lab smoke run (macOS, see [e2e/README.md](e2e/README.md))               |
| `npm run doctor`      | `ow-tauri doctor`: checks the project's Overwolf setup                                |

## Based on

This app is based on Overwolf's
[ow-electron-packages-sample](https://github.com/overwolf/ow-electron-packages-sample)
(commit `8a27053`, MIT, Copyright Overwolf Ltd.; [LICENSE](LICENSE)). The page structure (logger,
ads tester, settings, packages), the ads tester's layouts and slot sizes, the log view (search,
expandable JSON values) and the dark theme come from it. What changed against upstream is listed in
[CHANGES-FROM-UPSTREAM.md](CHANGES-FROM-UPSTREAM.md).
