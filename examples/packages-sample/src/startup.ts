/**
 * What the sample logs once at launch: the app info (`getInfo()`) and
 * whether consent is required (`isCMPRequired()`).
 *
 * @packageDocumentation
 */
import { getInfo, isCMPRequired, type OverwolfInfo } from 'tauri-plugin-overwolf-api';

import { logged } from './log/logged';
import type { LogStore } from './log/store';

/**
 * Logs the launch, the app info and the consent state.
 *
 * @param log - the log store
 * @returns the app info, or `null` when `getInfo()` failed
 */
export async function startup(log: LogStore): Promise<OverwolfInfo | null> {
  log.push('info', 'app', 'ow-tauri Packages Sample started');
  const info = await logged(log, 'getInfo()', getInfo, { source: 'app' });
  await logged(log, 'isCMPRequired()', isCMPRequired, { source: 'app' });
  return info.ok ? info.value : null;
}
