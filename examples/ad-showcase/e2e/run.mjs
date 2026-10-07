#!/usr/bin/env node
// The showcase in the invisible lab (macOS). See e2e/README.md.
//
//   node e2e/run.mjs [--host tauri|electron] [--steps smoke|tour] [--run-id ID]
//                    [--no-build] [--identity FILE] [--timeout S]
//                    [--ad-wait MS] [--dwell MS]
//
// Test ads only (--test-ad): this runner never starts a LIVE run.
//
// --host tauri (default): stages the ow-tauri frontend with the lab driver
//   (scripts/stage.mjs --lab) and builds the debug app with the `lab`
//   feature into src-tauri/target/e2e.
// --host electron: stages the ow-electron app and runs it on the workspace's
//   ow-electron, from a throwaway folder whose main entry
//   (e2e/electron-main.cjs) keeps every window at opacity 0.
// --steps smoke (default): start, page 1, wait for display_ad_loaded, quit.
//   --steps tour: every page and its buttons (no ad is ever clicked).
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
  readFileSync,
  rmSync,
  symlinkSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

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
    timeout: { type: 'string', default: '300' },
    'ad-wait': { type: 'string', default: '60000' },
    dwell: { type: 'string', default: '8000' },
  },
});

function fail(message) {
  console.error(`e2e: ${message}`);
  process.exit(2);
}
if (process.platform !== 'darwin') fail('the invisible lab and the window monitor are macOS only');
if (!['tauri', 'electron'].includes(opts.host)) fail(`unknown --host ${opts.host}`);
if (!['smoke', 'tour'].includes(opts.steps)) fail(`unknown --steps ${opts.steps}`);

const runId =
  opts['run-id'] ?? `${opts.host}-${opts.steps}-${new Date().toISOString().replace(/[:.]/g, '-')}`;
const outRoot = join(here, 'out');
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
function buildTauri() {
  const exe = join(exampleDir, 'src-tauri', 'target', 'e2e', 'debug', 'ow-tauri-ad-showcase');
  if (opts['no-build'] && existsSync(exe)) return exe;
  stage('tauri', true);
  // generate_context! embeds the frontend when the crate compiles: touch
  // the lab module so the new stage is picked up.
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
        TAURI_CONFIG: JSON.stringify({ build: { frontendDist: join(stageDir, 'tauri-lab') } }),
        // A separate target directory: skip the incremental cache (gigabytes).
        CARGO_INCREMENTAL: '0',
      },
    },
  );
  return exe;
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
  const require = createRequire(join(exampleDir, 'package.json'));
  return {
    appDir,
    exe: require('@overwolf/ow-electron'),
    version: require('@overwolf/ow-electron/package.json').version,
  };
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
function ownedProcesses(probe, appPid, before, appOwner) {
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
        (isWebKit(p) && !before.has(p.pid) && owner !== undefined && owners.get(p.pid) === owner),
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
    adWaitMs: Number(opts['ad-wait']),
    dwellMs: Number(opts.dwell),
  };
  env.OW_SHOWCASE_E2E_CONFIG = JSON.stringify(config);
  const meta = {
    runId,
    host: opts.host,
    steps: opts.steps,
    mode: 'test',
    startedAt: new Date().toISOString(),
  };
  let exe;
  let args;
  if (tauri) {
    exe = buildTauri();
    Object.assign(env, {
      HOME: home,
      OW_TAURI_LAB_DIR: runDir,
      OW_TAURI_LAB_INVISIBLE: '1',
      OW_TAURI_LAB_PACKAGE_JSON: join(stageDir, 'package.json'),
    });
    args = ['--test-ad'];
    meta.exe = exe;
  } else {
    const prepared = prepareElectron();
    exe = prepared.exe;
    args = ['--test-ad', '--use-mock-keychain', prepared.appDir];
    meta.owElectron = prepared.version;
  }
  writeFileSync(join(runDir, 'meta.json'), JSON.stringify(meta, null, 2));

  const webKitBefore = new Set(
    processList()
      .filter(isWebKit)
      .map((p) => p.pid),
  );
  let appOwner;
  log(`launching ${opts.host} (test ads, ${opts.steps})`);
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
    appOwner ??= ownedProcesses(procOwner, child.pid, webKitBefore, appOwner).owner;
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
  killTree(child, 'SIGKILL');
  running.delete(child);
  await new Promise((ok) => setTimeout(ok, 500));
  killTree(mon, 'SIGTERM');
  running.delete(mon);

  const monitorEnd = readJsonl(monitorFile).find((r) => r.kind === 'end') ?? null;
  const records = readJsonl(e2eFile);
  const steps = records.filter((r) => r.kind === 'step');
  const page1 = steps.find((s) => s.name === 'page-1');
  const done = records.find((r) => r.kind === 'done');
  const summary = {
    runId,
    host: opts.host,
    steps: opts.steps,
    mode: 'test',
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
    owned = ownedProcesses(procOwner, child.pid, webKitBefore, appOwner).procs;
    if (!owned.length) break;
    await new Promise((ok) => setTimeout(ok, 500));
  }
  summary.leftProcesses = owned;
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
    summary.displayAdLoaded;
  process.exitCode = ok ? 0 : 1;
}

main().catch((error) => {
  console.error(error);
  killAll();
  process.exit(1);
});
