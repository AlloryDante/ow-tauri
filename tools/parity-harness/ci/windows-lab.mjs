#!/usr/bin/env node
// Windows CI lab (.github/workflows/windows-lab.yml): runs each scenario on
// ow-electron and then on ow-tauri on the same runner, both with TEST ads,
// compares the two captures with parity-diff.mjs and evaluates the Windows
// lab checks (lib/windows-checks.mjs). Never loads a live ad.
//
//   node ci/windows-lab.mjs --shard 1/4 [--scenarios a,b] [--out <dir>]
//
// Each run starts from a clean state: the ow-electron state folder, the
// app's userData (ads data store included) and the Tauri app's WebView2
// folder are removed before every launch, which stands in for the fresh
// per-run home of the macOS lab (Windows reads those folders from the
// shell, not from the environment); WebView2 processes a run left behind
// are ended first. The app identity is the harness's
// neutral default (formula uid); a local identity file is refused.
//
// Writes <out>/summary.json and <out>/summary.md, and appends the table to
// $GITHUB_STEP_SUMMARY. Exits 1 when a scenario has a BUG difference, a
// Windows check fails (advisory checks only report), or a run failed.

import { spawnSync } from 'node:child_process';
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { fileURLToPath } from 'node:url';

import { DEFAULT_IDENTITY, displayName } from '../lib/identity.mjs';
import { SCENARIOS } from '../lib/scenarios.mjs';
import { windowsChecks } from '../lib/windows-checks.mjs';

const harnessDir = resolve(dirname(fileURLToPath(import.meta.url)), '..');

/** The R2 base runs and every R3 ad-format scenario the macOS lab compares. */
export const LAB_SCENARIOS = [
  'A',
  'cmp',
  'messages',
  'lab-layers',
  'audio',
  'sizes',
  'high-impact',
  'high-impact-small-zone',
  'high-impact-only',
  'perf',
  'perf-sample',
  'perf-small',
  'perf-twice',
  'perf-remove',
  'perf-with-standard',
  'perf-unit',
  'perf-minimize',
  'reward',
  'reward-visibility-probe',
  'reward-play-probe',
  'reward-two-slots',
  'reward-optin',
  'adstyle-probe',
  'tower-plus',
  'standard-remove',
  'owadtestad',
  'introspect',
];

/** Run options of the R2 base run `A` (the README's T-A run). */
const BASE_A = ['--present', 'transparent', '--layout', '400x600', '--duration', '90'];

/** Seconds a scenario runs (its preset, else run.mjs's default). */
export function durationOf(name) {
  if (name === 'A') return 90;
  return Number(SCENARIOS[name]?.defaults?.duration ?? 90);
}

/**
 * Splits `names` into `count` shards of about the same run time (longest
 * first, each to the lightest shard) and returns shard `index` (1-based),
 * in the original order.
 */
export function shard(names, index, count) {
  const loads = Array.from({ length: count }, () => ({ total: 0, names: new Set() }));
  for (const name of [...names].sort((a, b) => durationOf(b) - durationOf(a))) {
    const lightest = loads.reduce((min, l) => (l.total < min.total ? l : min));
    lightest.total += durationOf(name);
    lightest.names.add(name);
  }
  return names.filter((n) => loads[index - 1].names.has(n));
}

/** The folders a run leaves state in on Windows (see the header). */
export function stateDirs(env = process.env) {
  const roaming = env.APPDATA;
  const local = env.LOCALAPPDATA;
  if (!roaming || !local) return [];
  return [
    join(roaming, 'ow-electron'),
    join(roaming, displayName(DEFAULT_IDENTITY)),
    join(local, displayName(DEFAULT_IDENTITY)),
    join(roaming, 'dev.ow-tauri.parity-harness'),
    join(local, 'dev.ow-tauri.parity-harness'),
  ];
}

/** Error codes of a folder a live WebView2 process still uses. */
const WEBVIEW_HELD = new Set(['EBUSY', 'EPERM', 'ENOTEMPTY']);

/** Ends the WebView2 processes a finished run left behind (CI runner only). */
function killWebViews() {
  spawnSync('taskkill', ['/F', '/T', '/IM', 'msedgewebview2.exe'], { windowsHide: true });
}

/**
 * Removes a state folder. WebView2's browser processes outlive the app
 * for a moment and keep files such as `EBWebView\Default\DIPS` open, or
 * still write new ones while the folder is emptied (`ENOTEMPTY` on its
 * `rmdir`), so removal is retried (Node retries `EBUSY`, `EPERM` and
 * `ENOTEMPTY`) and, if that keeps failing, the leftover WebView2 processes
 * are ended and it is tried once more.
 * @param {string} dir
 * @param {{rm?: typeof rmSync, kill?: () => void}} [deps]
 * @returns {'removed' | 'removed after ending WebView2'}
 */
export function resetDir(dir, { rm = rmSync, kill = killWebViews } = {}) {
  const options = { recursive: true, force: true, maxRetries: 10, retryDelay: 500 };
  try {
    rm(dir, options);
    return 'removed';
  } catch (error) {
    const code = /** @type {{code?: unknown}} */ (error)?.code;
    if (!WEBVIEW_HELD.has(String(code))) throw error;
  }
  kill();
  rm(dir, options);
  return 'removed after ending WebView2';
}

/**
 * Clean state for the next launch: ends any WebView2 process a finished
 * run left behind, then removes every state folder. A leftover process
 * keeps writing into `EBWebView` while it is removed (`ENOTEMPTY`) and
 * holds up the next app's WebView2 environment for the same folder [OBS:
 * Windows lab, 4510781: the `sizes` run's main webview started loading
 * 45 s after its navigation, against 1 to 5 s in every other run, and its
 * consent window never loaded].
 * @param {{dirs?: string[], kill?: () => void, reset?: (dir: string) => unknown}} [deps]
 */
