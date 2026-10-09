import { describe, expect, it } from 'vitest';

import {
  IMAGES,
  expectedFiles,
  failureOf,
  formulaUid,
  placeholderOf,
  reportedAppIds,
} from './screenshots.mjs';

describe('formulaUid', () => {
  it('matches the documented example', () => {
    expect(formulaUid('Example Studio', 'Parity Harness')).toBe(
      'bijigndkghcikkfmhgkmicdkjpdehpjafgpmdhcc',
    );
  });
});

describe('reportedAppIds', () => {
  const counter = (id) =>
    JSON.stringify({
      path: `/analytics/Counter?Name=x&Extra=${encodeURIComponent(JSON.stringify({ app_id: id }))}`,
    });

  it('collects the app ids of Counter requests only', () => {
    const text = [
      counter('aaa'),
      counter('aaa'),
      JSON.stringify({ path: '/tracking/InsertStats?Stats=true' }),
      'not json',
      '',
    ].join('\n');
    expect([...reportedAppIds(text)]).toEqual(['aaa']);
  });

  it('sees a second uid', () => {
    expect(reportedAppIds([counter('aaa'), counter('bbb')].join('\n')).size).toBe(2);
  });
});

describe('placeholderOf', () => {
  it('takes the formula uid of plugins.overwolf', () => {
    expect(
      placeholderOf({
        plugins: { overwolf: { author: 'Example Studio', name: 'Parity Harness' } },
      }),
    ).toEqual({ uid: 'bijigndkghcikkfmhgkmicdkjpdehpjafgpmdhcc' });
  });

  it('refuses a fixed uid or a missing identity', () => {
    expect(
      placeholderOf({ plugins: { overwolf: { author: 'a', name: 'b', uid: 'abcdefgh' } } }),
    ).toHaveProperty('error');
    expect(placeholderOf({})).toHaveProperty('error');
    expect(placeholderOf({ plugins: { overwolf: { author: 'a' } } })).toHaveProperty('error');
  });
});

describe('failureOf', () => {
  it('names the step a stalled run stopped after', () => {
    expect(
      failureOf({ verdict: 'stalled', stalled: { after: 'still p5-reward-ready' }, fatal: [] }),
    ).toBe('verdict stalled, stalled after still p5-reward-ready');
  });

  it('names a visible window, a frontmost app, the first fatal line and a failed check', () => {
    expect(
      failureOf({
        verdict: 'safety-kill',
        safetyKill: {},
        everFront: true,
        fatal: ['Error: boom\n  at x'],
        check: false,
      }),
    ).toBe(
      'verdict safety-kill, a window became visible, the app became frontmost, fatal: Error: boom, its check failed',
    );
    expect(failureOf(null)).toBe('no summary');
  });
});

describe('IMAGES', () => {
  it('has one image per ad format of docs/AD-FORMATS.md', () => {
    expect(IMAGES.filter((i) => i.area === 'ad-formats').map((i) => i.name)).toEqual([
      'standard-display',
      'standard-video',
      'house',
      'high-impact',
      'interstitial',
      'reward',
    ]);
  });

  it('covers every packages-sample page, the consent windows and the quickstart window', () => {
    expect(IMAGES.filter((i) => i.tour === 'packages-sample').map((i) => i.still)).toEqual([
      'logger',
      'ads-tester',
      'settings',
      'updater',
      'packages',
      'privacy-settings',
    ]);
    expect(IMAGES.filter((i) => i.area === 'consent')).toHaveLength(2);
    expect(IMAGES.some((i) => i.area === 'quickstart' && i.name === 'window')).toBe(true);
  });

  it('uses unique, safe file names', () => {
    const files = IMAGES.map((i) => `${i.area}/${i.name}`);
    expect(new Set(files).size).toBe(files.length);
    for (const f of files) expect(f).toMatch(/^[a-z-]+\/[a-z0-9-]+$/);
  });
});

describe('expectedFiles', () => {
  it('lists every image per theme, only for the chosen tours', () => {
    expect(expectedFiles(['dark', 'light'])).toHaveLength(IMAGES.length * 2);
    expect(expectedFiles(['light'], ['quickstart'])).toEqual(['quickstart/window-light.png']);
  });
});
