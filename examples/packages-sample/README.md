# Running on ow-tauri

This folder is Overwolf's [ow-electron-packages-sample](https://github.com/overwolf/ow-electron-packages-sample)
(commit `8a27053`, MIT, Overwolf Ltd.) ported to Tauri 2 with ow-tauri. The
TypeScript app is the upstream code: the main process (`src/browser`) runs in
a hidden webview, `electron` imports resolve to `ow-tauri/electron`, and
`src-tauri/` is a thin native shell around `tauri-plugin-overwolf`. Every
change against upstream is listed in [CHANGES-FROM-UPSTREAM.md](CHANGES-FROM-UPSTREAM.md);
the design is in `docs/PORT-MAP.md` at the repository root.

## Prerequisites

- Node 22.12 or newer and the [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/)
  for your OS (Rust, WebView2 on Windows, WebKitGTK on Linux).
- Install from the repository root, so `ow-tauri` links to `packages/ow-tauri`:

```shell
npm install
npm run build --workspace ow-tauri
```

## Run

| Script | What it does |
|---|---|
| `npm run build` | webpack: the main bundle (`dist/browser`), the renderer, the preload, the OSR and exclusive pages |
| `npm run start` | `tauri dev` on the last build |
| `npm run build:start` | `build`, then `tauri dev` |
| `npm run start-ad` | `tauri dev` with `--test-ad`: test ad inventory (`OW_TAURI_TEST_AD=1` does the same) |
| `npm run build:dev` | webpack in watch mode |
| `npm run build:ow-tauri` | loads `.env`, then `ow-tauri sign` (Overwolf signing, CONTRACT G.4) and `tauri build` (NSIS installer on Windows) |
| `npm run typecheck` | `tsc --noEmit` on the TypeScript app |
| `npm run check:rust` | `cargo fmt --check` and `cargo clippy -D warnings` on `src-tauri` (its own workspace and `Cargo.lock`) |

`npm run build:ow-tauri` replaces `build:ow-electron` and, like it, loads
`.env` through `dotenv --override --no-expand` (values in `.env` win over the
environment). Signing runs when `OW_CLI_EMAIL`, `OW_CLI_API_KEY` and
`OW_BUILD_KEY` are set; copy [.env.example](.env.example), which lists every
optional variable, to `.env`. Run `npm run build` first: `ow-tauri sign`
hashes the built main bundle. On
Windows, set `OW_TAURI_REMOTE_DEBUGGING_PORT=9222` to attach a debugger to the
webviews (see [.vscode/launch.json](.vscode/launch.json)).

## What works where

| Feature | Windows | macOS | Linux |
|---|---|---|---|
| App windows, IPC, dialogs, screen, shell | yes | yes | yes |
| Ads (`<owadview>`), test ads, consent (CMP) | yes | yes, with the request-shaping and web-security gaps of `docs/ARCHITECTURE.md` section 6 | yes, same gaps as macOS |
| Anonymous analytics, email hashes, `uid` / `muid` | yes | yes | yes |
| Update check against Overwolf's feed | yes | self-hosted feed only (Overwolf serves Windows setups only) | self-hosted feed only |
| NSIS installer with Overwolf's install and uninstall steps | yes | n/a | n/a |
| GEP, overlay, recorder, utility | reported as unavailable | reported as unavailable | reported as unavailable |

## What the packages report

ow-tauri behaves as ow-electron does where packages are not available
(CONTRACT H): `app.overwolf.packages.gep`, `.overlay`, `.recorder` and
`.utility` are `undefined`, no `ready` or `failed-to-initialize` event fires,
`getChannel()` resolves `{}`, `getAvailableChannels()` and `setChannel()`
reject with "package '<name>' is not registered in this app", and
`hasPendingUpdates()` returns `{ hasPendingUpdate: false, details: [] }`. The
sample's package screens therefore stay in their waiting state, as they do on
ow-electron without a package runtime.

## Parity notes

- The main process runs in a webview: `fs` and `child_process` are replaced
  by `files` from `ow-tauri/main` and `shell.openPath`, `path` and `events`
  by browser polyfills, `__dirname` is `/browser` (an app-asset path).
- `electron-updater` is replaced by `autoUpdater` from `ow-tauri/main`, with
  the same properties, methods and events.
- `crashReporter.start` is a documented no-op; use a Rust crash handler.
- "Open folder" uses `shell.openPath`, which opens folders inside the
  `plugins.overwolf.fs.scope` of `src-tauri/tauri.conf.json` only. A capture
  folder picked elsewhere, or the logs folder (which nothing creates while
  packages are unavailable), is refused; the button then returns `false` and
  the reason is logged.
- The DPI test window loads a remote page, so it has native window
  decorations instead of the injected drag header: remote pages get no IPC.
