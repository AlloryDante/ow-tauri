// `run.mjs --host tauri`: launches the Tauri edition of the harness app
// (tauri-app/) in lab mode and turns the plugin's lab trace into the
// ow-electron capture schema, so analyze.mjs and parity-diff.mjs read both.

import { spawn, spawnSync } from 'node:child_process';
import {
  appendFileSync,
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const harnessDir = dirname(dirname(fileURLToPath(import.meta.url)));

/**
 * The ow-tauri version under test: the plugin crate's version (the
 * workspace's `[workspace.package] version`, which the crate inherits).
 */
export function owTauriVersion() {
  const read = (path) => readFileSync(join(harnessDir, '..', '..', ...path), 'utf8');
  const own = read(['crates', 'tauri-plugin-overwolf', 'Cargo.toml']).match(
    /^version\s*=\s*"([^"]+)"/m,
  );
  if (own) return own[1];
  const workspace = read(['Cargo.toml']).match(
    /^\[workspace\.package\][^[]*?^version\s*=\s*"([^"]+)"/ms,
  );
  return workspace ? workspace[1] : null;
}

/**
 * Environment of a Tauri run: home isolation (the plugin resolves appData
 * from $HOME, Cocoa from CFFIXED_USER_HOME), the lab switches and the
 * harness configuration. `invisible` (default) turns on the invisible lab
 * windows; only a CI runner's desktop (`run.mjs --ci-visible`) goes without.
 * @param {{home: string | null, runDir: string, configPath: string, packageJsonPath: string, mode: string, invisible?: boolean}} o
 */
export function tauriEnv({ home, runDir, configPath, packageJsonPath, mode, invisible = true }) {
  return {
    ...(home ? { HOME: home, CFFIXED_USER_HOME: home } : {}),
    OW_TAURI_LAB_DIR: runDir,
    ...(invisible ? { OW_TAURI_LAB_INVISIBLE: '1' } : {}),
    ...(mode === 'test' ? { OW_TAURI_TEST_AD: '1' } : {}),
    PARITY_HARNESS_CONFIG: configPath,
    PARITY_HARNESS_PACKAGE_JSON: packageJsonPath,
  };
}

/** Every process this module started and has not seen exit. */
const running = new Set();

