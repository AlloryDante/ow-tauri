/**
 * `ow-tauri/electron`: an Electron-compatible subset for a bundler alias
 * `electron -> ow-tauri/electron`.
 *
 * Main-process code and preload scripts written for ow-electron keep their
 * imports; members ow-tauri does not implement throw
 * {@link OwTauriUnsupportedError}. The member-by-member table is in
 * `docs/CONTRACT.md` section B.2.
 *
 * Status: scaffold. Only the shared error types are exported so far.
 *
 * @packageDocumentation
 */
export { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
export type { OwTauriErrorCode } from '../shared/errors.js';
