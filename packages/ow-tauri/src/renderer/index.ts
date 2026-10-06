/**
 * `ow-tauri/renderer`: runtime for UI windows.
 *
 * Provides the `<owadview>` element (backed by a native child webview) and the
 * `ipcRenderer` / `contextBridge` pair for preload code. Specified in
 * `docs/CONTRACT.md` section B.3.
 *
 * Status: scaffold. Only the shared error types are exported so far.
 *
 * @packageDocumentation
 */
export { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
export type { OwTauriErrorCode } from '../shared/errors.js';
