#!/usr/bin/env node
// The documentation images (`npm run screenshots`), recorded in the
// invisible lab (macOS) with test ads only:
//
//   node scripts/screenshots.mjs [--theme dark|light|both]
//                                [--only showcase,packages-sample,quickstart]
//                                [--out DIR] [--no-build]
//
// For each theme it runs three lab tours with in-process stills (no screen
// capture, never a visible window, never frontmost):
//
// - showcase: this app's tour (e2e/run.mjs --steps tour) with the tracked
//   placeholder identity of package.json, never identity.local.json. Its
//   stills give one image per format of docs/AD-FORMATS.md (`ad-formats/`)
//   and the showcase's consent page (`consent/consent-page`).
// - packages-sample: every page of examples/packages-sample
//   (`packages-sample/`) and the ad privacy settings window the plugin
//   opens (`consent/privacy-settings`), from its e2e/run.mjs --steps tour.
// - quickstart: the quickstart window (`quickstart/window`), from
//   examples/packages-sample's page-host mode (e2e/run.mjs --page
//   quickstart), which hosts examples/quickstart-vanilla's page and identity.
//
// Each lab run has a stall watchdog (a run that records nothing for 60 s is
// killed); a failed run stops this script at once and names the step it
// stalled after. Each app must report only its placeholder formula uid (the
// loopback analytics sink of the run), so no image can show another app's
// uid, and an app configured with a fixed uid is refused. Each image is
// made at most 300 KB (scaled down with `sips`) and the image folder at
// most 8 MB.
//
// Output: DIR/<area>/<name>-<theme>.png (default: the repository's
// docs/images). The raw stills stay in each example's e2e/out/
// (git-ignored).

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
const repoRoot = resolve(root, '..', '..');
const sampleDir = join(repoRoot, 'examples', 'packages-sample');
const quickstartDir = join(repoRoot, 'examples', 'quickstart-vanilla');

/** The largest image the docs take. */
export const MAX_BYTES = 300 * 1024;
/** The largest the image folder may grow. */
export const MAX_TOTAL_BYTES = 8 * 1024 * 1024;
/** The widest image the docs take (pixels); wider stills are scaled down first. */
export const MAX_WIDTH = 1600;

/** The tours, in run order. */
export const TOURS = ['showcase', 'packages-sample', 'quickstart'];

/**
 * The documentation images: `area/name` and the tour still each comes from.
 * The `ad-formats` images follow the sections of docs/AD-FORMATS.md.
 */
export const IMAGES = [
  { tour: 'showcase', still: 'p1-sizes-towers', area: 'ad-formats', name: 'standard-display' },
  { tour: 'showcase', still: 'p7-controls', area: 'ad-formats', name: 'standard-video' },
  { tour: 'showcase', still: 'p6-house', area: 'ad-formats', name: 'house' },
  { tour: 'showcase', still: 'p3-high-impact-takeover', area: 'ad-formats', name: 'high-impact' },
  { tour: 'showcase', still: 'p4-interstitial-default', area: 'ad-formats', name: 'interstitial' },
  { tour: 'showcase', still: 'p5-reward-playing', area: 'ad-formats', name: 'reward' },
  { tour: 'showcase', still: 'p8-consent', area: 'consent', name: 'consent-page' },
  { tour: 'packages-sample', still: 'logger', area: 'packages-sample', name: 'logger' },
  { tour: 'packages-sample', still: 'ads-tester', area: 'packages-sample', name: 'ads-tester' },
  { tour: 'packages-sample', still: 'settings', area: 'packages-sample', name: 'settings' },
  { tour: 'packages-sample', still: 'updater', area: 'packages-sample', name: 'updater' },
  { tour: 'packages-sample', still: 'packages', area: 'packages-sample', name: 'packages' },
  { tour: 'packages-sample', still: 'privacy-settings', area: 'consent', name: 'privacy-settings' },
  { tour: 'quickstart', still: 'quickstart-window', area: 'quickstart', name: 'window' },
];

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

/**
 * The placeholder identity of a Tauri example's `tauri.conf.json`
 * (`plugins.overwolf`), or why it cannot be used for the images.
 *
 * @param {Record<string, any>} conf
 * @returns {{ uid: string } | { error: string }}
 */
