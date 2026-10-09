import { describe, expect, it } from 'vitest';

import {
  adLoaded,
  LAB_IDENTIFIER,
  QUICKSTART_LAB_IDENTIFIER,
  labTauriConfig,
  lastProgress,
  parseJsonl,
  passed,
  quickstartTauriConfig,
  quickstartWindow,
  restartResult,
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

describe('quickstartTauriConfig', () => {
  const conf = {
    productName: 'My Game App',
    version: '1.0.0',
    app: {
      windows: [{ label: 'main', width: 1200, height: 800 }],
      security: { csp: "default-src 'self'" },
    },
    plugins: { overwolf: { author: 'Example Studio', name: 'My Game App', ads: { testAd: true } } },
  };

  it("hosts the quickstart's page with its identity under a lab bundle id", () => {
    expect(quickstartTauriConfig(conf, '/abs/qs/dist')).toEqual({
      identifier: QUICKSTART_LAB_IDENTIFIER,
      productName: 'My Game App',
      version: '1.0.0',
      build: { devUrl: null, frontendDist: '/abs/qs/dist' },
      app: { security: { csp: "default-src 'self'" } },
      plugins: {
        overwolf: {
          analytics: null,
          author: 'Example Studio',
          name: 'My Game App',
          ads: { testAd: true },
        },
      },
    });
    expect(QUICKSTART_LAB_IDENTIFIER.endsWith('.lab')).toBe(true);
  });

  it('takes the main window size, 800x600 without one', () => {
    expect(quickstartWindow(conf)).toBe('1200x800');
    expect(quickstartWindow({})).toBe('800x600');
  });
});

describe('verdictOf', () => {
  it('puts a safety kill first, then done, then stalled, then timeout', () => {
    expect(verdictOf({ safetyKill: true, done: true, timedOut: false })).toBe('safety-kill');
    expect(verdictOf({ safetyKill: false, done: true, timedOut: true })).toBe('done');
    expect(verdictOf({ safetyKill: false, done: false, timedOut: true, stalled: true })).toBe(
      'stalled',
    );
    expect(verdictOf({ safetyKill: false, done: false, timedOut: true })).toBe('timeout');
    expect(verdictOf({ safetyKill: false, done: false, timedOut: false })).toBe('exited-early');
  });
});

describe('lastProgress', () => {
  it('names the last step or still, ignoring heartbeats', () => {
    expect(
      lastProgress([
        { kind: 'step', name: 'shown' },
        { kind: 'still', name: 'logger' },
        { kind: 'beat' },
        { kind: 'visibility', state: 'hidden' },
      ]),
    ).toBe('still logger');
    expect(lastProgress([{ kind: 'driver' }, { kind: 'beat' }])).toBe('driver');
    expect(lastProgress([])).toBe(null);
  });
});

describe('restartResult', () => {
  const phase = (n, pid, mode, startHash) => ({
    kind: 'step',
    name: `restart-${n}`,
    phase: n,
    pid,
    mode,
    startHash,
  });
  const good = [
    phase(1, 10, 'test', ''),
    phase(2, 11, 'live', '#settings'),
    phase(3, 12, 'test', '#settings'),
    { kind: 'done', restarted: true },
  ];

  it('passes TEST -> LIVE -> TEST in three processes on the same page', () => {
    expect(restartResult(good, { 2: true, 3: true })).toMatchObject({ ok: true, why: [] });
  });

  it('names what went wrong', () => {
    expect(restartResult(good, { 2: true, 3: false }).why).toEqual([
      "phase 2's process outlived it",
    ]);
    const samePid = [...good.slice(0, 2), phase(3, 11, 'test', '#settings'), good[3]];
    expect(restartResult(samePid, { 2: true, 3: true }).why).toContain(
      'the three phases did not run in three processes',
    );
    const lostPage = [good[0], phase(2, 11, 'live', ''), good[2], good[3]];
    expect(restartResult(lostPage, { 2: true, 3: true }).why).toContain('phase 2 lost the page');
    const noLive = [good[0], phase(2, 11, 'test', '#settings'), good[2], good[3]];
    expect(restartResult(noLive, { 2: true, 3: true }).why).toContain('phase 2 is not LIVE');
    expect(restartResult(good.slice(0, 1), {}).why).toContain('a phase is missing');
  });
});

describe('passed', () => {
  const ok = {
    verdict: 'done',
    everVisible: false,
    everFront: false,
    check: true,
    fatal: [],
    leftProcesses: [],
  };

  it('passes a clean run', () => {
    expect(passed(ok)).toBe(true);
  });

  it('fails on any visible window, front app, failed check, error or left process', () => {
    expect(passed({ ...ok, verdict: 'timeout' })).toBe(false);
    expect(passed({ ...ok, everVisible: true })).toBe(false);
    expect(passed({ ...ok, everVisible: null })).toBe(false);
    expect(passed({ ...ok, everFront: true })).toBe(false);
    expect(passed({ ...ok, check: false })).toBe(false);
    expect(passed({ ...ok, verdict: 'stalled' })).toBe(false);
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

describe('adLoaded', () => {
  const event = (channel, dir = 'page->host') => ({ dir, channel });
  it('takes a display ad or a video player', () => {
    expect(adLoaded([event('__host:ready'), event('display_ad_loaded')])).toBe(true);
    expect(adLoaded([event('player_loaded')])).toBe(true);
    expect(adLoaded([event('impression')])).toBe(true);
  });
  it('ignores host messages and other events', () => {
    expect(adLoaded([event('display_ad_loaded', 'host->page'), event('__host:domReady')])).toBe(
      false,
    );
    expect(adLoaded([])).toBe(false);
  });
});
