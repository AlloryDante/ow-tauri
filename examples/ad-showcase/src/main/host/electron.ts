/**
 * `#host` for ow-electron: Node's `fs` and Electron's version strings.
 *
 * @packageDocumentation
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import type { HostAdapter } from './contract.js';

/** The ow-electron adapter. */
export const host: HostAdapter = {
  name: 'ow-electron',
  versions() {
    // ow-electron versions follow the Electron version they ship (42.11.4).
    const electron = process.versions['electron'] ?? 'unknown';
    return {
      hostVersion: electron,
      engine: `Electron ${electron} · Chromium ${process.versions['chrome'] ?? 'unknown'}`,
    };
  },
  async readText(path) {
    try {
      return await readFile(path, 'utf8');
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') return null;
      throw error;
    }
  },
  async writeText(path, text) {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, text, 'utf8');
  },
};
