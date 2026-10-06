/**
 * `ow-tauri/renderer`: runtime for UI windows (`docs/CONTRACT.md` section
 * B.3). Exports the same `ipcRenderer` and `contextBridge` objects as
 * `ow-tauri/electron`.
 *
 * @packageDocumentation
 */
import type { IpcRenderer } from '../bootstrap/ipc-renderer.js';
import { kernel } from '../electron/runtime.js';

/** Electron's `ipcRenderer` (UI windows only). */
export const ipcRenderer: IpcRenderer = kernel.ipcRenderer;

export { contextBridge } from '../electron/context-bridge.js';
export type { ContextBridge } from '../electron/context-bridge.js';
export { IpcRenderer } from '../bootstrap/ipc-renderer.js';
export type { IpcRendererEvent } from '../bootstrap/ipc-renderer.js';
export { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
export type { OwTauriErrorCode, OwTauriErrorOptions } from '../shared/errors.js';
