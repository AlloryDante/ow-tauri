#!/usr/bin/env node
// Runs ow-electron, the baseline host, from the parity harness's own install
// (tools/parity-harness, installed with `npm install --workspaces=false`).
//
//   node scripts/ow-electron.mjs [--test-ad] <app folder>
//
// The showcase does not depend on @overwolf/ow-electron itself: installed in
// the repository's workspace it would be hoisted next to ow-tauri, whose
// ambient `electron` types then merge with ow-electron's.

import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const harness = join(repo, 'tools', 'parity-harness');

/**
 * The ow-electron executable of the parity harness, or `null`.
 *
 * @returns {{ exe: string, version: string } | null}
 */
export function owElectron() {
  if (!existsSync(join(harness, 'node_modules', '@overwolf', 'ow-electron', 'package.json')))
    return null;
  const require = createRequire(join(harness, 'package.json'));
  return {
    exe: require('@overwolf/ow-electron'),
    version: require('@overwolf/ow-electron/package.json').version,
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const found = owElectron();
  if (!found) {
    console.error(
      'ow-electron is not installed. Run once:\n  cd tools/parity-harness && npm install --workspaces=false',
    );
    process.exit(2);
  }
  const env = { ...process.env };
  delete env.ELECTRON_RUN_AS_NODE;
  const child = spawn(found.exe, process.argv.slice(2), { stdio: 'inherit', env });
  child.on('exit', (code, signal) => process.exit(signal ? 1 : (code ?? 0)));
}
