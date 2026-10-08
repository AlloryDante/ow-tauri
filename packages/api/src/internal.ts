/**
 * Internal helpers shared by the entry points: the command call, the
 * runtime version, the reserved webview labels and the log.
 *
 * @packageDocumentation
 * @internal
 */
import { invoke, type InvokeArgs } from '@tauri-apps/api/core';

import { toOverwolfError } from './errors.js';

/** The plugin name in `plugin:<name>|<command>` and in permission identifiers. */
export const PLUGIN = 'overwolf';

/**
 * The version of this package, sent to the plugin with every `adview_mount`
 * (`runtimeVersion`). The plugin refuses a different major and warns once on
 * a different minor. A unit test keeps it equal to `package.json`.
 */
export const RUNTIME_VERSION = '0.1.0';

/**
 * Calls `plugin:overwolf|<command>` and maps every rejection to an
 * `OverwolfError`.
 *
 * @param command - the command name
 * @param args - its arguments
 * @returns the command result
 */
export async function call<T>(command: string, args?: InvokeArgs): Promise<T> {
  try {
    return await invoke<T>(`plugin:${PLUGIN}|${command}`, args);
  } catch (error) {
    throw toOverwolfError(error, command);
  }
}

/**
 * Whether `label` is reserved for a webview the plugin creates: an ad guest
 * (`owad-*`) or a consent window (`ow-cmp*`). Code of this package never runs
 * its app-page features there.
 *
 * @param label - a webview label
 * @returns whether it is reserved
 */
export function isReservedLabel(label: string): boolean {
  return label.startsWith('owad-') || label.startsWith('ow-cmp');
}

/** The part of `window.__TAURI_INTERNALS__` this package reads. */
interface TauriInternals {
  metadata?: { currentWebview?: { label?: unknown } };
}

/**
 * The label of the webview this page runs in, from Tauri's metadata, or
 * `undefined` outside Tauri.
 *
 * @param scope - the global object (tests pass their own)
 * @returns the label
 */
export function currentWebviewLabel(scope: object = globalThis): string | undefined {
  const internals = Reflect.get(scope, '__TAURI_INTERNALS__') as TauriInternals | undefined;
  const label = internals?.metadata?.currentWebview?.label;
  return typeof label === 'string' ? label : undefined;
}

/**
 * Whether the page runs inside Tauri (its IPC is installed).
 *
 * @param scope - the global object (tests pass their own)
 * @returns whether `__TAURI_INTERNALS__` exists
 */
export function inTauri(scope: object = globalThis): boolean {
  const internals: unknown = Reflect.get(scope, '__TAURI_INTERNALS__');
  return typeof internals === 'object' && internals !== null;
}

/** Log levels of {@link log}. */
export type LogLevel = 'debug' | 'warn';

const warned = new Set<string>();

/**
 * Logs a message on the page console with the package prefix.
 *
 * @param level - `debug` (detail) or `warn` (something the developer should fix)
 * @param message - the message
 */
export function log(level: LogLevel, message: string): void {
  const text = `[tauri-plugin-overwolf] ${message}`;
  if (level === 'warn') console.warn(text);
  else console.debug(text);
}

/**
 * Logs a warning once per key for the lifetime of the page.
 *
 * @param key - deduplication key
 * @param message - the message
 */
export function warnOnce(key: string, message: string): void {
  if (warned.has(key)) return;
  warned.add(key);
  log('warn', message);
}
