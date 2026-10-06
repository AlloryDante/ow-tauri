/**
 * Electron's `contextBridge` (`docs/CONTRACT.md` section B.2.4). A Tauri
 * webview has one JavaScript world, so preload code and page code share
 * `window`; the bridge only makes the exposed API read-only.
 *
 * @packageDocumentation
 */
import { OwTauriError } from '../shared/errors.js';
import { defineUnsupported } from '../shared/unsupported.js';
import { kernel } from './runtime.js';

/**
 * Freezes `value` and every plain object and array reachable from it.
 * Functions and class instances (an exposed `ipcRenderer`, a `Date`) are
 * left as they are, so they keep working; functions are not wrapped.
 *
 * @param value - the API object
 * @param seen - objects already visited
 * @returns `value`
 */
export function deepFreeze<T>(value: T, seen = new WeakSet()): T {
  if (typeof value !== 'object' || value === null) return value;
  const object = value as object;
  if (seen.has(object)) return value;
  seen.add(object);
  const proto: unknown = Object.getPrototypeOf(object);
  if (!Array.isArray(object) && proto !== Object.prototype && proto !== null) return value;
  for (const key of Reflect.ownKeys(object)) {
    const descriptor = Object.getOwnPropertyDescriptor(object, key);
    if (descriptor && 'value' in descriptor) deepFreeze(descriptor.value as unknown, seen);
  }
  Object.freeze(object);
  return value;
}

/** Electron's `contextBridge` (CONTRACT B.2.4). */
export interface ContextBridge {
  /**
   * Defines a non-writable, non-configurable `window[apiKey]`. Objects are
   * deep-frozen; functions are called directly (no cloning).
   *
   * @param apiKey - the global name
   * @param api - the value to expose
   */
  exposeInMainWorld(apiKey: string, api: unknown): void;
}

/** Electron's `contextBridge` (UI windows only). */
export const contextBridge: ContextBridge = {
  exposeInMainWorld(apiKey, api) {
    kernel.require('ui', 'contextBridge.exposeInMainWorld');
    if (typeof apiKey !== 'string' || apiKey === '') {
      throw new OwTauriError(
        'invalid-argument',
        'contextBridge.exposeInMainWorld: apiKey must be a non-empty string',
      );
    }
    if (Object.prototype.hasOwnProperty.call(globalThis, apiKey)) {
      throw new Error('Cannot bind an API on top of an existing property on the window object');
    }
    Object.defineProperty(globalThis, apiKey, {
      value: deepFreeze(api),
      writable: false,
      configurable: false,
      enumerable: true,
    });
  },
};
defineUnsupported(contextBridge, 'contextBridge.', ['exposeInIsolatedWorld', 'executeInMainWorld']);
