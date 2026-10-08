#!/usr/bin/env node
// End-to-end run of the packages sample in invisible lab mode (macOS).
// See e2e/README.md.
//
//   node e2e/run.mjs [--host tauri|electron] [--run-id ID] [--no-build]
//                    [--upstream-dir DIR] [--identity FILE] [--timeout S]
//                    [--ad-wait MS] [--build-only]
//                    [--idle-ms MS [--idle-sample-ms MS] [--idle-layout L]
//                     [--idle-reload-ms MS]]
//                    [--sample-ms MS]
//
// --host tauri (default): builds the ported sample (webpack, plus the main
//   bundle with e2e/tauri-driver.js in front, and a debug Tauri build with
//   the `lab` feature into src-tauri/target/e2e) and runs it.
// --host electron: the baseline. Runs the UPSTREAM sample, already built in
//   --upstream-dir (its own `webpack` build, plus `electron-updater` in its
//   node_modules), on the ow-electron of tools/parity-harness.
//
// Both runs: test ads (--test-ad), an isolated home, the lab identity from
// the git-ignored tools/parity-harness/local.identity.json (or --identity),
// a local update feed, the window monitor (CGWindowListCopyWindowInfo; the
// app is killed the moment one of its windows becomes visible) and a kill
// of the whole process group on every exit path. Output: e2e/out/<run-id>/.
//
// --idle-ms: instead of the full pass, start every slot of one Ads Tester
//   layout and leave the ads running that long (the idle run, steps.js).
//   --idle-reload-ms also reloads every <owadview> that often.
// --sample-ms (default 10000): how often the memory of the app and of every
//   process it owns (WebKit's XPC services included; RSS and physical
//   footprint) goes to proc-samples.jsonl. After the app quits, none of them may be left.

