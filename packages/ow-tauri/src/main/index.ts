/**
 * `ow-tauri/main`: the host runtime for the hidden main webview
 * (`docs/CONTRACT.md` section B.1). Using it anywhere else throws
 * `OwTauriError('forbidden')`.
 *
 * @packageDocumentation
 */
import { attachRuntime } from '../bootstrap/install.js';
import { createFiles, type Files } from './files.js';

const kernel = attachRuntime();

/** Scoped file access replacing Node `fs` in main-process code (B.1.7). */
export const files: Files = kernel.singleton('main.files', () => createFiles(kernel));

/**
 * Resolves once the host acknowledged `ipc_main_ready` and `main_ready`.
 *
 * @returns the readiness promise
 */
export function whenHostReady(): Promise<void> {
  kernel.require('main', 'whenHostReady');
  return kernel.whenHostReady();
}

export { RecorderError } from './recorder-error.js';
export type { Files, MkdirOptions } from './files.js';
export { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
export type { OwTauriErrorCode, OwTauriErrorOptions } from '../shared/errors.js';
