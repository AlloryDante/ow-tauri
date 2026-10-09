#!/usr/bin/env node
// The showcase in the invisible lab (macOS). See e2e/README.md.
//
//   node e2e/run.mjs [--host tauri|electron] [--steps smoke|tour|live-*]
//                    [--mode test|live] [--run-id ID] [--no-build]
//                    [--identity FILE] [--timeout S] [--ad-wait MS]
//                    [--dwell MS] [--stills DIR] [--theme dark|light]
//                    [--live-cap N]
//
// --mode test (default): test ads (--test-ad), smoke or tour.
// --mode live: real ads, only with a live-* scenario (live-layout,
//   live-300x250, live-reward, live-perf), which starts the app on one page
//   (--showcase-page) so that it mounts only the planned ad guests. Every
//   live ad load is logged in e2e/out/live-loads.jsonl with its run id and
//   running count; the run is refused when the planned loads would take
//   that count over --live-cap (default 10). The steps never click an ad
//   and never play a reward video in live mode.
//
// --host tauri (default): stages the ow-tauri page with the lab driver
//   (scripts/stage.mjs --lab) and builds the debug app with the `lab`
//   feature into src-tauri/target/e2e, under the lab bundle id. The
//   plugin's analytics and consent experiment go to a loopback sink the
//   runner starts (sink.jsonl), never to Overwolf.
// --host electron: stages the ow-electron app and runs it on the parity
//   harness's ow-electron (scripts/ow-electron.mjs), from a throwaway folder whose main entry
//   (e2e/electron-main.cjs) keeps every window at opacity 0.
// --steps smoke (default): start, page 1, wait for display_ad_loaded, quit.
//   --steps tour: every page and its buttons (no ad is ever clicked).
//   --steps restart (ow-tauri): start on the parity page (no ad guest),
//   restart in TEST through the app, and follow the new process (its own
//   window monitor and front check) until it reports the page it came back
//   on and quits.
// --theme dark (default) or light: the showcase theme the steps set before
//   the first still, so both themes can be recorded.
//
// Both: an isolated home, the window monitor (CGWindowListCopyWindowInfo;
// the app is killed the moment one of its windows becomes visible), a check
// that the app never becomes the frontmost app, and a kill of the whole
// process group on every exit path. Output: e2e/out/<run-id>/summary.json.

import { spawn, spawnSync } from 'node:child_process';
import {
  appendFileSync,
  copyFileSync,
  createWriteStream,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { createServer } from 'node:http';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { owElectron } from '../scripts/ow-electron.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const exampleDir = resolve(here, '..');
const repoRoot = resolve(exampleDir, '..', '..');
const stageDir = join(exampleDir, '.stage');

const { values: opts } = parseArgs({
  options: {
    host: { type: 'string', default: 'tauri' },
    steps: { type: 'string', default: 'smoke' },
    'run-id': { type: 'string' },
    'no-build': { type: 'boolean', default: false },
    identity: { type: 'string' },
    timeout: { type: 'string', default: '1500' },
    'ad-wait': { type: 'string', default: '60000' },
    dwell: { type: 'string', default: '8000' },
    mode: { type: 'string', default: 'test' },
    stills: { type: 'string' },
    theme: { type: 'string', default: 'dark' },
    'live-cap': { type: 'string', default: '10' },
    'live-observe': { type: 'string', default: '90000' },
  },
});

function fail(message) {
  console.error(`e2e: ${message}`);
  process.exit(2);
}
if (process.platform !== 'darwin') fail('the invisible lab and the window monitor are macOS only');
if (!['tauri', 'electron'].includes(opts.host)) fail(`unknown --host ${opts.host}`);
if (!['dark', 'light'].includes(opts.theme)) fail(`unknown --theme ${opts.theme}`);
const LIVE_STEPS = {
  'live-layout': { route: 'layouts/combo-classic', loads: 2 },
  'live-300x250': { route: 'sizes/300x250', loads: 1 },
  'live-reward': { route: 'reward', loads: 2 },
  'live-perf': { route: 'interstitial', loads: 1 },
};
if (!['test', 'live'].includes(opts.mode)) fail(`unknown --mode ${opts.mode}`);
if (opts.mode === 'test' && !['smoke', 'tour', 'restart'].includes(opts.steps))
  fail(`--mode test runs smoke, tour or restart, not ${opts.steps}`);
if (opts.steps === 'restart' && opts.host !== 'tauri') fail('--steps restart is an ow-tauri check');
if (opts.mode === 'live' && !(opts.steps in LIVE_STEPS))
  fail(`--mode live runs one of ${Object.keys(LIVE_STEPS).join(', ')}`);
const live = opts.mode === 'live';

const runId =
  opts['run-id'] ?? `${opts.host}-${opts.steps}-${new Date().toISOString().replace(/[:.]/g, '-')}`;
const outRoot = join(here, 'out');
const ledgerFile = join(outRoot, 'live-loads.jsonl');
const runDir = join(outRoot, runId);
rmSync(runDir, { recursive: true, force: true });
mkdirSync(runDir, { recursive: true });
const log = (message) => {
  const line = `[${new Date().toISOString()}] ${message}`;
  console.log(line);
  appendFileSync(join(runDir, 'runner.log'), line + '\n');
};

// ---------------------------------------------------------- process safety
const running = new Set();
function killTree(child, signal) {
  try {
    process.kill(-child.pid, signal);
  } catch {
    try {
      child.kill(signal);
    } catch {
      // gone
    }
  }
}
function killAll() {
  for (const child of running) killTree(child, 'SIGKILL');
}
process.on('exit', killAll);
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(signal, () => {
    killAll();
    process.exit(130);
  });
}

