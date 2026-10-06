#!/usr/bin/env node
// Parity harness CLI: runs a hidden ow-electron app that shows <owadview> ads
// and records what ow-electron does. See README.md.
//
//   node run.mjs --mode test --layout 400x600,728x90 --duration 90
//   node run.mjs --mode live --live-ok --duration 90 --max-live-loads 10
//
// Output: captures/<run-id>/ (git-ignored).

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { diffSnapshots, snapshotDir } from './lib/fs-snapshot.mjs';
import { displayName, loadIdentity } from './lib/identity.mjs';
import { launch, makeAppDir, owElectronVersion } from './lib/launch.mjs';
import { waitForQuietMachine } from './lib/load-guard.mjs';
import { parseNetlog, summarize } from './lib/netlog-parse.mjs';
import { appDataDir, isolationEnv } from './lib/paths.mjs';
import { electronUid } from './lib/uid.mjs';

const harnessDir = dirname(fileURLToPath(import.meta.url));

/** Slot sizes used by Overwolf's official sample. */
const DEFAULT_LAYOUTS = ['400x600', '400x60', '160x600', '300x250', '728x90', '400x300'];
const MAX_DURATION_S = 1800;

const USAGE = `Usage: node run.mjs [options]

  --mode test|live        test ads (--test-ad) or live ads (default: test)
  --live-ok               required with --mode live: confirms live ads may load
  --layout WxH[,WxH...]   owadview slot sizes (default: ${DEFAULT_LAYOUTS.join(',')})
  --duration SECONDS      time before the quit flow starts (default: 90, max ${MAX_DURATION_S})
  --max-live-loads N      live mode: stop all ads after N loads (default: 10)
  --present hidden|transparent
                          hidden: windows never shown (default);
                          transparent: shown inactive with opacity 0, ignoring the mouse
  --home isolated|real|profile:NAME
                          user directories: a fresh per-run home (default), the real
                          home, or a reusable home under captures/profiles/NAME
  --packages a,b          overwolf.packages to list in package.json (default: none)
  --webrequest            also hook session.webRequest (can displace ow-electron's own
                          listeners; compare with a run without it)
  --no-cdp                do not attach the DevTools protocol to webContents (control run:
                          the net log still records every request)
  --quit-style close-then-quit|quit
                          end of run: close the ad window, then app.quit() 2 s later
                          (default), or app.quit() with the window still open
  --disable-analytics     call app.overwolf.disableAnonymousAnalytics() at startup
  --window-name NAME      BrowserWindow 'name' option for the ad window
  --identity FILE         identity JSON (default: local.identity.json, else neutral example)
  --run-id ID             capture folder name (default: timestamp + mode)
  --no-wait               do not wait for a quiet machine before launching
  --help`;

function parseCli() {
  const { values } = parseArgs({
    options: {
      mode: { type: 'string', default: 'test' },
      'live-ok': { type: 'boolean', default: false },
      layout: { type: 'string' },
      duration: { type: 'string', default: '90' },
      'max-live-loads': { type: 'string', default: '10' },
      present: { type: 'string', default: 'hidden' },
      home: { type: 'string', default: 'isolated' },
      packages: { type: 'string', default: '' },
      webrequest: { type: 'boolean', default: false },
      'no-cdp': { type: 'boolean', default: false },
      'quit-style': { type: 'string', default: 'close-then-quit' },
      'disable-analytics': { type: 'boolean', default: false },
      'window-name': { type: 'string' },
      identity: { type: 'string' },
      'run-id': { type: 'string' },
      'no-wait': { type: 'boolean', default: false },
      help: { type: 'boolean', default: false },
    },
  });
  if (values.help) {
    console.log(USAGE);
    process.exit(0);
  }
  const fail = (message) => {
    console.error(`${message}\n\n${USAGE}`);
    process.exit(2);
  };
  if (!['test', 'live'].includes(values.mode)) fail(`--mode must be test or live`);
  if (values.mode === 'live' && !values['live-ok']) fail('--mode live needs --live-ok');
  if (!['hidden', 'transparent'].includes(values.present))
    fail('--present must be hidden or transparent');
  if (!['close-then-quit', 'quit'].includes(values['quit-style'])) fail('bad --quit-style');
  const duration = Number(values.duration);
  if (!(duration > 0 && duration <= MAX_DURATION_S))
    fail(`--duration must be 1..${MAX_DURATION_S}`);
  const layouts = values.layout ? values.layout.split(',') : DEFAULT_LAYOUTS;
  for (const layout of layouts) if (!/^\d+x\d+$/.test(layout)) fail(`bad layout ${layout}`);
  const maxLiveLoads = Number(values['max-live-loads']);
  if (!(maxLiveLoads >= 1 && maxLiveLoads <= 50)) fail('--max-live-loads must be 1..50');
  return { ...values, duration, layouts, maxLiveLoads };
}

