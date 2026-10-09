import { describe, expect, it } from 'vitest';

import {
  LAB_IDENTIFIER,
  labTauriConfig,
  parseJsonl,
  passed,
  sinkSummary,
  verdictOf,
} from './lab.mjs';

describe('labTauriConfig', () => {
  it('uses the lab bundle id and the embedded lab page', () => {
    expect(labTauriConfig('/abs/dist-lab')).toEqual({
      identifier: 'dev.ow-tauri.packages-sample.lab',
      build: { devUrl: null, frontendDist: '/abs/dist-lab' },
    });
    expect(LAB_IDENTIFIER.endsWith('.lab')).toBe(true);
  });
});

describe('verdictOf', () => {
  it('puts a safety kill first, then done, then timeout', () => {
    expect(verdictOf({ safetyKill: true, done: true, timedOut: false })).toBe('safety-kill');
    expect(verdictOf({ safetyKill: false, done: true, timedOut: true })).toBe('done');
    expect(verdictOf({ safetyKill: false, done: false, timedOut: true })).toBe('timeout');
    expect(verdictOf({ safetyKill: false, done: false, timedOut: false })).toBe('exited-early');
  });
});

describe('passed', () => {
  const ok = {
    verdict: 'done',
    everVisible: false,
    everFront: false,
    displayAdLoaded: true,
    fatal: [],
    leftProcesses: [],
  };

  it('passes a clean run', () => {
    expect(passed(ok)).toBe(true);
  });

  it('fails on any visible window, front app, missing ad, error or left process', () => {
    expect(passed({ ...ok, verdict: 'timeout' })).toBe(false);
    expect(passed({ ...ok, everVisible: true })).toBe(false);
    expect(passed({ ...ok, everVisible: null })).toBe(false);
    expect(passed({ ...ok, everFront: true })).toBe(false);
    expect(passed({ ...ok, displayAdLoaded: false })).toBe(false);
    expect(passed({ ...ok, fatal: ['x'] })).toBe(false);
    expect(passed({ ...ok, leftProcesses: [{ pid: 1 }] })).toBe(false);
  });
});

describe('sinkSummary', () => {
  it('counts requests by path', () => {
    expect(
      sinkSummary([
        { path: '/analytics/Counter?a=1' },
        { path: '/analytics/Counter?a=2' },
        { path: '/experiments/cmp-eu-only' },
        {},
      ]),
    ).toEqual({ '/analytics/Counter': 2, '/experiments/cmp-eu-only': 1, '/': 1 });
  });
});

describe('parseJsonl', () => {
  it('skips broken lines', () => {
    expect(parseJsonl('{"a":1}\nnot json\n\n{"b":2}\n')).toEqual([{ a: 1 }, { b: 2 }]);
  });
});