function killTree(child, signal) {
  if (process.platform === 'win32') {
    // No process groups: end the app and every process it started
    // (WebView2's browser and renderer processes).
    spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], {
      stdio: 'ignore',
      windowsHide: true,
    });
    return;
  }
  try {
    // Detached: the child leads its own process group.
    process.kill(-child.pid, signal);
  } catch {
    try {
      child.kill(signal);
    } catch {
      // already gone
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

/** The console debugger of the Windows SDK, when the machine has it. */
export function windowsDebugger(env = process.env, exists = existsSync) {
  const roots = [env['ProgramFiles(x86)'], env.ProgramFiles].filter(Boolean);
  for (const root of roots) {
    const cdb = join(root, 'Windows Kits', '10', 'Debuggers', 'x64', 'cdb.exe');
    if (exists(cdb)) return cdb;
  }
  return null;
}

/**
 * Windows: before a timed-out app is killed, writes the call stack of each
 * of its threads to `file` (the SDK's cdb, attached non-invasively), so a
 * hang can be read from the run. Does nothing elsewhere or without cdb.
 */
function dumpStacks(pid, exe, file) {
  if (process.platform !== 'win32') return;
  const cdb = windowsDebugger();
  if (!cdb) return;
  const symbols = `${dirname(exe)};srv*${join(process.env.RUNNER_TEMP ?? dirname(exe), 'symbols')}*https://msdl.microsoft.com/download/symbols`;
  const r = spawnSync(cdb, ['-pv', '-p', String(pid), '-y', symbols, '-c', '~*kn 60; qd'], {
    encoding: 'utf8',
    timeout: 240_000,
    maxBuffer: 32 * 1024 * 1024,
    windowsHide: true,
  });
  writeFileSync(file, `${r.stdout ?? ''}\n${r.stderr ?? ''}${r.error ? `\n${r.error}` : ''}`);
}

/**
 * Safety net: polls the window monitor's file and calls `onVisible` the
 * first time it reports a visible window of the app.
 * @returns {() => void} stops the watch
 */
function watchVisibility(file, onVisible) {
  let offset = 0;
  let fired = false;
  const timer = setInterval(() => {
    if (fired || !existsSync(file)) return;
    const text = readFileSync(file, 'utf8');
    const fresh = text.slice(offset);
    const end = fresh.lastIndexOf('\n');
    if (end < 0) return;
    offset += end + 1;
    for (const line of fresh.slice(0, end).split('\n')) {
      let entry;
      try {
        entry = JSON.parse(line);
      } catch {
        continue;
      }
      if (entry.anyVisible === true || entry.everVisible === true) {
        fired = true;
        onVisible(entry);
        return;
      }
    }
  }, 100);
  return () => clearInterval(timer);
}

/**
 * Launches the harness binary and resolves with its exit status. The app is
 * always killed: at the timeout, when the window monitor sees one of its
 * windows (`monitorFile`), and when this process exits for any reason.
 * @param {{exe: string, env: Record<string, string>, logDir: string, timeoutMs: number, monitorFile?: string, onSpawn?: (child: import('node:child_process').ChildProcess) => void}} options
 */
export function launchTauri({ exe, env, logDir, timeoutMs, monitorFile, onSpawn }) {
  mkdirSync(logDir, { recursive: true });
  const started = Date.now();
  const child = spawn(exe, [], {
    env: { ...process.env, ...env },
    stdio: ['ignore', 'pipe', 'pipe'],
    // Its own process group (killTree); on Windows that would open a
    // console window, and taskkill /T ends the tree instead.
    detached: process.platform !== 'win32',
    windowsHide: true,
  });
  running.add(child);
  onSpawn?.(child);
  child.stdout.pipe(createWriteStream(join(logDir, 'stdout.log')));
  child.stderr.pipe(createWriteStream(join(logDir, 'stderr.log')));
  let safetyKill = null;
  const stopWatch = monitorFile
    ? watchVisibility(monitorFile, (entry) => {
        safetyKill = { at: new Date().toISOString(), monitorMs: entry.ms ?? null };
        killTree(child, 'SIGKILL');
        appendFileSync(
          join(logDir, 'events.jsonl'),
          JSON.stringify({ kind: 'safety-kill', reason: 'visible window', ...safetyKill }) + '\n',
        );
      })
    : () => {};
  return new Promise((resolve) => {
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      dumpStacks(child.pid, exe, join(logDir, 'hang-stacks.txt'));
      killTree(child, 'SIGTERM');
      setTimeout(() => killTree(child, 'SIGKILL'), 10_000).unref();
    }, timeoutMs);
    child.on('exit', (code, signal) => {
      clearTimeout(timer);
      stopWatch();
      // Anything left in its process group goes too.
      killTree(child, 'SIGKILL');
      running.delete(child);
      resolve({ code, signal, timedOut, safetyKill, ms: Date.now() - started });
    });
  });
}

function readJsonl(file) {
  if (!existsSync(file)) return [];
  return readFileSync(file, 'utf8')
    .split('\n')
    .filter(Boolean)
    .map((line) => {
      try {
        return JSON.parse(line);
      } catch {
        return null;
      }
    })
    .filter(Boolean);
}

/** `HTTP/2.0` (Rust `Debug` of the version) -> the net log's `h2`. */
function protocolName(version) {
  if (!version) return null;
  if (/2/.test(version)) return 'h2';
  if (/3/.test(version)) return 'h3';
  return 'http/1.1';
}

/**
 * HTTP/2 pseudo-headers in the order the Rust stack (the `h2` crate) puts
 * them on the wire: `:method`, `:scheme`, `:authority`, `:path`.
 */
function pseudoHeaders(method, url) {
  const u = new URL(url);
  return [
    `:method: ${method}`,
    `:scheme: ${u.protocol.replace(':', '')}`,
    `:authority: ${u.host}`,
    `:path: ${u.pathname}${u.search}`,
  ];
}

