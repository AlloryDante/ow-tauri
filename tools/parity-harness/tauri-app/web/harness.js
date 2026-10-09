// Parity harness, Tauri edition: the ad window page's own harness code,
// loaded before the shared harness page (../../app/page.js).
//
// - installs the plugin's <owadview> runtime (tauri-plugin-overwolf-api/adview),
//   as a Tauri app's page does;
// - answers the driver (src-tauri/src/driver.rs): `pageEval` runs a
//   scenario's page code, `owCall` makes an `app.overwolf` call of an
//   ow-electron scenario through the JavaScript API
//   (tauri-plugin-overwolf-api); each answer goes back through the
//   `harness_reply` command;
// - reports the page's user agent and the API's functions once.

import 'tauri-plugin-overwolf-api/adview';
import * as api from 'tauri-plugin-overwolf-api';
import { invoke } from '@tauri-apps/api/core';

/** JSON-safe copy of an arbitrary value (functions and cycles described). */
function safe(value, depth = 0, seen = new WeakSet()) {
  if (value === null || value === undefined) return value ?? null;
  const type = typeof value;
  if (type === 'string' || type === 'number' || type === 'boolean') return value;
  if (type === 'bigint') return { bigint: String(value) };
  if (type === 'function') return { function: value.name || '(anonymous)', length: value.length };
  if (type !== 'object') return { [type]: String(value) };
  if (seen.has(value)) return '[cycle]';
  if (depth > 6) return '[depth]';
  seen.add(value);
  if (Array.isArray(value)) return value.map((v) => safe(v, depth + 1, seen));
  const out = {};
  for (const key of Object.keys(value)) out[key] = safe(value[key], depth + 1, seen);
  return out;
}

/** `{ $undefined: true }` in a scenario stands for `undefined`. */
const unwrap = (a) =>
  a && typeof a === 'object' && !Array.isArray(a) && a.$undefined === true ? undefined : a;

/**
 * ow-electron's `app.overwolf.<fn>(...args)` mapped onto the JavaScript API
 * (same names; `generateUserEmailHashes` is asynchronous here).
 */
async function owCall(fn, args = [], generateFrom = null) {
  let callArgs = args.map(unwrap);
  if (generateFrom !== null) callArgs = [await api.generateUserEmailHashes(generateFrom)];
  const f = api[fn];
  if (typeof f !== 'function') throw new TypeError(`app.overwolf.${fn} is not a function`);
  return f(...callArgs);
}

function reply(id, promise) {
  Promise.resolve()
    .then(promise)
    .then(
      (value) => invoke('harness_reply', { id, reply: { ok: true, value: safe(value) } }),
      (error) => invoke('harness_reply', { id, reply: { ok: false, error: String(error) } }),
    )
    .catch(() => {});
}

window.__harness = Object.freeze({
  /** Runs `code` (an expression) in the page; its awaited value is the answer. */
  pageEval(id, code) {
    // Indirect eval: the page's global scope, as executeJavaScript runs it.
    reply(id, () => (0, eval)(code));
  },
  /** An `app.overwolf` call of a scenario, through the JavaScript API. */
  owCall(fn, args, generateFrom) {
    return owCall(fn, args, generateFrom);
  },
});

invoke('harness_page_info', {
  info: {
    userAgent: navigator.userAgent,
    api: Object.keys(api)
      .filter((k) => typeof api[k] === 'function')
      .sort(),
  },
}).catch(() => {});
