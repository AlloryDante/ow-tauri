// Prepares a throwaway app directory and launches it under ow-electron with
// the observation switches, then waits for it to exit.

import { spawn } from 'node:child_process';
import { copyFileSync, createWriteStream, mkdirSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const harnessDir = dirname(dirname(fileURLToPath(import.meta.url)));

/** Absolute path of the ow-electron executable installed in node_modules. */
export function owElectronBinary() {
  return require('@overwolf/ow-electron');
}

/** The installed @overwolf/ow-electron version. */
export function owElectronVersion() {
  return require('@overwolf/ow-electron/package.json').version;
}

/**
 * Writes `<appDir>/package.json` (the identity under test) and copies the
 * harness app files next to it.
 * @param {string} appDir
 * @param {Record<string, unknown>} packageJson fields other than `main`
 */
export function makeAppDir(appDir, packageJson) {
  mkdirSync(appDir, { recursive: true });
  for (const file of ['main.cjs', 'index.html', 'page.js']) {
    copyFileSync(join(harnessDir, 'app', file), join(appDir, file));
  }
  writeFileSync(
    join(appDir, 'package.json'),
    JSON.stringify({ ...packageJson, main: 'main.cjs' }, null, 2) + '\n',
  );
}

/**
 * Launches ow-electron on `appDir` and resolves with its exit status.
 * @param {{appDir: string, switches: string[], env: Record<string, string>, logDir: string, timeoutMs: number}} options
 * @returns {Promise<{code: number | null, signal: string | null, timedOut: boolean, ms: number}>}
 */
export function launch({ appDir, switches, env, logDir, timeoutMs }) {
  mkdirSync(logDir, { recursive: true });
  const childEnv = { ...process.env, ...env };
  delete childEnv.ELECTRON_RUN_AS_NODE;
  const started = Date.now();
  const child = spawn(owElectronBinary(), [...switches, appDir], {
    env: childEnv,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stdout.pipe(createWriteStream(join(logDir, 'stdout.log')));
  child.stderr.pipe(createWriteStream(join(logDir, 'stderr.log')));
  return new Promise((resolve) => {
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill('SIGTERM');
      setTimeout(() => child.kill('SIGKILL'), 10_000).unref();
    }, timeoutMs);
    child.on('exit', (code, signal) => {
      clearTimeout(timer);
      resolve({ code, signal, timedOut, ms: Date.now() - started });
    });
  });
}
