/**
 * The sample's restart in TEST or LIVE mode: the app's own command
 * `sample_restart` (`src-tauri/src/sample.rs`), which relaunches the app with
 * or without `--test-ad` on the page shown now, once this process has exited
 * (as ow-electron's `app.relaunch()` and `app.exit(0)`). Under `tauri dev`
 * the app refuses: the new process would lose the Tauri CLI's dev server.
 *
 * @packageDocumentation
 */
import { invoke } from '@tauri-apps/api/core';

/** Test ads (`--test-ad`) or live ads. */
export type AdMode = 'test' | 'live';

/** Calls an app command (tests pass a fake). */
export type Invoke = (command: string, args: Record<string, unknown>) => Promise<unknown>;

/**
 * Restarts the app in `mode`.
 *
 * @param mode - `test` or `live`
 * @param call - the command call (default: Tauri's `invoke`)
 * @returns `null` once the restart is under way (this process then exits),
 *   or why the app refused it
 */
export async function restartApp(mode: AdMode, call: Invoke = invoke): Promise<string | null> {
  try {
    await call('sample_restart', { mode });
    return null;
  } catch (error) {
    return typeof error === 'string' ? error : String(error);
  }
}