function sh(cmd, args, options = {}) {
  log(`$ ${cmd} ${args.join(' ')}`);
  const r = spawnSync(cmd, args, { stdio: 'inherit', ...options });
  if (r.status !== 0) fail(`${cmd} failed (${r.status})`);
}

function stage(host, lab) {
  const args = [join(exampleDir, 'scripts', 'stage.mjs'), '--host', host];
  if (lab) args.push('--lab');
  if (opts.identity) args.push('--identity', resolve(opts.identity));
  sh(process.execPath, args, { cwd: exampleDir });
}

// ------------------------------------------------------------------- build
/** The lab build's bundle id: its data never mixes with a normal build's. */
const LAB_IDENTIFIER = 'dev.ow-tauri.ad-showcase.lab';

/**
 * The Tauri config of the lab build: the staged identity, the lab page
 * (with the driver), the lab bundle id, and a CSP that lets the driver
 * evaluate its steps in the page (lab builds only).
 */
function labTauriConfig() {
  const identity = JSON.parse(readFileSync(join(stageDir, 'tauri.conf.json'), 'utf8'));
  const base = JSON.parse(readFileSync(join(exampleDir, 'src-tauri', 'tauri.conf.json'), 'utf8'));
  const csp = base.app.security.csp.replace("script-src 'self'", "script-src 'self' 'unsafe-eval'");
  return {
    ...identity,
    identifier: LAB_IDENTIFIER,
    build: { frontendDist: join(stageDir, 'tauri-lab') },
    app: { security: { csp } },
  };
}

function buildTauri() {
  const exe = join(exampleDir, 'src-tauri', 'target', 'e2e', 'debug', 'ow-tauri-ad-showcase');
  if (opts['no-build'] && existsSync(exe)) return exe;
  stage('tauri', true);
  // generate_context! embeds the page when the crate compiles: touch the
  // lab module so the new stage is picked up.
  const now = new Date();
  utimesSync(join(exampleDir, 'src-tauri', 'src', 'lab.rs'), now, now);
  sh(
    'taskpolicy',
    [
      '-b',
      'cargo',
      'build',
      '-j',
      '4',
      '--features',
      'lab',
      '--manifest-path',
      join(exampleDir, 'src-tauri', 'Cargo.toml'),
      '--target-dir',
      join(exampleDir, 'src-tauri', 'target', 'e2e'),
    ],
    {
      env: {
        ...process.env,
        TAURI_CONFIG: JSON.stringify(labTauriConfig()),
        // A separate target directory: skip the incremental cache (gigabytes).
        CARGO_INCREMENTAL: '0',
      },
    },
  );
  return exe;
}

