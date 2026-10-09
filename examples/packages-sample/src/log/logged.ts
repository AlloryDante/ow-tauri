/**
 * Runs one API call and writes it to the log: the call, then its result or
 * its error (an `OverwolfError` keeps its `code`, so a missing permission
 * shows as `forbidden` and the updater off Windows as `unsupported`).
 *
 * @packageDocumentation
 */
import { OverwolfError } from 'tauri-plugin-overwolf-api';

import { jsonSafe } from './format';
import type { LogSource, LogStore } from './store';

/** What a logged call ended with. */
export type Outcome<T> =
  | {
      /** It resolved. */
      ok: true;
      /** Its value. */
      value: T;
    }
  | {
      /** It rejected. */
      ok: false;
      /** The `OverwolfError` code, or `error` for anything else. */
      code: string;
      /** The error message. */
      message: string;
    };

/** Options of {@link logged}. */
export interface LoggedOptions<T> {
  /** The log source (default `api`). */
  source?: LogSource;
  /** What the log shows of the result (e.g. a masked copy). */
  shown?: (value: T) => unknown;
}

/**
 * The code and message of any rejection value.
 *
 * @param error - what the call rejected with
 * @returns code and message
 */
export function describeError(error: unknown): { code: string; message: string } {
  if (error instanceof OverwolfError) return { code: error.code, message: error.message };
  if (error instanceof Error) return { code: 'error', message: error.message };
  return { code: 'error', message: String(error) };
}

/**
 * Runs `run`, logging `label` before and the result (`result`) or the error
 * (`error`) after. Never rejects.
 *
 * @param log - the log store
 * @param label - the call as text, e.g. `getInfo()`
 * @param run - the call
 * @param options - the log source (default `api`) and how the result is
 *   shown in the log (default: as it is)
 * @returns the outcome
 */
export async function logged<T>(
  log: LogStore,
  label: string,
  run: () => Promise<T>,
  options: LoggedOptions<T> = {},
): Promise<Outcome<T>> {
  const source = options.source ?? 'api';
  log.push('info', source, label);
  try {
    const value = await run();
    const nothing = (value as unknown) === undefined || (value as unknown) === null;
    if (nothing) log.push('result', source, `${label} → done`);
    else
      log.push(
        'result',
        source,
        `${label} →`,
        jsonSafe(options.shown ? options.shown(value) : value),
      );
    return { ok: true, value };
  } catch (error) {
    const { code, message } = describeError(error);
    log.push('error', source, `${label} failed: ${code}`, message);
    return { ok: false, code, message };
  }
}
