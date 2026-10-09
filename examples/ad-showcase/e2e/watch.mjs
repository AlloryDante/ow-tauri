// Pure helpers of the lab runner (e2e/run.mjs): the stall watchdog's report
// and the restart check's verdict. Unit-tested in watch.test.mjs.

/** The restart check's phases, in order (`restartPhaseOf` in steps.cjs). */
export const RESTART_PHASES = ['first', 'second', 'third'];

/** What each restart check phase must show: its ad mode and the route it starts on. */
const EXPECTED = {
  first: { mode: 'test', route: '#parity' },
  second: { mode: 'live', route: '#parity/restarted' },
  third: { mode: 'test', route: '#parity/restarted-again' },
};

/**
 * What the driver last finished, for a stalled run's report: its last step,
 * still or action, else its last record's kind (heartbeats and visibility
 * changes are not progress).
 *
 * @param {Record<string, unknown>[]} records the run's e2e.jsonl records
 * @returns {string | null}
 */
export function lastProgress(records) {
  const named = records.filter(
    (r) =>
      ((r.kind === 'step' || r.kind === 'still') && typeof r.name === 'string') ||
      (r.kind === 'action' && typeof r.action === 'string'),
  );
  const last = named.at(-1);
  if (last) return `${String(last.kind)} ${String(last.name ?? last.action)}`;
  const any = records.filter((r) => r.kind !== 'beat' && r.kind !== 'visibility').at(-1);
  return any ? String(any.kind) : null;
}

/**
 * Whether a run has stalled: its driver has not finished and has recorded
 * nothing (not even a heartbeat) for `stallMs`.
 *
 * @param {{ now: number, lastChange: number, stallMs: number, finished: boolean }} state
 * @returns {boolean}
 */
export function isStalled({ now, lastChange, stallMs, finished }) {
  return !finished && stallMs > 0 && now - lastChange > stallMs;
}

/**
 * The restart check's verdict from the driver records of the three
 * processes (`restart-<phase>` steps) and, per later phase, whether the
 * previous phase's process had exited once it reported (`gone`).
 *
 * @param {Record<string, unknown>[]} records
 * @param {Record<string, boolean>} gone phase -> the previous process was gone
 * @returns {{ ok: boolean, phases: Record<string, unknown>[], why: string[] }}
 */
export function restartVerdict(records, gone) {
  const steps = RESTART_PHASES.map(
    (phase) => records.find((r) => r.kind === 'step' && r.name === `restart-${phase}`) ?? null,
  );
  const why = [];
  steps.forEach((step, i) => {
    const phase = RESTART_PHASES[i];
    if (!step) {
      why.push(`phase ${phase} is missing`);
      return;
    }
    const mode = step.snapshot?.mode ?? step.mode;
    if (mode !== EXPECTED[phase].mode) why.push(`phase ${phase} is not ${EXPECTED[phase].mode}`);
    if (step.route !== EXPECTED[phase].route) why.push(`phase ${phase} is not on its page`);
  });
  const pids = steps.map((s) => s?.pid);
  if (pids.some((p) => typeof p !== 'number') || new Set(pids).size !== 3)
    why.push('the three phases did not run in three processes');
  for (const phase of ['second', 'third']) {
    if (gone[phase] !== true) why.push(`the process before phase ${phase} outlived it`);
  }
  if (!records.some((r) => r.kind === 'done' && r.restarted === true))
    why.push('phase third never finished');
  return {
    ok: why.length === 0,
    phases: steps.filter(Boolean).map((s) => ({
      name: s.name,
      pid: s.pid,
      mode: s.snapshot?.mode ?? s.mode,
      route: s.route,
    })),
    why,
  };
}