import { spawn, spawnSync } from 'node:child_process';
import {
  appendFileSync,
  copyFileSync,
  cpSync,
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const here = dirname(fileURLToPath(import.meta.url));
const exampleDir = resolve(here, '..');
const repoRoot = resolve(exampleDir, '..', '..');
const harnessDir = join(repoRoot, 'tools', 'parity-harness');

const { values: opts } = parseArgs({
  options: {
    host: { type: 'string', default: 'tauri' },
    'run-id': { type: 'string' },
    'no-build': { type: 'boolean', default: false },
    'build-only': { type: 'boolean', default: false },
    'upstream-dir': { type: 'string' },
    identity: { type: 'string' },
    timeout: { type: 'string', default: '900' },
    'ad-wait': { type: 'string', default: '8000' },
    'idle-ms': { type: 'string' },
    'idle-sample-ms': { type: 'string' },
    'idle-layout': { type: 'string' },
    'idle-reload-ms': { type: 'string' },
    'sample-ms': { type: 'string', default: '10000' },
  },
});

function fail(message) {
  console.error(`e2e: ${message}`);
  process.exit(2);
}
if (process.platform !== 'darwin') fail('the invisible lab and the window monitor are macOS only');
if (!['tauri', 'electron'].includes(opts.host)) fail(`unknown --host ${opts.host}`);

const runId = opts['run-id'] ?? `${opts.host}-${new Date().toISOString().replace(/[:.]/g, '-')}`;
const outRoot = join(here, 'out');
const runDir = join(outRoot, runId);
rmSync(runDir, { recursive: true, force: true });
mkdirSync(runDir, { recursive: true });
const log = (message) => {
  const line = `[${new Date().toISOString()}] ${message}`;
  console.log(line);
  appendFileSync(join(runDir, 'runner.log'), line + '\n');
};

// ---------------------------------------------------------------- identity
function labPackageJson(base) {
  const file = opts.identity ?? join(harnessDir, 'local.identity.json');
  if (!existsSync(file)) {
    log('no lab identity file: running with the sample identity');
    return base;
  }
  const identity = JSON.parse(readFileSync(file, 'utf8'));
  const pkg = { ...base };
  for (const key of ['name', 'productName', 'author', 'version']) {
    if (identity[key] !== undefined) pkg[key] = identity[key];
  }
  if (identity.uid) pkg.overwolf = { ...pkg.overwolf, uid: identity.uid };
  return pkg;
}

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
async function buildTauri() {
  const webDir = join(outRoot, '.build', 'web');
  const exe = join(exampleDir, 'src-tauri', 'target', 'e2e', 'debug', 'overwolf-official-sample-app');
  if (opts['no-build'] && existsSync(exe)) return exe;
  // 1. ow-tauri (the package, and the scripts the plugin injects, so the
  //    run has the current bootstrap), then the sample as `npm run build`
  //    builds it.
  sh('npm', ['run', 'build', '--workspace', 'ow-tauri'], { cwd: repoRoot });
  sh('npm', ['run', 'build:injected', '--workspace', 'ow-tauri'], { cwd: repoRoot });
  sh('npx', ['webpack', '--mode=development'], { cwd: exampleDir });
  // 2. The main bundle again, with the driver in front, into a separate
  //    frontend folder (dist/ stays the normal build).
  const require = createRequire(join(exampleDir, 'package.json'));
  const webpack = require('webpack');
  const HtmlWebpackPlugin = require('html-webpack-plugin');
  const mainConfig = require(join(exampleDir, 'webpack.main.config.js'));
  rmSync(webDir, { recursive: true, force: true });
  const config = {
    ...mainConfig,
    mode: 'development',
    context: exampleDir,
    entry: { index: [join(here, 'tauri-driver.js'), './src/browser/index.ts'] },
    output: { ...mainConfig.output, path: join(webDir, 'browser') },
    plugins: [
      ...mainConfig.plugins.filter((p) => !(p instanceof HtmlWebpackPlugin)),
      new HtmlWebpackPlugin({
        title: 'main',
        filename: join(webDir, 'browser', 'main.html'),
        chunks: ['index'],
        inject: true,
      }),
    ],
  };
  await new Promise((ok, ko) =>
    webpack(config, (err, stats) => {
      if (err || stats.hasErrors()) ko(err ?? new Error(stats.toString('errors-only')));
      else ok();
    }),
  );
  for (const dir of ['renderer', 'preload', 'exclusive', 'osr']) {
    cpSync(join(exampleDir, 'dist', dir), join(webDir, dir), { recursive: true });
  }
  // 3. The debug app with the `lab` feature, embedding that folder.
  //    generate_context! embeds the assets when the crate compiles, so the
  //    lab-only module is touched to recompile it.
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
        TAURI_CONFIG: JSON.stringify({ build: { frontendDist: webDir } }),
        // A separate target directory: skip the incremental cache (gigabytes).
        CARGO_INCREMENTAL: '0',
      },
    },
  );
  return exe;
}

function prepareElectron() {
  const upstream = opts['upstream-dir'] && resolve(opts['upstream-dir']);
  if (!upstream || !existsSync(join(upstream, 'dist', 'browser', 'index.js'))) {
    fail('--host electron needs --upstream-dir: the upstream sample, built (dist/browser/index.js)');
  }
  const appDir = join(runDir, 'app');
  mkdirSync(appDir, { recursive: true });
  symlinkSync(join(upstream, 'dist'), join(appDir, 'dist'));
  symlinkSync(join(upstream, 'node_modules'), join(appDir, 'node_modules'));
  copyFileSync(join(here, 'electron-main.cjs'), join(appDir, 'e2e-main.cjs'));
  copyFileSync(join(here, 'steps.js'), join(appDir, 'steps.js'));
  const pkg = labPackageJson(JSON.parse(readFileSync(join(upstream, 'package.json'), 'utf8')));
  writeFileSync(join(appDir, 'package.json'), JSON.stringify({ ...pkg, main: 'e2e-main.cjs' }, null, 2));
  const require = createRequire(join(harnessDir, 'package.json'));
  return { appDir, exe: require('@overwolf/ow-electron'), version: require('@overwolf/ow-electron/package.json').version };
}

