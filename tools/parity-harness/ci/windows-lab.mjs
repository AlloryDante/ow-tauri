#!/usr/bin/env node
// CI parity lab (.github/workflows/windows-lab.yml): runs each scenario on
// ow-electron and then on ow-tauri on the same runner, both with TEST ads,
// compares the two captures with parity-diff.mjs and evaluates the Windows
// lab checks (lib/windows-checks.mjs). Never loads a live ad.
//
//   node ci/windows-lab.mjs --shard 1/8 [--scenarios a,b] [--out <dir>]
//   node ci/windows-lab.mjs --list [--suite windows|macos-drift] [--shard 3/8]   # the plan, no runs
//
// On Windows (the lab proper) each run starts from a clean state: the
// ow-electron state folder, the app's userData (ads data store included)
// and the Tauri app's WebView2 folder are removed before every launch,
// which stands in for the fresh per-run home of the macOS lab (Windows
// reads those folders from the shell, not from the environment); WebView2
// processes a run left behind are ended first. On macOS (the weekly drift
// subset on a hosted runner) every run gets its own home instead and the
// Tauri app runs in the invisible lab. The app identity is the harness's
// neutral default (formula uid); a local identity file is refused.
//
// A lab entry is a scenario name, or `corrupt-state:<kind>` (one state-file
// corruption, DESIGN §5.2 #18 / W4 ruling L3). Entries that need an earlier
// launch (corrupt-state, no-analytics-persisted) run a seed launch per host
// first, which is not compared. Scenarios that run on ow-tauri only
// (`hosts: ['tauri']`, dialog-probe) are recorded without a comparison.
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

/** The state-file corruptions of the corrupt-state scenario (run.mjs). */
export const CORRUPTIONS = [
  'garbage',
  'truncated',
  'empty',
  'array',
  'null',
  'wrong-types',
  'missing',
];

/**
 * Every lab entry of the Windows lab: the R2 base runs, every R3 ad-format
 * scenario, the W3-B record-first scenarios (W4 rulings L1-L4) and the
 * DESIGN §5.2 checks that apply to Windows. Not here: `exit-terminate`,
 * `recreate-reload*` and `crash-fallback` (macOS lab, SECTION_5_2 `lab`),
 * `long` (13 h soak), the live-ad scenarios, and `build-identity`
 * (installer steps, its own job).
 */
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
  'windows',
  'crash',
  'inview-probe',
  'send-command-probe',
  // W3-B record-first (W4 rulings L1-L4).
  'email-hashes-clear',
  'last-window-during-consent',
  'last-window-before-consent',
  'last-window-consent-saved',
  'last-window-after-consent',
  ...CORRUPTIONS.map((kind) => `corrupt-state:${kind}`),
  'gesture-timing',
  // DESIGN §5.2, the Windows half (lib/scenarios.mjs SECTION_5_2).
  'windows-urls',
  'title-default',
  'title-set-in-setup',
  'custom-ua',
  'ready-burst',
  'exit-last-window',
  'exit-app-exit',
  'exit-tray-alive',
  'close-to-tray',
  'close-to-tray-js',
  'close-js-delayed',
  'close-confirm-5s',
  'destroy-direct',
  'heartbeat-silence',
  'cmpreq-hang-long',
  'window-before-ready',
  'parked-show-unfocused',
  'email-hashes-golden',
  'dialog-probe',
  'no-analytics-config',
  'no-analytics-setup',
  'no-analytics-persisted',
  'local-frame',
];

/**
 * The weekly macOS drift subset (DESIGN §7.5): what Overwolf's live ad page
 * and the newest ow-electron change first (the guest config, messages,
 * request shapes, consent), short enough for a hosted runner.
 */
export const MACOS_DRIFT_SCENARIOS = ['A', 'cmp', 'messages', 'sizes', 'windows-urls', 'perf'];

/** Lab entries that need a seed launch on the same home before the compared one. */
const SEEDED = new Set(['corrupt-state', 'no-analytics-persisted']);