/**
 * A loopback sink for the plugin's analytics and consent experiment: it
 * answers every request with 200 and logs method, path and body size to
 * `sink.jsonl`. A lab build never reports to Overwolf.
 */
async function startSink(dir) {
  const server = createServer((req, res) => {
    let size = 0;
    req.on('data', (chunk) => {
      size += chunk.length;
    });
    req.on('end', () => {
      appendFileSync(
        join(dir, 'sink.jsonl'),
        JSON.stringify({ at: Date.now(), method: req.method, path: req.url, bytes: size }) + '\n',
      );
      const json = req.url && req.url.startsWith('/experiments/');
      res.writeHead(200, { 'content-type': json ? 'application/json' : 'text/plain' });
      res.end(json ? '{}' : 'ok');
    });
  });
  await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
  return { server, url: `http://127.0.0.1:${server.address().port}` };
}

function prepareElectron() {
  if (!opts['no-build'] || !existsSync(join(stageDir, 'electron', 'main', 'main.js'))) {
    stage('electron', false);
  }
  const staged = join(stageDir, 'electron');
  const appDir = join(runDir, 'app');
  mkdirSync(appDir, { recursive: true });
  for (const dir of ['main', 'preload', 'renderer'])
    symlinkSync(join(staged, dir), join(appDir, dir));
  copyFileSync(join(here, 'electron-main.cjs'), join(appDir, 'e2e-main.cjs'));
  copyFileSync(join(here, 'steps.cjs'), join(appDir, 'steps.cjs'));
  const pkg = JSON.parse(readFileSync(join(staged, 'package.json'), 'utf8'));
  writeFileSync(
    join(appDir, 'package.json'),
    JSON.stringify({ ...pkg, main: 'e2e-main.cjs' }, null, 2),
  );
  const found = owElectron();
  if (!found)
    fail('ow-electron is not installed: cd tools/parity-harness && npm install --workspaces=false');
  return { appDir, exe: found.exe, version: found.version };
}

// ------------------------------------------------------- monitor and probe
function tool(name, source) {
  const binary = join(outRoot, '.tools', name);
  if (existsSync(binary)) return binary;
  mkdirSync(dirname(binary), { recursive: true });
  sh('swiftc', ['-O', source, '-o', binary]);
  return binary;
}
function processList() {
  return spawnSync('ps', ['-axo', 'pid=,ppid=,comm='], { encoding: 'utf8' })
    .stdout.split('\n')
    .map((l) => /^\s*(\d+)\s+(\d+)\s+(.*)$/.exec(l))
    .filter(Boolean)
    .map((m) => ({ pid: Number(m[1]), ppid: Number(m[2]), name: m[3].split('/').pop() }));
}
const isWebKit = (p) => p.name.startsWith('com.apple.WebKit.');
// The processes the app owns: its children, and WebKit's XPC services it is
// responsible for (they are not in its process group). An app started from
// a terminal is not responsible for itself, so WebKit processes of the same
// responsible process that appeared after the launch count too.
function ownedProcesses(probe, appPid, before, appOwner, webKit) {
  const ps = processList();
  const owners = new Map(
    spawnSync(
      probe,
      ps.map((p) => String(p.pid)),
      { encoding: 'utf8' },
    )
      .stdout.split('\n')
      .filter(Boolean)
      .map((l) => l.split(' ').map(Number)),
  );
  const owner = appOwner ?? owners.get(appPid);
  return {
    owner,
    procs: ps.filter(
      (p) =>
        p.pid === appPid ||
        p.ppid === appPid ||
        owners.get(p.pid) === appPid ||
        (webKit &&
          isWebKit(p) &&
          !before.has(p.pid) &&
          owner !== undefined &&
          owners.get(p.pid) === owner),
    ),
  };
}
function frontPid() {
  const asn = spawnSync('lsappinfo', ['front'], { encoding: 'utf8' }).stdout.trim();
  if (!asn) return null;
  const info = spawnSync('lsappinfo', ['info', '-only', 'pid', asn], { encoding: 'utf8' }).stdout;
  const m = /"pid"\s*=\s*(\d+)/.exec(info);
  return m ? Number(m[1]) : null;
}

