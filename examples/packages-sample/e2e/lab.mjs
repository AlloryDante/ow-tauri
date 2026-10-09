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

/** The lab build's bundle id for the quickstart page (page-host mode). */
export const QUICKSTART_LAB_IDENTIFIER = 'dev.ow-tauri.quickstart-host.lab';

/**
 * The `TAURI_CONFIG` of page-host mode: this shell built around the
 * quickstart's page with the quickstart's product name, page security and
 * `plugins.overwolf` (so the plugin reports the quickstart's identity), under
 * a lab bundle id. `analytics` of the sample's own plugin configuration is
 * removed (`null` deletes a key in Tauri's config merge).
 *
 * @param {Record<string, any>} conf the quickstart's tauri.conf.json
 * @param {string} distDir absolute path of the quickstart's page build
 * @returns {Record<string, unknown>}
 */
export function quickstartTauriConfig(conf, distDir) {
  return {
    identifier: QUICKSTART_LAB_IDENTIFIER,
    productName: conf.productName,
    version: conf.version,
    build: { devUrl: null, frontendDist: distDir },
    app: { security: { csp: conf.app?.security?.csp ?? null } },
    plugins: { overwolf: { analytics: null, ...conf.plugins?.overwolf } },
  };
}

/**
 * The quickstart window's size, `<width>x<height>`, from its tauri.conf.json.
 *
 * @param {Record<string, any>} conf
 * @returns {string}
 */
export function quickstartWindow(conf) {
  const main = (conf.app?.windows ?? []).find((w) => (w.label ?? 'main') === 'main') ?? {};
  return `${main.width ?? 800}x${main.height ?? 600}`;
}

/**
 * How a run ended.
 *
 * @param {{ safetyKill: boolean, done: boolean, timedOut: boolean, stalled?: boolean }} state
 * @returns {'safety-kill' | 'done' | 'stalled' | 'timeout' | 'exited-early'}
 */
export function verdictOf({ safetyKill, done, timedOut, stalled = false }) {
  if (safetyKill) return 'safety-kill';
  if (done) return 'done';
  if (stalled) return 'stalled';
  return timedOut ? 'timeout' : 'exited-early';
}

/**
 * What the driver last finished, for a stalled run's report: the name of its
 * last step, still or restart record, else its last record's kind.
 *
 * @param {Record<string, unknown>[]} records the run's e2e.jsonl records
 * @returns {string | null}
 */
export function lastProgress(records) {
  const named = records.filter(
    (r) => (r.kind === 'step' || r.kind === 'still') && typeof r.name === 'string',
  );
  const last = named.at(-1);
  if (last) return `${String(last.kind)} ${String(last.name)}`;
  const any = records.filter((r) => r.kind !== 'beat' && r.kind !== 'visibility').at(-1);
  return any ? String(any.kind) : null;
}

/**
 * The restart check's result from the driver records of every process of
 * the run (`restart-<n>` steps) and whether each previous process was gone
 * once the next one reported (`gone`, by phase).
 *
 * @param {Record<string, unknown>[]} records
 * @param {Record<number, boolean>} gone phase n -> process of phase n - 1 had exited
 * @returns {{ ok: boolean, phases: Record<string, unknown>[], why: string[] }}
 */
export function restartResult(records, gone) {
  const phases = [1, 2, 3].map(
    (n) => records.find((r) => r.kind === 'step' && r.name === `restart-${n}`) ?? null,
  );
  const why = [];
  const [p1, p2, p3] = phases;
  if (!p1 || !p2 || !p3) why.push('a phase is missing');
  const pids = phases.map((p) => p?.pid);
  if (new Set(pids).size !== 3 || pids.some((p) => typeof p !== 'number'))
    why.push('the three phases did not run in three processes');
  if (p1 && p1.mode !== 'test') why.push('phase 1 is not TEST');
  if (p2 && p2.mode !== 'live') why.push('phase 2 is not LIVE');
  if (p3 && p3.mode !== 'test') why.push('phase 3 is not TEST');
  for (const p of [p2, p3]) {
    if (p && p.startHash !== '#settings') why.push(`phase ${String(p.phase)} lost the page`);
  }
  for (const n of [2, 3]) if (gone[n] !== true) why.push(`phase ${n - 1}'s process outlived it`);
  if (!records.some((r) => r.kind === 'done' && r.restarted === true))
    why.push('phase 3 never finished');
  return { ok: why.length === 0, phases: phases.filter(Boolean), why };
}

/** The guest page events that mean an ad has loaded: a display ad, or a video ad's player. */
const AD_LOADED = new Set(['display_ad_loaded', 'player_loaded', 'impression']);

/**
 * Whether an ad guest reported a loaded ad, from the plugin's lab IPC log
 * (`ipc.jsonl` records): `display_ad_loaded` for a display ad, or
 * `player_loaded` / `impression` for a video ad (the quickstart's 400x300
 * slot may serve either).
 *
 * @param {Record<string, unknown>[]} ipc
 * @returns {boolean}
 */
export function adLoaded(ipc) {
  return ipc.some(
    (r) => r.dir === 'page->host' && typeof r.channel === 'string' && AD_LOADED.has(r.channel),
  );
}

/**
 * Whether a run passed: it finished, no window was ever visible, the app
 * was never frontmost, nothing failed, no process it owned is left, and
 * its steps' own check holds (`check`: a test ad loaded for smoke, every
 * still for tour, all three phases for restart, the still with a loaded ad
 * for the quickstart page).
 *
 * @param {{ verdict: string, everVisible: boolean | null, everFront: boolean,
 *   check: boolean, fatal: string[], leftProcesses: unknown[] }} summary
 * @returns {boolean}
 */
export function passed(summary) {
  return (
    summary.verdict === 'done' &&
    summary.everVisible === false &&
    !summary.everFront &&
    summary.check === true &&
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
