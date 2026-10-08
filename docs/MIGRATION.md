# Migrating from ow-electron to ow-tauri

This guide moves an Overwolf ow-electron app onto Tauri 2 with ow-tauri. The
TypeScript app stays: the main process runs in a hidden webview (`ow-main`,
[ADR 0001](adr/0001-hidden-main-webview.md)), `electron` imports resolve to
`ow-tauri/electron` ([ADR 0002](adr/0002-electron-subset-alias.md)),
`package.json` stays the manifest, and a small `src-tauri/` crate registers
the plugin. Ads, consent, analytics, the uid and the update feed behave as
on ow-electron ([PARITY.md](PARITY.md)).

Every step below was checked against the two examples in this repository:

- [`examples/packages-sample`](../examples/packages-sample/README.md): Overwolf's
  official ow-electron packages sample, ported with webpack. Every change
  against upstream is listed in
  [CHANGES-FROM-UPSTREAM.md](../examples/packages-sample/CHANGES-FROM-UPSTREAM.md),
  and the design behind it in [PORT-MAP.md](PORT-MAP.md).
- [`examples/ad-showcase`](../examples/ad-showcase/README.md): one app that
  builds for ow-electron and for ow-tauri from the same sources, with
  rolldown.

Where this guide and [CONTRACT.md](CONTRACT.md) differ, the contract wins.

## Contents