// ------------------------------------------------------------- update feed
function feedYaml(version) {
  const file = `Overwolf-Electron-Official-Sample-App-${version}-mac.zip`;
  const sha512 = Buffer.alloc(64, 7).toString('base64');
  return [
    `version: ${version}`,
    'files:',
    `  - url: ${file}`,
    `    sha512: ${sha512}`,
    '    size: 1024',
    `path: ${file}`,
    `sha512: ${sha512}`,
    "releaseDate: '2026-10-07T00:00:00.000Z'",
    '',
  ].join('\n');
}
function startFeed() {
  const server = createServer((req, res) => {
    appendFileSync(
      join(runDir, 'feed-requests.jsonl'),
      JSON.stringify({ wall: Date.now(), method: req.method, url: req.url, headers: req.rawHeaders }) + '\n',
    );
    const m = /^\/(newer|same)\/([^/?]+\.yml)/.exec(req.url ?? '');
    if (!m) {
      res.writeHead(404).end('not found');
      return;
    }
    res.writeHead(200, { 'content-type': 'text/yaml' }).end(feedYaml(m[1] === 'newer' ? '1.0.1' : '1.0.0'));
  });
  return new Promise((ok) => server.listen(0, '127.0.0.1', () => ok(server)));
}

// ---------------------------------------------------------- window monitor
function windowMonitorBinary() {
  const binary = join(outRoot, '.tools', 'window-monitor');
  if (existsSync(binary)) return binary;
  mkdirSync(dirname(binary), { recursive: true });
  sh('swiftc', ['-O', join(harnessDir, 'lib', 'window-monitor.swift'), '-o', binary]);
  return binary;
}

