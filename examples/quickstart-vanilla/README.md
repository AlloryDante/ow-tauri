# Quickstart (vanilla TypeScript)

The getting-started app of `tauri-plugin-overwolf`: an app created with
`npm create tauri-app@latest my-game-app -- --template vanilla-ts`, then the
steps of the walkthrough applied. One window, one 400x300 Overwolf ad, test
ads on. Every file the walkthrough shows is here as it shows it; the
repository builds it in CI on Windows, macOS and Linux.

| File                                                                       | What the walkthrough added                                                                          |
| -------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| [src-tauri/Cargo.toml](src-tauri/Cargo.toml)                               | `tauri-plugin-overwolf` (and as a build dependency with `features = ["build"]`), `tauri-plugin-log` |
| [src-tauri/build.rs](src-tauri/build.rs)                                   | the plugin's build step before `tauri_build::build()`                                               |
| [src-tauri/src/lib.rs](src-tauri/src/lib.rs)                               | the plugins, and the macOS crashed-guest hook                                                       |
| [src-tauri/tauri.conf.json](src-tauri/tauri.conf.json)                     | `plugins.overwolf` (author and name: the uid inputs; test ads on), the CSP                          |
| [src-tauri/tauri.windows.conf.json](src-tauri/tauri.windows.conf.json)     | NSIS with the plugin's installer hooks                                                              |
| [src-tauri/capabilities/default.json](src-tauri/capabilities/default.json) | `overwolf:default` for the `main` **webview** (never `windows`: that also matches the ad guests)    |
| [src/main.ts](src/main.ts), [index.html](index.html)                       | the `<owadview>` runtime, the ad and the ad privacy settings button                                 |
| [src-tauri/.gitignore](src-tauri/.gitignore)                               | `/gen/overwolf` (the build step writes it)                                                          |

In your own app the dependencies are `tauri-plugin-overwolf = "1"`,
`tauri-plugin-overwolf-api` and `tauri-plugin-overwolf-cli` from the
registries; here they are the repository's crate (by path) and npm
workspaces.

## Run

From the repository root, `npm install` once (Node 22.12 or newer and the
[Tauri 2 prerequisites](https://tauri.app/start/prerequisites/)). Then, from
`examples/quickstart-vanilla`:

| Script              | What it does                                                                                                           |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `npm run tauri dev` | the dev server with hot reload                                                                                         |
| `npm start`         | a debug build with the page embedded, then the app ([scripts/run.mjs](scripts/run.mjs); args after `--` go to the app) |
| `npm run doctor`    | `ow-tauri doctor`: the resolved uid, the configuration and the capabilities                                            |

Test ads are on (`"ads": { "testAd": true }`); remove it before shipping.
Run the CLI from the local install (`npm run doctor`, `npm exec --no --
ow-tauri ...`), never as `npx ow-tauri`.

## Other plugins

[src-tauri/src/lib.rs](src-tauri/src/lib.rs) also shows two common plugins
next to this one:

- `tauri-plugin-single-instance`, registered **first**: a second launch
  focuses the running app, and the Overwolf plugin writes nothing in the
  second process before it exits.
- `tauri-plugin-window-state` with a filter that leaves out the plugin's
  consent windows (labels starting with `ow-cmp`). The ad guests are
  webviews inside your window, not windows, so they need no filter.

The window is looked up with `get_window("main")`: a window that hosts an ad
has more than one webview, so `get_webview_window` does not find it.