/**
 * The launches of one lab entry.
 * @param {string} entry a scenario name or `corrupt-state:<kind>`
 * @returns {{entry: string, scenario: string, hosts: string[], seeded: boolean, extra: string[]}}
 */
export function planOf(entry) {
  const [scenario, kind] = entry.split(':');
  if (scenario !== 'A' && !SCENARIOS[scenario]) throw new Error(`unknown scenario ${scenario}`);
  if (scenario === 'corrupt-state' && !CORRUPTIONS.includes(kind ?? ''))
    throw new Error(`corrupt-state needs a kind: corrupt-state:<${CORRUPTIONS.join('|')}>`);
  if (scenario !== 'corrupt-state' && kind !== undefined)
    throw new Error(`only corrupt-state takes a kind (${entry})`);
  const hosts = SCENARIOS[scenario]?.hosts ?? ['electron', 'tauri'];
  return {
    entry,
    scenario,
    hosts: ['electron', 'tauri'].filter((h) => hosts.includes(h)),
    seeded: SEEDED.has(scenario),
    extra: kind ? ['--corrupt-state', kind] : [],
  };
}

/**
 * The lab entries `--scenarios` names: `all`, or a comma list where
 * `corrupt-state` stands for every corruption kind.
 * @param {string} option
 * @param {string[]} [all]
 */
export function entriesOf(option, all = LAB_SCENARIOS) {
  if (option === 'all') return [...all];
  const names = option
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
    .flatMap((n) => (n === 'corrupt-state' ? CORRUPTIONS.map((k) => `${n}:${k}`) : [n]));
  for (const name of names) planOf(name);
  return names;
}

/** Run options of the R2 base run `A` (the README's T-A run). */
const BASE_A = ['--present', 'transparent', '--layout', '400x600', '--duration', '90'];

/** Seconds of start-up, shut-down and capture work around each launch. */
const LAUNCH_OVERHEAD_S = 40;

