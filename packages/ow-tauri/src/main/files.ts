/**
 * `files` (`docs/CONTRACT.md` section B.1.7): scoped file access replacing
 * Node `fs` in main-process code. Scope and atomicity are enforced by the
 * plugin's `fs_*` commands (A.2.3).
 *
 * @packageDocumentation
 */
import type { Kernel } from '../bootstrap/kernel.js';

/** Options of {@link Files.mkdir}. */
export interface MkdirOptions {
  /** Create missing parent directories. */
  recursive?: boolean;
}

/** Scoped file access (main webview only). */
export interface Files {
  /**
   * Reads a UTF-8 text file.
   *
   * @param path - an absolute path inside the file scope
   * @returns the text, or `null` when the file does not exist
   */
  readText(path: string): Promise<string | null>;
  /**
   * Writes a UTF-8 text file atomically, creating parent directories.
   *
   * @param path - an absolute path inside the writable scope
   * @param data - the text
   */
  writeText(path: string, data: string): Promise<void>;
  /**
   * Whether a file or directory exists.
   *
   * @param path - an absolute path inside the file scope
   * @returns `true` when it exists
   */
  exists(path: string): Promise<boolean>;
  /**
   * Creates a directory; succeeds if it already exists.
   *
   * @param path - an absolute path inside the writable scope
   * @param options - `recursive`
   */
  mkdir(path: string, options?: MkdirOptions): Promise<void>;
}

/**
 * Builds the `files` object over a kernel.
 *
 * @param kernel - the kernel
 * @returns the `files` object
 * @internal
 */
export function createFiles(kernel: Kernel): Files {
  return Object.freeze({
    async readText(path: string): Promise<string | null> {
      kernel.require('main', 'files.readText');
      const text = await kernel.command('fs_read_text', { path });
      return typeof text === 'string' ? text : null;
    },
    async writeText(path: string, data: string): Promise<void> {
      kernel.require('main', 'files.writeText');
      await kernel.command('fs_write_text', { path, data });
    },
    async exists(path: string): Promise<boolean> {
      kernel.require('main', 'files.exists');
      return (await kernel.command('fs_exists', { path })) === true;
    },
    async mkdir(path: string, options?: MkdirOptions): Promise<void> {
      kernel.require('main', 'files.mkdir');
      await kernel.command(
        'fs_mkdir',
        options?.recursive === undefined ? { path } : { path, recursive: options.recursive },
      );
    },
  });
}
