#!/usr/bin/env node
// Parity harness CLI: runs a hidden ow-electron app that shows <owadview> ads
// and records what ow-electron does. See README.md. With --host tauri it runs
// the same scenario on ow-tauri (tauri-app/, plugin lab mode) instead.
//
//   node run.mjs --mode test --layout 400x600,728x90 --duration 90
//   node run.mjs --mode live --live-ok --duration 90 --max-live-loads 10
//   node run.mjs --host tauri --scenario messages
//
// Output: captures/<run-id>/ (git-ignored).

import { spawn, spawnSync } from 'node:child_process';
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
import { FEATURE_PRESETS, SCENARIOS } from './lib/scenarios.mjs';
import { launchTauri, owTauriVersion, tauriEnv, writeLabRequests } from './lib/tauri-host.mjs';
import { buildTauriApp } from './tauri-app/build.mjs';
import { electronUid } from './lib/uid.mjs';

const harnessDir = dirname(fileURLToPath(import.meta.url));

/** Slot sizes used by Overwolf's official sample. */
const DEFAULT_LAYOUTS = ['400x600', '400x60', '160x600', '300x250', '728x90', '400x300'];
const MAX_DURATION_S = 1800;
const MAX_LONG_DURATION_S = 14 * 3600;

/** Built-in defaults; a --scenario preset overrides them, explicit options override both. */
const DEFAULTS = {
  mode: 'test',
  duration: '90',
  'max-live-loads': '10',
  present: 'hidden',
  home: 'isolated',
  packages: '',
  'quit-style': 'close-then-quit',
};

const USAGE = `Usage: node run.mjs [options]

  --host electron|tauri   the host under test (default: electron). tauri builds
                          tauri-app/ once in debug and runs it in lab mode: every
                          window invisible, the window monitor always on
  --no-build              --host tauri: run the binary built last time
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
  --scenario NAME         round-2 preset with a timed action script (see README):
                          ${Object.keys(SCENARIOS).join(', ')}
  --features PRESET|JSON  answer the consent feature-flag request from a local stand-in:
                          ${Object.keys(FEATURE_PRESETS).join(', ')},
                          or a JSON array of {status, body, delayMs}
  --offline               send nothing to the internet (--proxy-server=127.0.0.1:9;
                          loopback, used by --features, stays reachable)
  --offline-allow H1,H2   with --offline: hosts that still go direct (for example
                          content.overwolf.com so the consent page loads)
  --position X,Y          move the ad window there after it is shown
  --overwolf-uid UID      set package.json overwolf.uid
  --window-monitor        macOS: record this app's windows (CGWindowList) in
                          window-monitor.jsonl to prove none became visible
  --allow-long            allow --duration up to ${MAX_LONG_DURATION_S} s (long scenario)
  --caffeinate            macOS: hold an idle-sleep assertion while the app runs
  --screencapture         macOS: let 'screencapture' actions capture the main display
                          (may raise a system screen-recording prompt)
  --run-id ID             capture folder name (default: timestamp + mode)
  --no-wait               do not wait for a quiet machine before launching
  --help`;

