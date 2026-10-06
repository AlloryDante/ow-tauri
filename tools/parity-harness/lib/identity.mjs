// App identity used for harness runs. The committed default is a neutral
// example; a git-ignored local.identity.json next to package.json overrides it
// (for example to observe a uid that Overwolf has enabled for ads).

import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

/** Neutral identity committed with the harness. */
export const DEFAULT_IDENTITY = Object.freeze({
  name: 'parity-harness',
  productName: 'Parity Harness',
  author: { name: 'Example Studio' },
  version: '0.1.0',
});

/**
 * Loads the identity: DEFAULT_IDENTITY merged with `local.identity.json`
 * (or the file given with --identity).
 * @param {string} harnessDir
 * @param {string} [file]
 */
export function loadIdentity(harnessDir, file) {
  const path = file ?? join(harnessDir, 'local.identity.json');
  if (!existsSync(path)) return { ...DEFAULT_IDENTITY, source: 'default' };
  const local = JSON.parse(readFileSync(path, 'utf8'));
  return { ...DEFAULT_IDENTITY, ...local, source: path };
}

/**
 * The app name ow-electron shows for an identity: productName, else name.
 * @param {{productName?: string, name: string, build?: {productName?: string}}} pkg
 */
export function displayName(pkg) {
  return (pkg.build && pkg.build.productName) || pkg.productName || pkg.name;
}