- `app.disableHardwareAcceleration()` (`--test-osr-app-level-disable-gpu`)
  records `--disable-gpu` for the next launch on Windows (Tauri fixes the
  browser arguments before app code runs) and is a no-op elsewhere.
- Installer: Tauri names the setup `<productName>_<version>_x64-setup.exe`
  (no `artifactName`) and has no `legalTrademarks` field.

---

# ow-electron-packages-sample

A basic sample app, demonstrating how [@overwolf/ow-electron](https://npmjs.com/package/@overwolf/ow-electron) packages (Overlay, Game Events) work.

For more details about ow-electron, as well as how to fully utilize it, please refer to the official [documentation](https://overwolf.github.io/tools/ow-electron).

## Setup

To set up this app, you must first install its dependencies (using [yarn](https://yarnpkg.com/), [npm](https://www.npmjs.com/), or any other package manager).

From there, you can easily run/interact with it.

## Quick start 

To run the app in development mode, simply run the `build` script, followed by the `start` script from the package.json (on ow-tauri, `start` runs `tauri dev`).  
For example:

```shell
# Using npm
npm run build
npm run start

# Using yarn
yarn build
yarn start
```

### VSCode launch.json

This repository also includes a working `.vscode/launch.json` file, meaning that you can launch the app by simply clicking `F5` on your keyboard (for default vscode settings).

## Quick Build

To build the app for production, you must run the `build` script, followed by the `build:ow-electron` script from the package.json.  
For example:

```shell
# Using npm
npm run build
npm run build:ow-electron

# Using yarn
yarn build
yarn build:ow-electron
```

## Dev Mode

*Available since `ow-electron@39.8.10` (Windows only).*

Dev Mode lets you run and test the gaming packages (GEP, Overlay, Recorder) locally, without having to sign your app first. Without valid credentials, the app still runs, but the gaming packages stay inactive and production validation kicks in instead.

To enable it, provide credentials using one of the following methods:

```shell
# Option A: environment variables (recommended for CI)
# Windows (PowerShell)
$env:OW_CLI_EMAIL = "your-email@example.com"
$env:OW_CLI_API_KEY = "your-api-key-from-console"
# Or dev token
$env:OW_DEV_KEY=your-dev-token

# Linux / macOS
export OW_CLI_EMAIL=your-email@example.com
export OW_CLI_API_KEY=your-api-key-from-console
# Or dev token
export OW_DEV_KEY=your-dev-token
```

Option C: set them in [.vscode/launch.json](.vscode/launch.json), under the `env` section of the `OW-Electron: Main Process` configuration:

```jsonc
"env": {
  "OW_CLI_EMAIL": "your-email@example.com",
  "OW_CLI_API_KEY": "your-api-key-from-console"
  // or, instead of the two above:
  // "OW_DEV_KEY": "your-dev-token"
}
```

For full details, see the [Dev Mode guide](https://dev.overwolf.com/ow-electron/guides/dev-tools/dev-mode).

## App Signing

Before releasing your app, both Overwolf and you need to sign it: Overwolf signs the gaming package integrity, and you sign the exe with your own code-signing certificate. Without both signatures, the gaming packages will not load at runtime.

To sign your app:

1. Set the following environment variables:
   ```shell
   OW_CLI_EMAIL=your-email@example.com      # Your Overwolf Console account email
   OW_CLI_API_KEY=your-api-key-from-console # Console > Profile > API Keys
   OW_BUILD_KEY=your-build-key              # Console > Release management > App Keys
   ```
2. Run the builder:
   ```shell
   npx @overwolf/ow-electron-builder
   ```

This also requires a registered app (with a UID) in the Overwolf Console, and your own code-signing certificate for the executable.

For full details, see the [App Signing guide](https://dev.overwolf.com/ow-electron/guides/dev-tools/app-signing).

> **Note:** If you plan to host your app on the Overwolf app store, a Digital Code Signature is **mandatory** as part of the [pre-submission checklist](https://dev.overwolf.com/ow-electron/getting-started/release-your-app#pre-submission-checklist). It's strongly recommended to obtain a digital certificate to avoid Windows installation warnings for your users.

## Working with ow-electron packages

In order to add more/remove certain ow-electron "packages" from the project, simply edit the `overwolf.packages` array in the [package.json](/package.json) file, like so:

```json
{
  ...
  "overwolf": {
    "packages": [
      "gep",
      "overlay",
      "recorder"
    ]
  },
  ...
}
```

### Available packages detailed information
* [Recorder](./docs/recorder/recorder.md)
* [Game Events Provider](./docs/gep/game-events-provider.md)
* [Overlay](./docs//overlay/overlay.md)