function parseCli() {
  const { values } = parseArgs({
    options: {
      mode: { type: 'string' },
      'live-ok': { type: 'boolean', default: false },
      layout: { type: 'string' },
      duration: { type: 'string' },
      'max-live-loads': { type: 'string' },
      present: { type: 'string' },
      home: { type: 'string' },
      packages: { type: 'string' },
      webrequest: { type: 'boolean', default: false },
      'no-cdp': { type: 'boolean', default: false },
      'quit-style': { type: 'string' },
      scenario: { type: 'string' },
      features: { type: 'string' },
      offline: { type: 'boolean' },
      'offline-allow': { type: 'string' },
      position: { type: 'string' },
      'overwolf-uid': { type: 'string' },
      'window-monitor': { type: 'boolean', default: false },
      'allow-long': { type: 'boolean', default: false },
      caffeinate: { type: 'boolean', default: false },
      screencapture: { type: 'boolean', default: false },
      'disable-analytics': { type: 'boolean', default: false },
      'window-name': { type: 'string' },
      identity: { type: 'string' },
      'run-id': { type: 'string' },
      'no-wait': { type: 'boolean', default: false },
      host: { type: 'string', default: 'electron' },
      'no-build': { type: 'boolean', default: false },
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
  let scenario = null;
  if (values.scenario) {
    scenario = SCENARIOS[values.scenario];
    if (!scenario) fail(`unknown --scenario ${values.scenario}`);
    const preset = { ...scenario.defaults };
    if (preset.quitStyle) preset['quit-style'] = preset.quitStyle;
    delete preset.quitStyle;
    for (const [key, value] of Object.entries(preset)) {
      if (values[key] === undefined)
        values[key] = typeof value === 'number' ? String(value) : value;
    }
  }
  for (const [key, value] of Object.entries(DEFAULTS)) {
    if (values[key] === undefined) values[key] = value;
  }
  if (!['test', 'live'].includes(values.mode)) fail(`--mode must be test or live`);
  if (!['electron', 'tauri'].includes(values.host)) fail('--host must be electron or tauri');
  if (values.host === 'tauri') {
    // Electron switches and hooks with no Tauri counterpart.
    for (const option of ['offline', 'features', 'webrequest', 'screencapture']) {
      if (values[option]) fail(`--${option} is not available with --host tauri`);
    }
    if (process.platform !== 'darwin') fail('--host tauri runs on macOS only (window monitor)');
  }
  if (values.mode === 'live' && !values['live-ok']) fail('--mode live needs --live-ok');
  if (!['hidden', 'transparent'].includes(values.present))
    fail('--present must be hidden or transparent');
  if (!['close-then-quit', 'quit'].includes(values['quit-style'])) fail('bad --quit-style');
  const duration = Number(values.duration);
  const maxDuration = values['allow-long'] ? MAX_LONG_DURATION_S : MAX_DURATION_S;
  if (!(duration > 0 && duration <= maxDuration)) fail(`--duration must be 1..${maxDuration}`);
  const layouts =
    values.layout === 'none' ? [] : values.layout ? values.layout.split(',') : DEFAULT_LAYOUTS;
  for (const layout of layouts) if (!/^\d+x\d+$/.test(layout)) fail(`bad layout ${layout}`);
  let features = null;
  if (values.features) {
    features = FEATURE_PRESETS[values.features] ?? null;
    if (!features) {
      try {
        features = JSON.parse(values.features);
      } catch {
        fail(`--features must be a preset or a JSON array`);
      }
    }
  }
  let position = null;
  if (values.position) {
    position = values.position.split(',').map(Number);
    if (position.length !== 2 || position.some((n) => !Number.isFinite(n))) fail('bad --position');
  }
  const maxLiveLoads = Number(values['max-live-loads']);
  if (!(maxLiveLoads >= 1 && maxLiveLoads <= 50)) fail('--max-live-loads must be 1..50');
  return { ...values, duration, layouts, maxLiveLoads, scenarioDef: scenario, features, position };
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

function resolveHome(option, runDir, host) {
  if (option === 'real') return { home: homedir(), env: {} };
  const home =
    option === 'isolated'
      ? join(runDir, 'home')
      : option.startsWith('profile:')
        ? join(harnessDir, 'captures', 'profiles', option.slice('profile:'.length))
        : null;
  if (!home) throw new Error(`bad --home ${option}`);
  // The plugin resolves appData from $HOME (dirs); Cocoa reads CFFIXED_USER_HOME.
  const env = host === 'tauri' ? { HOME: home, CFFIXED_USER_HOME: home } : isolationEnv(home);
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
  const tauri = opts.host === 'tauri';
  const { home, env: homeEnv } = resolveHome(opts.home, runDir, opts.host);
  // Build before anything is recorded, so a failed build leaves no run.
  const tauriExe = tauri
    ? opts['no-build']
      ? join(harnessDir, 'tauri-app', 'src-tauri', 'target', 'debug', 'ow-tauri-parity-harness')
      : await buildTauriApp()
    : null;

  const pkg = {
    name: identity.name,
    productName: identity.productName,
    version: identity.version,
    ...(identity.author !== undefined ? { author: identity.author } : {}),
    ...(identity.build ? { build: identity.build } : {}),
    ...(opts.packages || opts['overwolf-uid']
      ? {
          overwolf: {
            ...(opts.packages ? { packages: opts.packages.split(',') } : {}),
            ...(opts['overwolf-uid'] ? { uid: opts['overwolf-uid'] } : {}),
          },
        }
      : {}),
  };
  const appDir = join(runDir, 'app');
  if (tauri) {
    mkdirSync(appDir, { recursive: true });
    writeJson(join(appDir, 'package.json'), pkg);
  } else {
    makeAppDir(appDir, pkg);
  }

  const authorName = typeof pkg.author === 'object' ? pkg.author.name : pkg.author;
  const expectedUid =
    opts['overwolf-uid'] ?? identity.uid ?? electronUid(authorName ?? '', displayName(pkg));
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
  const scenarioConfig = opts.scenarioDef?.config ?? {};
  const wantMonitor = opts['window-monitor'] || scenarioConfig.windowMonitorRequired || tauri;
  const windowMonitor = wantMonitor ? buildWindowMonitor() : null;
  if ((scenarioConfig.windowMonitorRequired || tauri) && !windowMonitor) {
    console.error('this scenario needs the window monitor (macOS + swiftc); not running it');
    process.exit(3);
  }
  const config = {
    ...scenarioConfig,
    scenario: opts.scenario ?? null,
    features: opts.features ? { match: 'experiments/cmp-eu-only', responses: opts.features } : null,
    windowPosition: opts.position ?? scenarioConfig.windowPosition ?? null,
    windowMonitor,
    screencapture: opts.screencapture,
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
    window: scenarioConfig.window ?? windowSize(opts.layouts),
    windowTitle: displayName(pkg),
    windowName: opts['window-name'],
    host: opts.host,
  };
  const configPath = join(runDir, 'config.json');
  writeFileSync(configPath, JSON.stringify(config, null, 2) + '\n');
  const switches = tauri
    ? []
    : [
        `--log-net-log=${netlog}`,
        '--net-log-capture-mode=Everything',
        '--use-mock-keychain',
        ...(opts.mode === 'test' ? ['--test-ad'] : []),
        ...(opts.offline ? ['--proxy-server=127.0.0.1:9'] : []),
        ...(opts.offline && opts['offline-allow']
          ? [`--proxy-bypass-list=${opts['offline-allow'].split(',').join(';')}`]
          : []),
      ];
  const meta = {
    runId,
    startedAt: new Date().toISOString(),
    host: opts.host,
    ...(tauri ? { owTauri: owTauriVersion(), exe: tauriExe } : { owElectron: owElectronVersion() }),
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
  console.error(
    `run ${runId} (${opts.host}): ${opts.mode} ads, ${opts.layouts.join(' ')}, ${opts.duration}s`,
  );
  const onSpawn = (child) => {
    writeFileSync(join(runDir, 'app.pid'), `${child.pid}\n`);
    if (windowMonitor) {
      // Started with the app so the very first window is covered.
      spawn(windowMonitor, [String(child.pid), join(runDir, 'window-monitor.jsonl'), '25'], {
        stdio: 'ignore',
      }).unref();
    }
    if (opts.caffeinate && process.platform === 'darwin') {
      // Idle-sleep assertion that ends with the app (-w).
      spawn('/usr/bin/caffeinate', ['-i', '-w', String(child.pid)], {
        stdio: 'ignore',
        detached: true,
      }).unref();
    }
  };
  const exit = tauri
    ? await launchTauri({
        exe: tauriExe,
        env: tauriEnv({
          home: opts.home === 'real' ? null : home,
          runDir,
          configPath,
          packageJsonPath: join(appDir, 'package.json'),
          mode: opts.mode,
        }),
        logDir: runDir,
        timeoutMs: opts.duration * 1000 + 60_000,
        monitorFile: join(runDir, 'window-monitor.jsonl'),
        onSpawn,
      })
    : await launch({
        appDir,
        switches,
        env: { ...homeEnv, PARITY_HARNESS_CONFIG: configPath },
        logDir: runDir,
        timeoutMs: opts.duration * 1000 + 60_000,
        onSpawn,
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
  if (tauri) {
    // The plugin's lab trace stands in for the net log.
    netlogSummary = summarize(writeLabRequests(runDir));
  } else if (existsSync(netlog)) {
    const requests = parseNetlog(readFileSync(netlog, 'utf8'));
    writeJson(join(runDir, 'netlog-requests.json'), requests);
    netlogSummary = summarize(requests);
  }
  const result = { ...meta, finishedAt: new Date().toISOString(), exit, fileDiff, netlogSummary };
  writeJson(join(runDir, 'meta.json'), result);
  console.log(JSON.stringify({ runDir, exit, netlogSummary }, null, 2));
}

/**
 * Compiles lib/window-monitor.swift once (macOS) and returns the binary path,
 * or null when it cannot be built.
 */
function buildWindowMonitor() {
  if (process.platform !== 'darwin') return null;
  const source = join(harnessDir, 'lib', 'window-monitor.swift');
  const binary = join(harnessDir, 'captures', '.tools', 'window-monitor');
  if (existsSync(binary)) return binary;
  mkdirSync(dirname(binary), { recursive: true });
  const result = spawnSync('swiftc', ['-O', source, '-o', binary], { stdio: 'inherit' });
  return result.status === 0 && existsSync(binary) ? binary : null;
}

function writeJson(path, value) {
  writeFileSync(path, JSON.stringify(value, null, 2) + '\n');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
