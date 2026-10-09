# Changes from upstream

This sample is based on Overwolf's
[ow-electron-packages-sample](https://github.com/overwolf/ow-electron-packages-sample)
at commit `8a27053` (MIT, Copyright Overwolf Ltd.; [LICENSE](LICENSE)). It
differs from it in these ways:

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
