# Quickstart (React)

The React 19 version of [quickstart-vanilla](../quickstart-vanilla/README.md): one window with one
400x300 Overwolf ad, test ads on, rendered under `React.StrictMode`. It started as
`npm create tauri-app@latest -- --template react-ts`, then took the same Rust, configuration and
capability steps as the vanilla app.

Use it if your app's page is React. It shows the two things React needs to host `<owadview>`: the
runtime import before React renders, and event listeners attached through a `ref`.

## What the app shows

The window shows a heading, a line that says "Test ads" or "Live ads" from `getInfo()`, the ad slot,
and the last ten ad events (`display_ad_loaded`, `impression`, `video_ad_ready`, `complete`). An "Ad
privacy settings" button appears only when `isCMPRequired()` says consent rules apply, and it calls
`openAdPrivacySettingsWindow()`.

## Run it

You need Node 22.12 or newer and the
[Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for your OS. Ads show on Windows
and macOS; on Linux they report `unsupported`.

From the repository root, run once:

```sh
npm install
npm run build --workspace tauri-plugin-overwolf-api --workspace tauri-plugin-overwolf-cli
```

Then, from `examples/quickstart-react`:

```sh
npm run tauri dev   # hot reload
npm start           # a debug build with the page embedded (scripts/run.mjs)
npm run doctor      # ow-tauri doctor
```

Test ads are on in `src-tauri/tauri.conf.json`. Remove `"ads": { "testAd": true }` before you ship.

## Where the plugin is wired in

The React parts:

- [src/main.tsx](src/main.tsx) imports `tauri-plugin-overwolf-api/adview` once, before React
  renders. The import installs the `<owadview>` element runtime.
- [src/AdSlot.tsx](src/AdSlot.tsx) renders `<owadview>`, typed by
  `import type {} from 'tauri-plugin-overwolf-api/jsx'`. It attaches the listeners with a `ref` and
  `addEventListener`, because the ad events are DOM events with underscore names
  (`display_ad_loaded`) that React's `on*` props do not map. StrictMode runs the effect twice, but
  the element stays mounted, so the ad loads once.
- [src/App.tsx](src/App.tsx) reads `getInfo()` and `isCMPRequired()` and shows the ad privacy
  settings button only when consent rules apply.

The Rust and configuration files match the vanilla app, except that this app's
[src-tauri/src/lib.rs](src-tauri/src/lib.rs) registers only the log plugin and the Overwolf plugin
(no single-instance or window-state plugin):

- [src-tauri/Cargo.toml](src-tauri/Cargo.toml)
- [src-tauri/build.rs](src-tauri/build.rs)
- [src-tauri/tauri.conf.json](src-tauri/tauri.conf.json)
- [src-tauri/tauri.windows.conf.json](src-tauri/tauri.windows.conf.json)
- [src-tauri/capabilities/default.json](src-tauri/capabilities/default.json)

The [vanilla README](../quickstart-vanilla/README.md#where-the-plugin-is-wired-in) explains what
each of these files adds, and how to install the crate and the npm packages in your own app. They
are not on crates.io or npm yet.

## Next steps

- [GETTING-STARTED: the React version](../../docs/GETTING-STARTED.md#the-react-version): the same
  React pattern in a short component.
- [api/owadview.md](../../docs/api/owadview.md#frameworks): `<owadview>` in frameworks, its
  attributes and its events.
- [packages-sample](../packages-sample): a larger React app that calls the plugin APIs one by one
  and logs the results.
