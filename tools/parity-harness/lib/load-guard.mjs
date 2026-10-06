// Waits while the machine is busy. The harness launches a full Chromium; on a
// small or fanless machine that should not pile onto a heavy build.

import { loadavg } from 'node:os';
import { setTimeout as sleep } from 'node:timers/promises';

/**
 * Resolves once the 1-minute load average is at or below `maxLoad`, checking
 * every `intervalMs`, or after `maxWaitMs` (then it logs and proceeds).
 * @param {{maxLoad?: number, intervalMs?: number, maxWaitMs?: number, log?: (s: string) => void}} [options]
 */
export async function waitForQuietMachine(options = {}) {
  const { maxLoad = 8, intervalMs = 60_000, maxWaitMs = 600_000, log = console.error } = options;
  if (process.platform === 'win32') return; // loadavg() is always 0 on Windows
  const started = Date.now();
  for (;;) {
    const [load] = loadavg();
    if (load <= maxLoad) return;
    if (Date.now() - started >= maxWaitMs) {
      log(`load ${load.toFixed(1)} still above ${maxLoad} after ${maxWaitMs / 1000}s; continuing`);
      return;
    }
    log(`load ${load.toFixed(1)} > ${maxLoad}; waiting ${intervalMs / 1000}s`);
    await sleep(intervalMs);
  }
}
