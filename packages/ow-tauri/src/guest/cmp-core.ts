/**
 * The consent page shim (`docs/CONTRACT.md` D.6.6): `window.cmp`,
 * `window.privacy` and `window.close()` in the main frame of Overwolf's
 * consent pages under `https://content.overwolf.com/monsdk/electron/`.
 * Built into `crates/tauri-plugin-overwolf/js/cmp.js` by `cmp.ts`.
 *
 * @packageDocumentation
 */
import { MAX_DATA_BYTES, Outbox } from './outbox.js';

/** The only origin the consent shim runs on (D.1). */
export const CMP_ORIGIN = 'https://content.overwolf.com';

/** The command consent windows send with (A.2.7). */
export const CMP_COMMAND = 'plugin:overwolf|cmp_event';

/**
 * The consent string of a `saveConsent` / `saveUnifiedConsent` argument: a
 * string, or a TCData object reduced to its `tcString` (D.6.6). Empty when
 * there is none or it is longer than 16 KiB.
 *
 * @param value - what the page passed
 * @returns the consent string, or `""`
 */
export function consentString(value: unknown): string {
  let s: unknown = value;
  if (typeof value === 'object' && value !== null) s = Reflect.get(value, 'tcString');
  return typeof s === 'string' && s.length <= MAX_DATA_BYTES ? s : '';
}

/**
 * Installs the consent shim in `win`. Does nothing (and returns `false`)
 * outside the main frame of {@link CMP_ORIGIN}, or when it already ran in
 * this document (D.1).
 *
 * @param win - the consent window
 * @param config - `{ adOptimization: boolean }` (D.6.6)
 * @returns whether the shim was installed
 */
export function installCmp(win: Window, config: unknown): boolean {
  let top: boolean;
  try {
    top = win.top === win;
  } catch {
    top = false;
  }
  if (!top || win.location.origin !== CMP_ORIGIN) return false;
  if (Object.prototype.hasOwnProperty.call(win, 'cmp')) return false;

  const outbox = new Outbox(win, CMP_COMMAND);
  const post = (name: string, data?: Record<string, unknown>): void => {
    outbox.post(data === undefined ? { name } : { name, data });
  };
  let adOptimization =
    typeof config === 'object' && config !== null && Reflect.get(config, 'adOptimization') === true;

  const fn = <A extends unknown[], R>(body: (...args: A) => R): ((...args: A) => R) =>
    Object.freeze((...args: A) => body(...args));
  const save = (name: string) =>
    fn((...args: unknown[]) => {
      const consent = consentString(args[0]);
      if (consent !== '') post(name, { consent });
    });

  const cmp = Object.freeze({
    saveConsent: save('saveConsent'),
    saveUnifiedConsent: save('saveUnifiedConsent'),
  });
  const privacy = Object.freeze({
    enableAdOptimization: fn((...args: unknown[]) => {
      adOptimization = args[0] === true;
      post('enableAdOptimization', { enabled: adOptimization });
      return Promise.resolve();
    }),
    getIsAdOptimizationEnabled: fn(() => Promise.resolve(adOptimization)),
  });
  Object.defineProperty(win, 'cmp', {
    value: cmp,
    writable: false,
    configurable: false,
    enumerable: true,
  });
  Object.defineProperty(win, 'privacy', {
    value: privacy,
    writable: false,
    configurable: false,
    enumerable: true,
  });
  // A webview window cannot close itself: the host closes it.
  try {
    Object.defineProperty(win, 'close', {
      value: fn(() => {
        post('close');
      }),
      writable: true,
      configurable: true,
    });
  } catch {
    // Not redefinable here; the readiness timeout closes the window.
  }

  post('ready');
  return true;
}
