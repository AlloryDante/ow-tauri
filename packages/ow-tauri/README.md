# ow-tauri (npm package)

The JavaScript half of [ow-tauri](../../README.md). Four entry points, plus
the typings and the `ow-tauri` CLI:

| Import | Runs in | Gives you |
|---|---|---|
| `ow-tauri/main` | the hidden main webview | `app.overwolf`: the full ow-electron `OverwolfApi` mirror, including `packages` |
| `ow-tauri/electron` | main webview and preload scripts | an Electron-compatible subset (`app`, `BrowserWindow`, `ipcMain`, `ipcRenderer`, `contextBridge`, ...) for a bundler alias `electron -> ow-tauri/electron` |
| `ow-tauri/renderer` | UI windows | the `<owadview>` element runtime, plus `ipcRenderer` / `contextBridge` |
| `ow-tauri/testing` | unit tests | a fake plugin for tests of code that uses ow-tauri (below) |
| `ow-tauri/types` | TypeScript only | the `electron` and `@overwolf/ow-electron` declarations and the global `overwolf` namespace |
| `npx ow-tauri sign`, `npx ow-tauri sign-exe` | the app's build | Overwolf signing for a Tauri build ([CONTRACT G.4](../../docs/CONTRACT.md#g4-signing)) |

The surface is specified member by member in
[docs/CONTRACT.md](../../docs/CONTRACT.md) section B.

Status: the IPC core (section C), the state cache, `ow-tauri/electron`
(B.2), `ipcRenderer` / `contextBridge` and the `<owadview>` runtime (B.3) in
`ow-tauri/renderer`, `app.overwolf` with its packages manager (B.1.1 to
B.1.3), `files`, `whenHostReady` and the electron-updater compatible
`autoUpdater` (I.5) in `ow-tauri/main`, `ow-tauri/testing`, the typings
(B.4) and the signing CLI (G.4) are implemented, and so are the plugin's
`updater_*` commands behind `autoUpdater`. Moving an app over is described in
[docs/MIGRATION.md](../../docs/MIGRATION.md); the API reference is built by
`npm run docs` ([docs/api](../../docs/api/README.md)).

## Runtime and facades

The plugin injects the runtime (built from `src/bootstrap/`) into every app
webview; the entry points above attach to it, so every bundle in a webview
shares one IPC channel and one set of registries
([ADR 0012](../../docs/adr/0012-js-runtime-singleton.md)). The entry points may
use only the `FacadeKernel` interface (`src/bootstrap/facade-kernel.ts`). The
injected runtime and the npm package are released separately, so an entry
point attaches only when the runtime reports the same contract version and
`RUNTIME_API_VERSION`; otherwise every member throws `OwTauriError('not-ready')`.
Any incompatible change to `FacadeKernel` must increment `RUNTIME_API_VERSION`.

Every module under `dist/bootstrap`, `dist/electron`, `dist/main` and
`dist/renderer` is listed in `sideEffects`: they register host-message
handlers and window hooks when imported, so bundlers must not drop them.

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

Unsupported Electron members and modules (`Menu`, `Tray`, `clipboard`,
`BrowserWindow#setVibrancy`, `dialog.showMessageBoxSync`, ...) are declared
and marked `@deprecated`, so ported code still compiles and editors flag each
use; at run time they throw `OwTauriUnsupportedError`.

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
| `npm run build:injected` | bundles the injected scripts (`src/bootstrap/`, and `src/guest/` when present) into the IIFEs the plugin embeds, in `crates/tauri-plugin-overwolf/js/`; the output is committed |
| `npm run check:injected` | rebuilds them into a temporary directory and fails when the committed copies are stale (CI, Linux) |
| `npm run typecheck` | the package, then the B.4 declarations against `test-d/` |
| `npm test`, `npm run test:coverage` | vitest (2 workers) |
| `npm run docs` | typedoc, warnings are errors |
