# tauri-plugin-overwolf-guest-shims (private)

Sources of the scripts `tauri-plugin-overwolf` injects into the webviews it creates. Not published:
the built files are committed in `crates/tauri-plugin-overwolf/js/` so the crate builds without Node.

| Source | Output | Runs in |
|---|---|---|
| `src/adview-host.ts` | `js/adview-host.js` | ad guests (`owad-*`): `window.__overwolf__`, host API, silent page dialogs |
| `src/cmp.ts` | `js/cmp.js` | consent windows (`ow-cmp*`) |
| `src/session-restore.ts` | `js/session-restore.js` | a recreated ad guest on macOS, once: restores the carried `sessionStorage` |

```sh
npm run build:generated --workspace tauri-plugin-overwolf-guest-shims   # rebuild js/*.js
npm run check:generated                                                  # (root) fail when stale
```

Each output contains one comment token (`/*__OW_TAURI_ADVIEW_CONFIG__*/null`,
`/*__OW_TAURI_CMP_CONFIG__*/null`, `/*__OW_TAURI_SESSION_SNAPSHOT__*/null`) that the plugin replaces
with the webview's configuration before injecting the script.