// ------------------------------------------------------- process sampling
// WebKit's web content, networking and GPU processes are XPC services that
// launchd starts: they are not in the app's process group, so they are found
// by their responsible process (proc-owner.swift). An app started from a
// terminal is not responsible for itself (the terminal's app is), so its
// WebKit processes are the ones of that same responsible process that
// started after the launch (`before`: the WebKit pids seen at launch).
// Another WebKit app launched from the same terminal during the run would
// be counted too; the samples name every process.
function procOwnerBinary() {
  const binary = join(outRoot, '.tools', 'proc-owner');
  if (existsSync(binary)) return binary;
  mkdirSync(dirname(binary), { recursive: true });
  sh('swiftc', ['-O', join(here, 'proc-owner.swift'), '-o', binary]);
  return binary;
}
function processList() {
  return spawnSync('ps', ['-axo', 'pid=,ppid=,rss=,comm='], { encoding: 'utf8' })
    .stdout.split('\n')
    .map((l) => /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(.*)$/.exec(l))
    .filter(Boolean)
    .map((m) => ({
      pid: Number(m[1]),
      ppid: Number(m[2]),
      rssKb: Number(m[3]),
      name: m[4].split('/').pop(),
    }));
}
const isWebKit = (p) => p.name.startsWith('com.apple.WebKit.');
function webKitPids() {
  return new Set(processList().filter(isWebKit).map((p) => p.pid));
}
function ownedProcesses(probe, appPid, before, appOwner) {
  const ps = processList();
  const owners = new Map(
    spawnSync(probe, ps.map((p) => String(p.pid)), { encoding: 'utf8' })
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
// The physical footprint of each pid in KiB (what Activity Monitor shows as
// Memory: resident and compressed pages the process owns). RSS alone drops
// whenever macOS compresses pages under memory pressure, so a growth
// comparison reads the footprint.
function footprintKb(pids) {
  const out = new Map();
  if (!pids.length) return out;
  const args = ['-l', '1', '-stats', 'pid,mem', ...pids.flatMap((p) => ['-pid', String(p)])];
  const text = spawnSync('top', args, { encoding: 'utf8' }).stdout ?? '';
  const unit = { B: 1 / 1024, K: 1, M: 1024, G: 1024 * 1024 };
  for (const line of text.split('\n')) {
    const m = /^(\d+)\s+([\d.]+)([BKMG])[+-]?\s*$/.exec(line.trim());
    if (m) out.set(Number(m[1]), Math.round(Number(m[2]) * unit[m[3]]));
  }
  return out;
}
// The pid of the frontmost app (the one that has the keyboard), or null.
function frontPid() {
  const asn = spawnSync('lsappinfo', ['front'], { encoding: 'utf8' }).stdout.trim();
  if (!asn) return null;
  const info = spawnSync('lsappinfo', ['info', '-only', 'pid', asn], { encoding: 'utf8' }).stdout;
  const m = /"pid"\s*=\s*(\d+)/.exec(info);
  return m ? Number(m[1]) : null;
}

// --------------------------------------------------------------------- run
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

async function main() {
  const tauri = opts.host === 'tauri';
  const home = join(runDir, 'home');
  mkdirSync(home, { recursive: true });
  const monitor = windowMonitorBinary();
  let exe;
  let args;
  const env = { ...process.env, CFFIXED_USER_HOME: home, OW_TAURI_TEST_AD: '1' };
  delete env.ELECTRON_RUN_AS_NODE;
  const feed = await startFeed();
  const feedUrl = `http://127.0.0.1:${feed.address().port}`;
  const config = { runDir, feedUrl, adWaitMs: Number(opts['ad-wait']) };
  if (opts['idle-ms']) {
    config.idleMs = Number(opts['idle-ms']);
    if (opts['idle-sample-ms']) config.idleSampleMs = Number(opts['idle-sample-ms']);
    if (opts['idle-layout']) config.idleLayout = opts['idle-layout'];
    if (opts['idle-reload-ms']) config.idleReloadMs = Number(opts['idle-reload-ms']);
  }
  env.OW_SAMPLE_E2E_CONFIG = JSON.stringify(config);
  const meta = { runId, host: opts.host, startedAt: new Date().toISOString(), feedUrl };
  if (tauri) {
    exe = await buildTauri();
    if (opts['build-only']) {
      log(`built ${exe}`);
      feed.close();
      return;
    }
    const pkg = labPackageJson(JSON.parse(readFileSync(join(exampleDir, 'package.json'), 'utf8')));
    const pkgPath = join(runDir, 'package.json');
    writeFileSync(pkgPath, JSON.stringify(pkg, null, 2));
    Object.assign(env, {
      HOME: home,
      OW_TAURI_LAB_DIR: runDir,
      OW_TAURI_LAB_INVISIBLE: '1',
      OW_TAURI_LAB_PACKAGE_JSON: pkgPath,
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

  const procOwner = procOwnerBinary();
  const webKitBefore = webKitPids();
  let appOwner;
  log(`launching ${opts.host}: ${exe} ${args.join(' ')}`);
  const child = spawn(exe, args, { env, cwd: runDir, stdio: ['ignore', 'pipe', 'pipe'], detached: true });
  running.add(child);
  child.stdout.pipe(createWriteStream(join(runDir, 'stdout.log')));
  child.stderr.pipe(createWriteStream(join(runDir, 'stderr.log')));
  const monitorFile = join(runDir, 'window-monitor.jsonl');
  const mon = spawn(monitor, [String(child.pid), monitorFile, '25'], { stdio: 'ignore', detached: true });
  running.add(mon);
  const samplesFile = join(runDir, 'proc-samples.jsonl');
  const sample = () => {
    const { owner, procs } = ownedProcesses(procOwner, child.pid, webKitBefore, appOwner);
    appOwner ??= owner;
    const footprints = footprintKb(procs.map((p) => p.pid));
    for (const p of procs) p.footprintKb = footprints.get(p.pid) ?? null;
    const sum = (key) => procs.reduce((n, p) => n + (p[key] ?? 0), 0);
    appendFileSync(
      samplesFile,
      JSON.stringify({ t: Date.now(), totalKb: sum('rssKb'), totalFootprintKb: sum('footprintKb'), procs }) + '\n',
    );
  };
  const sampler = setInterval(sample, Number(opts['sample-ms']));
  // The invisible app must never be the frontmost app: it would take the
  // keyboard from the app the user is typing in. It is killed at once if it
  // does (verdict safety-kill), as for a visible window.
  let everFront = false;
  let frontKill = null;
  const frontWatch = setInterval(() => {
    if (!everFront && frontPid() === child.pid) {
      everFront = true;
      frontKill = { at: new Date().toISOString(), entry: { kind: 'front', pid: child.pid } };
      killTree(child, 'SIGKILL');
      log('SAFETY: the app became the frontmost app; app killed');
    }
  }, 200);

  const e2eFile = join(runDir, 'e2e.jsonl');
  const timeoutMs = Number(opts.timeout) * 1000;
  const exited = new Promise((ok) => child.on('exit', (code, signal) => ok({ code, signal })));
  let verdict = 'timeout';
  let safetyKill = null;
  let doneAt = null;
  const started = Date.now();
  let monitorOffset = 0;
  // Poll: visibility (kill at once), the driver's done record, exit, timeout.
  // eslint-disable-next-line no-constant-condition
  while (true) {
    const race = await Promise.race([exited, new Promise((ok) => setTimeout(() => ok(null), 100))]);
    if (existsSync(monitorFile)) {
      const text = readFileSync(monitorFile, 'utf8');
      const fresh = text.slice(monitorOffset);
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
      safetyKill ??= frontKill;
      verdict = safetyKill ? 'safety-kill' : doneAt ? 'done' : 'exited-early';
      break;
    }
    if (!doneAt && readJsonl(e2eFile).some((r) => r.kind === 'done' || r.kind === 'fatal')) {
      doneAt = Date.now();
      log('driver finished; waiting for the app to quit');
    }
    if (doneAt && Date.now() - doneAt > 20000) {
      log('app did not quit after the driver finished; killing it');
      killTree(child, 'SIGTERM');
      setTimeout(() => killTree(child, 'SIGKILL'), 5000).unref();
    }
    if (Date.now() - started > timeoutMs) {
      log('timeout; killing the app');
      killTree(child, 'SIGTERM');
      setTimeout(() => killTree(child, 'SIGKILL'), 5000).unref();
    }
  }
  const exit = await exited;
  clearInterval(sampler);
  clearInterval(frontWatch);
  killTree(child, 'SIGKILL');
  running.delete(child);
  // Let the monitor write its end record.
  await new Promise((ok) => setTimeout(ok, 500));
  killTree(mon, 'SIGTERM');
  running.delete(mon);
  feed.close();

  const monitorEnd = readJsonl(monitorFile).find((r) => r.kind === 'end') ?? null;
  const records = readJsonl(e2eFile);
  const steps = records.filter((r) => r.kind === 'step');
  const problems = steps.filter(
    (s) =>
      s.error ||
      (s.events ?? []).some((p) => ['error', 'unhandledrejection', 'exec-error'].includes(p.kind)) ||
      (s.main ?? []).some((p) => ['error', 'unhandledrejection'].includes(p.kind)),
  ).length;
  const summary = {
    runId,
    host: opts.host,
    verdict,
    exit,
    safetyKill,
    everVisible: monitorEnd ? monitorEnd.everVisible : null,
    everFront,
    steps: steps.length,
    stepsWithErrors: problems,
    blocked: readJsonl(join(runDir, 'blocked.jsonl')).length,
    feedRequests: readJsonl(join(runDir, 'feed-requests.jsonl')).length,
    ms: Date.now() - started,
  };
  writeFileSync(join(runDir, 'summary.json'), JSON.stringify(summary, null, 2));
  log(JSON.stringify(summary));
  // The whole process group is gone (the app, its helpers, the monitor).
  const left = spawnSync('pgrep', ['-g', String(child.pid)], { encoding: 'utf8' }).stdout.trim();
  if (left) log(`WARNING: processes left in the app's group: ${left}`);
  // And no process the app owned (WebKit's XPC services) outlives it; they
  // exit shortly after their client, so wait up to 10 s.
  let owned = [];
  for (let i = 0; i < 20; i += 1) {
    owned = ownedProcesses(procOwner, child.pid, webKitBefore, appOwner).procs;
    if (!owned.length) break;
    await new Promise((ok) => setTimeout(ok, 500));
  }
  summary.leftProcesses = owned;
  writeFileSync(join(runDir, 'summary.json'), JSON.stringify(summary, null, 2));
  if (owned.length) log(`WARNING: processes the app owned are still running: ${JSON.stringify(owned)}`);
  process.exitCode = verdict === 'done' && summary.everVisible === false && !everFront && !owned.length ? 0 : 1;
}

main().catch((error) => {
  console.error(error);
  killAll();
  process.exit(1);
});