/**
 * Converts the lab trace's host requests and ad document navigations into
 * `netlog-requests.json` records (lib/netlog-parse.mjs shape; fields the lab
 * cannot see are null).
 * @param {string} runDir
 * @returns {Array<Record<string, unknown>>}
 */
export function labRequests(runDir) {
  const out = [];
  const ends = new Map();
  const host = readJsonl(join(runDir, 'host-requests.jsonl'));
  for (const r of host) if (r.phase === 'end') ends.set(r.id, r);
  for (const r of host) {
    if (r.phase !== 'start') continue;
    const end = ends.get(r.id) ?? {};
    const protocol = protocolName(end.protocol);
    const body = r.uploadBody ?? null;
    out.push({
      id: `host-${r.id}`,
      url: r.url,
      method: r.method,
      startedAt: new Date(r.wall).toISOString(),
      t: r.t,
      initiator: 'not an origin',
      requestType: 'other',
      protocol,
      sentHeaders: [...(protocol === 'h2' ? pseudoHeaders(r.method, r.url) : []), ...r.sentHeaders],
      cookiesSent: (r.cookiesSent ?? []).map((name) => ({ name })),
      cookiesStored: (end.setCookies ?? []).map((name) => ({ name })),
      uploadBody: body === null ? null : { text: body, bytes: Buffer.byteLength(body) },
      status: end.status === undefined ? null : `HTTP/1.1 ${end.status}`,
      responseHeaders: null,
      responseBody:
        end.responseBody === undefined
          ? null
          : { text: end.responseBody, bytes: Buffer.byteLength(end.responseBody) },
      netError: end.error ?? null,
      source: 'lab:host-requests',
    });
  }
  for (const r of readJsonl(join(runDir, 'shaped-requests.jsonl'))) {
    // Ad documents: the lab sees the fields the host sets on the load
    // request; WebKit adds the rest, which only a proxy could record.
    out.push({
      id: `doc-${out.length}`,
      url: r.url,
      method: r.method ?? 'GET',
      startedAt: new Date(r.wall).toISOString(),
      t: r.t,
      initiator: 'not an origin',
      requestType: 'main frame',
      protocol: null,
      sentHeaders: r.hostHeaders ?? [],
      hostHeadersOnly: true,
      via: r.via ?? null,
      label: r.label ?? null,
      cookiesSent: [],
      cookiesStored: [],
      uploadBody: null,
      status: null,
      responseHeaders: null,
      responseBody: null,
      netError: null,
      source: 'lab:shaped-requests',
    });
  }
  const wc = readJsonl(join(runDir, 'wc-events.jsonl'));
  // Consent window documents (the host loads them; no custom headers): every
  // web page a consent window starts loading. A window's `created` record
  // names the page it was created for, but on Windows that is still
  // `about:blank` when the window loads its page after creation (and no
  // navigation record need come first), so creation records only count in
  // captures without load records.
  const loads = wc.some((r) => r.kind === 'did-start-loading' && r.type === 'cmp');
  for (const r of wc) {
    if (r.type !== 'cmp' || !/^https?:/.test(r.url ?? '')) continue;
    if (r.kind !== (loads ? 'did-start-loading' : 'created')) continue;
    out.push({
      id: `cmp-${out.length}`,
      url: r.url,
      method: 'GET',
      startedAt: new Date(r.wall).toISOString(),
      t: r.t,
      initiator: 'not an origin',
      requestType: 'main frame',
      protocol: null,
      sentHeaders: [],
      hostHeadersOnly: true,
      label: r.label ?? null,
      cookiesSent: [],
      cookiesStored: [],
      uploadBody: null,
      status: null,
      responseHeaders: null,
      responseBody: null,
      netError: null,
      source: 'lab:wc-events',
    });
  }
  out.sort((a, b) => a.t - b.t);
  return out;
}

/** Writes netlog-requests.json from the lab trace and returns it. */
export function writeLabRequests(runDir) {
  const requests = labRequests(runDir);
  writeFileSync(join(runDir, 'netlog-requests.json'), JSON.stringify(requests, null, 2) + '\n');
  return requests;
}
