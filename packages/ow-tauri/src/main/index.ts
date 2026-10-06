/**
 * `ow-tauri/main`: the host runtime for the hidden main webview.
 *
 * Exposes `app.overwolf`, a mirror of ow-electron's `OverwolfApi` including the
 * package manager, with Node-style `EventEmitter` semantics. Specified in
 * `docs/CONTRACT.md` section B.1.
 *
 * Status: scaffold. Only the shared error types are exported so far.
 *
 * @packageDocumentation
 */
export { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
export type { OwTauriErrorCode } from '../shared/errors.js';
