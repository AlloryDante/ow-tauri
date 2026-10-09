// Pure helpers of the invisible-lab runner (e2e/run.mjs): the lab build's
// Tauri config, the run verdict and its pass check, and the loopback sink's
// summary. Kept apart so they are unit tested (lab.test.mjs).

/** The lab build's bundle id: its data and single-instance socket never mix with a normal build's. */
export const LAB_IDENTIFIER = 'dev.ow-tauri.packages-sample.lab';

/**
 * The `TAURI_CONFIG` of the lab build: the lab bundle id and the lab page
 * (`vite build --mode lab`, with the driver), embedded in the binary. The
 * dev server URL is removed (`null` deletes a key in Tauri's config merge),
 * so the debug build loads its embedded page instead of `tauri dev`'s
 * server.
 *
 * @param {string} distDir absolute path of the lab page build
 * @returns {Record<string, unknown>}
 */
export function labTauriConfig(distDir) {
  return {
    identifier: LAB_IDENTIFIER,
    build: { devUrl: null, frontendDist: distDir },
  };
}

/**
 * How a run ended.
 *
 * @param {{ safetyKill: boolean, done: boolean, timedOut: boolean }} state
 * @returns {'safety-kill' | 'done' | 'timeout' | 'exited-early'}
 */
export function verdictOf({ safetyKill, done, timedOut }) {
  if (safetyKill) return 'safety-kill';
  if (done) return 'done';
  return timedOut ? 'timeout' : 'exited-early';
}

/**
 * Whether a smoke run passed: it finished, no window was ever visible, the
 * app was never frontmost, it loaded a test ad, nothing failed and no
 * process it owned is left.
 *
 * @param {{ verdict: string, everVisible: boolean | null, everFront: boolean,
 *   displayAdLoaded: boolean, fatal: string[], leftProcesses: unknown[] }} summary
 * @returns {boolean}
 */
export function passed(summary) {
  return (
    summary.verdict === 'done' &&
    summary.everVisible === false &&
    !summary.everFront &&
    summary.displayAdLoaded &&
    summary.fatal.length === 0 &&
    summary.leftProcesses.length === 0
  );
}

/**
 * Requests the loopback sink received, counted by path without the query.
 *
 * @param {{ path?: string }[]} lines the sink's records
 * @returns {Record<string, number>}
 */
export function sinkSummary(lines) {
  /** @type {Record<string, number>} */
  const out = {};
  for (const { path } of lines) {
    const key = (path ?? '').split('?')[0] || '/';
    out[key] = (out[key] ?? 0) + 1;
  }
  return out;
}

/**
 * The records of a JSON-lines text; broken lines are skipped.
 *
 * @param {string} text
 * @returns {Record<string, unknown>[]}
 */
export function parseJsonl(text) {
  return text
    .split('\n')
    .filter(Boolean)
    .flatMap((line) => {
      try {
        return [JSON.parse(line)];
      } catch {
        return [];
      }
    });
}
