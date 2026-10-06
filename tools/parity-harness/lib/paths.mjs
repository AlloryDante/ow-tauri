// OS locations the harness snapshots, and per-OS home isolation.

import { homedir } from 'node:os';
import { join } from 'node:path';

/**
 * Electron's `appData` directory for a given home directory.
 * @param {string} home
 */
export function appDataDir(home) {
  switch (process.platform) {
    case 'darwin':
      return join(home, 'Library', 'Application Support');
    case 'win32':
      return process.env.APPDATA ?? join(home, 'AppData', 'Roaming');
    default:
      return process.env.XDG_CONFIG_HOME && home === homedir()
        ? process.env.XDG_CONFIG_HOME
        : join(home, '.config');
  }
}

/**
 * Environment variables that make Electron resolve its user directories under
 * `home` instead of the real home directory. macOS honours CFFIXED_USER_HOME
 * for Cocoa path lookups; Linux uses HOME and XDG_CONFIG_HOME. Windows reads
 * known folders from the shell API, so isolation is not available there.
 * @param {string} home
 * @returns {Record<string, string> | null} null when isolation is unsupported
 */
export function isolationEnv(home) {
  switch (process.platform) {
    case 'darwin':
      return { CFFIXED_USER_HOME: home };
    case 'linux':
      return { HOME: home, XDG_CONFIG_HOME: join(home, '.config') };
    default:
      return null;
  }
}
