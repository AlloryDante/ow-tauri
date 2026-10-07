/**
 * Small pure helpers shared by the main process and the window: the formula
 * uid, id masking and the relaunch arguments. They run unchanged in Node
 * (ow-electron's main process), in a webview (ow-tauri's main webview) and in
 * the window.
 *
 * @packageDocumentation
 */
import type { AdMode } from './ipc.js';

/**
 * The formula uid ow-electron derives from `package.json` (`app_cuid` in
 * analytics; CONTRACT G.2): `sha1("{'author':'<author>','name':'<name>.electron'}")`,
 * each byte written as two letters `a`..`p`, low nibble first.
 *
 * @param author - the author name (`author` string or `author.name`; `"unknown"` when missing)
 * @param name - `productName`, or `name` when there is no product name
 * @returns the 40-letter uid
 *
 * @example
 * ```ts
 * await formulaUid('Example Studio', 'Parity Harness');
 * // 'bijigndkghcikkfmhgkmicdkjpdehpjafgpmdhcc'
 * ```
 */
export async function formulaUid(author: string, name: string): Promise<string> {
  const text = `{'author':'${author}','name':'${name}.electron'}`;
  const digest = new Uint8Array(
    await globalThis.crypto.subtle.digest('SHA-1', new TextEncoder().encode(text)),
  );
  let out = '';
  for (const b of digest) out += String.fromCharCode(97 + (b & 15), 97 + (b >> 4));
  return out;
}

/**
 * The author input of the uid formula, read the way ow-electron reads
 * `package.json` `author` (CONTRACT G.2).
 *
 * @param author - the manifest's `author` value
 * @returns the author name, or `"unknown"`
 */
export function authorName(author: unknown): string {
  if (typeof author === 'string' && author !== '') return author;
  if (typeof author === 'object' && author !== null) {
    const name = (author as { name?: unknown }).name;
    if (typeof name === 'string' && name !== '') return name;
  }
  return 'unknown';
}

/**
 * Masks an identifier for display: the first and last four characters with
 * an ellipsis between them (`abcd…wxyz`). Short values are fully masked.
 *
 * @param id - the identifier
 * @returns the masked form
 */
export function maskId(id: string): string {
  if (id.length === 0) return '(none)';
  if (id.length <= 10) return '•'.repeat(id.length);
  return `${id.slice(0, 4)}…${id.slice(-4)}`;
}

/** The switch that selects test ads on both hosts. */
export const TEST_AD_SWITCH = '--test-ad';

/**
 * The arguments for `app.relaunch({ args })` that restart the app in `mode`:
 * the current arguments after the executable (Electron's
 * `process.argv.slice(1)`), without `--test-ad`, with `--test-ad` first for
 * test mode.
 *
 * @param argv - `process.argv`
 * @param mode - the mode to restart in
 * @returns the new arguments
 */
export function relaunchArgs(argv: readonly string[], mode: AdMode): string[] {
  const rest = argv.slice(1).filter((a) => a !== TEST_AD_SWITCH);
  return mode === 'test' ? [TEST_AD_SWITCH, ...rest] : rest;
}

/**
 * The file name of a timeline export: `timeline-<host>-<mode>-<iso>.json`,
 * with the ISO time made file-name safe.
 *
 * @param host - the host name
 * @param mode - the ad mode
 * @param at - the export time
 * @returns the file name
 */
export function exportFileName(host: string, mode: AdMode, at: Date): string {
  const iso = at.toISOString().replace(/[:.]/g, '-');
  return `timeline-${host}-${mode}-${iso}.json`;
}
