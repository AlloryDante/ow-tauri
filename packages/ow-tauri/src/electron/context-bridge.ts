/**
 * Electron's `contextBridge` (`docs/CONTRACT.md` section B.2.4). A Tauri
 * webview has one JavaScript world, so preload code and page code share
 * `window`. The bridge still passes values the way Electron's does between
 * its two worlds [OBS]: data is copied (class instances lose their
 * prototype, errors become plain `Error`s), functions are proxied (their
 * arguments, results, rejections and exceptions are passed the same way),
 * and the exposed API is deep-frozen. Elements and functions that come back
 * keep their identity.
 *
 * @packageDocumentation
 */
import { OwTauriError } from '../shared/errors.js';
import { defineUnsupported, type UnsupportedMethod } from '../shared/unsupported.js';
import { kernel } from './runtime.js';

/**
 * The message of the `Error` the other side gets when a function throws
 * something that is not an `Error` (a string, a plain object) [OBS].
 */
export const UNKNOWN_EXCEPTION_MESSAGE =
  'An unknown exception occurred in the isolated context, an error occurred but a valid exception was not thrown.';

/**
 * Freezes `value` and everything reachable from it through own properties:
 * objects, arrays, functions, dates, maps (as Electron freezes the API it
 * exposes [OBS]). A frozen `Date` or `Map` still works.
 *
 * @param value - the API value
 * @param seen - objects already visited
 * @returns `value`
 */
export function deepFreeze<T>(value: T, seen = new WeakSet()): T {
  if ((typeof value !== 'object' && typeof value !== 'function') || value === null) return value;
  const object = value as object;
  if (seen.has(object) || isNode(object)) return value;
  seen.add(object);
  for (const key of Reflect.ownKeys(object)) {
    const descriptor = Object.getOwnPropertyDescriptor(object, key);
    if (descriptor && 'value' in descriptor) deepFreeze(descriptor.value as unknown, seen);
  }
  Object.freeze(object);
  return value;
}

/** Every proxy this bridge made, with the function it stands for. */
const proxied = new WeakMap<object, (...args: unknown[]) => unknown>();

function isNode(value: object): boolean {
  return typeof Node === 'function' && value instanceof Node;
}

/** Values Electron passes with the structured clone algorithm. */
function isCloneable(value: object): boolean {
  return (
    value instanceof Date ||
    value instanceof RegExp ||
    value instanceof Map ||
    value instanceof Set ||
    value instanceof ArrayBuffer ||
    ArrayBuffer.isView(value) ||
    (typeof Blob === 'function' && value instanceof Blob)
  );
}

/**
 * A plain `Error` with `error`'s message: the name, the class and every
 * other property stay behind [OBS].
 *
 * @param error - what was thrown or rejected
 * @param thrown - `true` for an exception, `false` for a rejection
 * @returns the error the other side gets
 */
function passError(error: unknown, thrown: boolean): unknown {
  if (error instanceof Error) {
    const copy = new Error(error.message);
    // A thrown error's message arrives as an own enumerable property; a
    // rejected one's as usual [OBS].
    if (thrown) {
      Object.defineProperty(copy, 'message', {
        value: error.message,
        enumerable: true,
        writable: true,
        configurable: true,
      });
    }
    return copy;
  }
  if (thrown) return new Error(UNKNOWN_EXCEPTION_MESSAGE);
  return passValue(error);
}

/**
 * Proxies `fn` (called with `holder` as `this`): arguments are passed in,
 * the result, a rejection or an exception are passed out. A function that is
 * itself a proxy is unwrapped, so a function passed there and back is the
 * same function [OBS].
 *
 * @param fn - the function to proxy
 * @param holder - the object `fn` was read from, or `undefined`
 * @returns the proxy
 */
function passFunction(fn: (...args: unknown[]) => unknown, holder: object | undefined): unknown {
  const original = proxied.get(fn);
  if (original) return original;
  // An anonymous function expression: `name` is '' and `length` 0 [OBS].
  const proxy = (() =>
    function (...args: unknown[]): unknown {
      const passed = args.map((a) => passValue(a));
      let result: unknown;
      try {
        result = Reflect.apply(fn, holder, passed);
      } catch (error) {
        throw passError(error, true);
      }
      return passValue(result);
    })();
  proxied.set(proxy, fn);
  return proxy;
}

/**
 * Passes `value` to the other world the way Electron's bridge does [OBS]:
 * primitives (symbols and bigints included) as they are; functions proxied;
 * promises as new promises whose result is passed; errors as plain `Error`s;
 * elements by reference; dates, regular expressions, maps, sets and binary
 * data cloned; arrays and objects copied (own enumerable keys, getters read
 * once, prototypes and non-enumerable properties dropped, holes kept).
 * Cycles and shared objects stay so within one pass.
 *
 * @param value - the value
 * @param copies - objects already copied in this pass
 * @param holder - the object `value` was read from (for a function's `this`)
 * @returns the passed value
 */
export function passValue(
  value: unknown,
  copies = new Map<object, unknown>(),
  holder?: object,
): unknown {
  if (typeof value === 'function') {
    return passFunction(value as (...args: unknown[]) => unknown, holder);
  }
  if (typeof value !== 'object' || value === null) return value;
  const done = copies.get(value);
  if (done !== undefined) return done;
  if (isNode(value)) return value;
  if (value instanceof Promise) {
    const promise = (value as Promise<unknown>).then(
      (v) => passValue(v),
      (error: unknown) => {
        throw passError(error, false);
      },
    );
    copies.set(value, promise);
    return promise;
  }
  if (value instanceof Error) {
    const error = passError(value, false);
    copies.set(value, error);
    return error;
  }
  if (isCloneable(value)) {
    let clone: unknown;
    try {
      clone = structuredClone(value);
    } catch {
      clone = value;
    }
    copies.set(value, clone);
    return clone;
  }
  if (Array.isArray(value)) {
    const out: unknown[] = new Array<unknown>(value.length);
    copies.set(value, out);
    for (let i = 0; i < value.length; i += 1) {
      out[i] = passValue((value as unknown[])[i], copies, value);
    }
    return out;
  }
  const out: Record<PropertyKey, unknown> = {};
  copies.set(value, out);
  for (const key of Reflect.ownKeys(value)) {
    if (!Object.prototype.propertyIsEnumerable.call(value, key)) continue;
    out[key] = passValue((value as Record<PropertyKey, unknown>)[key], copies, value);
  }
  return out;
}

/** Electron's `contextBridge` (CONTRACT B.2.4). */
export interface ContextBridge {
  /**
   * Defines a non-writable, non-configurable `window[apiKey]` holding a
   * deep-frozen copy of `api`, as Electron's bridge passes it: data copied,
   * functions proxied (their arguments and results are passed the same way),
   * errors made plain `Error`s.
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
    if (typeof apiKey !== 'string') {
      throw new OwTauriError(
        'invalid-argument',
        'contextBridge.exposeInMainWorld: apiKey must be a string',
      );
    }
    if (Object.prototype.hasOwnProperty.call(globalThis, apiKey)) {
      throw new Error('Cannot bind an API on top of an existing property on the window object');
    }
    Object.defineProperty(globalThis, apiKey, {
      value: deepFreeze(passValue(api)),
      writable: false,
      configurable: false,
      enumerable: true,
    });
  },
} as ContextBridge;
defineUnsupported(contextBridge, 'contextBridge.', ['exposeInIsolatedWorld', 'executeInMainWorld']);
