/**
 * The narrow kernel interface the runtime's parts depend on, so each part can
 * be unit-tested without a full kernel.
 *
 * @packageDocumentation
 */
import type { HostContext } from '../shared/protocol.js';

/** Log levels, as the plugin's `log` command accepts them. */
export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

/** Kernel services used by IPC, windows and the Electron facade. */
export interface KernelServices {
  /** The context the runtime runs in. */
  readonly context: HostContext;
  /**
   * Throws `OwTauriError('forbidden')` unless the runtime runs in `context`
   * (or the not-ready contract-mismatch error for a mismatched page).
   *
   * @param context - the required context
   * @param api - the member being used, for the error message
   */
  require(context: 'main' | 'ui', api: string): void;
  /**
   * Invokes `plugin:overwolf|<name>`; rejections become `OwTauriError`.
   *
   * @param name - the command name, e.g. `ipc_send`
   * @param args - the command arguments
   * @returns the response
   */
  command(name: string, args?: Record<string, unknown>): Promise<unknown>;
  /**
   * Invokes any other Tauri command (e.g. `plugin:window|show`); rejections
   * become `OwTauriError`.
   *
   * @param command - the full command name
   * @param args - the command arguments
   * @returns the response
   */
  raw(command: string, args?: Record<string, unknown>): Promise<unknown>;
  /**
   * Logs to the console and, in the main webview, to the ow-tauri log file.
   *
   * @param level - the level
   * @param message - the message
   */
  log(level: LogLevel, message: string): void;
  /**
   * Logs a warning once per `key` for the lifetime of the document.
   *
   * @param key - deduplication key
   * @param message - the message
   */
  warnOnce(key: string, message: string): void;
}
