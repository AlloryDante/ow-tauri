#!/usr/bin/env node
// The packages sample in the invisible lab (macOS). See e2e/README.md.
//
//   node e2e/run.mjs [--steps smoke|restart|tour] [--page sample|quickstart]
//                    [--theme dark|light] [--stills DIR] [--run-id ID]
//                    [--no-build] [--timeout S] [--stall S] [--ad-wait MS]
//
// Builds the lab page (`vite build --mode lab`, which adds the driver) and a
// debug app with the `lab` Cargo feature under the lab bundle id, then runs
// it with test ads (--test-ad). The plugin's analytics, consent experiment
// and update feed go to a loopback sink the runner starts (sink.jsonl),
// never to Overwolf.
//
// --steps smoke (default): the driver opens the ads tester, starts both
//   slots, waits for display_ad_loaded and quits the app.
// --steps restart: the restart check. The app restarts itself twice through
//   the sample's own buttons, TEST -> LIVE -> TEST, on the CMP & settings
//   page (no ad guest, so no live ad loads). The runner follows every process
//   of the run (each under its own window monitor and front check) and
//   checks three processes, the modes, the kept page and that each old
//   process is gone.
// --steps tour: a still of every page, the ads tester with both test ads,
//   and the ad privacy settings window (in-process stills, no screen
//   capture) into --stills.
// --page quickstart: page-host mode. The quickstart's page and identity
//   (examples/quickstart-vanilla) in this lab shell, at the quickstart's
//   window size: one still of the window 15 s after its page loaded; the
//   check needs its test ad (display or video) to have loaded by then.
//
// Safety: an isolated home, the window monitor (CGWindowListCopyWindowInfo;
// the app is killed the moment one of its windows becomes visible), a check
// that the app never becomes the frontmost app, and a kill of the whole
// process group on every exit path. A run whose driver records nothing for
// --stall seconds (it records a heartbeat every 5 s) is killed at once with
// verdict `stalled` and the last step it finished. Output:
// e2e/out/<run-id>/summary.json.
//
// The Cargo target folder is $CARGO_TARGET_DIR, else src-tauri/target/e2e.
// Each build's binary is kept in e2e/out/.bin/<page>/ (the two pages are two
// builds of the same crate).

import { spawn, spawnSync } from 'node:child_process';
import {
  appendFileSync,
  copyFileSync,
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import {
  adLoaded,
  labTauriConfig,
  lastProgress,
  parseJsonl,
  passed,
  quickstartTauriConfig,
  quickstartWindow,
  restartResult,
  sinkSummary,
  verdictOf,
} from './lab.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const exampleDir = resolve(here, '..');
const repoRoot = resolve(exampleDir, '..', '..');
const tauriDir = join(exampleDir, 'src-tauri');
const labDist = join(exampleDir, 'dist-lab');
const quickstartDir = join(repoRoot, 'examples', 'quickstart-vanilla');
const EXE = 'ow-tauri-packages-sample';

const { values: opts } = parseArgs({
  options: {
    'run-id': { type: 'string' },
    'no-build': { type: 'boolean', default: false },
    steps: { type: 'string', default: 'smoke' },
    page: { type: 'string', default: 'sample' },
    theme: { type: 'string' },
    stills: { type: 'string' },
    timeout: { type: 'string' },
    stall: { type: 'string', default: '60' },
    'ad-wait': { type: 'string', default: '60000' },
  },
});

function fail(message) {
  console.error(`e2e: ${message}`);
  process.exit(2);
}
if (process.platform !== 'darwin') fail('the invisible lab and the window monitor are macOS only');
if (!['smoke', 'restart', 'tour'].includes(opts.steps)) fail(`unknown --steps ${opts.steps}`);
if (!['sample', 'quickstart'].includes(opts.page)) fail(`unknown --page ${opts.page}`);
if (opts.theme !== undefined && !['dark', 'light'].includes(opts.theme))
  fail(`unknown --theme ${opts.theme}`);
const quickstart = opts.page === 'quickstart';
const steps = quickstart ? 'still' : opts.steps;

const runId =
  opts['run-id'] ??
  `${quickstart ? 'quickstart' : steps}-${new Date().toISOString().replace(/[:.]/g, '-')}`;
const outRoot = join(here, 'out');
const runDir = join(outRoot, runId);
rmSync(runDir, { recursive: true, force: true });
mkdirSync(runDir, { recursive: true });
const stillsDir = resolve(opts.stills ?? join(runDir, 'stills'));
const timeoutMs = Number(opts.timeout ?? (steps === 'tour' ? 600 : 300)) * 1000;
const stallMs = Number(opts.stall) * 1000;
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

// ------------------------------------------------------------------- build
function buildApp() {
  const kept = join(outRoot, '.bin', opts.page, EXE);
  if (opts['no-build'] && existsSync(kept)) return kept;
  const targetDir = process.env.CARGO_TARGET_DIR
    ? resolve(process.env.CARGO_TARGET_DIR)
    : join(tauriDir, 'target', 'e2e');
  // vite exports no bin path: find it next to its package.json.
  const vite = join(
    dirname(createRequire(import.meta.url).resolve('vite/package.json')),
    'bin',
    'vite.js',
  );
  let config;
  if (quickstart) {
    // The quickstart's own build (`tsc && vite build` into its dist/).
    sh('npm', ['run', 'build'], { cwd: quickstartDir });
    const conf = JSON.parse(
      readFileSync(join(quickstartDir, 'src-tauri', 'tauri.conf.json'), 'utf8'),
    );
    config = quickstartTauriConfig(conf, join(quickstartDir, 'dist'));
  } else {
    sh(process.execPath, [vite, 'build', '--mode', 'lab', '--outDir', labDist, '--emptyOutDir'], {
      cwd: exampleDir,
    });
    config = labTauriConfig(labDist);
  }
  // generate_context! embeds the page when the crate compiles: touch the
  // module that calls it so the new page and configuration are picked up.
  const now = new Date();
  utimesSync(join(tauriDir, 'src', 'sample.rs'), now, now);
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
      join(tauriDir, 'Cargo.toml'),
      '--target-dir',
      targetDir,
    ],
    {
      env: {
        ...process.env,
        TAURI_CONFIG: JSON.stringify(config),
        // A separate target directory: skip the incremental cache (gigabytes).
        CARGO_INCREMENTAL: '0',
      },
    },
  );
  mkdirSync(dirname(kept), { recursive: true });
  copyFileSync(join(targetDir, 'debug', EXE), kept);
  return kept;
}