- [Before you start](#before-you-start)
- [1. Audit the app](#1-audit-the-app)
- [2. Install ow-tauri](#2-install-ow-tauri)
- [3. Add the Tauri shell](#3-add-the-tauri-shell)
- [4. Bundle for webviews](#4-bundle-for-webviews)
- [5. Point TypeScript at the ow-tauri typings](#5-point-typescript-at-the-ow-tauri-typings)
- [6. Replace Node built-ins and Electron-only libraries](#6-replace-node-built-ins-and-electron-only-libraries)
- [7. Make synchronous calls asynchronous](#7-make-synchronous-calls-asynchronous)
- [8. Network: fetch, CORS and CSP](#8-network-fetch-cors-and-csp)
- [9. Your own Rust commands](#9-your-own-rust-commands)
- [10. Run with test ads](#10-run-with-test-ads)
- [11. Sign the build](#11-sign-the-build)
- [12. Keep the update feed](#12-keep-the-update-feed)
- [13. CI](#13-ci)
- [14. Debug the main process](#14-debug-the-main-process)
- [15. What changes for your users](#15-what-changes-for-your-users)
- [API mapping](#api-mapping)

## Before you start

- **Packages are not available.** GEP, overlay, recorder, utility and CRN
  need Overwolf's package runtime, which exists for ow-electron only. ow-tauri
  reports them as ow-electron does where they are not available:
  `app.overwolf.packages.gep` and the others are `undefined` and no `ready`
  event fires ([CONTRACT H](CONTRACT.md#h-packages)). An app whose core
  feature is game events or the overlay cannot move yet.
- **ow-tauri is not published** to npm or crates.io. Use a git or path
  dependency ([step 2](#2-install-ow-tauri)).
- **Toolchain:** Node 22.12 or newer, Rust 1.90 or newer, and the
  [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for each
  OS you build on (WebView2 on Windows, WebKitGTK 4.1 on Linux).
- **Platforms:** Windows is the target with full parity. macOS and Linux run
  the app and its ads with known gaps
  ([PARITY.md, Known platform gaps](PARITY.md#known-platform-gaps)), and
  Overwolf's update feed serves Windows setups only.

## 1. Audit the app

Run these from the app's root before changing anything. Each hit is a line
to change in steps 6 and 7, or an API from the
[not supported](#electron-api) list.

```shell
# Node built-ins in main-process and preload code (the webview has none)
grep -rnE "(from|require\()\s*['\"](node:)?(fs|fs/promises|os|child_process|crypto|net|http|https|stream|zlib|worker_threads|buffer|util|url|readline|dgram|dns|tls|cluster|v8|vm)['\"]" src

# Node globals the webview does not define
grep -rnE "\b(__filename|Buffer|global|setImmediate)\b" src

# Synchronous IPC and synchronous dialogs
grep -rnE "sendSync|returnValue|show(Open|Save)DialogSync|showMessageBoxSync" src

# Electron modules ow-tauri does not provide
grep -rnE "\b(Menu|MenuItem|Tray|Notification|session|protocol|clipboard|nativeImage|safeStorage|BrowserView|WebContentsView|BaseWindow|utilityProcess|MessageChannelMain|desktopCapturer|powerMonitor|powerSaveBlocker|systemPreferences|webFrame)\b" src

# Electron-only and Node-only dependencies
npm ls --depth=0 | grep -E "electron|keytar|ffi|serialport|sqlite|sharp|node-gyp|\.node"
```

Native Node modules (`.node` files) cannot load in a webview. Their work
moves to Rust ([step 9](#9-your-own-rust-commands)).

## 2. Install ow-tauri

Remove the Electron toolchain and add ow-tauri and the Tauri CLI:

```shell
npm uninstall @overwolf/ow-electron @overwolf/ow-electron-builder electron-updater
npm install --save-dev @tauri-apps/cli@2.12.1 @tauri-apps/api@2.12.1
```

Keep `@overwolf/ow-electron-packages-types` on its `latest` dist-tag: the
ow-tauri typings re-declare the `overwolf.packages` types from it. Remove
`@overwolf/ow-electron` itself: its Electron typings would then sit next to
ow-tauri's `electron` declarations.

Until ow-tauri is published, install it from a checkout. `<ow-tauri>` is
the folder of your clone of this repository:

```shell
git clone <ow-tauri repository URL> <ow-tauri>
cd <ow-tauri> && npm install && npm pack --workspace ow-tauri
# in the app:
npm install --save-dev <ow-tauri>/ow-tauri-0.1.0.tgz
```

`npm pack` builds the package (`prepack`) and writes a tarball, so the app
gets a copy with its `dist/` folder. A `file:` dependency on
`<ow-tauri>/packages/ow-tauri` works too, but then run
`npm run build --workspace ow-tauri` in the checkout after every update.

The Rust crate comes from the same checkout, as a git or a path dependency
([step 3.1](#31-cargotoml)).

Add the scripts the examples use:

```jsonc
{
  "scripts": {
    "start": "tauri dev",
    "start-ad": "tauri dev -- -- --test-ad",
    "build:ow-tauri": "dotenv --override --no-expand -- ow-tauri sign && dotenv --override --no-expand -- tauri build"
  }
}
```

`build:ow-tauri` replaces `build:ow-electron`. `dotenv-cli` (a
devDependency) loads `.env` for signing ([step 11](#11-sign-the-build)).

Apart from dependencies and scripts, `package.json` does not change: `name`,
`productName`, `author`, `version`, `overwolf` and `build.overwolf` keep
their meaning, so the uid stays the same ([CONTRACT G](CONTRACT.md#g-manifest)). The other `build.*`
keys belonged to electron-builder and are ignored; their Tauri equivalents
go into `tauri.conf.json` ([step 3.4](#34-tauriconfjson)).

> **`productName` versus `build.productName`.** The uid and the app name
> come from the top-level `productName`, else `name`. ow-electron ignores
> `build.productName` at run time, and so does ow-tauri: the build prints a
> warning when only `build.productName` is set. Do not "fix" this by adding
> a top-level `productName` to an app that already shipped: the uid would
> change. The packages sample keeps upstream's layout, so its build shows
> that warning.

## 3. Add the Tauri shell

Create `src-tauri/` next to `package.json`. The packages sample's
[`src-tauri`](../examples/packages-sample/src-tauri/) is the template.

### 3.1 `Cargo.toml`

```toml
[package]
name = "my-app"
version = "1.0.0"
edition = "2024"
rust-version = "1.90"

[dependencies]
tauri = { version = "2.12.1", features = ["unstable"] }
tauri-plugin-single-instance = "2.5.2"
serde_json = "1" # generate_context! expands to serde_json calls
tauri-plugin-overwolf = { git = "<ow-tauri repository URL>", tag = "<release tag>" }

[build-dependencies]
tauri-build = { version = "2.7.1", features = [] }
# Only the manifest parser and build helpers; add "embed-resource" for
# signed Windows builds (step 11).
tauri-plugin-overwolf = { git = "<ow-tauri repository URL>", tag = "<release tag>", default-features = false }
```

- `unstable` is required: ad guests are Tauri child webviews
  ([ADR 0003](adr/0003-owadview-native-child-webviews.md)).
- With a local checkout, use `path = "<ow-tauri>/crates/tauri-plugin-overwolf"`
  instead of `git` and `tag`, as the examples do.
- The plugin's `lab` feature is for this repository's end-to-end lab. Never
  enable it in an app you ship.

### 3.2 `build.rs`

```rust
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Embeds package.json (CONTRACT G.3) and prints a cargo warning for
    // every manifest, tauri.conf.json and signing problem it finds.
    tauri_plugin_overwolf::build::embed_manifest("../package.json")?;

    // Overwolf's install and uninstall work in the NSIS installer (CONTRACT I.6).
    let dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let dir = Path::new(&dir);
    tauri_plugin_overwolf::build::write_nsis_installer_hooks(
        &dir.join("../package.json"),
        None,    // the plugin's `uid` override, if you set one
        "tauri", // analytics.hostLabel
        &dir.join("windows/hooks.nsh"),
    )?;

    tauri_build::try_build(tauri_build::Attributes::new())?;
    Ok(())
}
```

Add `src-tauri/target/`, `src-tauri/gen/`, `src-tauri/windows/hooks.nsh`,
`ow-tauri-signed/` and `.env` to `.gitignore`: all of them are generated or
local.

### 3.3 `main.rs`

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri_plugin_overwolf::OverwolfExt;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let builder = tauri::Builder::default()
        // Single instance must be the first plugin; a second launch reaches
        // the running app as app.on('second-instance').
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            app.overwolf().emit_second_instance(argv, cwd);
        }))
        .plugin(
            tauri_plugin_overwolf::Builder::new()
                .manifest_json(tauri_plugin_overwolf::embedded_manifest!())
                .build(),
        );

    // macOS reports a crashed web content process only through this hook.
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(|webview| {
        webview
            .overwolf()
            .report_web_content_terminated(webview.label());
    });

    builder.build(tauri::generate_context!())?.run(|_, _| {});
    Ok(())
}
```

Nothing else belongs here. The app's logic stays in TypeScript.

### 3.4 `tauri.conf.json`

```jsonc
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "<the productName (else name) of package.json>",
  "version": "<the version of package.json>",
  "identifier": "com.example.my-app",
  "build": { "frontendDist": "../dist" },
  "app": {
    // No windows: the plugin creates ow-main, your code creates the rest.
    "windows": [],
    "security": {
      "csp": "default-src 'self'; script-src 'self'; connect-src 'self' ipc: http://ipc.localhost; img-src 'self' data:; style-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "windows": {
      "nsis": { "installMode": "both", "installerHooks": "windows/hooks.nsh" }
    }
  },
  "plugins": {
    "overwolf": {
      // The page that loads the main-process bundle (step 4).
      "main": { "url": "browser/main.html" },
      // Extra folders files and shell.openPath may use.
      "fs": { "scope": ["$PICTURES/Overwolf/$APPNAME", "$VIDEOS/$APPNAME"] }
    }
  }
}
```

- `productName` and `version` must match `package.json`; the build warns
  when they differ.
- Do not set `app.security.freezePrototype`. Tauri would inject it into
  every webview, ad guests and consent pages included, and ow-electron
  changes nothing in those pages.
- The CSP is the baseline of
  [ARCHITECTURE section 5.5](ARCHITECTURE.md#55-content-security-policy).
  Add what your pages load: the packages sample adds
  `'unsafe-inline' https://fonts.googleapis.com` to `style-src`,
  `font-src https://fonts.gstatic.com`, and
  `"dangerousDisableAssetCspModification": ["style-src"]` for its inline
  styles. Hosts your code fetches go into `connect-src`
  ([step 8](#8-network-fetch-cors-and-csp)).
- Every `plugins.overwolf` field is optional; the full list is
  [CONTRACT A.1](CONTRACT.md#a1-configuration).
- electron-builder's `build.win`, `build.nsis`, `build.mac` and so on map to
  `bundle.*`. The packages sample keeps NSIS only on Windows through
  `tauri.windows.conf.json` (`"bundle": { "targets": ["nsis"] }`).

### 3.5 Capabilities

`src-tauri/capabilities/ui.json` gives your `BrowserWindow` webviews the
renderer permission set. Nothing else is needed:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "ui",
  "local": true,
  "webviews": ["bw-*"],
  "permissions": ["overwolf:renderer"]
}
```

Name webview labels only, never `windows`: a capability matched by window
label would also reach the ad guests and remote pages inside that window.
The plugin adds the capabilities of `ow-main`, the ad guests, the consent
windows and window dragging itself
([ARCHITECTURE section 5.2](ARCHITECTURE.md#52-capabilities)). Grant no
`core:event:*` permission.

## 4. Bundle for webviews

All app code now runs in webviews, so every bundle targets the browser:

| Bundle | ow-electron | ow-tauri |
|---|---|---|
| main process | Node, CommonJS, `electron` external | browser; `electron` aliased to `ow-tauri/electron`; loaded by an HTML page named in `plugins.overwolf.main.url` |
| preload | Node-flavoured renderer script | browser, a classic script (no `import` statements left), `electron` aliased; injected as an initialization script through `webPreferences.preload` |
| renderer pages | browser | browser, unchanged; `electron` aliased if they import it |

Rules for every bundler:

- Alias `electron` to `ow-tauri/electron` in the main and preload bundles
  (and in renderer bundles that import `electron`). `@overwolf/ow-electron`
  imports, if any, get the same alias.
- Polyfill or remove Node built-ins ([step 6](#6-replace-node-built-ins-and-electron-only-libraries)).
- Define `__dirname` for the main bundle as the folder of the bundle inside
  the frontend dist (the sample: `'/browser'`). `loadFile(path.join(__dirname,
  '../renderer/index.html'))` then names an app asset.
- Define `process.env.NODE_ENV`. ow-tauri's `process` shim has an empty
  `env` apart from `OVERWOLF_APP_UID`.
- Emit an HTML page that loads the main bundle, and point
  `plugins.overwolf.main.url` at it.

### webpack (used by the packages sample)

```js
// webpack.main.config.js
const path = require('path');
const webpack = require('webpack');
const HtmlWebpackPlugin = require('html-webpack-plugin');

module.exports = {
  target: 'web',
  entry: { index: './src/browser/index.ts' },
  output: { path: path.join(__dirname, 'dist/browser'), filename: '[name].js' },
  resolve: {
    extensions: ['.ts', '.tsx', '.js', '.json'],
    alias: { electron: 'ow-tauri/electron' },
    fallback: {
      path: require.resolve('path-browserify'),
      events: require.resolve('events/'),
      fs: false,
      child_process: false,
    },
  },
  node: { __dirname: false },
  plugins: [
    new webpack.DefinePlugin({ __dirname: JSON.stringify('/browser') }),
    new HtmlWebpackPlugin({ title: 'main', filename: 'main.html', chunks: ['index'] }),
  ],
  module: { rules: [{ test: /\.tsx?$/, use: 'ts-loader', exclude: /node_modules/ }] },
};
```

The renderer config needs only `target: 'web'` and the same alias. webpack 5
defines `process.env.NODE_ENV` from `mode`. Install `path-browserify` and
`events` as devDependencies. Do not share one `plugins` array between the
main and renderer configs: the sample's upstream did, and renderer pages
leaked into the main bundle.

### rolldown (used by the ad showcase)

```js
// rolldown.config.mjs
import { defineConfig } from 'rolldown';

const alias = { electron: 'ow-tauri/electron' };
export default defineConfig([
  {
    input: 'src/main/main.ts',
    platform: 'browser',
    resolve: { alias },
    tsconfig: false, // the tsconfig paths point at .d.ts files (step 5)
    transform: { define: { 'process.env.NODE_ENV': '"production"' } },
    output: { file: 'dist/main/main.js', format: 'iife' },
  },
  {
    input: 'src/preload/preload.ts',
    platform: 'browser',
    resolve: { alias },
    tsconfig: false,
    output: { file: 'dist/preload/preload.js', format: 'iife' },
  },
]);
```

The showcase writes `main/main.html` with a single
`<script src="main.js"></script>` and sets `plugins.overwolf.main.url` to
`main/main.html`. `tsconfig: false` matters: with it on, rolldown would
resolve `electron` through the tsconfig `paths` to a declaration file.

### Vite

Not used by the examples; checked with Vite 8.3 on a small main process and
preload. Build the main process as its own entry page and the preload as a
classic script:

```js
// vite.config.js: the main process. index.html at the project root holds
// <script type="module" src="/src/main/main.ts"></script>.
import { defineConfig } from 'vite';

export default defineConfig({
  base: './',
  resolve: { alias: { electron: 'ow-tauri/electron', path: 'path-browserify' } },
  define: { __dirname: JSON.stringify('/') },
  build: { outDir: 'dist', target: 'es2022' },
});
```

```js
// vite.preload.config.js: library mode, one classic script.
import { defineConfig } from 'vite';

export default defineConfig({
  resolve: { alias: { electron: 'ow-tauri/electron' } },
  define: { 'process.env.NODE_ENV': JSON.stringify('production') },
  build: {
    outDir: 'dist/preload',
    emptyOutDir: false,
    lib: { entry: 'src/preload/preload.ts', formats: ['iife'], name: 'preload', fileName: () => 'preload.js' },
  },
});
```

Without the `path` alias Vite replaces `path` with an empty browser stub,
and the main process fails at run time. Vite replaces
`process.env.NODE_ENV` in app builds but not in library mode, hence the
`define` in the preload config.

### esbuild

Not used by the examples; checked with esbuild 0.28 on the same main
process and preload, with the tsconfig `paths` of step 5 in place.

```js
import { build } from 'esbuild';

const common = {
  bundle: true,
  platform: 'browser',
  alias: { electron: 'ow-tauri/electron', path: 'path-browserify' },
  define: { 'process.env.NODE_ENV': '"production"', __dirname: '"/main"' },
};
await build({ ...common, entryPoints: ['src/main/main.ts'], outfile: 'dist/main/main.js', format: 'iife' });
await build({ ...common, entryPoints: ['src/preload/preload.ts'], outfile: 'dist/preload/preload.js', format: 'iife' });
```

Write the main page yourself (`dist/main/main.html` with
`<script src="main.js"></script>`) and set `plugins.overwolf.main.url` to
`main/main.html`.

## 5. Point TypeScript at the ow-tauri typings

```jsonc
{
  "compilerOptions": {
    "types": ["node", "ow-tauri/types"],
    "paths": {
      "electron": ["./node_modules/ow-tauri/dist/types/electron.d.ts"],
      "@overwolf/ow-electron": ["./node_modules/ow-tauri/dist/types/ow-electron.d.ts"]
    },
    "skipLibCheck": true
  }
}
```

- The bundler alias and the TypeScript path must point at the same module.
- `ow-tauri/types` brings the global `overwolf` namespace, `Electron.*`, the
  `app.overwolf` augmentation and `document.createElement('owadview')`
  ([CONTRACT B.4](CONTRACT.md#b4-typings)).
- Members ow-tauri does not support are marked
  `@deprecated Unsupported in ow-tauri`, so editors strike them through.
- Keep `@types/node`: the typings refer to `NodeJS.EventEmitter`.
- The declarations use TypeScript 5 syntax; the sample moved from 4.7 to 5.9.
- `autoUpdater` code imports `UpdateCheckResult` and `UpdateInfo` from
  `ow-tauri/main` instead of `electron-updater`.

## 6. Replace Node built-ins and Electron-only libraries

| ow-electron code | ow-tauri replacement |
|---|---|
| `fs.readFileSync`, `fs.writeFileSync`, `fs.existsSync`, `fs.promises.mkdir` | `files.readText` (resolves `null` for a missing file), `files.writeText` (atomic), `files.exists`, `files.mkdir` from `ow-tauri/main`, all async ([CONTRACT B.1.7](CONTRACT.md#b17-files)). They reach `userData`, the read-only app `package.json` (`path.join(app.getAppPath(), 'package.json')`) and the folders of `plugins.overwolf.fs.scope` |
| `child_process.exec('explorer.exe ...')`, `open` | `shell.openPath` (folders and files in the file scope; no executables unless `shell.openPathAllowExecutables`) and `shell.openExternal` (`http`, `https`, `mailto`) |
| `path` | `path-browserify` through the bundler |
| `events` | the `events` npm package through the bundler |
| `__dirname` | a bundler define ([step 4](#4-bundle-for-webviews)) |
| `__filename`, `require.resolve`, dynamic `require(variable)` | static imports; the bundle has no file system |
| `Buffer` | `Uint8Array`, `TextEncoder` and `TextDecoder`, `btoa` / `atob`; or the `buffer` npm package through the bundler |
| `crypto` | Web Crypto: `crypto.randomUUID()`, `crypto.getRandomValues()`, `crypto.subtle.digest()` |
| `os.platform()`, `os.arch()`, `os.homedir()` | `process.platform`, `process.arch`, `app.getPath('home')`; `app.getLocale()` for the locale |
| `http`, `https`, `net`, Node `fetch` | `fetch` and `WebSocket` of the webview, subject to CORS and the CSP ([step 8](#8-network-fetch-cors-and-csp)) |
| global `process` | a shim exists in `ow-main` and every `bw-*` webview: `platform`, `arch`, `argv`, `env`, `versions` (`owTauri`, `tauri`, no `electron`), `type`, `nextTick` ([CONTRACT B.2.5](CONTRACT.md#b25-other-modules)) |
| `process.versions.electron` | `process.versions.owTauri` (the sample's preload) |
| `electron-updater` | `autoUpdater` from `ow-tauri/main`: same properties, methods and events ([step 12](#12-keep-the-update-feed)) |
| `electron-store`, hand-written JSON prefs | `JSON.parse` / `JSON.stringify` over `files` in `app.getPath('userData')`; existing prefs files are found ([step 15](#15-what-changes-for-your-users)) |
| `electron-log` | `console` (visible in the `ow-main` devtools, [step 14](#14-debug-the-main-process)), or a Rust logger |
| `electron-is-dev` | `!app.isPackaged` |
| `crashReporter.start` | a documented no-op; use a Rust crash handler |
| native Node modules (`.node`) | a Rust command ([step 9](#9-your-own-rust-commands)) |

## 7. Make synchronous calls asynchronous

Every call from a webview to the host is asynchronous. Go through this list
for each hit of the audit in [step 1](#1-audit-the-app):

- [ ] `ipcRenderer.sendSync` and `event.returnValue`: use
      `ipcRenderer.invoke` with `ipcMain.handle`, and return the value from
      the handler.
- [ ] `dialog.showOpenDialogSync`, `showSaveDialogSync`,
      `showMessageBoxSync`: await `showOpenDialog`, `showSaveDialog`,
      `showMessageBox` (at most three buttons).
- [ ] `fs.*Sync` reads at module load (prefs, `package.json`): move them into
      an `async` start-up function that runs after `app.whenReady()`, and
      await `files.*`. Callers of a function that now reads a file must await
      it too; in the packages sample this reached `getTargetDisplay` and
      `createAndShow`.
- [ ] Code that ran between a synchronous read and the next statement (event
      listeners attached right after reading a file) now runs one read later.
      Attach listeners first when an event could fire in between.
- [ ] `new BrowserWindow()` still returns at once and its `id` is valid, but
      the native window is created asynchronously. Await `whenCreated()`
      when you need to know creation succeeded.
- [ ] `webContents.executeJavaScript` and `loadURL` / `loadFile` already
      returned promises; they still do.

These stay synchronous, served from a cache that the host keeps current:
`app.getPath`, `app.getName`, `app.getVersion`, `app.isPackaged`,
`app.overwolf.uid`, `muid`, `phasePercent`, `utmParams`,
`generateUserEmailHashes`, `BrowserWindow` state getters (`getBounds`,
`isVisible`, `isMinimized` and so on), and the `screen` getters
([CONTRACT B.1.6](CONTRACT.md#b16-synchronous-members-and-the-state-cache)).

## 8. Network: fetch, CORS and CSP

Main-process code used Node's networking, which has no CORS and no CSP. In
ow-tauri it runs in a webview whose origin is the app origin
(`http://tauri.localhost` on Windows, `tauri://localhost` on macOS and
Linux, the dev server in `tauri dev`). So:

- **CSP.** Add every host your main or renderer code fetches, or opens a
  WebSocket to, to `connect-src` in `app.security.csp`. ow-tauri's own
  traffic (analytics, consent, ads, the update check) is sent by Rust or by
  Overwolf's pages and needs no entry.
- **CORS.** A cross-origin `fetch` needs the server to answer with
  `Access-Control-Allow-Origin` for the app origin. If the API is yours, add
  it. If it is not, call it from a Rust command ([step 9](#9-your-own-rust-commands)).
- **Cookies.** The webview's cookie jar applies; Node `fetch` had none.
  Cross-origin requests send cookies only with `credentials: 'include'` and
  a matching CORS answer.
- **Remote pages.** `loadURL('https://...')` in a `BrowserWindow` still
  works, but the page gets no IPC, no preload and no `require('electron')`
  ([ADR 0011](adr/0011-remote-guest-ipc.md)).

## 9. Your own Rust commands

Work that needs Node or native code becomes a Tauri command. Declare it in
`build.rs` so Tauri applies its ACL, and grant it in a capability of your
own; otherwise Tauri lets every local webview call it
([ARCHITECTURE section 5.2](ARCHITECTURE.md#52-capabilities)):

```rust
// build.rs: replace the try_build line of step 3.2
tauri_build::try_build(
    tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(&["read_game_log"])),
)?;
```

```json
{
  "identifier": "app-main",
  "local": true,
  "webviews": ["ow-main"],
  "permissions": ["allow-read-game-log"]
}
```

Register it with `.invoke_handler(tauri::generate_handler![read_game_log])`
in `main.rs` and call it with `invoke` from `@tauri-apps/api/core`. Grant it
to `ow-main` only when only the main process needs it, as here.

## 10. Run with test ads

```shell
npm run build      # your bundler
npm run start-ad   # tauri dev -- -- --test-ad
```

`--test-ad` (or `OW_TAURI_TEST_AD=1`, or `plugins.overwolf.ads.testAd`)
switches every `<owadview>` to Overwolf's test inventory, as
`ow-electron --test-ad .` does. Test mode first, always: live ads count as
real impressions. [AD-FORMATS.md](AD-FORMATS.md) describes each format and
what to expect in test mode.

The first launch shows Overwolf's consent window where it is required, as
ow-electron does ([CONTRACT D.6.1](CONTRACT.md#d61-startup-consent-window)).

## 11. Sign the build

Overwolf signs the app's manifest and integrity data; without it GEP,
overlay and recorder do not load, while ads and analytics never depend on
it ([CONTRACT G.4](CONTRACT.md#g4-signing),
[ADR 0016](adr/0016-signing-approach.md)). `ow-tauri sign` runs the
published builder's flow before `tauri build`.

1. Copy the sample's [`.env.example`](../examples/packages-sample/.env.example)
   to `.env` (git-ignored) and fill in `OW_CLI_EMAIL`, `OW_CLI_API_KEY` and
   `OW_BUILD_KEY`. Never commit values.
2. Build the frontend first: `ow-tauri sign` hashes the built main entry
   (`main` of `package.json`, or `--main`).
3. `npm run build:ow-tauri` runs `ow-tauri sign`, then `tauri build`.
   `ow-tauri sign` writes `ow-tauri-signed/` next to `package.json` (the
   signed `package.json`, `_metadata.json`, `integrity.dll`, `owe.json`,
   `sign-result.json`). In a release build `embed_manifest` picks it up; the
   console-assigned `overwolf.uid` becomes the uid.
4. Ship `integrity.dll` and `_metadata.json` next to the exe:

   ```json
   {
     "bundle": {
       "resources": {
         "../ow-tauri-signed/integrity.dll": "integrity.dll",
         "../ow-tauri-signed/_metadata.json": "_metadata.json"
       }
     }
   }
   ```

5. Enable the `embed-resource` feature on the `tauri-plugin-overwolf`
   build-dependency, so Windows release builds carry the
   `OWEINTEGRITY/OWE` resource before Authenticode signing.
6. With `build.overwolf.enableOWCertSigning`, let Overwolf sign the app exe
   through Tauri's sign command. Use the object form: a string starting with
   `npx` does not run under Windows `cmd`.

   ```json
   {
     "bundle": {
       "windows": {
         "signCommand": { "cmd": "npx.cmd", "args": ["ow-tauri", "sign-exe", "%1"] }
       }
     },
     "plugins": { "overwolf": { "updater": { "publisherNames": ["<the name on your certificate>"] } } }
   }
   ```

   `sign-exe` posts the app exe only; other files go to
   `--fallback "<your signing command> %1"`, or stay unsigned. Set
   `publisherNames`: the exe then carries Overwolf's signature, so the
   updater cannot take the expected publisher from it.

The build prints a cargo warning for each of these that is missing. A
Windows release build with `requireSigning` on (the default, or
`OW_REQUIRE_SIGNING`) fails without the signed output, as Overwolf's builder
does. `OW_TAURI_ALLOW_UNSIGNED=1` turns that into a warning for local
builds. `npx ow-tauri sign --dry-run` prints the request without sending it.

Not done: the asar signature (`/sign/asar`). Tauri has no asar, and
ow-tauri does not fake one
([OQ-09](OPEN-QUESTIONS.md#oq-09-signing-and-integrity-for-tauri-builds)).

## 12. Keep the update feed

Replace the `electron-updater` import; the rest of the updater code stays:

```ts
import { autoUpdater } from 'ow-tauri/main';

autoUpdater.setFeedURL({
  provider: 'generic',
  url: `https://electron-updates.overwolf.com/electron-updates/electron/${app.overwolf.uid}`,
});
await autoUpdater.checkForUpdates();
```

- Overwolf's feed serves Windows setups only. macOS and Linux need a
  self-hosted generic feed with the same YAML shape; Linux also needs
  `plugins.overwolf.updater.pubkey` ([CONTRACT I](CONTRACT.md#i-updater-and-distribution)).
- Downloads are verified (size and SHA-512; the installer's publisher on
  Windows, the code signature on macOS, a minisign signature when `pubkey`
  is set) and fail closed ([ADR 0008](adr/0008-updater-client.md)).
- Whether the developers console accepts a Tauri NSIS setup in this feed is
  open ([OQ-18](OPEN-QUESTIONS.md#oq-18-updates-and-the-console)).
  Test with a console test channel before switching users.
- Assigning `channel` sets `allowDowngrade = true`, as in electron-updater.
  Set `allowDowngrade = false` after `channel` if you need it off.

## 13. CI

A job per OS that builds without bundling or launching:

```shell
npm ci
npm run build                                   # your bundler
npx tsc --noEmit
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo build --manifest-path src-tauri/Cargo.toml --locked
```

Linux runners need the Tauri prerequisites (`libwebkit2gtk-4.1-dev`,
`build-essential`, `libssl-dev`, `libayatana-appindicator3-dev`,
`librsvg2-dev`, `libxdo-dev`). The `example` job of this repository's
[`ci.yml`](../.github/workflows/ci.yml) runs exactly this for both
examples. For release builds, run `npm run build:ow-tauri` on Windows with
the signing variables as CI secrets. Keep the Overwolf npm packages on their
`latest` dist-tag. Never build a release with the plugin's `lab` feature.

## 14. Debug the main process

- **Devtools for `ow-main`.** `plugins.overwolf.main.devtools: true` opens
  them in debug builds. `console.log` from the main process appears there,
  not in the terminal.
- **Devtools for windows.** `webContents.openDevTools()` works in debug
  builds, and in release builds with the plugin's `devtools` feature.
- **Remote debugging (Windows).** `OW_TAURI_REMOTE_DEBUGGING_PORT=9222`
  (debug builds) opens a WebView2 debugging port; attach from
  `edge://inspect` or VS Code. The packages sample's
  `.vscode/launch.json` attaches to the webviews and to the Rust shell.
- **Logs.** `plugins.overwolf.logging.enabled: true` writes
  `<appData>/ow-electron/<uid>/logs/ow-tauri.log`
  ([CONTRACT F.4](CONTRACT.md#f4-logs)). Off by default, as ow-electron
  writes no log.
- **Crashes.** A crashed `ow-main` restarts; after more than
  `main.crashRestartLimit` crashes (default 3) within 60 s the app exits.
  App windows get `render-process-gone`
  ([CONTRACT A.6](CONTRACT.md#a6-main-webview-liveness-and-lifecycle)).
- **Build warnings.** `embed_manifest` reports manifest, `tauri.conf.json`,
  signing and configuration problems as cargo warnings. Read them on every
  build.

## 15. What changes for your users

Kept, for a user who ran the ow-electron build of the same app
([CONTRACT F.5](CONTRACT.md#f5-migration-from-ow-electron)):

- the uid, when `productName` (else `name`) and `author` are unchanged;
- the machine id (`muid`), so analytics continue;
- consent: the same stored `cmp` block is read, and the startup consent
  window reuses it;
- first launch is not reported again, and UTM parameters stay;
- app prefs in `app.getPath('userData')`, the same folder.

Different:

- A smaller download: the app uses the system webview (WebView2 on Windows,
  installed by the Tauri setup when missing) instead of a bundled Chromium.
- The installer is Tauri's NSIS setup with Overwolf's install and uninstall
  steps ([CONTRACT I.6](CONTRACT.md#i6-installer-parity-tauri-nsis-hooks)).
  Its install folder and uninstall entry may differ from electron-builder's;
  test an upgrade over an ow-electron install before shipping.
- Analytics and ad requests say `tauri` where ow-electron says `electron`
  ([CONTRACT section 0, Host label](CONTRACT.md#host-label)).
- Window sizes: `width` and `height` are the content size, so a framed
  window is slightly larger than on ow-electron (`useContentSize` is
  ignored).
- No game events, overlay or recorder ([Before you start](#before-you-start)).
- macOS and Linux keep the ad gaps of
  [PARITY.md](PARITY.md#known-platform-gaps).

## API mapping

### Electron API

`electron` resolves to `ow-tauri/electron`. **Same**: Electron semantics.
**Partial**: works, with the difference noted. **Not supported**: calling
it throws `OwTauriUnsupportedError`. The member-by-member table is
[CONTRACT B.2](CONTRACT.md#b2-ow-taurielectron).

| Module | Same | Partial | Not supported |
|---|---|---|---|
| `app` | lifecycle events, `whenReady`, `quit`, `exit`, `relaunch`, `getPath`, `getAppPath`, `getName`, `getVersion`, `isPackaged`, `getLocale`, `commandLine.hasSwitch` / `getSwitchValue` | `requestSingleInstanceLock` (real locking by the single-instance plugin), `focus`, `setPath`, `setName`, `commandLine.appendSwitch` (next launch), `disableHardwareAcceleration` (Windows, next launch), `setAppUserModelId` (no-op) | `dock`, `setLoginItemSettings`, `setBadgeCount`, `setJumpList`, `setAsDefaultProtocolClient`, `getGPUInfo`, `getAppMetrics`, `showAboutPanel` |
| `BrowserWindow` | sizes and positions, visibility, focus, state, `alwaysOnTop`, `transparent`, `parent`, `loadURL`, `loadFile`, events | `frame: false` (overlay title bar on macOS), `modal`, `useContentSize` (ignored), `webPreferences` (`preload` and `devTools` only), `blur`, `showInactive`, `setMovable`, `moveTop`, menus (no-op) | `setOpacity`, `setShape`, `capturePage`, `setBrowserView`, `setTouchBar`, `vibrancy`, `titleBarStyle`, `kiosk` |
| `webContents` | `send`, `ipc`, `getURL`, `reload`, `setZoomFactor`, `did-finish-load`, `dom-ready`, `did-navigate-in-page` | `executeJavaScript`, devtools, `did-fail-load` (Windows), `render-process-gone`, `setWindowOpenHandler` (allow opens the system browser), `will-navigate` (already cancelled) | `session`, `debugger`, `print`, `printToPDF`, `setAudioMuted`, `insertCSS`, `sendInputEvent`, `postMessage` |
| `ipcMain`, `ipcRenderer` | `on`, `once`, `handle`, `invoke`, `send`, `reply` | `event.senderFrame` (`url` only) | `sendSync`, `returnValue`, `sendTo`, `sendToHost`, `postMessage`, `ports` |
| `contextBridge` | `exposeInMainWorld` (preload and page share one world; globals are frozen) | | `exposeInIsolatedWorld`, `executeInMainWorld` |
| `screen` | display getters | `getCursorScreenPoint` (cached), display events (2 s poll), DIP conversions | |
| `shell` | `openExternal` (`http`, `https`, `mailto`), `openPath`, `showItemInFolder` | | `trashItem`, `beep`, shortcut links |
| `dialog` | `showOpenDialog`, `showSaveDialog`, `showErrorBox` | `showMessageBox` (three buttons) | the `*Sync` variants, `showCertificateTrustDialog` |
| `globalShortcut` | `unregister`, `unregisterAll`, `isRegistered`, `registerAll` | `register` (failure reported later) | |
| `nativeTheme`, `crashReporter` | | `shouldUseDarkColors`, `updated`; `crashReporter.start` (no-op) | other members |
| everything else | | | `Menu`, `Tray`, `Notification`, `session`, `protocol`, `net`, `clipboard`, `nativeImage`, `safeStorage`, `powerMonitor`, `desktopCapturer`, `BrowserView`, `WebContentsView`, `utilityProcess`, Electron's `autoUpdater` and the rest of [B.2.5](CONTRACT.md#b25-other-modules) |

For an unsupported module, use the Tauri plugin that covers it from Rust
(tray, notification, clipboard, global menu) and expose what the app needs
through your own command ([step 9](#9-your-own-rust-commands)).

### `app.overwolf`

Every member of the ow-electron typings exists with the same signature:
`disableAnonymousAnalytics`, `disableAdsOptimization`, `disableAdsFPD`,
`isCMPRequired`, `openCMPWindow`, `openAdPrivacySettingsWindow`,
`generateUserEmailHashes`, `setUserEmailHashes`,
`setExternalPaymentUserId`, `uid`, `muid`, `phasePercent`, `utmParams` and
`packages` ([CONTRACT B.1.1](CONTRACT.md#b11-appoverwolf-overwolfapi)).
`packages` reports every package as unavailable
([CONTRACT H](CONTRACT.md#h-packages)). `ow-tauri/main` also exports
`files`, `autoUpdater` and `whenHostReady()`.

### `<owadview>`

No change in renderer code. The element, its attributes (`cid`,
`slotsize`, `adstyle`, `customTracking`, `performance`, `unit`, `pageurl`),
its methods (`setAudioMuted`, `reload`, `setPageUrl`, `sendCommand`) and its
events (`display_ad_loaded`, `impression`, `performance_ad_loaded`,
`shutdown` and the others) behave as on ow-electron
([CONTRACT B.3](CONTRACT.md#b3-ow-taurirenderer)). Each ad renders in a
native child webview placed over the element's box. Electron's generic
`<webview>` methods (`executeJavaScript`, `getURL`, `send`) are not
provided. [AD-FORMATS.md](AD-FORMATS.md) covers every format.

### Commands and tools

| ow-electron | ow-tauri |
|---|---|
| `ow-electron .` | `tauri dev` (after your bundler's build) |
| `ow-electron --test-ad .` | `tauri dev -- -- --test-ad`, or `OW_TAURI_TEST_AD=1` |
| `ow-electron-builder` (`build:ow-electron`) | `ow-tauri sign`, then `tauri build` (`build:ow-tauri`, [step 11](#11-sign-the-build)) |
| builder `requireSigning`, `enableOWCertSigning` | the same `package.json` flags, read by `ow-tauri sign`, `embed_manifest` and `ow-tauri sign-exe` |
| builder NSIS script | `write_nsis_installer_hooks` in `build.rs` and `bundle.windows.nsis.installerHooks` |
| `electron-updater` generic feed | `autoUpdater` from `ow-tauri/main`, same feed ([step 12](#12-keep-the-update-feed)) |
| `ow client calc-electron-uid` | the same formula, computed by the plugin ([CONTRACT G.2](CONTRACT.md#g2-app-uid)) |
| console upload of the setup | not verified for a Tauri setup ([OQ-18](OPEN-QUESTIONS.md#oq-18-updates-and-the-console)) |
