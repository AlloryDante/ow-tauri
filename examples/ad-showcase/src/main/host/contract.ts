/**
 * The per-host adapter the main process imports as `#host`. It is the only
 * code that differs between the two builds: the bundler maps `#host` to
 * `electron.ts` (Node's `fs` exists in ow-electron's main process) or to
 * `tauri.ts` (ow-tauri's main process runs in a webview and uses the scoped
 * `files` API of `ow-tauri/main` instead, MIGRATION.md). This file declares
 * the shape both implement, for the type checker.
 *
 * @packageDocumentation
 */
import type { HostName } from '../../shared/ipc.js';

/** What the main process needs from its host besides the Electron API. */
export interface HostAdapter {
  /** `ow-electron` or `ow-tauri`. */
  readonly name: HostName;
  /** The host version and engine line for the top bar. */
  versions(): { hostVersion: string; engine: string };
  /**
   * Reads a UTF-8 text file.
   *
   * @param path - an absolute path under `userData`
   * @returns the text, or `null` when the file does not exist
   */
  readText(path: string): Promise<string | null>;
  /**
   * Writes a UTF-8 text file, creating parent folders.
   *
   * @param path - an absolute path under `userData`
   * @param text - the text
   */
  writeText(path: string, text: string): Promise<void>;
}

/** The adapter of the host this build targets. */
export declare const host: HostAdapter;
