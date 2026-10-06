# ow-tauri (npm package)

The JavaScript half of [ow-tauri](../../README.md). Three entry points:

| Import | Runs in | Gives you |
|---|---|---|
| `ow-tauri/main` | the hidden main webview | `app.overwolf`: the full ow-electron `OverwolfApi` mirror, including `packages` |
| `ow-tauri/electron` | main webview and preload scripts | an Electron-compatible subset (`app`, `BrowserWindow`, `ipcMain`, `ipcRenderer`, `contextBridge`, ...) for a bundler alias `electron -> ow-tauri/electron` |
| `ow-tauri/renderer` | UI windows | the `<owadview>` element runtime, plus `ipcRenderer` / `contextBridge` |

The surface is specified member by member in
[docs/CONTRACT.md](../../docs/CONTRACT.md) section B.

Status: the IPC core (section C), the state cache, `ow-tauri/electron`
(B.2), `ipcRenderer` / `contextBridge` in `ow-tauri/renderer`, `files` and
`whenHostReady` in `ow-tauri/main`, `ow-tauri/testing` and the typings (B.4)
are implemented. `app.overwolf`, `autoUpdater` and `<owadview>` are not yet.

## TypeScript setup

After removing `@overwolf/ow-electron`, point TypeScript at the declarations
this package ships. The bundler alias `electron -> ow-tauri/electron` and the
`paths` entry must agree:

```jsonc
{
  "compilerOptions": {
    "types": ["node", "ow-tauri/types"],
    "paths": {
      "electron": ["./node_modules/ow-tauri/dist/types/electron.d.ts"],
      "@overwolf/ow-electron": ["./node_modules/ow-tauri/dist/types/ow-electron.d.ts"]
    }
  }
}
```

`ow-tauri/types` alone (without `paths`) also declares both modules, plus the
global `Electron` and `overwolf` namespaces.

## Tests

`ow-tauri/testing` fakes the plugin for unit tests (vitest + happy-dom):

```ts
import { mockHost, setHostContext, settle } from 'ow-tauri/testing';

const host = mockHost({ label: 'ow-main' });     // main webview
await settle();
host.push({ type: 'lifecycle', event: 'before-quit', requestId: 1 });
expect(host.callsOf('app_quit_reply')).toHaveLength(1);
setHostContext('ui');                             // act as a UI window
```

## Scripts

| Script | Does |
|---|---|
| `npm run build` | `tsc` to `dist/`, then copies `src/types/*.d.ts` to `dist/types/` |
| `npm run build:injected` | bundles `src/bootstrap/` into the IIFE the plugin injects (`crates/tauri-plugin-overwolf/js/bootstrap.js`) |
| `npm run typecheck` | the package, then the B.4 declarations against `test-d/` |
| `npm test`, `npm run test:coverage` | vitest (2 workers) |
| `npm run docs` | typedoc, warnings are errors |
