// Memory sampler for the macOS lab (`run.mjs --mem-sample-ms N`): every N ms
// it writes the app's processes and their physical footprint to
// proc-samples.jsonl.
//
// WebKit runs each WKWebView's web content, networking and GPU work in XPC
// service processes that launchd starts, so they are not in the app's
// process group; they are found by their responsible process
// (proc-owner.swift). An app started from a terminal is not responsible for
// itself (the terminal's app is), so its WebKit processes are the ones of
// that same responsible process that started after the launch. Electron's
// helpers are children of the app. Another WebKit app launched from the same
// terminal during the run would be counted too; mem-summary.mjs drops such
// groups (each brings its own GPU process).

import { spawnSync } from 'node:child_process';
import { appendFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const libDir = dirname(fileURLToPath(import.meta.url));

/** Parses `ps -axo pid=,ppid=,rss=,comm=` output. */
export function parsePs(text) {
  return text
    .split('\n')
    .map((l) => /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(.*)$/.exec(l))
    .filter(Boolean)
    .map((m) => ({
      pid: Number(m[1]),
      ppid: Number(m[2]),
      rssKb: Number(m[3]),
      name: m[4].split('/').pop(),
    }));
}

/**
 * Parses `top -l 1 -stats pid,mem` output into pid → footprint KiB (what
 * Activity Monitor shows as Memory). RSS alone drops whenever macOS
 * compresses pages, so a growth comparison reads the footprint.
 */
export function parseTopFootprints(text) {
  const out = new Map();
  const unit = { B: 1 / 1024, K: 1, M: 1024, G: 1024 * 1024 };
  for (const line of text.split('\n')) {
    const m = /^(\d+)\s+([\d.]+)([BKMG])[+-]?\s*$/.exec(line.trim());
    if (m) out.set(Number(m[1]), Math.round(Number(m[2]) * unit[m[3]]));
  }
  return out;
}

export const isWebKit = (p) => p.name.startsWith('com.apple.WebKit.');

/**
 * The processes that belong to `appPid`: itself, its children, processes it
 * is responsible for, and WebKit processes of its responsible process that
 * were not there at launch (`before`).
 */
export function ownedProcesses(ps, owners, appPid, before, appOwner) {
  return ps.filter(
    (p) =>
      p.pid === appPid ||
      p.ppid === appPid ||
      owners.get(p.pid) === appPid ||
      (isWebKit(p) &&
        !before.has(p.pid) &&
        appOwner !== undefined &&
        owners.get(p.pid) === appOwner),
  );
}

function procOwnerBinary(toolsDir) {
  const binary = join(toolsDir, 'proc-owner');
  if (existsSync(binary)) return binary;
  mkdirSync(toolsDir, { recursive: true });
  const r = spawnSync('swiftc', ['-O', join(libDir, 'proc-owner.swift'), '-o', binary], {
    stdio: 'inherit',
  });
  return r.status === 0 && existsSync(binary) ? binary : null;
}

const processList = () =>
  parsePs(spawnSync('ps', ['-axo', 'pid=,ppid=,rss=,comm='], { encoding: 'utf8' }).stdout ?? '');

/** The WebKit pids alive now (taken before the launch). */
export function webKitPidsNow() {
  return new Set(
    processList()
      .filter(isWebKit)
      .map((p) => p.pid),
  );
}

/**
 * Starts sampling `appPid` every `everyMs` into `file` (macOS only).
 * @returns {() => void} stops the sampler
 */
export function startProcSampler({ appPid, file, everyMs, before, toolsDir }) {
  if (process.platform !== 'darwin') return () => {};
  const probe = procOwnerBinary(toolsDir);
  if (!probe) return () => {};
  let appOwner;
  const sample = () => {
    const ps = processList();
    const owners = new Map(
      (
        spawnSync(
          probe,
          ps.map((p) => String(p.pid)),
          { encoding: 'utf8' },
        ).stdout ?? ''
      )
        .split('\n')
        .filter(Boolean)
        .map((l) => l.split(' ').map(Number)),
    );
    appOwner ??= owners.get(appPid);
    const procs = ownedProcesses(ps, owners, appPid, before, appOwner);
    if (!procs.length) return;
    const args = ['-l', '1', '-stats', 'pid,mem', ...procs.flatMap((p) => ['-pid', String(p.pid)])];
    const footprints = parseTopFootprints(
      spawnSync('top', args, { encoding: 'utf8' }).stdout ?? '',
    );
    for (const p of procs) p.footprintKb = footprints.get(p.pid) ?? null;
    const sum = (key) => procs.reduce((n, p) => n + (p[key] ?? 0), 0);
    appendFileSync(
      file,
      JSON.stringify({
        t: Date.now(),
        totalKb: sum('rssKb'),
        totalFootprintKb: sum('footprintKb'),
        procs,
      }) + '\n',
    );
  };
  const timer = setInterval(sample, everyMs);
  return () => clearInterval(timer);
}
