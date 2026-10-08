# `ow-tauri/electron`

The Electron-compatible subset. Apps alias `electron` to it in their
bundler ([MIGRATION step 4](../MIGRATION.md#4-bundle-for-webviews)), so
existing `import { app, BrowserWindow } from 'electron'` lines keep working
in the main process, in preload scripts and in renderer pages. Which members
exist depends on the context, as in Electron.

Generated reference: the `electron` module in `packages/ow-tauri/docs-out`
([how to build it](README.md#build-the-reference)).

| Export | Context | Specification |
|---|---|---|
| `app` | main | [CONTRACT B.2.1](../CONTRACT.md#b21-app-main) |
| `BrowserWindow`, `WebContents` | main | [CONTRACT B.2.2](../CONTRACT.md#b22-browserwindow-main) |
| `ipcMain`, `ipcRenderer` | main; preload and renderer | [CONTRACT B.2.3](../CONTRACT.md#b23-ipcmain-main-and-ipcrenderer-preload-and-renderer) |
| `contextBridge` | preload | [CONTRACT B.2.4](../CONTRACT.md#b24-contextbridge-preload) |
| `screen`, `shell`, `dialog`, `globalShortcut`, `crashReporter`, `nativeTheme`, `process` | main (`process` everywhere) | [CONTRACT B.2.5](../CONTRACT.md#b25-other-modules) |
| `Menu`, `Tray`, `clipboard`, `session` and the other unsupported modules | none: every member throws `OwTauriUnsupportedError` | [CONTRACT B.2.5](../CONTRACT.md#b25-other-modules) |

Each member is supported (Electron semantics), partial (the contract says
what differs) or unsupported. The mapping from Electron's API is summarised
in [MIGRATION.md](../MIGRATION.md#electron-api).

## Typings

`ow-tauri/types` declares `module 'electron'`, `module '@overwolf/ow-electron'`,
the global `Electron` and `overwolf` namespaces, the `app.overwolf`
augmentation and `document.createElement('owadview')`. Unsupported members
are marked `@deprecated Unsupported in ow-tauri`. Set up through
`tsconfig.json` `types` and `paths`
([CONTRACT B.4](../CONTRACT.md#b4-typings),
[MIGRATION step 5](../MIGRATION.md#5-point-typescript-at-the-ow-tauri-typings)).
