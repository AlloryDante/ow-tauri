/**
 * `ow-tauri/renderer`: runtime for UI windows (`docs/CONTRACT.md` section
 * B.3). Exports the same `ipcRenderer` and `contextBridge` objects as
 * `ow-tauri/electron`, and `owadview`, the document's `<owadview>` runtime.
 *
 * @packageDocumentation
 */
import type { IpcRenderer } from '../bootstrap/ipc-renderer.js';
import { kernel } from '../electron/runtime.js';
import { adviewRuntimeOf } from './owadview.js';

/**
 * Electron's `ipcRenderer` (UI windows only), the same object as
 * `ow-tauri/electron`'s.
 *
 * @example
 * ```ts
 * import { ipcRenderer } from 'ow-tauri/renderer';
 *
 * ipcRenderer.send('log', 'renderer started');
 * ```
 */
export const ipcRenderer: IpcRenderer = kernel.ipcRenderer;

/** The `<owadview>` runtime as `ow-tauri/renderer` exposes it (B.3). */
export interface OwadviewApi {
  /**
   * Registers an `<owadview>` element the runtime has not seen (one created
   * in a way the runtime cannot observe) and mounts it if it is ready.
   *
   * @param el - the element
   */
  upgrade(el: Element): void;
  /**
   * The `<owadview>` elements the runtime tracks in this document.
   *
   * @returns the elements, in discovery order
   */
  elements(): HTMLElement[];
}

/**
 * The document's `<owadview>` runtime (UI windows only). The bootstrap starts
 * it before page scripts run, so apps rarely need this object; it is useful
 * in tests and for elements created in ways the runtime cannot see.
 *
 * @example
 * ```ts
 * import { owadview } from 'ow-tauri/renderer';
 *
 * const ad = document.createElement('owadview');
 * ad.setAttribute('cid', 'main_menu');
 * ad.setAttribute('slotsize', '400x600');
 * document.querySelector('.ad-container')?.append(ad);
 * ad.addEventListener('display_ad_loaded', () => console.log('filled'));
 * owadview.elements(); // [ad]
 * ```
 */
export const owadview: OwadviewApi = {
  upgrade: (el) => {
    kernel.require('ui', 'owadview.upgrade');
    adviewRuntimeOf(kernel).upgrade(el);
  },
  elements: () => {
    kernel.require('ui', 'owadview.elements');
    return adviewRuntimeOf(kernel).elements();
  },
};

export { contextBridge } from '../electron/context-bridge.js';
export type { ContextBridge } from '../electron/context-bridge.js';
export { IpcRenderer } from '../bootstrap/ipc-renderer.js';
export type { IpcRendererEvent } from '../bootstrap/ipc-renderer.js';
export type { AdviewAttributes, AdviewRect } from './owadview-attributes.js';
export { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
export type { OwTauriErrorCode, OwTauriErrorOptions } from '../shared/errors.js';
