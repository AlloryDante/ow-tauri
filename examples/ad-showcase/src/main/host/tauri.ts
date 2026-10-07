/**
 * `#host` for ow-tauri: the scoped `files` API of `ow-tauri/main` (the main
 * process runs in a webview, where Node's `fs` does not exist) and the
 * versions from ow-tauri's `process.versions` shim.
 *
 * @packageDocumentation
 */
import { files } from 'ow-tauri/main';
import type { HostAdapter } from './contract.js';

/** The ow-tauri adapter. */
export const host: HostAdapter = {
  name: 'ow-tauri',
  versions() {
    const versions = process.versions as Record<string, string | undefined>;
    const owTauri = versions['owTauri'] ?? 'unknown';
    return { hostVersion: owTauri, engine: `Tauri ${versions['tauri'] ?? 'unknown'}` };
  },
  readText(path) {
    return files.readText(path);
  },
  // `files.writeText` creates parent folders and writes atomically.
  writeText(path, text) {
    return files.writeText(path, text);
  },
};