/**
 * A loopback sink for the plugin's analytics, consent experiment and update
 * feed: it answers every request with 200 and logs method, path and body
 * size to `sink.jsonl`. A lab build never reports to Overwolf.
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

// ------------------------------------------------------- monitor and probe
function tool(name, source) {
  const binary = join(outRoot, '.tools', name);
  if (existsSync(binary)) return binary;
  mkdirSync(dirname(binary), { recursive: true });
  sh('swiftc', ['-O', source, '-o', binary]);
  return binary;
}
function processList() {
  return spawnSync('ps', ['-axo', 'pid=,ppid=,pgid=,comm='], { encoding: 'utf8' })
    .stdout.split('\n')
    .map((l) => /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(.*)$/.exec(l))
    .filter(Boolean)
    .map((m) => ({
      pid: Number(m[1]),
      ppid: Number(m[2]),
      pgid: Number(m[3]),
      name: m[4].split('/').pop(),
    }));
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
function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}
const readJsonl = (file) => (existsSync(file) ? parseJsonl(readFileSync(file, 'utf8')) : []);
const sizeOf = (file) => (existsSync(file) ? statSync(file).size : 0);

// --------------------------------------------------------------------- run
async function main() {
  const home = join(runDir, 'home');
  mkdirSync(home, { recursive: true });
  const monitor = tool(
    'window-monitor',
    join(repoRoot, 'tools', 'parity-harness', 'lib', 'window-monitor.swift'),
  );
  const procOwner = tool('proc-owner', join(here, 'proc-owner.swift'));
  const exe = buildApp();
  const sink = await startSink(runDir);
  const env = {
    ...process.env,
    HOME: home,
    CFFIXED_USER_HOME: home,
    OW_TAURI_LAB_DIR: runDir,
    OW_TAURI_LAB_INVISIBLE: '1',
    OW_SAMPLE_LAB_SINK: sink.url,
  };
  if (opts.theme) env.OW_SAMPLE_LAB_APPEARANCE = opts.theme;
  let args = ['--test-ad'];
  if (quickstart) {
    // The quickstart turns test ads on in its configuration.
    args = [];
    const conf = JSON.parse(
      readFileSync(join(quickstartDir, 'src-tauri', 'tauri.conf.json'), 'utf8'),
    );
    env.OW_SAMPLE_LAB_WINDOW = quickstartWindow(conf);
    env.OW_SAMPLE_LAB_STILL = join(stillsDir, 'quickstart-window.png');
  } else {
    env.OW_SAMPLE_E2E_CONFIG = JSON.stringify({
      steps,
      adWaitMs: Number(opts['ad-wait']),
      stillsDir,
    });
  }
  delete env.OW_SAMPLE_LAB_STILL_AFTER_MS;
  writeFileSync(
    join(runDir, 'meta.json'),
    JSON.stringify(
      {
        runId,
        steps,
        page: opts.page,
        theme: opts.theme ?? null,
        startedAt: new Date().toISOString(),
      },
      null,
      2,
    ),
  );

  const webKitBefore = new Set(
    processList()
      .filter(isWebKit)
      .map((p) => p.pid),
  );
  let appOwner;
  log(`launching the lab build (${opts.page}, ${steps}, ${opts.theme ?? 'system'} appearance)`);
  const child = spawn(exe, args, {
    env,
    cwd: runDir,
    stdio: ['ignore', 'pipe', 'pipe'],
    detached: true,
  });
  running.add(child);
  child.stdout.pipe(createWriteStream(join(runDir, 'stdout.log')));
  child.stderr.pipe(createWriteStream(join(runDir, 'stderr.log')));
  const exited = new Promise((ok) => child.on('exit', (code, signal) => ok({ code, signal })));

  // Every process of the run (a restart starts the next one in the same
  // process group), each under its own window monitor.
  const procs = new Map();
  const watch = (pid) => {
    const file = join(runDir, procs.size ? `window-monitor-${pid}.jsonl` : 'window-monitor.jsonl');
    const mon = spawn(monitor, [String(pid), file, '25'], { stdio: 'ignore', detached: true });
    running.add(mon);
    procs.set(pid, { pid, file, mon, offset: 0, seen: Date.now() });
    if (procs.size > 1) log(`following process ${pid}`);
  };
  watch(child.pid);
  const ownerProbe = setTimeout(() => {
    appOwner ??= ownedProcesses(procOwner, child.pid, webKitBefore, appOwner).owner;
  }, 2000);

  const e2eFile = join(runDir, 'e2e.jsonl');
  let safetyKill = null;
  let everFront = false;
  let doneAt = null;
  let timedOut = false;
  let stalled = null;
  let lastSize = 0;
  let lastChange = Date.now();
  let lastFront = 0;
  const gone = {};
  const goneWait = {};
  const started = Date.now();
  const killGroup = (signal) => killTree(child, signal);
  for (;;) {
    await new Promise((ok) => setTimeout(ok, 100));
    // New processes of the group (the restarts).
    for (const p of processList()) {
      if (p.pgid === child.pid && p.name === basename(exe) && !procs.has(p.pid)) watch(p.pid);
    }
    for (const proc of procs.values()) {
      if (!existsSync(proc.file)) continue;
      const fresh = readFileSync(proc.file, 'utf8').slice(proc.offset);
      const end = fresh.lastIndexOf('\n');
      if (end < 0) continue;
      proc.offset += end + 1;
      for (const entry of parseJsonl(fresh.slice(0, end))) {
        if (!safetyKill && (entry.anyVisible === true || entry.everVisible === true)) {
          safetyKill = { at: new Date().toISOString(), pid: proc.pid, entry };
          killGroup('SIGKILL');
          log(`SAFETY: a window of process ${proc.pid} became visible; app killed`);
        }
      }
    }
    // The invisible app must never be the frontmost app.
    if (Date.now() - lastFront > 500) {
      lastFront = Date.now();
      const front = frontPid();
      if (!everFront && front !== null && procs.has(front)) {
        everFront = true;
        log(`WARNING: process ${front} became the frontmost app`);
      }
    }
    const size = sizeOf(e2eFile);
    if (size !== lastSize) {
      lastSize = size;
      lastChange = Date.now();
    }
    const records = readJsonl(e2eFile);
    // The restart check: once phase n reports, phase n - 1's process must be
    // gone (within 10 s).
    if (steps === 'restart') {
      for (const n of [2, 3]) {
        if (gone[n] === true) continue;
        const prev = records.find((r) => r.kind === 'step' && r.name === `restart-${n - 1}`);
        const cur = records.find((r) => r.kind === 'step' && r.name === `restart-${n}`);
        if (!prev || !cur || typeof prev.pid !== 'number') continue;
        goneWait[n] ??= Date.now();
        if (!alive(prev.pid)) gone[n] = true;
        else if (Date.now() - goneWait[n] > 10000) gone[n] = false;
      }
    }
    const finished = records.some(
      (r) => r.kind === 'fatal' || (r.kind === 'done' && (steps !== 'restart' || r.restarted)),
    );
    if (!doneAt && finished) {
      doneAt = Date.now();
      log('driver finished; waiting for the app to quit');
    }
    const anyAlive = [...procs.keys()].some(alive);
    if (!anyAlive && Date.now() - Math.max(...[...procs.values()].map((p) => p.seen)) > 1500) break;
    if (
      !doneAt &&
      !stalled &&
      !safetyKill &&
      Date.now() - lastChange > stallMs &&
      Date.now() - started > stallMs
    ) {
      stalled = {
        at: new Date().toISOString(),
        after: lastProgress(records),
        quietMs: Date.now() - lastChange,
      };
      log(
        `STALLED: nothing recorded for ${Math.round(stalled.quietMs / 1000)} s after ${stalled.after}; killing the app`,
      );
      killGroup('SIGTERM');
      setTimeout(() => killGroup('SIGKILL'), 3000).unref();
    }
    if (
      !timedOut &&
      ((doneAt && Date.now() - doneAt > 20000) || Date.now() - started > timeoutMs)
    ) {
      timedOut = true;
      log(
        doneAt
          ? 'app did not quit after the driver finished; killing it'
          : 'timeout; killing the app',
      );
      killGroup('SIGTERM');
      setTimeout(() => killGroup('SIGKILL'), 5000).unref();
    }
  }
  const exit = await exited;
  clearTimeout(ownerProbe);
  killGroup('SIGKILL');
  running.delete(child);
  await new Promise((ok) => setTimeout(ok, 500));
  for (const proc of procs.values()) {
    killTree(proc.mon, 'SIGTERM');
    running.delete(proc.mon);
  }
  sink.server.closeAllConnections();
  await new Promise((ok) => sink.server.close(ok));

  const records = readJsonl(e2eFile);
  const verdict = verdictOf({
    safetyKill: safetyKill !== null,
    done: records.some((r) => r.kind === 'done' && (steps !== 'restart' || r.restarted === true)),
    timedOut,
    stalled: stalled !== null,
  });
  const monitors = [...procs.values()].map((p) => ({
    pid: p.pid,
    end: readJsonl(p.file).find((r) => r.kind === 'end') ?? null,
  }));
  const everVisible = monitors.every((m) => m.end) ? monitors.some((m) => m.end.everVisible) : null;
  let owned = [];
  for (let i = 0; i < 20; i += 1) {
    owned = [...procs.keys()].flatMap(
      (pid) => ownedProcesses(procOwner, pid, webKitBefore, appOwner).procs,
    );
    owned = [...new Map(owned.map((p) => [p.pid, p])).values()];
    if (!owned.length) break;
    await new Promise((ok) => setTimeout(ok, 500));
  }
  const stepRecords = records.filter((r) => r.kind === 'step');
  const done = records.find((r) => r.kind === 'done');
  const summary = {
    runId,
    steps,
    page: opts.page,
    theme: opts.theme ?? null,
    verdict,
    exit,
    safetyKill,
    stalled,
    everVisible,
    everFront,
    processes: monitors.map((m) => ({ pid: m.pid, everVisible: m.end?.everVisible ?? null })),
    started: stepRecords.some((s) => s.name === 'started') || quickstart,
    fatal: records.filter((r) => r.kind === 'fatal').map((r) => r.text),
    hidden: records.filter((r) => r.kind === 'visibility' && r.state === 'hidden').length,
    sink: sinkSummary(readJsonl(join(runDir, 'sink.jsonl'))),
    leftProcesses: owned,
    ms: Date.now() - started,
  };
  if (steps === 'smoke') {
    const ads = stepRecords.find((s) => s.name === 'ads');
    summary.displayAdLoaded = done?.displayAdLoaded ?? false;
    summary.owadviews = ads?.owadviews ?? null;
    summary.adEvents = ads?.events ?? [];
    summary.apiErrors = ads?.errors ?? [];
    summary.check = summary.displayAdLoaded;
  } else if (steps === 'restart') {
    summary.restart = { ...restartResult(records, gone), gone };
    summary.check = summary.restart.ok;
  } else if (steps === 'tour') {
    const stills = records.filter((r) => r.kind === 'still');
    summary.stills = stills.map((s) => ({ name: s.name, path: s.out?.path ?? null }));
    summary.check =
      done !== undefined &&
      stills.length === done.stills &&
      stills.every((s) => typeof s.out?.path === 'string' && existsSync(s.out.path));
  } else {
    const loaded = adLoaded(readJsonl(join(runDir, 'ipc.jsonl')));
    const path = done?.still?.path ?? null;
    summary.still = path;
    summary.adLoaded = loaded;
    summary.check = loaded && typeof path === 'string' && existsSync(path);
  }
  writeFileSync(join(runDir, 'summary.json'), JSON.stringify(summary, null, 2));
  log(JSON.stringify(summary));
  if (owned.length)
    log(`WARNING: processes the app owned are still running: ${JSON.stringify(owned)}`);
  process.exitCode = passed(summary) ? 0 : 1;
}

main().catch((error) => {
  console.error(error);
  killAll();
  process.exit(1);
});
