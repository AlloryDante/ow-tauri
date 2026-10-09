/**
 * The banner text of a refused restart. ow-tauri refuses a restart under
 * `tauri dev` (the new process would lose the CLI's dev server) and says
 * why; ow-electron rejects only on a bad mode. Pure; unit-tested in
 * `restart.test.ts`.
 *
 * @packageDocumentation
 */

/** Electron's prefix on a rejected `ipcRenderer.invoke`. */
const ELECTRON_PREFIX = /^Error invoking remote method '[^']*': (?:\w*Error: )?/;

/**
 * The text the banner shows for a restart that did not happen.
 *
 * @param error - the rejection of `api.restart` (a string on ow-tauri, an
 *   `Error` on ow-electron)
 * @returns one line, `Restart refused: <reason>`
 */
export function restartRefusal(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error);
  const reason = raw.replace(ELECTRON_PREFIX, '').trim();
  return `Restart refused: ${reason === '' ? 'unknown reason' : reason}`;
}