export function resetState({ dirs = stateDirs(), kill = killWebViews, reset = resetDir } = {}) {
  kill();
  for (const dir of dirs) reset(dir);
}

function runArgs(name, host, runId) {
  const args = ['run.mjs', '--host', host, '--mode', 'test', '--home', 'real'];
  args.push('--ci-visible', '--no-wait', '--run-id', runId);
  if (host === 'tauri') args.push('--no-build');
  if (name === 'A') args.push(...BASE_A);
  else args.push('--scenario', name);
  return args;
}

function node(args, log) {
  const started = Date.now();
  const r = spawnSync(process.execPath, args, {
    cwd: harnessDir,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true,
  });
  writeFileSync(log, `${r.stdout ?? ''}\n--- stderr ---\n${r.stderr ?? ''}`);
  return { status: r.status, ms: Date.now() - started };
}

function readJson(file) {
  try {
    return JSON.parse(readFileSync(file, 'utf8'));
  } catch {
    return null;
  }
}

/** Counts of parity-diff classes in a parity-diff.json. */
export function classCounts(diff) {
  const counts = {};
  for (const d of diff?.diffs ?? []) counts[d.class] = (counts[d.class] ?? 0) + 1;
  return counts;
}

function main() {
  const { values } = parseArgs({
    options: {
      shard: { type: 'string', default: '1/1' },
      scenarios: { type: 'string', default: 'all' },
      out: { type: 'string', default: join(harnessDir, 'captures', 'windows-lab') },
    },
  });
  if (process.env.GITHUB_ACTIONS !== 'true') {
    console.error('windows-lab.mjs runs on GitHub Actions runners only');
    process.exit(2);
  }
  if (existsSync(join(harnessDir, 'local.identity.json'))) {
    console.error('a local identity file is present; the CI lab runs the neutral identity only');
    process.exit(2);
  }
  const [index, count] = values.shard.split('/').map(Number);
  const wanted =
    values.scenarios === 'all'
      ? LAB_SCENARIOS
      : values.scenarios
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean);
  for (const name of wanted) {
    if (name !== 'A' && !SCENARIOS[name]) {
      console.error(`unknown scenario ${name}`);
      process.exit(2);
    }
  }
  const names = shard(wanted, index, count);
  mkdirSync(values.out, { recursive: true });
  const rows = [];
  for (const name of names) {
    const electronId = `WE-${name}`;
    const tauriId = `WT-${name}`;
    console.error(`== ${name}: ow-electron`);
    resetState();
    const e = node(runArgs(name, 'electron', electronId), join(values.out, `${electronId}.log`));
    console.error(`== ${name}: ow-tauri`);
    resetState();
    const t = node(runArgs(name, 'tauri', tauriId), join(values.out, `${tauriId}.log`));
    const eDir = join(harnessDir, 'captures', electronId);
    const tDir = join(harnessDir, 'captures', tauriId);
    const d = node(['parity-diff.mjs', eDir, tDir], join(values.out, `diff-${name}.log`));
    const diff = readJson(join(tDir, 'parity-diff.json'));
    const counts = classCounts(diff);
    const checks = windowsChecks(name, eDir, tDir);
    const eMeta = readJson(join(eDir, 'meta.json'));
    const tMeta = readJson(join(tDir, 'meta.json'));
    rows.push({
      scenario: name,
      electron: { status: e.status, exit: eMeta?.exit ?? null },
      tauri: { status: t.status, exit: tMeta?.exit ?? null },
      diffStatus: d.status,
      counts,
      diffFound: diff !== null,
      bugs: (diff?.diffs ?? []).filter((x) => x.class === 'BUG'),
      checks,
    });
    console.error(
      `   BUG ${counts.BUG ?? 0}; checks ${checks.map((c) => `${c.id}:${c.pass}`).join(' ')}`,
    );
  }
  const failed = rows.filter(
    (r) =>
      r.electron.status !== 0 ||
      r.tauri.status !== 0 ||
      r.electron.exit?.timedOut ||
      r.tauri.exit?.timedOut ||
      !r.diffFound ||
      (r.counts.BUG ?? 0) > 0 ||
      r.checks.some((c) => c.pass === false && !c.advisory),
  );
  writeFileSync(
    join(values.out, 'summary.json'),
    JSON.stringify({ shard: values.shard, rows, failed: failed.map((r) => r.scenario) }, null, 2) +
      '\n',
  );
  const md = [
    `### Windows lab, shard ${values.shard} (test ads only)`,
    '',
    '| scenario | ow-electron | ow-tauri | BUG | variance | intended | not-mirrored | Windows checks |',
    '|---|---|---|---|---|---|---|---|',
    ...rows.map((r) => {
      const intended = Object.entries(r.counts)
        .filter(([k]) => k.startsWith('intended'))
        .reduce((n, [, v]) => n + v, 0);
      const checks = r.checks.map((c) => `${c.id} ${c.probe}: ${c.pass}`).join('<br>') || '-';
      const how = (h) =>
        h.exit?.timedOut ? 'timed out' : h.exit ? `exit ${h.exit.code}` : `run ${h.status}`;
      return `| ${r.scenario} | ${how(r.electron)} | ${how(r.tauri)} | ${r.counts.BUG ?? 0} | ${r.counts.variance ?? 0} | ${intended} | ${r.counts['not-mirrored'] ?? 0} | ${checks} |`;
    }),
    '',
  ].join('\n');
  writeFileSync(join(values.out, 'summary.md'), md);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, md);
  console.log(md);
  process.exit(failed.length ? 1 : 0);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
