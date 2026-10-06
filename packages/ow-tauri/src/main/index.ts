/**
 * `ow-tauri/main`: the host runtime for the hidden main webview
 * (`docs/CONTRACT.md` section B.1). Using it anywhere else throws
 * `OwTauriError('forbidden')`.
 *
 * @packageDocumentation
 */
import type { FacadeKernel } from '../bootstrap/facade-kernel.js';
import { attachRuntime } from '../bootstrap/install.js';
import { createFiles, type Files } from './files.js';

const kernel: FacadeKernel = attachRuntime();

/**
 * Scoped file access replacing Node `fs` in main-process code (B.1.7).
 *
 * @example
 * ```ts
 * import { files } from 'ow-tauri/main';
 * import { app } from 'electron';
 *
 * const path = `${app.getPath('userData')}/settings.json`;
 * const text = (await files.readText(path)) ?? '{}';
 * await files.writeText(path, JSON.stringify({ ...JSON.parse(text), seen: true }));
 * ```
 */
export const files: Files = kernel.singleton('main.files', () => createFiles(kernel));

/**
 * Resolves once `ipc_main_ready` and `main_ready` were sent to the host.
 *
 * @returns the readiness promise
 *
 * @example
 * ```ts
 * import { whenHostReady } from 'ow-tauri/main';
 *
 * await whenHostReady(); // the same moment as app.whenReady()
 * ```
 */
export function whenHostReady(): Promise<void> {
  kernel.require('main', 'whenHostReady');
  return kernel.whenHostReady();
}

export { RecorderError } from './recorder-error.js';
export type { Files, MkdirOptions } from './files.js';
export { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
export type { OwTauriErrorCode, OwTauriErrorOptions } from '../shared/errors.js';
