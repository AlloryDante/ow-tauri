#!/usr/bin/env node
// The documentation images of the showcase (`npm run screenshots`), recorded
// in the invisible lab (macOS) with test ads only:
//
//   node scripts/screenshots.mjs [--theme dark|light|both] [--out DIR] [--no-build]
//
// For each theme it runs the lab tour on Tauri with in-process stills
// (e2e/run.mjs --steps tour --stills; no screen capture, never a visible
// window, never frontmost) and the tracked placeholder identity of
// package.json, never identity.local.json. It then checks that the app
// reported the placeholder's formula uid (the loopback analytics sink of the
// run), so no image can show another app's uid, and makes each image at
// most 300 KB (halved with `sips` while larger).
//
// Output: e2e/out/screenshots/<theme>/<still>.png (git-ignored). With
// --out DIR every still is also copied to DIR/<still>-<theme>.png.

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
/** The largest image the docs take. */
export const MAX_BYTES = 300 * 1024;

/**
 * The formula uid ow-electron derives from `package.json` (CONTRACT G.2):
 * `sha1("{'author':'<author>','name':'<name>.electron'}")`, each byte as two
 * letters `a`..`p`, low nibble first (`formulaUid` in src/shared/identity.ts).
 *
 * @param {string} author
 * @param {string} name
 * @returns {string}
 */
export function formulaUid(author, name) {
  const digest = createHash('sha1')
    .update(`{'author':'${author}','name':'${name}.electron'}`)
    .digest();
  let out = '';
  for (const b of digest) out += String.fromCharCode(97 + (b & 15), 97 + (b >> 4));
  return out;
}

/**
 * The `app_id` values of the Counter requests in a run's `sink.jsonl` text.
 *
 * @param {string} text
 * @returns {Set<string>}
 */
export function reportedAppIds(text) {
  const ids = new Set();
  for (const line of text.split('\n')) {
    if (!line) continue;
    let path;
    try {
      path = JSON.parse(line).path;
    } catch {
      continue;
    }
    if (typeof path !== 'string' || !path.startsWith('/analytics/Counter')) continue;
    const extra = new URL(path, 'http://sink').searchParams.get('Extra');
    if (!extra) continue;
    try {
      const id = JSON.parse(extra).app_id;
      if (typeof id === 'string') ids.add(id);
    } catch {
      // not JSON
    }
  }
  return ids;
}

function fail(message) {
  console.error(`screenshots: ${message}`);
  process.exit(1);
}

function main() {
  const { values: opts } = parseArgs({
    options: {
      theme: { type: 'string', default: 'both' },
      out: { type: 'string' },
      'no-build': { type: 'boolean', default: false },
    },
  });
  if (process.platform !== 'darwin') fail('the invisible lab is macOS only');
  if (!['dark', 'light', 'both'].includes(opts.theme)) fail(`unknown --theme ${opts.theme}`);
  const themes = opts.theme === 'both' ? ['dark', 'light'] : [opts.theme];

  const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  const author = typeof pkg.author === 'string' ? pkg.author : pkg.author?.name;
  const name = pkg.productName ?? pkg.name;
  if (pkg.overwolf?.uid) fail('package.json carries a uid; the images use the formula uid only');
  const expected = formulaUid(author, name);

  const outRoot = join(root, 'e2e', 'out', 'screenshots');
  mkdirSync(outRoot, { recursive: true });
  // The placeholder identity, passed explicitly so identity.local.json is
  // never staged.
  const identity = join(outRoot, 'placeholder-identity.json');
  writeFileSync(identity, JSON.stringify({ author, productName: name, uid: '' }, null, 2));

  let first = true;
  for (const theme of themes) {
    const runId = `screenshots-${theme}`;
    const stills = join(outRoot, theme);
    const args = [
      join(root, 'e2e', 'run.mjs'),
      '--host',
      'tauri',
      '--mode',
      'test',
      '--steps',
      'tour',
      '--theme',
      theme,
      '--stills',
      stills,
      '--identity',
      identity,
      '--run-id',
      runId,
    ];
    if (opts['no-build'] || !first) args.push('--no-build');
    first = false;
    const r = spawnSync(process.execPath, args, { cwd: root, stdio: 'inherit' });
    if (r.status !== 0) fail(`the ${theme} lab tour failed (e2e/out/${runId}/summary.json)`);
    const sink = join(root, 'e2e', 'out', runId, 'sink.jsonl');
    const ids = existsSync(sink) ? reportedAppIds(readFileSync(sink, 'utf8')) : new Set();
    if (ids.size !== 1 || !ids.has(expected))
      fail(`the ${theme} run did not use the placeholder formula uid only`);
    for (const file of readdirSync(stills).filter((f) => /^[a-z0-9-]+\.png$/.test(f))) {
      const path = join(stills, file);
      while (statSync(path).size > MAX_BYTES) {
        const width = Number(
          /pixelWidth: (\d+)/.exec(
            spawnSync('sips', ['-g', 'pixelWidth', path], { encoding: 'utf8' }).stdout,
          )?.[1],
        );
        if (!width || width < 400) fail(`${file} stays over ${MAX_BYTES} bytes`);
        spawnSync('sips', ['--resampleWidth', String(Math.round(width / 2)), path], {
          stdio: 'ignore',
        });
      }
      if (opts.out) {
        const dir = resolve(opts.out);
        mkdirSync(dir, { recursive: true });
        copyFileSync(path, join(dir, file.replace(/\.png$/, `-${theme}.png`)));
      }
    }
    console.log(`screenshots: ${theme} -> ${stills}`);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