const readJsonl = (file) =>
  existsSync(file)
    ? readFileSync(file, 'utf8')
        .split('\n')
        .filter(Boolean)
        .flatMap((l) => {
          try {
            return [JSON.parse(l)];
          } catch {
            return [];
          }
        })
    : [];

// --------------------------------------------------------------------- run
async function main() {
  const tauri = opts.host === 'tauri';
  const home = join(runDir, 'home');
  mkdirSync(home, { recursive: true });
  const monitor = tool(
    'window-monitor',
    join(repoRoot, 'tools', 'parity-harness', 'lib', 'window-monitor.swift'),
  );
  const procOwner = tool('proc-owner', join(here, 'proc-owner.swift'));
  const env = { ...process.env, CFFIXED_USER_HOME: home };
  delete env.ELECTRON_RUN_AS_NODE;
  const config = {
    runDir,
    steps: opts.steps,
    mode: opts.mode,
    adWaitMs: Number(opts['ad-wait']),
    dwellMs: Number(opts.dwell),
    liveObserveMs: Number(opts['live-observe']),
    theme: opts.theme,
    ...(opts.stills ? { stillsDir: resolve(opts.stills) } : {}),
  };
  env.OW_SHOWCASE_E2E_CONFIG = JSON.stringify(config);
  const meta = {
    runId,
    host: opts.host,
    steps: opts.steps,
    mode: opts.mode,
    startedAt: new Date().toISOString(),
  };
  // Live: the budget, reserved before the launch.
  const plan = live ? LIVE_STEPS[opts.steps] : null;
  const modeArgs = live ? [] : ['--test-ad'];
  const restartCheck = opts.steps === 'restart';
  const routeArgs = plan
    ? [`--showcase-page=${plan.route}`]
    : restartCheck
      ? ['--showcase-page=parity']
      : [];
  if (plan) {
    const used = liveTotal();
    const cap = Number(opts['live-cap']);
    if (used + plan.loads > cap)
      fail(`live budget: ${used} loads used, ${plan.loads} planned, cap ${cap}`);
    appendFileSync(
      ledgerFile,
      JSON.stringify({
        kind: 'reserve',
        runId,
        host: opts.host,
        steps: opts.steps,
        planned: plan.loads,
        usedBefore: used,
        at: new Date().toISOString(),
      }) + '\n',
    );
    meta.live = { planned: plan.loads, usedBefore: used, cap };
    log(`LIVE run ${runId}: ${plan.loads} planned loads, ${used} used before, cap ${cap}`);
  }
  let exe;
  let args;
  let sink = null;
  if (tauri) {
    exe = buildTauri();
    sink = await startSink(runDir);
    Object.assign(env, {
      HOME: home,
      OW_TAURI_LAB_DIR: runDir,
      OW_TAURI_LAB_INVISIBLE: '1',
      OW_SHOWCASE_LAB_SINK: sink.url,
    });
    args = [...modeArgs, ...routeArgs];
    meta.exe = exe;
  } else {
    const prepared = prepareElectron();
    exe = prepared.exe;
    args = [
      ...modeArgs,
      ...routeArgs,
      '--use-mock-keychain',
      // The live fill check reads the impression pings from the net log.
      `--log-net-log=${join(runDir, 'netlog.json')}`,
      '--net-log-capture-mode=Default',
      prepared.appDir,
    ];
    meta.owElectron = prepared.version;
  }
  writeFileSync(join(runDir, 'meta.json'), JSON.stringify(meta, null, 2));

  const webKitBefore = new Set(
    processList()
      .filter(isWebKit)
      .map((p) => p.pid),
  );
  let appOwner;
  log(`launching ${opts.host} (${live ? 'LIVE' : 'test'} ads, ${opts.steps})`);
  const child = spawn(exe, args, {
    env,
    cwd: runDir,
    stdio: ['ignore', 'pipe', 'pipe'],
    detached: true,
  });
  running.add(child);
  child.stdout.pipe(createWriteStream(join(runDir, 'stdout.log')));
  child.stderr.pipe(createWriteStream(join(runDir, 'stderr.log')));
  const monitorFile = join(runDir, 'window-monitor.jsonl');
  const mon = spawn(monitor, [String(child.pid), monitorFile, '25'], {
    stdio: 'ignore',
    detached: true,
  });
  running.add(mon);
  const ownerProbe = setTimeout(() => {
    appOwner ??= ownedProcesses(procOwner, child.pid, webKitBefore, appOwner, tauri).owner;
  }, 2000);
  // The invisible app must never be the frontmost app.
  let everFront = false;
  const frontWatch = setInterval(() => {
    if (!everFront && frontPid() === child.pid) {
      everFront = true;
      log('WARNING: the app became the frontmost app');
    }
  }, 500);

  const e2eFile = join(runDir, 'e2e.jsonl');
  const timeoutMs = Number(opts.timeout) * 1000;
  const exited = new Promise((ok) => child.on('exit', (code, signal) => ok({ code, signal })));
  let safetyKill = null;
  let doneAt = null;
  let verdict;
  let timedOut = false;
  const started = Date.now();
  let monitorOffset = 0;
  for (;;) {
    const race = await Promise.race([exited, new Promise((ok) => setTimeout(() => ok(null), 100))]);
    if (existsSync(monitorFile)) {
      const fresh = readFileSync(monitorFile, 'utf8').slice(monitorOffset);
      const end = fresh.lastIndexOf('\n');
      if (end >= 0) {
        monitorOffset += end + 1;
        for (const line of fresh.slice(0, end).split('\n')) {
          let entry;
          try {
            entry = JSON.parse(line);
          } catch {
            continue;
          }
          if (entry.anyVisible === true || entry.everVisible === true) {
            safetyKill = { at: new Date().toISOString(), entry };
            killTree(child, 'SIGKILL');
            log('SAFETY: a window became visible; app killed');
          }
        }
      }
    }
    if (race) {
      // The app may quit within one poll of the driver's last record.
      if (!doneAt && readJsonl(e2eFile).some((r) => r.kind === 'done')) doneAt = Date.now();
      verdict = safetyKill
        ? 'safety-kill'
        : doneAt
          ? 'done'
          : timedOut
            ? 'timeout'
            : 'exited-early';
      break;
    }
    if (!doneAt && readJsonl(e2eFile).some((r) => r.kind === 'done' || r.kind === 'fatal')) {
      doneAt = Date.now();
      log('driver finished; waiting for the app to quit');
    }
    if (doneAt && !timedOut && Date.now() - doneAt > 20000) {
      timedOut = true;
      log('app did not quit after the driver finished; killing it');
      killTree(child, 'SIGTERM');
      setTimeout(() => killTree(child, 'SIGKILL'), 5000).unref();
    }
    if (!timedOut && Date.now() - started > timeoutMs) {
      timedOut = true;
      log('timeout; killing the app');
      killTree(child, 'SIGTERM');
      setTimeout(() => killTree(child, 'SIGKILL'), 5000).unref();
    }
  }
  const exit = await exited;
  clearTimeout(ownerProbe);
  clearInterval(frontWatch);
  // The restarted process is in the first one's process group: keep it.
  if (!restartCheck) killTree(child, 'SIGKILL');
  running.delete(child);
  await new Promise((ok) => setTimeout(ok, 500));
  killTree(mon, 'SIGTERM');
  running.delete(mon);
  let restart = null;
  if (restartCheck) {
    restart = await followRestart(child.pid, monitor, e2eFile);
    if (restart.verdict === 'done' && verdict === 'exited-early') verdict = 'done';
    else if (restart.verdict !== 'done') verdict = `restart-${restart.verdict}`;
    killTree(child, 'SIGKILL');
  }
  if (sink) {
    sink.server.closeAllConnections();
    await new Promise((ok) => sink.server.close(ok));
  }

  const monitorEnd = readJsonl(monitorFile).find((r) => r.kind === 'end') ?? null;
  const records = readJsonl(e2eFile);
  const steps = records.filter((r) => r.kind === 'step');
  const page1 = steps.find((s) => s.name === 'page-1');
  const done = records.find((r) => r.kind === 'done');
  const summary = {
    runId,
    host: opts.host,
    steps: opts.steps,
    mode: opts.mode,
    verdict,
    exit,
    safetyKill,
    everVisible: monitorEnd ? monitorEnd.everVisible : null,
    everFront,
    started: steps.some((s) => s.name === 'started'),
    page: page1?.snapshot?.page ?? null,
    owadviews: page1?.snapshot?.owadviews ?? null,
    displayAdLoaded: done?.displayAdLoaded ?? false,
    counts: page1?.snapshot?.counts ?? null,
    fatal: records.filter((r) => r.kind === 'fatal').map((r) => r.text),
    blocked: readJsonl(join(runDir, 'blocked.jsonl')).length,
    ms: Date.now() - started,
  };
  const left = spawnSync('pgrep', ['-g', String(child.pid)], { encoding: 'utf8' }).stdout.trim();
  if (left) log(`WARNING: processes left in the app's group: ${left}`);
  let owned = [];
  for (let i = 0; i < 20; i += 1) {
    owned = ownedProcesses(procOwner, child.pid, webKitBefore, appOwner, tauri).procs;
    if (!owned.length) break;
    await new Promise((ok) => setTimeout(ok, 500));
  }
  summary.leftProcesses = owned;
  if (restart) summary.restart = restart;
  summary.stepsRun = steps.length;
  summary.export = records.find((r) => r.kind === 'export') ?? null;
  summary.stills = records.filter((r) => r.kind === 'still').length;
  summary.ads = adSummary(records, runDir, tauri);
  if (plan) {
    // One ledger line per live ad load (each guest the app mounted).
    let n = liveTotal();
    for (const cid of summary.ads.mounted) {
      n += 1;
      appendFileSync(
        ledgerFile,
        JSON.stringify({ kind: 'load', runId, host: opts.host, cid, count: n }) + '\n',
      );
      log(`live load ${n}: ${runId} ${opts.host} ${cid}`);
    }
    summary.live = { planned: plan.loads, loads: summary.ads.mounted.length, total: n };
  }
  writeFileSync(join(runDir, 'summary.json'), JSON.stringify(summary, null, 2));
  log(JSON.stringify(summary));
  if (owned.length)
    log(`WARNING: processes the app owned are still running: ${JSON.stringify(owned)}`);
  const ok =
    verdict === 'done' &&
    summary.everVisible === false &&
    !everFront &&
    !owned.length &&
    summary.started &&
    !summary.fatal.length &&
    (restart
      ? restart.everVisible === false && !restart.everFront
      : live || summary.displayAdLoaded);
  process.exitCode = ok ? 0 : 1;
}

