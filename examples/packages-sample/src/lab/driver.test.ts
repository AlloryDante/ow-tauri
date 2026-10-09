import { describe, expect, it, vi } from 'vitest';

import { createLogStore } from '../log/store';
import {
  BEAT_MS,
  restartPhase,
  startDriver,
  TOUR,
  type DriverHost,
  type DriverRecord,
} from './driver';

/** A fake lab host that records what the driver does. */
function fakeHost(config: Awaited<ReturnType<DriverHost['config']>>) {
  const records: DriverRecord[] = [];
  const host: DriverHost = {
    config: () => Promise.resolve(config),
    record: (entry) => {
      records.push(entry);
      return Promise.resolve();
    },
    window: () => Promise.resolve({ visible: true }),
    quit: vi.fn(() => Promise.resolve()),
    sleep: () => Promise.resolve(),
  };
  return { host, records };
}

/** The ads tester's two start buttons in a shown page (unless `hidden`). */
function adsPage(onStart: (slot: string) => void, hidden = false): Document {
  const doc = document.implementation.createHTMLDocument('lab');
  Object.defineProperty(doc, 'visibilityState', { value: hidden ? 'hidden' : 'visible' });
  for (const slot of ['ad1', 'ad2']) {
    const wrapper = doc.createElement('div');
    wrapper.dataset['slot'] = slot;
    const start = doc.createElement('button');
    start.textContent = 'Start ad';
    start.addEventListener('click', () => {
      onStart(slot);
    });
    wrapper.append(start);
    doc.body.append(wrapper);
  }
  return doc;
}

describe('startDriver', () => {
  it('stays inert outside a lab run', async () => {
    const { host, records } = fakeHost(null);
    await startDriver(
      createLogStore(),
      host,
      adsPage(() => undefined),
    );
    expect(records).toEqual([]);
    expect(host.quit).not.toHaveBeenCalled();
  });

  it('starts both slots, waits for display_ad_loaded, records and quits', async () => {
    const log = createLogStore();
    const { host, records } = fakeHost({ adWaitMs: 1000, pid: 42 });
    const doc = adsPage((slot) => {
      log.push('info', 'ad', `${slot}: start`);
      if (slot === 'ad1') log.push('success', 'ad', 'ad1 160x600: display_ad_loaded', {});
    });
    await startDriver(log, host, doc);
    expect(records.map((r) => r.kind)).toEqual(['driver', 'step', 'step', 'step', 'step', 'done']);
    expect(records[0]).toEqual({ kind: 'driver', pid: 42 });
    expect(records[2]).toEqual({ kind: 'step', name: 'shown', shown: true });
    expect(records[3]).toEqual({ kind: 'step', name: 'ads-started', slots: ['ad1', 'ad2'] });
    expect(records[4]).toMatchObject({
      name: 'ads',
      events: ['ad1: start', 'ad1 160x600: display_ad_loaded', 'ad2: start'],
      errors: [],
    });
    expect(records[4]).toMatchObject({
      view: { visibility: 'visible', slots: [] },
      window: { visible: true },
    });
    expect(records[5]).toEqual({ kind: 'done', displayAdLoaded: true });
    expect(host.quit).toHaveBeenCalledTimes(1);
  });

  it('records a run without an ad and still quits', async () => {
    const { host, records } = fakeHost({ adWaitMs: 300 });
    await startDriver(
      createLogStore(),
      host,
      adsPage(() => undefined),
    );
    expect(records.at(-1)).toEqual({ kind: 'done', displayAdLoaded: false });
    expect(records[0]).toEqual({ kind: 'driver', pid: null });
    expect(host.quit).toHaveBeenCalledTimes(1);
  });

  it('records a page that is never shown and starts the ads anyway', async () => {
    const { host, records } = fakeHost({ adWaitMs: 0 });
    await startDriver(
      createLogStore(),
      host,
      adsPage(() => undefined, true),
    );
    expect(records[2]).toEqual({ kind: 'step', name: 'shown', shown: false });
    expect(records[3]).toEqual({ kind: 'step', name: 'ads-started', slots: ['ad1', 'ad2'] });
    expect(records[4]).toMatchObject({ view: { visibility: 'hidden' } });
  });

  it('records a failure as fatal and quits', async () => {
    const { host, records } = fakeHost({ adWaitMs: 0 });
    let first = true;
    host.record = (entry) => {
      if (first) {
        first = false;
        return Promise.reject(new Error('disk full'));
      }
      records.push(entry);
      return Promise.resolve();
    };
    await startDriver(
      createLogStore(),
      host,
      adsPage(() => undefined),
    );
    expect(records).toHaveLength(1);
    expect(records[0]?.kind).toBe('fatal');
    expect(String(records[0]?.['text'])).toContain('disk full');
    expect(host.quit).toHaveBeenCalledTimes(1);
  });

  it('records heartbeats while it runs and stops them at the end', async () => {
    const { host, records } = fakeHost({ adWaitMs: 0 });
    const stop = vi.fn();
    host.every = (ms, tick) => {
      expect(ms).toBe(BEAT_MS);
      tick();
      return stop;
    };
    await startDriver(
      createLogStore(),
      host,
      adsPage(() => undefined),
    );
    expect(records.some((r) => r.kind === 'beat' && r['visibility'] === 'visible')).toBe(true);
    expect(stop).toHaveBeenCalledTimes(1);
  });
});

describe('restartPhase', () => {
  it('is 1 for the first launch, then 2 in LIVE and 3 in TEST mode', () => {
    expect(restartPhase('', 'test')).toBe(1);
    expect(restartPhase('#', 'test')).toBe(1);
    expect(restartPhase('#settings', 'live')).toBe(2);
    expect(restartPhase('#settings', 'test')).toBe(3);
  });
});

