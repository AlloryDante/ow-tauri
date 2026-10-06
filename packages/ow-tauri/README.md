# ow-tauri (npm package)

The JavaScript half of [ow-tauri](../../README.md). Three entry points:

| Import | Runs in | Gives you |
|---|---|---|
| `ow-tauri/main` | the hidden main webview | `app.overwolf`: the full ow-electron `OverwolfApi` mirror, including `packages` |
| `ow-tauri/electron` | main webview and preload scripts | an Electron-compatible subset (`app`, `BrowserWindow`, `ipcMain`, `ipcRenderer`, `contextBridge`, ...) for a bundler alias `electron -> ow-tauri/electron` |
| `ow-tauri/renderer` | UI windows | the `<owadview>` element runtime, plus `ipcRenderer` / `contextBridge` |

Status: scaffold. The surface is specified member by member in
[docs/CONTRACT.md](../../docs/CONTRACT.md) section B.
