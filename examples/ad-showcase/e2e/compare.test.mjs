import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import { cidNamer, compareRuns, compareTotals, loadRun } from './compare.mjs';

function run(host, steps) {
  const dir = mkdtempSync(join(tmpdir(), 'showcase-compare-'));
  writeFileSync(join(dir, 'summary.json'), JSON.stringify({ host }));
  writeFileSync(join(dir, 'e2e.jsonl'), steps.map((s) => JSON.stringify(s)).join('\n') + '\n');
  return dir;
}

const ev = (cid, name) => ({ cid, name });

describe('cidNamer', () => {
  it('drops per-creation counters and keeps the other cids', () => {
    const name = cidNamer();
    expect(name('ly7-0-300x250')).toBe('ly-0-300x250');
    expect(name('ly12-1-400x600')).toBe('ly-1-400x600');
    expect(name('sz3-fold-300x250')).toBe('sz-fold-300x250');
    expect(name('perf-12')).toBe('perf');
    expect(name('rw-400x300')).toBe('rw-400x300');
  });
});

describe('compareRuns', () => {
  it('matches slots across hosts and classifies event rows', () => {
    const a = run('tauri', [
      {
        kind: 'step',
        name: 'layouts-tower',
        snapshot: {
          owadviews: 2,
          slots: [{ cid: 'ly3-0-728x90', status: 'loaded' }],
          state: { layout: 'tower', zone: [1, 2] },
        },
        events: [
          ev('ly3-0-728x90', 'display_ad_loaded'),
          ev('ly3-0-728x90', 'display_ad_loaded'),
          ev('app', 'control:layout'),
        ],
        filled: { ok: true, ms: 4000 },
      },
    ]);
    const b = run('electron', [
      {
        kind: 'step',
        name: 'layouts-tower',
        snapshot: {
          owadviews: 2,
          slots: [{ cid: 'ly9-0-728x90', status: 'loaded' }],
          state: { layout: 'tower', zone: [3, 4] },
        },
        events: [
          ev('ly9-0-728x90', 'display_ad_loaded'),
          ev('ly9-0-728x90', 'display_ad_loaded'),
          ev('ly9-0-728x90', 'did-fail-load'),
        ],
        filled: { ok: true, ms: 5000 },
      },
    ]);
    const rows = compareRuns(loadRun(a), loadRun(b));
    const byWhat = Object.fromEntries(rows.map((r) => [r.what, r.verdict]));
    expect(byWhat['events ly-0-728x90']).toBe('lifecycle');
    expect(byWhat['status ly-0-728x90']).toBe('same');
    expect(byWhat['state layout']).toBe('same');
    expect(byWhat['state zone']).toBeUndefined();
    expect(byWhat['check filled']).toBe('same');
  });

  it('calls equal names with other counts a count difference and flags a missing step', () => {
    const a = run('tauri', [
      { kind: 'step', name: 's', snapshot: {}, events: [ev('x', 'play'), ev('x', 'play')] },
      { kind: 'step', name: 'only-a', snapshot: {}, events: [] },
    ]);
    const b = run('electron', [
      { kind: 'step', name: 's', snapshot: {}, events: [ev('x', 'play')] },
    ]);
    const rows = compareRuns(loadRun(a), loadRun(b));
    expect(rows.find((r) => r.what === 'events x')?.verdict).toBe('count');
    const c = run('electron', [
      { kind: 'step', name: 's', snapshot: {}, events: [ev('x', 'impression')] },
    ]);
    expect(compareRuns(loadRun(a), loadRun(c)).find((r) => r.what === 'events x')?.verdict).toBe(
      'differs',
    );
    expect(rows.find((r) => r.step === 'only-a')?.verdict).toBe('differs');
  });
});

describe('compareTotals', () => {
  it('compares the ad event names per slot over the run, without lifecycle', () => {
    const a = run('tauri', [
      { kind: 'step', name: '1', snapshot: {}, events: [ev('v', 'player_loaded')] },
      { kind: 'step', name: '2', snapshot: {}, events: [ev('v', 'play'), ev('w', 'shutdown')] },
    ]);
    const b = run('electron', [
      {
        kind: 'step',
        name: '1',
        snapshot: {},
        events: [ev('v', 'player_loaded'), ev('v', 'play'), ev('v', 'did-fail-load')],
      },
      { kind: 'step', name: '2', snapshot: {}, events: [] },
    ]);
    const totals = compareTotals(loadRun(a), loadRun(b));
    expect(totals).toEqual([
      { cid: 'v', a: 'play, player_loaded', b: 'play, player_loaded', verdict: 'same' },
      { cid: 'w', a: 'shutdown', b: '', verdict: 'differs' },
    ]);
  });
});