/** Seconds a lab entry takes: every launch of every host (for sharding). */
export function durationOf(entry) {
  const plan = planOf(entry);
  const seconds =
    plan.scenario === 'A' ? 90 : Number(SCENARIOS[plan.scenario]?.defaults?.duration ?? 90);
  return plan.hosts.length * (plan.seeded ? 2 : 1) * (seconds + LAUNCH_OVERHEAD_S);
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
  if (process.platform !== 'win32') return;
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

/**
 * The run.mjs arguments of one launch.
 * @param {ReturnType<typeof planOf>} plan
 * @param {string} host
 * @param {string} runId
 * @param {{seed?: boolean, platform?: string}} [o]
 */
export function runArgs(plan, host, runId, { seed = false, platform = process.platform } = {}) {
  // Windows: the real home (no isolation there; resetState cleans it).
  // macOS: a fresh home per run, or one profile per host and entry when a
  // seed launch must leave its state for the compared launch.
  const home =
    platform === 'win32'
      ? 'real'
      : plan.seeded
        ? `profile:lab-${host}-${plan.entry.replace(':', '-')}`
        : 'isolated';
  const args = ['run.mjs', '--host', host, '--mode', 'test', '--home', home];
  // The Windows runner's desktop is nobody's screen; macOS keeps the
  // invisible lab (window monitor, never frontmost).
  if (platform === 'win32') args.push('--ci-visible');
  args.push('--no-wait', '--run-id', runId);
  if (host === 'tauri') args.push('--no-build');
  if (plan.scenario === 'A') args.push(...BASE_A);
  else args.push('--scenario', plan.scenario);
  if (!seed) args.push(...plan.extra);
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

/** A file-name-safe form of a lab entry (`corrupt-state:null` -> `corrupt-state-null`). */
export function entryId(entry) {
  return entry.replace(/[^A-Za-z0-9._-]/g, '-');
}

/**
 * Whether a row failed the lab: a launch that failed or timed out, a
 * comparison that wrote nothing, a BUG difference, or a failed Windows
 * check that is not advisory.
 */
export function rowFailed(r) {
  const launches = [r.electron, r.tauri].filter(Boolean);
  return (
    launches.some((h) => h.status !== 0 || h.exit?.timedOut) ||
    (r.compared && !r.diffFound) ||
    (r.counts.BUG ?? 0) > 0 ||
    r.checks.some((c) => c.pass === false && !c.advisory)
  );
}

/** One launch of `host` (after its seed launch when the plan has one). */
function launchHost(plan, host, prefix, out) {
  const runId = `${prefix}-${entryId(plan.entry)}`;
  resetState();
  if (plan.seeded) {
    const seedId = `${runId}-seed`;
    console.error(`   ${host}: seed launch`);
    const seed = node(runArgs(plan, host, seedId, { seed: true }), join(out, `${seedId}.log`));
    if (seed.status !== 0) return { runId, status: seed.status, exit: null, seedFailed: true };
  }
  const run = node(runArgs(plan, host, runId), join(out, `${runId}.log`));
  const meta = readJson(join(harnessDir, 'captures', runId, 'meta.json'));
  return { runId, status: run.status, exit: meta?.exit ?? null };
}

function main() {
  const { values } = parseArgs({
    options: {
      shard: { type: 'string', default: '1/1' },
      scenarios: { type: 'string', default: 'all' },
      out: { type: 'string', default: join(harnessDir, 'captures', 'windows-lab') },
      list: { type: 'boolean', default: false },
      // Which entries `all` means: the Windows lab, or the macOS drift subset.
      suite: { type: 'string', default: process.platform === 'win32' ? 'windows' : 'macos-drift' },
    },
  });
  const suites = { windows: LAB_SCENARIOS, 'macos-drift': MACOS_DRIFT_SCENARIOS };
  const all = suites[/** @type {keyof typeof suites} */ (values.suite)];
  if (!all) {
    console.error(`--suite must be ${Object.keys(suites).join(' or ')}`);
    process.exit(2);
  }
  let wanted;
  try {
    wanted = entriesOf(values.scenarios, all);
  } catch (error) {
    console.error(String(error.message ?? error));
    process.exit(2);
  }
  const [index, count] = values.shard.split('/').map(Number);
  const names = shard(wanted, index, count);
  if (values.list) {
    for (const name of names) console.log(`${name}\t${durationOf(name)} s`);
    return;
  }
  if (process.env.GITHUB_ACTIONS !== 'true') {
    console.error('windows-lab.mjs runs on GitHub Actions runners only');
    process.exit(2);
  }
  if (existsSync(join(harnessDir, 'local.identity.json'))) {
    console.error('a local identity file is present; the CI lab runs the neutral identity only');
    process.exit(2);
  }
  mkdirSync(values.out, { recursive: true });
  const rows = [];
  for (const name of names) {
    const plan = planOf(name);
    const compared = plan.hosts.length === 2;
    console.error(`== ${name}${compared ? '' : ' (ow-tauri only, not compared)'}`);
    const row = { scenario: name, compared, counts: {}, checks: [], bugs: [], diffFound: false };
    for (const host of plan.hosts) {
      console.error(`   ${host}`);
      row[host] = launchHost(plan, host, host === 'electron' ? 'WE' : 'WT', values.out);
    }
    if (compared) {
      const eDir = join(harnessDir, 'captures', row.electron.runId);
      const tDir = join(harnessDir, 'captures', row.tauri.runId);
      row.diffStatus = node(
        ['parity-diff.mjs', eDir, tDir],
        join(values.out, `diff-${entryId(name)}.log`),
      ).status;
      const diff = readJson(join(tDir, 'parity-diff.json'));
      row.diffFound = diff !== null;
      row.counts = classCounts(diff);
      row.bugs = (diff?.diffs ?? []).filter((x) => x.class === 'BUG');
      row.checks = process.platform === 'win32' ? windowsChecks(plan.scenario, eDir, tDir) : [];
    }
    rows.push(row);
    console.error(
      `   BUG ${row.counts.BUG ?? 0}; checks ${row.checks.map((c) => `${c.id}:${c.pass}`).join(' ')}`,
    );
  }
  const failed = rows.filter(rowFailed);
  writeFileSync(
    join(values.out, 'summary.json'),
    JSON.stringify({ shard: values.shard, rows, failed: failed.map((r) => r.scenario) }, null, 2) +
      '\n',
  );
  const md = summaryMarkdown(values.shard, rows);
  writeFileSync(join(values.out, 'summary.md'), md);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, md);
  console.log(md);
  process.exit(failed.length ? 1 : 0);
}

/**
 * Replaces every UUID in `text` with a stable tag (`<uuid-1>`, `<uuid-2>`,
 * ...; one tag per distinct value, shared through `tags`). The summary is
 * printed to the public CI log, and a muid or another machine id must not
 * reach it; equal values keep equal tags, so a difference stays visible.
 * The captures artifact keeps the real values.
 * @param {string} text
 * @param {Map<string, string>} tags
 */
export function redactIds(text, tags) {
  return text.replace(
    /\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi,
    (id) => {
      const key = id.toLowerCase();
      if (!tags.has(key)) tags.set(key, `<uuid-${tags.size + 1}>`);
      return /** @type {string} */ (tags.get(key));
    },
  );
}

/**
 * The Markdown summary of one shard: a table row per lab entry, then every
 * BUG difference (section, key, field and both values, with UUIDs such as
 * the muid replaced by tags: see {@link redactIds}).
 * @param {string} shardName
 * @param {any[]} rows
 */
export function summaryMarkdown(shardName, rows) {
  const how = (h) =>
    !h
      ? '-'
      : h.seedFailed
        ? `seed run ${h.status}`
        : h.exit?.timedOut
          ? 'timed out'
          : h.exit
            ? `exit ${h.exit.code}`
            : `run ${h.status}`;
  const lines = [
    `### ${process.platform === 'win32' ? 'Windows' : 'macOS'} lab, shard ${shardName} (test ads only)`,
    '',
    '| scenario | ow-electron | ow-tauri | BUG | variance | intended | not-mirrored | Windows checks | verdict |',
    '|---|---|---|---|---|---|---|---|---|',
    ...rows.map((r) => {
      const intended = Object.entries(r.counts)
        .filter(([k]) => k.startsWith('intended'))
        .reduce((n, [, v]) => n + v, 0);
      const checks = r.checks.map((c) => `${c.id} ${c.probe}: ${c.pass}`).join('<br>') || '-';
      const bug = r.compared ? String(r.counts.BUG ?? 0) : 'n/a';
      return `| ${r.scenario} | ${how(r.electron)} | ${how(r.tauri)} | ${bug} | ${r.counts.variance ?? 0} | ${intended} | ${r.counts['not-mirrored'] ?? 0} | ${checks} | ${rowFailed(r) ? 'FAIL' : 'ok'} |`;
    }),
    '',
  ];
  const bugs = rows.flatMap((r) => r.bugs.map((b) => ({ scenario: r.scenario, ...b })));
  if (bugs.length) {
    const tags = new Map();
    const cell = (v) =>
      redactIds(String(typeof v === 'string' ? v : JSON.stringify(v ?? null)), tags)
        .replace(/\|/g, '\\|')
        .replace(/\n/g, ' ')
        .slice(0, 160);
    lines.push(
      '#### BUG differences',
      '',
      '| scenario | section | key | field | ow-electron | ow-tauri |',
      '|---|---|---|---|---|---|',
      ...bugs.map(
        (b) =>
          `| ${b.scenario} | ${cell(b.section)} | ${cell(b.key)} | ${cell(b.field)} | ${cell(b.electron)} | ${cell(b.tauri)} |`,
      ),
      '',
    );
  }
  return lines.join('\n');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
