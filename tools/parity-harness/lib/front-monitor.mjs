// macOS: proves the harness app never became the frontmost app. An
// invisible lab app that is activated would take the keyboard from the app
// the user is typing in, even at alpha 0. The front app is read from Launch
// Services (`lsappinfo front`), which answers from the system's own record
// and needs no run loop in this process.
//
// Writes `front-monitor.jsonl` into the run: one line per change of the
// front app ({kind: 'front', t, wall, pid, isApp}) and an end line
// ({kind: 'end', samples, appFrontSamples, everFront}).

import { spawnSync } from 'node:child_process';
import { appendFileSync } from 'node:fs';

/** The pid of the frontmost app (the one that has the keyboard), or null. */
export function frontPid() {
  const asn = spawnSync('lsappinfo', ['front'], { encoding: 'utf8' }).stdout?.trim();
  if (!asn) return null;
  const info = spawnSync('lsappinfo', ['info', '-only', 'pid', asn], { encoding: 'utf8' }).stdout;
  const m = /"pid"\s*=\s*(\d+)/.exec(info ?? '');
  return m ? Number(m[1]) : null;
}

/**
 * Starts sampling the front app every `everyMs` while `pid` runs. Returns a
 * function that stops sampling, writes the end line and returns it. Does
 * nothing (and returns `null` from the stop function) off macOS.
 * @param {number} pid the harness app's pid
 * @param {string} file where to write the JSON lines
 * @param {{everyMs?: number, readFront?: () => number | null, now?: () => number}} [options]
 * @returns {() => {kind: 'end', samples: number, appFrontSamples: number, everFront: boolean} | null}
 */
export function watchFront(
  pid,
  file,
  { everyMs = 500, readFront = frontPid, now = Date.now } = {},
) {
  if (process.platform !== 'darwin' && readFront === frontPid) return () => null;
  const started = now();
  let samples = 0;
  let appFront = 0;
  let last;
  const sample = () => {
    const front = readFront();
    samples += 1;
    if (front === pid) appFront += 1;
    if (front !== last) {
      last = front;
      appendFileSync(
        file,
        JSON.stringify({
          kind: 'front',
          t: now() - started,
          wall: now(),
          pid: front,
          isApp: front === pid,
        }) + '\n',
      );
    }
  };
  sample();
  const timer = setInterval(sample, everyMs);
  return () => {
    clearInterval(timer);
    const end = { kind: 'end', samples, appFrontSamples: appFront, everFront: appFront > 0 };
    appendFileSync(file, JSON.stringify(end) + '\n');
    return end;
  };
}