function windowSize(layouts) {
  // Flex-wrap layout in a window wide enough for the widest slot.
  const width = Math.max(1000, ...layouts.map((l) => Number(l.split('x')[0]) + 16));
  let rowWidth = 0;
  let rowHeight = 0;
  let height = 8;
  for (const layout of layouts) {
    const [w, h] = layout.split('x').map(Number);
    if (rowWidth + w + 8 > width) {
      height += rowHeight + 8;
      rowWidth = 0;
      rowHeight = 0;
    }
    rowWidth += w + 8;
    rowHeight = Math.max(rowHeight, h);
  }
  return { width, height: height + rowHeight + 16 };
}

function resolveHome(option, runDir) {
  if (option === 'real') return { home: homedir(), env: {} };
  const home =
    option === 'isolated'
      ? join(runDir, 'home')
      : option.startsWith('profile:')
        ? join(harnessDir, 'captures', 'profiles', option.slice('profile:'.length))
        : null;
  if (!home) throw new Error(`bad --home ${option}`);
  const env = isolationEnv(home);
  if (!env)
    throw new Error(`home isolation is not available on ${process.platform}; use --home real`);
  mkdirSync(home, { recursive: true });
  return { home, env };
}

async function main() {
  const opts = parseCli();
  const identity = loadIdentity(harnessDir, opts.identity);
  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  const runId = opts['run-id'] ?? `${stamp}-${opts.mode}`;
  const runDir = resolve(harnessDir, 'captures', runId);
  mkdirSync(runDir, { recursive: true });
  const { home, env: homeEnv } = resolveHome(opts.home, runDir);

  const pkg = {
    name: identity.name,
    productName: identity.productName,
    version: identity.version,
    ...(identity.author !== undefined ? { author: identity.author } : {}),
    ...(identity.build ? { build: identity.build } : {}),
    ...(opts.packages ? { overwolf: { packages: opts.packages.split(',') } } : {}),
  };
  const appDir = join(runDir, 'app');
  makeAppDir(appDir, pkg);

  const authorName = typeof pkg.author === 'object' ? pkg.author.name : pkg.author;
  const expectedUid = identity.uid ?? electronUid(authorName ?? '', displayName(pkg));
  const appData = appDataDir(home);
  const watched = {
    'ow-electron': join(appData, 'ow-electron', expectedUid),
    userData: join(appData, displayName(pkg)),
  };
  const before = Object.fromEntries(
    Object.entries(watched).map(([k, dir]) => [
      k,
      snapshotDir(dir, join(runDir, 'files', 'before', k)),
    ]),
  );

  const netlog = join(runDir, 'netlog.json');
  const config = {
    runDir,
    mode: opts.mode,
    layouts: opts.layouts,
    durationMs: opts.duration * 1000,
    maxLiveLoads: opts.maxLiveLoads,
    present: opts.present,
    webRequest: opts.webrequest,
    cdp: !opts['no-cdp'],
    quitStyle: opts['quit-style'],
    disableAnalytics: opts['disable-analytics'],
    packages: opts.packages ? opts.packages.split(',') : [],
    window: windowSize(opts.layouts),
    windowTitle: displayName(pkg),
    windowName: opts['window-name'],
  };
  const configPath = join(runDir, 'config.json');
  writeFileSync(configPath, JSON.stringify(config, null, 2) + '\n');
  const switches = [
    `--log-net-log=${netlog}`,
    '--net-log-capture-mode=Everything',
    '--use-mock-keychain',
    ...(opts.mode === 'test' ? ['--test-ad'] : []),
  ];
  const meta = {
    runId,
    startedAt: new Date().toISOString(),
    owElectron: owElectronVersion(),
    platform: process.platform,
    arch: process.arch,
    identitySource: identity.source,
    packageJson: pkg,
    expectedUid,
    home: opts.home,
    switches,
    options: opts,
  };
  writeJson(join(runDir, 'meta.json'), meta);

  if (!opts['no-wait']) await waitForQuietMachine();
  console.error(`run ${runId}: ${opts.mode} ads, ${opts.layouts.join(' ')}, ${opts.duration}s`);
  const exit = await launch({
    appDir,
    switches,
    env: { ...homeEnv, PARITY_HARNESS_CONFIG: configPath },
    logDir: runDir,
    timeoutMs: opts.duration * 1000 + 60_000,
  });

  const after = Object.fromEntries(
    Object.entries(watched).map(([k, dir]) => [
      k,
      snapshotDir(dir, join(runDir, 'files', 'after', k)),
    ]),
  );
  const fileDiff = Object.fromEntries(
    Object.keys(watched).map((k) => [k, diffSnapshots(before[k], after[k])]),
  );
  if (opts.home !== 'real') snapshotDir(home, join(runDir, 'files', 'home-listing'));

  let netlogSummary = null;
  if (existsSync(netlog)) {
    const requests = parseNetlog(readFileSync(netlog, 'utf8'));
    writeJson(join(runDir, 'netlog-requests.json'), requests);
    netlogSummary = summarize(requests);
  }
  const result = { ...meta, finishedAt: new Date().toISOString(), exit, fileDiff, netlogSummary };
  writeJson(join(runDir, 'meta.json'), result);
  console.log(JSON.stringify({ runDir, exit, netlogSummary }, null, 2));
}

function writeJson(path, value) {
  writeFileSync(path, JSON.stringify(value, null, 2) + '\n');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
