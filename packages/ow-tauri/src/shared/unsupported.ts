/**
 * Helpers that implement the "U" (unsupported) rows of `docs/CONTRACT.md`
 * section B.2: calling or constructing the member throws
 * {@link OwTauriUnsupportedError}; reading an unsupported property returns
 * `undefined` and logs a warning once.
 *
 * @packageDocumentation
 */
import { OwTauriUnsupportedError } from './errors.js';

/** A member ow-tauri does not support: calling it throws {@link OwTauriUnsupportedError}. */
export type UnsupportedMethod = (...args: unknown[]) => never;

/** Logs a warning once per key. */
export type WarnOnce = (key: string, message: string) => void;

/** Default reason used when a contract row gives none. */
export const DEFAULT_REASON = 'it has no equivalent in Tauri (see docs/CONTRACT.md section B.2)';

/**
 * A function that always throws {@link OwTauriUnsupportedError} for `api`.
 *
 * @param api - the member name, e.g. `app.getGPUInfo`
 * @param reason - why, and the alternative if there is one
 * @returns the throwing function
 */
export function unsupportedMethod(api: string, reason: string = DEFAULT_REASON): UnsupportedMethod {
  return () => {
    throw new OwTauriUnsupportedError(api, reason);
  };
}

/**
 * Defines unsupported methods and properties on `target` (non-enumerable).
 *
 * @param target - the object (or prototype) to extend
 * @param prefix - the API prefix, e.g. `app.` or `BrowserWindow#`
 * @param methods - member names that throw when called
 * @param properties - member names that read as `undefined` with a one-time warning
 * @param warnOnce - the warning sink
 */
export function defineUnsupported(
  target: object,
  prefix: string,
  methods: readonly string[],
  properties: readonly string[] = [],
  warnOnce?: WarnOnce,
): void {
  for (const name of methods) {
    Object.defineProperty(target, name, {
      value: unsupportedMethod(`${prefix}${name}`),
      enumerable: false,
      configurable: true,
      writable: true,
    });
  }
  for (const name of properties) {
    const api = `${prefix}${name}`;
    Object.defineProperty(target, name, {
      get: () => {
        (warnOnce ?? consoleWarnOnce)(
          api,
          `ow-tauri does not support ${api}; it reads as undefined`,
        );
        return undefined;
      },
      enumerable: false,
      configurable: true,
    });
  }
}

const warned = new Set<string>();

/**
 * Fallback warning sink: `console.warn` once per key.
 *
 * @param key - deduplication key
 * @param message - the message
 */
export function consoleWarnOnce(key: string, message: string): void {
  if (warned.has(key)) return;
  warned.add(key);
  console.warn(`[ow-tauri] ${message}`);
}

/** Members a module proxy must answer without throwing, so tooling works. */
const INERT = new Set<PropertyKey>([
  'then',
  'toJSON',
  '$$typeof',
  '__esModule',
  'constructor',
  'prototype',
  'asymmetricMatch',
  'nodeType',
]);

/**
 * An Electron module or class ow-tauri does not provide (`Menu`, `Tray`,
 * `clipboard`, ...): the object exists so imports compile, and every member
 * throws {@link OwTauriUnsupportedError} when called. The object itself
 * throws when called or constructed (for classes such as `new Tray()`).
 *
 * @param name - the Electron export name, e.g. `Tray`
 * @param reason - why, and the alternative if there is one
 * @returns the module stand-in
 */
export function unsupportedModule(name: string, reason: string = DEFAULT_REASON): object {
  // A bound function: callable and constructible, without the
  // non-configurable `prototype` property that would break the `has` trap.
  const target = function unsupported(): void {
    // never runs: the proxy traps every call
  }.bind(null);
  return new Proxy(target, {
    get(_t, key) {
      if (typeof key === 'symbol' || INERT.has(key)) return undefined;
      if (key === 'name') return name;
      return unsupportedMethod(`${name}.${key}`, reason);
    },
    has: () => false,
    set: () => {
      throw new OwTauriUnsupportedError(name, reason);
    },
    apply: () => {
      throw new OwTauriUnsupportedError(name, reason);
    },
    construct: () => {
      throw new OwTauriUnsupportedError(name, reason);
    },
  });
}
