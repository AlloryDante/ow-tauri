import { describe, expect, it, vi } from 'vitest';

import { createLogStore } from '../log/store';
import { startDriver, type DriverHost, type DriverRecord } from './driver';

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
});
