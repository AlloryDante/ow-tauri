#!/usr/bin/env node
// The packages sample in the invisible lab (macOS). See e2e/README.md.
//
//   node e2e/run.mjs [--run-id ID] [--no-build] [--timeout S] [--ad-wait MS]
//
// Builds the lab page (`vite build --mode lab`, which adds the driver) and a
// debug app with the `lab` Cargo feature under the lab bundle id, then runs
// it with test ads (--test-ad): the driver opens the ads tester, starts both
// slots, waits for display_ad_loaded and quits the app. The plugin's
// analytics, consent experiment and update feed go to a loopback sink the
// runner starts (sink.jsonl), never to Overwolf.
//
// Safety: an isolated home, the window monitor (CGWindowListCopyWindowInfo;
// the app is killed the moment one of its windows becomes visible), a check
// that the app never becomes the frontmost app, and a kill of the whole
// process group on every exit path. Output: e2e/out/<run-id>/summary.json.
//
// The Cargo target folder is $CARGO_TARGET_DIR, else src-tauri/target/e2e.

import { spawn, spawnSync } from 'node:child_process';
import {
  appendFileSync,
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { labTauriConfig, parseJsonl, passed, sinkSummary, verdictOf } from './lab.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const exampleDir = resolve(here, '..');
const repoRoot = resolve(exampleDir, '..', '..');
const tauriDir = join(exampleDir, 'src-tauri');
const labDist = join(exampleDir, 'dist-lab');

const { values: opts } = parseArgs({
  options: {
    'run-id': { type: 'string' },
    'no-build': { type: 'boolean', default: false },
    timeout: { type: 'string', default: '300' },
    'ad-wait': { type: 'string', default: '60000' },
  },
});

function fail(message) {
  console.error(`e2e: ${message}`);
  process.exit(2);
}
if (process.platform !== 'darwin') fail('the invisible lab and the window monitor are macOS only');

const runId = opts['run-id'] ?? `smoke-${new Date().toISOString().replace(/[:.]/g, '-')}`;
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

// ------------------------------------------------------------------- build
function buildApp() {
  const targetDir = process.env.CARGO_TARGET_DIR
    ? resolve(process.env.CARGO_TARGET_DIR)
    : join(tauriDir, 'target', 'e2e');
  const exe = join(targetDir, 'debug', 'ow-tauri-packages-sample');
  if (opts['no-build'] && existsSync(exe) && existsSync(labDist)) return exe;
  // vite exports no bin path: find it next to its package.json.
  const vite = join(
    dirname(createRequire(import.meta.url).resolve('vite/package.json')),
    'bin',
    'vite.js',
  );
  sh(process.execPath, [vite, 'build', '--mode', 'lab', '--outDir', labDist, '--emptyOutDir'], {
    cwd: exampleDir,
  });
  // generate_context! embeds the page when the crate compiles: touch the
  // module that calls it so the new lab page is picked up.
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
        TAURI_CONFIG: JSON.stringify(labTauriConfig(labDist)),
        // A separate target directory: skip the incremental cache (gigabytes).
        CARGO_INCREMENTAL: '0',
      },
    },
  );
  return exe;
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
const readJsonl = (file) => (existsSync(file) ? parseJsonl(readFileSync(file, 'utf8')) : []);

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
    OW_SAMPLE_E2E_CONFIG: JSON.stringify({ adWaitMs: Number(opts['ad-wait']) }),
  };
  writeFileSync(
    join(runDir, 'meta.json'),
    JSON.stringify({ runId, exe, startedAt: new Date().toISOString() }, null, 2),
  );

  const webKitBefore = new Set(
    processList()
      .filter(isWebKit)
      .map((p) => p.pid),
  );
  let appOwner;
  log('launching the lab build (test ads)');
  const child = spawn(exe, ['--test-ad'], {
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
  let timedOut = false;
  let verdict;
  const started = Date.now();
  let monitorOffset = 0;
  for (;;) {
    const race = await Promise.race([exited, new Promise((ok) => setTimeout(() => ok(null), 100))]);
    if (existsSync(monitorFile)) {
      const fresh = readFileSync(monitorFile, 'utf8').slice(monitorOffset);
      const end = fresh.lastIndexOf('\n');
      if (end >= 0) {
        monitorOffset += end + 1;
        for (const entry of parseJsonl(fresh.slice(0, end))) {
          if (!safetyKill && (entry.anyVisible === true || entry.everVisible === true)) {
            safetyKill = { at: new Date().toISOString(), entry };
            killTree(child, 'SIGKILL');
            log('SAFETY: a window became visible; app killed');
          }
        }
      }
    }
    const finished = readJsonl(e2eFile).some((r) => r.kind === 'done' || r.kind === 'fatal');
    if (race) {
      // The app may quit within one poll of the driver's last record.
      verdict = verdictOf({ safetyKill: safetyKill !== null, done: finished, timedOut });
      break;
    }
    if (!doneAt && finished) {
      doneAt = Date.now();
      log('driver finished; waiting for the app to quit');
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
  sink.server.closeAllConnections();
  await new Promise((ok) => sink.server.close(ok));

  const monitorEnd = readJsonl(monitorFile).find((r) => r.kind === 'end') ?? null;
  const records = readJsonl(e2eFile);
  const steps = records.filter((r) => r.kind === 'step');
  const ads = steps.find((s) => s.name === 'ads');
  const done = records.find((r) => r.kind === 'done');
  let owned = [];
  for (let i = 0; i < 20; i += 1) {
    owned = ownedProcesses(procOwner, child.pid, webKitBefore, appOwner).procs;
    if (!owned.length) break;
    await new Promise((ok) => setTimeout(ok, 500));
  }
  const summary = {
    runId,
    verdict,
    exit,
    safetyKill,
    everVisible: monitorEnd ? monitorEnd.everVisible : null,
    everFront,
    started: steps.some((s) => s.name === 'started'),
    displayAdLoaded: done?.displayAdLoaded ?? false,
    owadviews: ads?.owadviews ?? null,
    adEvents: ads?.events ?? [],
    apiErrors: ads?.errors ?? [],
    fatal: records.filter((r) => r.kind === 'fatal').map((r) => r.text),
    sink: sinkSummary(readJsonl(join(runDir, 'sink.jsonl'))),
    leftProcesses: owned,
    ms: Date.now() - started,
  };
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