export function placeholderOf(conf) {
  const ow = conf?.plugins?.overwolf;
  if (!ow || typeof ow.author !== 'string' || typeof ow.name !== 'string')
    return { error: 'no plugins.overwolf author and name' };
  if (ow.uid)
    return { error: 'plugins.overwolf carries a uid; the images use the formula uid only' };
  return { uid: formulaUid(ow.author, ow.name) };
}

/**
 * Why a lab run failed, from its summary: the verdict, the step a stalled
 * run stopped after, and the first fatal error.
 *
 * @param {Record<string, any> | null} summary
 * @returns {string}
 */
export function failureOf(summary) {
  if (!summary) return 'no summary';
  const parts = [`verdict ${String(summary.verdict)}`];
  if (summary.stalled) parts.push(`stalled after ${String(summary.stalled.after ?? 'start')}`);
  if (summary.safetyKill) parts.push('a window became visible');
  if (summary.everFront) parts.push('the app became frontmost');
  if (summary.fatal?.length) parts.push(`fatal: ${String(summary.fatal[0]).split('\n')[0]}`);
  if (summary.check === false) parts.push('its check failed');
  return parts.join(', ');
}

/**
 * The image files a run of `themes` must produce, as `area/name-theme.png`.
 *
 * @param {string[]} themes
 * @param {string[]} tours
 * @returns {string[]}
 */
export function expectedFiles(themes, tours = TOURS) {
  return themes.flatMap((theme) =>
    IMAGES.filter((i) => tours.includes(i.tour)).map((i) => `${i.area}/${i.name}-${theme}.png`),
  );
}

function fail(message) {
  console.error(`screenshots: ${message}`);
  process.exit(1);
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    return null;
  }
}

/** Runs one lab tour; fails with its summary's reason. */
function labRun(script, args, outDir, runId) {
  const r = spawnSync(process.execPath, [script, ...args, '--run-id', runId], {
    cwd: dirname(dirname(script)),
    stdio: 'inherit',
  });
  const summary = readJson(join(outDir, runId, 'summary.json'));
  if (r.status !== 0) fail(`${runId} failed (${failureOf(summary)}; ${join(outDir, runId)})`);
  return summary;
}

/** Fails unless the run reported exactly the placeholder uid. */
function checkUid(outDir, runId, expected) {
  const sink = join(outDir, runId, 'sink.jsonl');
  const ids = existsSync(sink) ? reportedAppIds(readFileSync(sink, 'utf8')) : new Set();
  if (ids.size !== 1 || !ids.has(expected))
    fail(`${runId} did not use the placeholder formula uid only`);
}

function width(path) {
  return Number(
    /pixelWidth: (\d+)/.exec(
      spawnSync('sips', ['-g', 'pixelWidth', path], { encoding: 'utf8' }).stdout,
    )?.[1],
  );
}

/** Copies a still to `to`, at most MAX_WIDTH wide and MAX_BYTES large. */
function place(from, to) {
  if (!existsSync(from)) fail(`missing still ${from}`);
  mkdirSync(dirname(to), { recursive: true });
  copyFileSync(from, to);
  if (width(to) > MAX_WIDTH)
    spawnSync('sips', ['--resampleWidth', String(MAX_WIDTH), to], { stdio: 'ignore' });
  while (statSync(to).size > MAX_BYTES) {
    const w = width(to);
    if (!w || w < 400) fail(`${to} stays over ${MAX_BYTES} bytes`);
    spawnSync('sips', ['--resampleWidth', String(Math.round(w * 0.8)), to], { stdio: 'ignore' });
  }
}

/** Total size of every file under `dir`. */
function folderBytes(dir) {
  let total = 0;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    total += entry.isDirectory() ? folderBytes(path) : statSync(path).size;
  }
  return total;
}