/**
 * The restart check's second half: the process the app started for its
 * restart (the driver record with `restartPhase: 'second'`), under its own
 * window monitor and front check, until it reports `done` and exits.
 */
async function followRestart(firstPid, monitor, e2eFile) {
  const deadline = Date.now() + 120000;
  let second = null;
  while (!second && Date.now() < deadline) {
    second = readJsonl(e2eFile).find((r) => r.kind === 'driver' && r.restartPhase === 'second');
    if (!second) await new Promise((ok) => setTimeout(ok, 250));
  }
  if (!second || typeof second.pid !== 'number' || second.pid === firstPid) {
    log('restart: no second process reported');
    return { verdict: 'no-second-process', firstPid };
  }
  const pid = second.pid;
  log(`restart: second process ${pid}`);
  const monitorFile = join(runDir, 'window-monitor-restart.jsonl');
  const mon = spawn(monitor, [String(pid), monitorFile, '25'], { stdio: 'ignore', detached: true });
  running.add(mon);
  const alive = () => {
    try {
      process.kill(pid, 0);
      return true;
    } catch {
      return false;
    }
  };
  let everFront = false;
  let safetyKill = null;
  let doneAt = null;
  let result = 'timeout';
  while (Date.now() < deadline) {
    if (frontPid() === pid) everFront = true;
    for (const entry of readJsonl(monitorFile)) {
      if (!safetyKill && (entry.anyVisible === true || entry.everVisible === true)) {
        safetyKill = { at: new Date().toISOString(), entry };
        log('SAFETY: a window of the restarted app became visible; killed');
        try {
          process.kill(pid, 'SIGKILL');
        } catch {
          // gone
        }
      }
    }
    if (!doneAt && readJsonl(e2eFile).some((r) => r.kind === 'done' && r.restarted)) {
      doneAt = Date.now();
    }
    if (!alive()) {
      result = safetyKill ? 'safety-kill' : doneAt ? 'done' : 'exited-early';
      break;
    }
    if (doneAt && Date.now() - doneAt > 20000) break;
    await new Promise((ok) => setTimeout(ok, 250));
  }
  if (alive()) {
    try {
      process.kill(pid, 'SIGKILL');
    } catch {
      // gone
    }
  }
  await new Promise((ok) => setTimeout(ok, 500));
  killTree(mon, 'SIGTERM');
  running.delete(mon);
  const end = readJsonl(monitorFile).find((r) => r.kind === 'end') ?? null;
  const back = readJsonl(e2eFile).find((r) => r.kind === 'step' && r.name === 'restart-second');
  return {
    verdict: result,
    firstPid,
    secondPid: pid,
    route: back?.route ?? null,
    everVisible: end ? end.everVisible : null,
    everFront,
    safetyKill,
  };
}

