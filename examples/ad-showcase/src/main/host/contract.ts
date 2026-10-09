/**
 * The adapter the ow-electron main process imports as `#host` (bundled from
 * `electron.ts`): file access and the version strings for the top bar. The
 * Tauri app does the same in Rust (`src-tauri/src/showcase.rs`). This file
 * declares the shape, for the type checker.
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