function main() {
  const { values: opts } = parseArgs({
    options: {
      theme: { type: 'string', default: 'both' },
      only: { type: 'string', default: TOURS.join(',') },
      out: { type: 'string' },
      'no-build': { type: 'boolean', default: false },
    },
  });
  if (process.platform !== 'darwin') fail('the invisible lab is macOS only');
  if (!['dark', 'light', 'both'].includes(opts.theme)) fail(`unknown --theme ${opts.theme}`);
  const themes = opts.theme === 'both' ? ['dark', 'light'] : [opts.theme];
  const tours = opts.only.split(',').filter(Boolean);
  for (const t of tours) if (!TOURS.includes(t)) fail(`unknown tour ${t}`);
  if (process.env.TAURI_CONFIG) fail('TAURI_CONFIG is set; the images use the tracked identities');
  const out = resolve(opts.out ?? join(repoRoot, 'docs', 'images'));

  // The placeholder identities.
  const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  const author = typeof pkg.author === 'string' ? pkg.author : pkg.author?.name;
  const name = pkg.productName ?? pkg.name;
  if (pkg.overwolf?.uid) fail('package.json carries a uid; the images use the formula uid only');
  const uids = { showcase: formulaUid(author, name) };
  for (const [tour, dir] of [
    ['packages-sample', sampleDir],
    ['quickstart', quickstartDir],
  ]) {
    const id = placeholderOf(readJson(join(dir, 'src-tauri', 'tauri.conf.json')));
    if ('error' in id) fail(`${tour}: ${id.error}`);
    uids[tour] = id.uid;
  }

  const showcaseOut = join(root, 'e2e', 'out');
  const sampleOut = join(sampleDir, 'e2e', 'out');
  const shotsRoot = join(showcaseOut, 'screenshots');
  mkdirSync(shotsRoot, { recursive: true });
  // The placeholder identity, passed explicitly so identity.local.json is
  // never staged.
  const identity = join(shotsRoot, 'placeholder-identity.json');
  writeFileSync(identity, JSON.stringify({ author, productName: name, uid: '' }, null, 2));

  const built = new Set(opts['no-build'] ? TOURS : []);
  const noBuild = (tour) => {
    const flag = built.has(tour) ? ['--no-build'] : [];
    built.add(tour);
    return flag;
  };
  for (const theme of themes) {
    const stills = {};
    if (tours.includes('showcase')) {
      const runId = `screenshots-${theme}`;
      const dir = join(shotsRoot, theme);
      labRun(
        join(root, 'e2e', 'run.mjs'),
        [
          '--host',
          'tauri',
          '--mode',
          'test',
          '--steps',
          'tour',
          '--theme',
          theme,
          '--stills',
          dir,
          '--identity',
          identity,
          ...noBuild('showcase'),
        ],
        showcaseOut,
        runId,
      );
      checkUid(showcaseOut, runId, uids.showcase);
      stills.showcase = dir;
    }
    if (tours.includes('packages-sample')) {
      const runId = `screenshots-sample-${theme}`;
      const dir = join(sampleOut, runId, 'stills');
      labRun(
        join(sampleDir, 'e2e', 'run.mjs'),
        ['--steps', 'tour', '--theme', theme, ...noBuild('packages-sample')],
        sampleOut,
        runId,
      );
      checkUid(sampleOut, runId, uids['packages-sample']);
      stills['packages-sample'] = dir;
    }
    if (tours.includes('quickstart')) {
      const runId = `screenshots-quickstart-${theme}`;
      labRun(
        join(sampleDir, 'e2e', 'run.mjs'),
        ['--page', 'quickstart', '--theme', theme, ...noBuild('quickstart')],
        sampleOut,
        runId,
      );
      checkUid(sampleOut, runId, uids.quickstart);
      stills.quickstart = join(sampleOut, runId, 'stills');
    }
    for (const image of IMAGES.filter((i) => tours.includes(i.tour))) {
      place(
        join(stills[image.tour], `${image.still}.png`),
        join(out, image.area, `${image.name}-${theme}.png`),
      );
    }
    console.log(`screenshots: ${theme} -> ${out}`);
  }
  const missing = expectedFiles(themes, tours).filter((f) => !existsSync(join(out, f)));
  if (missing.length) fail(`missing images: ${missing.join(', ')}`);
  const total = folderBytes(out);
  if (total > MAX_TOTAL_BYTES) fail(`${out} holds ${total} bytes, over ${MAX_TOTAL_BYTES}`);
  console.log(`screenshots: ${expectedFiles(themes, tours).length} images, ${out} ${total} bytes`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
