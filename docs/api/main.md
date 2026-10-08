# `ow-tauri/main`

The main-process API. It runs only in the hidden main webview (`ow-main`,
[ADR 0001](../adr/0001-hidden-main-webview.md)); importing it anywhere else
throws `OwTauriError('forbidden')`.

Generated reference: the `main` module in `packages/ow-tauri/docs-out`
([how to build it](README.md#build-the-reference)).

| Export | What it is | Specification |
|---|---|---|
| `overwolf` | the `app.overwolf` object (`OverwolfApi`): analytics and ads switches, consent windows, email hashes, payment id, `uid`, `muid`, `phasePercent`, `utmParams`, `packages`. `app.overwolf` in `ow-tauri/electron` is the same instance | [CONTRACT B.1.1](../CONTRACT.md#b11-appoverwolf-overwolfapi) |
| `overwolf.packages` | the package manager; every package reports as unavailable while no package runtime exists | [CONTRACT B.1.3](../CONTRACT.md#b13-overwolfpackages-overwolfpackagemanager), [H](../CONTRACT.md#h-packages) |
| `files` | `readText`, `writeText`, `exists`, `mkdir`: scoped, asynchronous file access in place of Node `fs` | [CONTRACT B.1.7](../CONTRACT.md#b17-files) |
| `autoUpdater` | an electron-updater compatible `AppUpdater` over Overwolf's generic feed | [CONTRACT I.5](../CONTRACT.md#i5-autoupdater-in-ow-taurimain) |
| `whenHostReady()` | resolves once the host acknowledged the main runtime | [CONTRACT B.1](../CONTRACT.md#b1-ow-taurimain) |
| `RecorderError` | the class for `instanceof` checks on recorder errors | [CONTRACT B.1.5](../CONTRACT.md#b15-recordererror) |
| `OwTauriError`, `OwTauriUnsupportedError` | the errors every ow-tauri entry point throws | [CONTRACT A.4](../CONTRACT.md#a4-errors) |
| `UpdateCheckResult`, `UpdateInfo`, `ProgressInfo`, `UpdaterConfig` | types for `autoUpdater` code | [CONTRACT I.1](../CONTRACT.md#i1-feed-and-configuration) |

Synchronous members (`uid`, `muid`, `generateUserEmailHashes` and the
others) are served from a state cache the host keeps current
([CONTRACT B.1.6](../CONTRACT.md#b16-synchronous-members-and-the-state-cache)).