/** Live ad loads logged so far (the ledger's `load` lines, else reservations). */
function liveTotal() {
  const lines = readJsonl(ledgerFile);
  const loads = lines.filter((l) => l.kind === 'load').length;
  // A reservation without its loads (a run that died) counts as planned.
  const done = new Set(lines.filter((l) => l.kind === 'load').map((l) => l.runId));
  const open = lines
    .filter((l) => l.kind === 'reserve' && !done.has(l.runId) && l.runId !== runId)
    .reduce((n, l) => n + l.planned, 0);
  return loads + open;
}

/**
 * Per slot: the guests mounted (did-attach), the fill events, and the
 * impression pings the ad pages sent (ow-electron: the net log; ow-tauri:
 * the lab's guest probes).
 */
function adSummary(records, dir, tauriHost) {
  const events = records.flatMap((r) => (r.kind === 'step' ? r.events : []));
  const perCid = {};
  for (const e of events) {
    if (e.cid === 'app' || e.name.startsWith('control:')) continue;
    perCid[e.cid] ??= {};
    perCid[e.cid][e.name] = (perCid[e.cid][e.name] ?? 0) + 1;
  }
  const mounted = Object.entries(perCid)
    .filter(([, c]) => c['did-attach'])
    .map(([cid]) => cid);
  let urls = [];
  if (tauriHost) {
    for (const f of readdirSync(dir).filter((n) => /^guest-\d+-.*\.json$/.test(n))) {
      try {
        const probe = JSON.parse(readFileSync(join(dir, f), 'utf8'));
        urls.push(...(probe.labResources ?? []));
      } catch {
        // A partial probe file.
      }
    }
  } else if (existsSync(join(dir, 'netlog.json'))) {
    const text = readFileSync(join(dir, 'netlog.json'), 'utf8');
    urls = [...text.matchAll(/"url":"([^"]+)"/g)].map((m) => m[1]);
  }
  const distinct = (re) => new Set(urls.filter((u) => re.test(u))).size;
  return {
    mounted,
    perCid,
    impressionPings: distinct(/owads_scl_impression/),
    gptAdRequests: distinct(/\/gampad\/ads/),
  };
}

main().catch((error) => {
  console.error(error);
  killAll();
  process.exit(1);
});
