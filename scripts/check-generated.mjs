#!/usr/bin/env node
// Drift check for the generated JavaScript the crate commits:
//
//   crates/tauri-plugin-overwolf/js/*.js     from packages/guest-shims
//   crates/tauri-plugin-overwolf/api-iife.js from packages/api (the `.` entry)
//
// Rebuilds both into a temporary directory and fails when a file differs from
// the committed one, when a built file is not committed, or when js/ holds a
// script no source builds any more.
//
// Usage: node scripts/check-generated.mjs   (npm run check:generated)

import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { runInNewContext } from 'node:vm';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const crate = join(root, 'crates', 'tauri-plugin-overwolf');
const rolldownCli = join(
  dirname(createRequire(import.meta.url).resolve('rolldown/package.json')),
  'bin',
  'cli.mjs',
);

/** @param {string} dir */
function scripts(dir) {
  return existsSync(dir)
    ? readdirSync(dir)
        .filter((name) => name.endsWith('.js'))
        .sort()
    : [];
}

/** Contents with line endings normalised (a Windows checkout may convert them). @param {string} file */
function read(file) {
  return readFileSync(file, 'utf8').replace(/\r\n/g, '\n');
}

/** @type {string[]} */
const problems = [];
const out = mkdtempSync(join(tmpdir(), 'ow-tauri-generated-'));
try {
  for (const pkg of ['guest-shims', 'api']) {
    execFileSync(process.execPath, [rolldownCli, '-c', 'rolldown.config.mjs'], {
      cwd: join(root, 'packages', pkg),
      stdio: ['ignore', 'ignore', 'inherit'],
      env: { ...process.env, OW_TAURI_GENERATED_OUT_DIR: out },
    });
  }
  const built = scripts(join(out, 'js'));
  const committed = scripts(join(crate, 'js'));
  if (built.length === 0) problems.push('the guest shim build produced no scripts');
  for (const name of built) {
    if (!committed.includes(name)) problems.push(`js/${name} is built but not committed`);
    else if (read(join(out, 'js', name)) !== read(join(crate, 'js', name)))
      problems.push(`js/${name} differs from a fresh build`);
  }
  for (const name of committed)
    if (!built.includes(name)) problems.push(`js/${name} is committed but no longer built`);
  if (!existsSync(join(crate, 'api-iife.js'))) problems.push('api-iife.js is not committed');
  else if (read(join(out, 'api-iife.js')) !== read(join(crate, 'api-iife.js')))
    problems.push('api-iife.js differs from a fresh build');
} finally {
  rmSync(out, { recursive: true, force: true });
}

/**
 * Runs api-iife.js against a fake `window.__TAURI__` (no Tauri needed) and
 * returns what it installed as `window.__TAURI__.overwolf`.
 * @param {string | undefined} label - the current webview label
 */
function loadIife(label) {
  const window = {
    __TAURI__: {
      core: { invoke: () => Promise.resolve(null), Channel: class {}, Resource: class {} },
    },
    __TAURI_INTERNALS__: { metadata: { currentWebview: { label } } },
  };
  runInNewContext(read(join(crate, 'api-iife.js')), { window });
  return /** @type {Record<string, unknown>} */ (window.__TAURI__).overwolf;
}

if (problems.length === 0) {
  const api = /** @type {Record<string, unknown> | undefined} */ (loadIife('main'));
  if (typeof api?.getInfo !== 'function')
    problems.push('api-iife.js does not install window.__TAURI__.overwolf.getInfo');
  for (const label of ['owad-1', 'ow-cmp'])
    if (loadIife(label) !== undefined)
      problems.push(
        `api-iife.js installs window.__TAURI__.overwolf in the reserved webview ${label}`,
      );
}

if (problems.length > 0) {
  console.error(
    `The generated scripts of crates/tauri-plugin-overwolf are stale:\n  ${problems.join('\n  ')}\n` +
      'Run `npm run build:generated` and commit crates/tauri-plugin-overwolf/js and api-iife.js.',
  );
  process.exit(1);
}
console.log('crates/tauri-plugin-overwolf/js and api-iife.js match a fresh build.');
