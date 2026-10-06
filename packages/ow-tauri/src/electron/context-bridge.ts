/**
 * Electron's `contextBridge` (`docs/CONTRACT.md` section B.2.4). A Tauri
 * webview has one JavaScript world, so preload code and page code share
 * `window`. Like Electron, the bridge exposes a copy of plain objects and
 * arrays (the preload keeps its own, mutable originals); the copy is frozen,
 * so page code cannot change the API.
 *
 * @packageDocumentation
 */
import { OwTauriError } from '../shared/errors.js';
import { defineUnsupported, type UnsupportedMethod } from '../shared/unsupported.js';
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

function isPlain(value: object): boolean {
  if (Array.isArray(value)) return true;
  const proto: unknown = Object.getPrototypeOf(value);
  return proto === Object.prototype || proto === null;
}

/**
 * Copies `value` the way Electron's bridge does for the data parts of an API:
 * plain objects and arrays are copied (own enumerable string keys, getters
 * read once), functions and class instances are kept by reference so they
 * keep working. Cycles and shared references are preserved in the copy.
 *
 * @param value - the API value
 * @param copies - originals already copied
 * @returns the copy
 */
export function copyForBridge<T>(value: T, copies = new Map<object, unknown>()): T {
  if (typeof value !== 'object' || value === null || !isPlain(value)) return value;
  const done = copies.get(value);
  if (done !== undefined) return done as T;
  if (Array.isArray(value)) {
    const out: unknown[] = [];
    copies.set(value, out);
    for (const item of value as unknown[]) out.push(copyForBridge(item, copies));
    return out as T;
  }
  const out: Record<string, unknown> =
    Object.getPrototypeOf(value) === null ? (Object.create(null) as Record<string, unknown>) : {};
  copies.set(value, out);
  for (const key of Object.keys(value)) {
    out[key] = copyForBridge((value as Record<string, unknown>)[key], copies);
  }
  return out as T;
}

/** Electron's `contextBridge` (CONTRACT B.2.4). */
export interface ContextBridge {
  /**
   * Defines a non-writable, non-configurable `window[apiKey]`. Plain objects
   * and arrays are copied and the copy is deep-frozen; functions are called
   * directly (no cloning).
   *
   * @param apiKey - the global name
   * @param api - the value to expose
   *
   * @example
   * ```ts
   * contextBridge.exposeInMainWorld('app', {
   *   version: '1.0.0',
   *   ping: () => ipcRenderer.invoke('ping'),
   * });
   * ```
   */
  exposeInMainWorld(apiKey: string, api: unknown): void;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.4).
   */
  readonly exposeInIsolatedWorld: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.4).
   */
  readonly executeInMainWorld: UnsupportedMethod;
}

/**
 * Electron's `contextBridge` (UI windows only).
 *
 * @example
 * ```ts
 * import { contextBridge, ipcRenderer } from 'electron'; // aliased to ow-tauri/electron
 *
 * contextBridge.exposeInMainWorld('games', {
 *   list: () => ipcRenderer.invoke('games:list'),
 * });
 * ```
 */
// The unsupported members are added by defineUnsupported below.
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
      value: deepFreeze(copyForBridge(api)),
      writable: false,
      configurable: false,
      enumerable: true,
    });
  },
} as ContextBridge;
defineUnsupported(contextBridge, 'contextBridge.', ['exposeInIsolatedWorld', 'executeInMainWorld']);
