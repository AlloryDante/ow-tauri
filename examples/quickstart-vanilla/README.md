# Quickstart (vanilla TypeScript)

The smallest complete app on `tauri-plugin-overwolf`: one window with one 400x300 Overwolf ad, test
ads on. It is the app that [GETTING-STARTED](../../docs/GETTING-STARTED.md) builds step by step,
starting from `npm create tauri-app@latest my-game-app -- --template vanilla-ts --manager npm`. CI
builds it on Windows, macOS and Linux. It differs from the walkthrough in two places. The crate and
the npm packages come from this repository by path, not from a git dependency and tarballs. And
`src-tauri/src/lib.rs` also registers the single-instance and window-state plugins
([Other plugins](#other-plugins)).

Use it to see an ad running in a few commands, or to copy the plugin setup into your own app. For
the same app in React, see [quickstart-react](../quickstart-react). To try the other API calls
with a log, see [packages-sample](../packages-sample).

## What the app shows

The window shows a heading, one line of text, the ad slot and an "Ad privacy settings" button. The
button appears only when `isCMPRequired()` says consent rules apply, and it calls
`openAdPrivacySettingsWindow()`. The page logs the uid and the test ad state from `getInfo()` to the
console, and logs `display_ad_loaded` and `impression` when the ad sends them.

## Run it

You need Node 22.12 or newer and the
[Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for your OS. Ads show on Windows
and macOS. On Linux the app builds and runs, but ads report `unsupported`
([COMPATIBILITY.md](../../docs/COMPATIBILITY.md)).

The app runs from a clone of this repository. Run this once:

```sh
git clone https://github.com/AlloryDante/ow-tauri
cd ow-tauri
npm install
npm run build --workspace tauri-plugin-overwolf-api --workspace tauri-plugin-overwolf-cli
```

The last line builds the `dist/` folders that the page and `ow-tauri` load. Then, from
`examples/quickstart-vanilla`:

| Script              | What it does                                                                                                           |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `npm run tauri dev` | the app under `tauri dev`, with hot reload                                                                             |
| `npm start`         | a debug build with the page embedded, then the app ([scripts/run.mjs](scripts/run.mjs))                                |
| `npm run doctor`    | `ow-tauri doctor`: prints the resolved uid and checks the capabilities, the Rust code and the installer settings        |

`npm run dev` starts only Vite, without the app. To pass arguments to the app, run
`node scripts/run.mjs -- <args>`; `node scripts/run.mjs --no-build` runs the last build again.

Test ads are on (`"ads": { "testAd": true }` in `tauri.conf.json`). Remove it before you ship.

Run the CLI from the local install, with `npm run doctor` or `npm exec --no -- ow-tauri ...`. Do not
use `npx ow-tauri`: without a local install, `npx` may download a different package from the
registry.

## Where the plugin is wired in

| File                                                                       | What the walkthrough added                                                                          |
| -------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| [src-tauri/Cargo.toml](src-tauri/Cargo.toml)                               | `tauri-plugin-overwolf` (and as a build dependency with `features = ["build"]`), `tauri-plugin-log` |
| [src-tauri/build.rs](src-tauri/build.rs)                                   | the plugin's build step before `tauri_build::build()`                                               |
| [src-tauri/src/lib.rs](src-tauri/src/lib.rs)                               | the plugins, and the macOS crashed-guest hook                                                       |
| [src-tauri/tauri.conf.json](src-tauri/tauri.conf.json)                     | `plugins.overwolf` (author and name: the uid inputs; test ads on), the CSP                          |
| [src-tauri/tauri.windows.conf.json](src-tauri/tauri.windows.conf.json)     | NSIS with the plugin's installer hooks                                                              |
| [src-tauri/capabilities/default.json](src-tauri/capabilities/default.json) | `overwolf:default` for the `main` webview (never `windows`: that also matches the ad guests)        |
| [src/main.ts](src/main.ts)                                                 | the `<owadview>` runtime import, the ad events and the privacy button                               |
| [index.html](index.html)                                                   | the `<owadview cid="main-mrec" slotsize="400x300">` tag and the button                              |
| [src-tauri/.gitignore](src-tauri/.gitignore)                               | `/gen/overwolf` (the build step writes it)                                                          |

Here the plugin is the repository's crate by path, and `tauri-plugin-overwolf-api` and
`tauri-plugin-overwolf-cli` are npm workspaces. In your own app the crate is a git dependency on
this repository and the npm packages are tarballs packed from a clone. They are not on crates.io or
npm yet. [GETTING-STARTED](../../docs/GETTING-STARTED.md#before-you-start) shows both.

## Other plugins

[src-tauri/src/lib.rs](src-tauri/src/lib.rs) also registers two common plugins next to this one.
[INTEROP.md](../../docs/INTEROP.md) explains the order and the filter they need.

- `tauri-plugin-single-instance`, registered first. A second launch focuses the running app, and
  the Overwolf plugin writes nothing in the second process before it exits.
- `tauri-plugin-window-state`, with a filter that leaves out the plugin's consent windows (labels
  that start with `ow-cmp`). The ad guests are webviews inside your window, not windows, so they
  need no filter.

The app looks the window up with `get_window("main")`. A window that hosts an ad has more than one
webview, so `get_webview_window` does not find it.

## Next steps

- [CONFIG.md](../../docs/CONFIG.md): every `plugins.overwolf` key.
- [api/owadview.md](../../docs/api/owadview.md): the `<owadview>` attributes and events.
- [AD-FORMATS.md](../../docs/AD-FORMATS.md): high impact, performance and the other formats.
- [OVERWOLF-ONBOARDING.md](../../docs/OVERWOLF-ONBOARDING.md): from test ads to live ads.
- [PRODUCTION-CHECKLIST.md](../../docs/PRODUCTION-CHECKLIST.md): before the first release.
- [MIGRATION.md](../../docs/MIGRATION.md): if your app comes from ow-electron.
