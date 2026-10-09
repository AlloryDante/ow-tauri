# Migrating from ow-electron

This guide moves an Overwolf app from ow-electron to Tauri 2 with
`tauri-plugin-overwolf`. Your ad HTML stays as it is. Your main-process code
moves to Rust or to your app's pages, the way any Electron app moves to
Tauri. The plugin keeps the app's Overwolf identity, so Overwolf and your
users see the same app.

This is the only page in these docs that uses Electron APIs, to show what
replaces them.

## Contents

- [Before you start](#before-you-start)
- [1. Create the Tauri app](#1-create-the-tauri-app)
- [2. Keep the uid with `ow-tauri migrate`](#2-keep-the-uid-with-ow-tauri-migrate)
- [3. Add the plugin](#3-add-the-plugin)
- [4. Keep the ad HTML](#4-keep-the-ad-html)
- [5. Move the main process](#5-move-the-main-process)
- [6. Move the `app.overwolf` calls](#6-move-the-appoverwolf-calls)
- [7. Move the privacy opt-outs](#7-move-the-privacy-opt-outs)
- [8. Check the window names](#8-check-the-window-names)
- [9. Run with test ads](#9-run-with-test-ads)
- [10. Sign the build (optional)](#10-sign-the-build-optional)
- [11. Keep the update feed](#11-keep-the-update-feed)
- [What your users keep](#what-your-users-keep)
- [Keys removed in 1.0](#keys-removed-in-10)

## Before you start

- **Overwolf packages are not available.** Game events (GEP), the overlay,
  the recorder and the other Overwolf packages need Overwolf's package
  runtime, which exists for ow-electron only. An app whose core feature is
  game events or the overlay cannot move yet.
- **Platforms.** Ads run on Windows 10/11 and macOS 14 or newer. Linux
  builds, but ads are `unsupported`. See
  [COMPATIBILITY.md](COMPATIBILITY.md).
- **Toolchain.** The
  [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/), Rust 1.90
  or newer, and Node.js 22.12 or newer.
- **Examples.** [examples/packages-sample](../examples/packages-sample)
  is Overwolf's ow-electron packages sample moved to Tauri. Every change
  against the original is listed in its
  [CHANGES-FROM-UPSTREAM.md](../examples/packages-sample/CHANGES-FROM-UPSTREAM.md).

## 1. Create the Tauri app

Create a Tauri 2 app next to the ow-electron one, or add Tauri to the
existing frontend with `npm run tauri init`. Then follow steps 2 to 5 of
[GETTING-STARTED.md](GETTING-STARTED.md), but do not pick a new `author` or
`name`: the next step brings over the ones your app already uses.

## 2. Keep the uid with `ow-tauri migrate`

Overwolf knows your app by its uid. ow-electron computes it from the
`author` and the name (`productName`, else `name`) in `package.json`, unless
`overwolf.uid` sets it. Keep the same inputs and the Tauri app has the same
uid.

```sh
npm add -D tauri-plugin-overwolf-cli@1.0.0-rc.1
npm exec --no -- ow-tauri migrate --from ../my-electron-app/package.json --write src-tauri/tauri.conf.json
```

`migrate` reads the `package.json` the way ow-electron reads its packaged
copy (`build.extraMetadata` merged in). It prints a `plugins.overwolf` block
and, with `--write`, merges it into the given `tauri.conf.json`:

| Written key | From |
|---|---|
| `author` | `author`, or `author.name`; `"unknown"` when there is none, which is what ow-electron hashed |
| `name` | `productName`, else `name` |
| `uid` | `overwolf.uid`, when it is set and valid |
| `ads.disableOptimization` | `build.overwolf.disableAdOptimization` |
| `signing.requireSigning` | `build.overwolf.requireSigning` (default `true`) |
| `signing.owCertSigning` | `build.overwolf.enableOWCertSigning` |

It always writes `author` and `name`, so the uid never depends on a default.
It then prints the uid, for example `uid <uid> (computed <uid>)`. It warns
when:

- the `package.json` has no author;
- `overwolf.uid` is not 1 to 64 ASCII letters or digits (ow-electron ignores
  such a value, so it is not written);
- `overwolf.packages` lists packages (they are not available);
- the target `tauri.conf.json` already pins a different `uid`.

Use `npm exec --no -- ow-tauri`, never `npx ow-tauri`: `npx` may download a
package of the same name from the registry.

After the change, `npm exec --no -- ow-tauri doctor` shows the uid the app
will use.

## 3. Add the plugin

Follow [GETTING-STARTED.md](GETTING-STARTED.md) from step 2: the Rust plugin
and its build step, the capability, the JavaScript API and the CLI. The
`plugins.overwolf` block from step 2 of this guide replaces the one in
GETTING-STARTED.

## 4. Keep the ad HTML

The element, its attributes, members and events are the same. Add one
import to every page that shows ads:

```ts
import 'tauri-plugin-overwolf-api/adview';
```

Two things differ, both because each ad is a native webview placed over the
element's box:

- the ad always paints above your page. A menu that must cover an ad has to
  hide the element;
- Electron's `<webview>` events that Tauri cannot observe
  (`did-start-navigation`, `console-message`, ...) are not dispatched.

[api/owadview.md](api/owadview.md) has the details.

## 5. Move the main process

There is no main process in JavaScript. Use Tauri's own tools for each job:

| ow-electron | Tauri |
|---|---|
| `main.ts`, `app.whenReady()` | `src-tauri/src/lib.rs`, `tauri::Builder::setup` |
| `new BrowserWindow(...)` | `app.windows` in `tauri.conf.json`, or `WebviewWindowBuilder` in Rust |
| `ipcMain.handle` + `ipcRenderer.invoke` | `#[tauri::command]` + `invoke` from `@tauri-apps/api/core` |
| `webContents.send` + `ipcRenderer.on` | `app.emit` (`tauri::Emitter`) + `listen` from `@tauri-apps/api/event` |
| preload scripts, `contextBridge` | not needed: pages call `@tauri-apps/api` and the plugin APIs, as capabilities allow |
| `shell.openExternal` | `tauri-plugin-opener` |
| `dialog`, `fs` | `tauri-plugin-dialog`, `tauri-plugin-fs` |
| `app.requestSingleInstanceLock()` | `tauri-plugin-single-instance` |
| `app.relaunch()` | `app.restart()` in Rust, or `relaunch()` from `tauri-plugin-process` |
| `Tray`, `Menu` | Tauri's `tray-icon` feature and `tauri::menu` |
| `globalShortcut` | `tauri-plugin-global-shortcut` |
| `electron-log` | `tauri-plugin-log` |
| `electron-updater` with Overwolf's feed | the plugin's `updater` feature on Windows ([step 11](#11-keep-the-update-feed)) |
| electron-builder NSIS script | Tauri's NSIS bundle with the hooks the build step writes |
| `app.getPath('userData')` | `app.path().app_data_dir()`. Tauri names the folder after the bundle `identifier`, not the product name, so app files written by the ow-electron build are not found there. Move them yourself if you need them. |

Read [INTEROP.md](INTEROP.md) before you add the official plugins: the
order of registration and a window filter matter.

## 6. Move the `app.overwolf` calls

`app.overwolf` lived in the main process. The same calls are now in the
JavaScript API ([api/js.md](api/js.md)), for your app's pages, and in Rust
through `OverwolfExt` ([api/rust.md](api/rust.md)).

| ow-electron | JavaScript (`tauri-plugin-overwolf-api`) | Rust (`app.overwolf()`) |
|---|---|---|
| `app.overwolf.uid` | `(await getInfo()).uid` | `uid()` |
| `app.overwolf.muid` | `(await getMachineIds()).muid` (permission `overwolf:machine-id`) | `muid()` |
| `app.overwolf.phasePercent` | `(await getInfo()).phasePercent` | `phase_percent()` |
| `app.overwolf.utmParams` | `(await getInfo()).utmParams` | `utm_params()` |
| `isCMPRequired()` | `isCMPRequired()` | `is_cmp_required().await` |
| `openAdPrivacySettingsWindow(options)` | `openAdPrivacySettingsWindow(options)` | `open_ad_privacy_settings_window(options).await` |
| `openCMPWindow(options)` | `openCMPWindow(options)` | `open_cmp_window(options).await` |
| `generateUserEmailHashes(email)` | `generateUserEmailHashes(email)` (permission `overwolf:email-hashes`) | `generate_user_email_hashes(email)` |
| `setUserEmailHashes(hashes)` | `setUserEmailHashes(hashes)` | `set_user_email_hashes(&hashes)` |
| `setExternalPaymentUserId(options)` | `setExternalPaymentUserId(options)` (permission `overwolf:analytics`) | `set_external_payment_user_id(&map).await` |
| `disableAnonymousAnalytics()` | see [step 7](#7-move-the-privacy-opt-outs) | `disable_anonymous_analytics()` |
| `disableAdsOptimization()` | `disableAdsOptimization()` | `disable_ads_optimization()` |
| `disableAdsFPD()` | `disableAdsFPD()` | `disable_ads_fpd()` |
| `app.overwolf.packages` | not available | not available |

The JavaScript functions are async. A page can call them only when its
capability grants the permission ([api/permissions.md](api/permissions.md)).

## 7. Move the privacy opt-outs

In ow-electron an app could call `disableAnonymousAnalytics()` in the main
process before the app was ready, so even the launch requests were reduced.
Your pages load after those requests, so a call from JavaScript affects only
what follows (the plugin logs a warning). Move each opt-out to a place that
runs before the launch:

| Who opted out | Use |
|---|---|
| every user of the app | `analytics.disableAnonymous: true` in `plugins.overwolf`, or `Builder::new().disable_anonymous_analytics()` |
| a user, decided in Rust before launch | `app.overwolf().disable_anonymous_analytics()` in `setup` |
| a user, from a settings page | `setAnonymousAnalyticsPreference(false)` (stored, applies from the next launch) plus `disableAnonymousAnalytics()` (the rest of this launch) |

The same holds for `disableAdsOptimization()` and `disableAdsFPD()`: use
`ads.disableOptimization` / `ads.disableFpd` in the config or the Builder
methods to have them from the start. [CONFIG.md](CONFIG.md) lists the keys.

## 8. Check the window names

Overwolf's analytics name each window after its page, as ow-electron does:
`index.html` gives `index`, `settings.html` gives `settings`. Tauri's
`tauri://localhost/` also gives `index`.

One case differs. An app that routes with the history API (`/settings`
rather than `#/settings`) reports `settings` where its ow-electron build,
loading `index.html` from a file, reported `index`. If that matters for your
reports, give the window a fixed name with `setWindowName('index')` or
`app.overwolf().set_window_name(label, "index")`. ow-electron has no such
call, so use it only for this case.

## 9. Run with test ads

| ow-electron | Tauri |
|---|---|
| `ow-electron --test-ad .` | `npm run tauri dev` with `OW_TAURI_TEST_AD=1` set, or with `"ads": { "testAd": true }` in `plugins.overwolf` |
| `ow-electron .` | `npm run tauri dev` |
| `ow-electron-builder` | `npm run tauri build` |

The app binary also accepts `--test-ad`. In-app restarts under
`tauri dev` lose the dev server ([TROUBLESHOOTING.md](TROUBLESHOOTING.md#restart-does-nothing-under-tauri-dev)).

<a id="11-sign-the-build"></a>

## 10. Sign the build (optional)

Overwolf signing is off by default. Nothing in the plugin needs the signed
build output, since packages are not available. Turn it on if Overwolf asks
for a signed build:

```json
"plugins": { "overwolf": { "signing": { "enabled": true } } }
```

Then, in `src-tauri/tauri.windows.conf.json`:

```json
{
  "build": { "beforeBuildCommand": "npm run build && npm exec --no -- ow-tauri sign --main dist/index.html" },
  "bundle": {
    "resources": { "../signed/integrity.dll": "integrity.dll", "../signed/_metadata.json": "_metadata.json" },
    "windows": { "signCommand": "npm exec --no -- ow-tauri sign-exe %1" }
  }
}
```

The `signCommand` line is needed only with `signing.owCertSigning: true`
(ow-electron's `enableOWCertSigning`).

`ow-tauri sign` reads `OW_CLI_EMAIL`, `OW_CLI_API_KEY` and `OW_BUILD_KEY`
from the environment, the variables ow-electron-builder reads. Keep them in
CI secrets or a git-ignored `.env`
([example](../examples/packages-sample/.env.example)). It writes `signed/`
in the project folder. If the uid Overwolf signed differs from the one the
app resolves, it stops:

```text
[OW] the console signed uid <signed> but plugins.overwolf resolves to <resolved>; set plugins.overwolf.uid to "<signed>" (or run ow-tauri sign --write-uid)
```

The [CLI README](../packages/cli/README.md) has every option.

## 11. Keep the update feed

Overwolf's update feed for your uid keeps working. Build with the
`updater` feature and call `check()` from
`tauri-plugin-overwolf-api/updater` or `app.overwolf().updater()` in Rust.
It runs on Windows only and needs `updater.publisherNames` (your installer's
certificate subject) or `updater.pubkey` in a release build. Overwolf's feed
serves NSIS installers; build your release with Tauri's NSIS target.

```toml
tauri-plugin-overwolf = { version = "1.0.0-rc.1", features = ["updater"] }
```

Test an update from your last ow-electron release to the Tauri build before
you ship it. Tauri's installer may use another install folder and uninstall
entry than electron-builder's.

## What your users keep

When the uid is the same:

- the state file `ow-electron/<uid>/ow-electron.json` in the app data
  folder: the plugin reads and writes the same file, so the consent answer,
  the first-launch state and the UTM parameters carry over;
- the machine id, which is per machine, not per app;
- on Windows, the ads data folder `<appData>/<product name>/EBWebView-ow`,
  the same path ow-electron uses.

What changes:

- the download is smaller: the app uses the system webview (WebView2 on
  Windows, WebKit on macOS) instead of a bundled Chromium;
- the analytics and ad requests carry the host label `tauri` where
  ow-electron sends `electron`, and the version derived from it. This is
  the one intended difference in the data;
  [PARITY.md](PARITY.md) lists what is compared and the known platform
  differences.

## Keys removed in 1.0

Preview releases of this plugin had keys that 1.0 refuses. The error names
the key and links here.

<a id="no-main-webview"></a>

### `main`

There is no hidden main webview. App code runs in your own webviews and in
Rust ([step 5](#5-move-the-main-process)). Remove the block.

<a id="no-ipc-bridge"></a>

### `ipc`

Your pages talk to the plugin through Tauri commands, called by the
JavaScript API. Your own messages use `#[tauri::command]` and Tauri events.

<a id="official-plugins"></a>

### `shell`, `fs`

Use the official plugins: `tauri-plugin-opener` or `tauri-plugin-shell`,
and `tauri-plugin-fs`.

<a id="browser-args"></a>

### `webview`

Browser arguments for the ad webviews moved to `ads.browserArgs` (Windows).
Arguments for your own webviews are Tauri's: `additionalBrowserArgs` on a
window in `tauri.conf.json`.

<a id="packages"></a>

### `packagesBackend`

Overwolf packages are not available on Tauri yet. Remove the key.

<a id="logging"></a>

### `logging`

The plugin logs through the `log` crate. Register `tauri-plugin-log` before
the plugin to see its messages, and set levels there. The targets are
`tauri_plugin_overwolf` and `tauri_plugin_overwolf::updater`.

<a id="gesture-window"></a>

### `ads.gestureWindowMs`

A click in an ad opens the browser only when the operating system reports a
real user action on the ad. There is no time window to set. Remove the key.

<a id="crash-recovery"></a>

### `ads.guestHeartbeatTimeoutMs`

Crashed ad pages are detected from the operating system, not from a
heartbeat. On macOS, wire the terminate hook
([api/rust.md](api/rust.md#the-macos-terminate-hook)). Remove the key.

<a id="recreate-on-reload"></a>

### `ads.recreateOnReload: "auto"` or `"never"`

The key is a boolean now: `"auto"` is `true` (the default) and `"never"` is
`false`.