/** A settings page in `mode` with the two restart buttons; a press calls `onRestart`. */
function settingsPage(hash: string, mode: 'test' | 'live', onRestart: (m: string) => void) {
  const doc = document.implementation.createHTMLDocument('lab');
  Object.defineProperty(doc, 'visibilityState', { value: 'visible' });
  const location = { hash, assign: (h: string) => void (location.hash = h) };
  Object.defineProperty(doc, 'defaultView', { value: { location } });
  const shown = doc.createElement('strong');
  shown.dataset['testid'] = 'ad-mode';
  shown.textContent = `${mode} ads`;
  doc.body.append(shown);
  for (const m of ['test', 'live']) {
    const b = doc.createElement('button');
    b.textContent = `Restart with ${m} ads`;
    b.addEventListener('click', () => {
      onRestart(m);
    });
    doc.body.append(b);
  }
  return { doc, location };
}

describe('startDriver restart', () => {
  it('first launch: opens settings and presses "Restart with live ads"', async () => {
    const { host, records } = fakeHost({ steps: 'restart', pid: 7 });
    host.restart = vi.fn();
    const pressed: string[] = [];
    const { doc, location } = settingsPage('', 'test', (m) => {
      pressed.push(m);
      // The process exits here; the page never answers.
    });
    const run = startDriver(createLogStore(), host, doc);
    await run;
    expect(location.hash).toBe('#settings');
    expect(pressed).toEqual(['live']);
    expect(records).toContainEqual(
      expect.objectContaining({ kind: 'step', name: 'restart-1', phase: 1, pid: 7, mode: 'test' }),
    );
    expect(records).toContainEqual({ kind: 'restart-requested', phase: 1, mode: 'live' });
    // The app did not exit (fake): the driver records it and quits.
    expect(records.at(-1)?.kind).toBe('fatal');
    expect(String(records.at(-1)?.['text'])).toContain('did not exit');
    expect(host.quit).toHaveBeenCalledTimes(1);
  });

  it('second launch (LIVE, on settings): presses "Restart with test ads"', async () => {
    const { host, records } = fakeHost({ steps: 'restart', pid: 8 });
    host.restart = vi.fn();
    const pressed: string[] = [];
    const { doc } = settingsPage('#settings', 'live', (m) => pressed.push(m));
    await startDriver(createLogStore(), host, doc);
    expect(pressed).toEqual(['test']);
    expect(records).toContainEqual(
      expect.objectContaining({
        name: 'restart-2',
        phase: 2,
        mode: 'live',
        startHash: '#settings',
      }),
    );
  });

  it('third launch (TEST, on settings): records done and quits', async () => {
    const { host, records } = fakeHost({ steps: 'restart', pid: 9 });
    host.restart = vi.fn();
    const pressed: string[] = [];
    const { doc } = settingsPage('#settings', 'test', (m) => pressed.push(m));
    await startDriver(createLogStore(), host, doc);
    expect(pressed).toEqual([]);
    expect(records.at(-1)).toEqual({
      kind: 'done',
      restarted: true,
      mode: 'test',
      page: 'settings',
    });
    expect(host.quit).toHaveBeenCalledTimes(1);
  });
});

describe('startDriver tour', () => {
  it('takes a still of every page, both ads loaded, and the privacy window', async () => {
    const log = createLogStore();
    const { host, records } = fakeHost({ steps: 'tour', adWaitMs: 1000 });
    const stills: [string, string | undefined][] = [];
    host.still = (name, window) => {
      stills.push([name, window]);
      return Promise.resolve({ path: `${name}.png` });
    };
    let opened = false;
    host.reveal = (label) =>
      opened ? Promise.resolve({ label, url: 'https://cmp/' }) : Promise.reject(new Error('no'));
    const { doc } = settingsPage('', 'test', () => undefined);
    const privacy = doc.createElement('button');
    privacy.textContent = 'openAdPrivacySettingsWindow()';
    privacy.addEventListener('click', () => {
      opened = true;
    });
    doc.body.append(privacy);
    for (const slot of ['ad1', 'ad2']) {
      const wrapper = doc.createElement('div');
      wrapper.dataset['slot'] = slot;
      const start = doc.createElement('button');
      start.textContent = 'Start ad';
      start.addEventListener('click', () => {
        log.push('success', 'ad', `${slot} 300x250: display_ad_loaded`, {});
      });
      wrapper.append(start);
      doc.body.append(wrapper);
    }
    await startDriver(log, host, doc);
    expect(stills).toEqual([
      ['logger', undefined],
      ['ads-tester', undefined],
      ['settings', undefined],
      ['privacy-settings', 'ow-cmp'],
      ['updater', undefined],
      ['packages', undefined],
    ]);
    expect(records).toContainEqual({ kind: 'step', name: 'ads-loaded', loaded: true });
    expect(records.at(-1)).toEqual({ kind: 'done', stills: TOUR.length + 1 });
    expect(host.quit).toHaveBeenCalledTimes(1);
  });

  it('fails the tour when the privacy window never opens', async () => {
    const { host, records } = fakeHost({ steps: 'tour', adWaitMs: 0 });
    host.still = () => Promise.resolve({});
    host.reveal = () => Promise.reject(new Error('no window ow-cmp'));
    const { doc } = settingsPage('', 'test', () => undefined);
    const privacy = doc.createElement('button');
    privacy.textContent = 'openAdPrivacySettingsWindow()';
    doc.body.append(privacy);
    await startDriver(createLogStore(), host, doc);
    expect(records.at(-1)?.kind).toBe('fatal');
    expect(String(records.at(-1)?.['text'])).toContain('never opened');
    expect(host.quit).toHaveBeenCalledTimes(1);
  });
});
