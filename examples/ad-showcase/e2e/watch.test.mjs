import { describe, expect, it } from 'vitest';

import { isStalled, lastProgress, restartVerdict } from './watch.mjs';

describe('lastProgress', () => {
  it('names the last step, still or action', () => {
    expect(
      lastProgress([
        { kind: 'step', name: 'started' },
        { kind: 'action', page: 'sizes', action: 'nav-sizes' },
        { kind: 'beat' },
      ]),
    ).toBe('action nav-sizes');
    expect(lastProgress([{ kind: 'still', name: 'p6-house' }, { kind: 'visibility' }])).toBe(
      'still p6-house',
    );
  });

  it('falls back to the last record that is not a heartbeat', () => {
    expect(lastProgress([{ kind: 'driver' }, { kind: 'beat' }])).toBe('driver');
    expect(lastProgress([{ kind: 'beat' }])).toBeNull();
    expect(lastProgress([])).toBeNull();
  });
});

describe('isStalled', () => {
  const base = { now: 100_000, lastChange: 30_000, stallMs: 60_000, finished: false };
  it('fires only after stallMs of silence', () => {
    expect(isStalled(base)).toBe(true);
    expect(isStalled({ ...base, lastChange: 50_000 })).toBe(false);
  });
  it('never fires once the driver finished or when off', () => {
    expect(isStalled({ ...base, finished: true })).toBe(false);
    expect(isStalled({ ...base, stallMs: 0 })).toBe(false);
  });
});

describe('restartVerdict', () => {
  const phase = (name, pid, mode, route) => ({
    kind: 'step',
    name: `restart-${name}`,
    pid,
    route,
    snapshot: { mode },
  });
  const good = [
    phase('first', 1, 'test', '#parity'),
    phase('second', 2, 'live', '#parity/restarted'),
    phase('third', 3, 'test', '#parity/restarted-again'),
    { kind: 'done', restarted: true },
  ];
  const gone = { second: true, third: true };

  it('passes TEST -> LIVE -> TEST in three processes', () => {
    const v = restartVerdict(good, gone);
    expect(v.why).toEqual([]);
    expect(v.ok).toBe(true);
    expect(v.phases.map((p) => p.mode)).toEqual(['test', 'live', 'test']);
  });

  it('fails a wrong mode, a lost page, a reused pid and a survivor', () => {
    const bad = [
      phase('first', 1, 'test', '#parity'),
      phase('second', 1, 'test', '#parity'),
      phase('third', 3, 'test', '#parity/restarted-again'),
      { kind: 'done', restarted: true },
    ];
    const v = restartVerdict(bad, { second: true, third: false });
    expect(v.ok).toBe(false);
    expect(v.why).toEqual([
      'phase second is not live',
      'phase second is not on its page',
      'the three phases did not run in three processes',
      'the process before phase third outlived it',
    ]);
  });

  it('fails a missing phase and an unfinished run', () => {
    const v = restartVerdict(good.slice(0, 2), gone);
    expect(v.why).toContain('phase third is missing');
    expect(v.why).toContain('phase third never finished');
  });
});
