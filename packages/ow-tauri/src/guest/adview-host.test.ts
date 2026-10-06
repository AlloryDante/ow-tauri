import { Window as HappyWindow } from 'happy-dom';
import { afterEach, describe, expect, it } from 'vitest';

import {
  ADVIEW_COMMAND,
  DATA_KEYS,
  MAX_HANDLERS,
  PAGE_URL_KEY,
  installAdviewHost,
  type AdviewHostApi,
} from './adview-host-core.js';

const ADVIEW_URL = 'https://www.overwolf.com/monsdk/electron/latest/adview.html';

/** A stand-in configuration as Rust builds it (`guest_config`, D.2). */
function config(extra: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    muid: 'STAND-IN-MUID',
    uid: 'abcdefghijklmnopabcdefghijklmnopabcdefgh',
    name: 'Example App',
    owVersion: 'tauri-2.12.1',
    version: '1.0.0',
    windowName: 'index',
    windowTitle: 'Example',
    windowFocused: false,
    testAd: true,
    consent: '',
    consentFull: '',
    slotSize: '300x250',
    containerId: 'c1',
    systemInfo: {
      gpus: [{ name: '', model: '', driverVersion: '', vendor: '' }],
      cpu: 'CPU',
      displays: [],
    },
    settings: { disableOptimization: false, anonymous: false },
    muidV2: 'STAND-IN-MUID',
    phasePercent: 42,
    pageUrl: '',
    performanceAd: false,
    adStyle: '',
    unit: 'testAd',
    customTracking: { a: 1 },
    slotId: 'owad-bw-1-1',
    visibilityState: 'visible',
    ...extra,
  };
}

interface Guest {
  win: Window;
  sent: { command: string; args: Record<string, unknown> }[];
  ow: Record<string, unknown>;
  host: AdviewHostApi;
}

const windows: HappyWindow[] = [];
afterEach(async () => {
  for (const w of windows.splice(0)) await w.happyDOM.close();
});

function guest(url = ADVIEW_URL, cfg: unknown = config(), withTransport = true): Guest {
  const happy = new HappyWindow({ url });
  windows.push(happy);
  const win = happy as unknown as Window;
  const sent: Guest['sent'] = [];
  if (withTransport) {
    Reflect.set(win, '__TAURI_INTERNALS__', {
      invoke: (command: string, args: Record<string, unknown>) => {
        sent.push({ command, args });
        return Promise.resolve();
      },
    });
  }
  installAdviewHost(win, cfg);
  return {
    win,
    sent,
    ow: Reflect.get(win, '__overwolf__') as Record<string, unknown>,
    host: Reflect.get(win, '__owTauriHost') as AdviewHostApi,
  };
}

const call = (ow: Record<string, unknown>, name: string, ...args: unknown[]): unknown =>
  (ow[name] as (...a: unknown[]) => unknown)(...args);

const names = (g: Guest): string[] => g.sent.map((s) => String(s.args['name']));

describe('installAdviewHost (D.1, D.2)', () => {
  it('defines __overwolf__ with the observed key order, frozen and read-only', () => {
    const g = guest();
    expect(Object.keys(g.ow)).toEqual([
      'muid',
      'setMute',
      'triggerEvent',
      'applySetting',
      'crash',
      'reload',
      'getSystemInformation',
      'getCustomTracking',
      'hasWindowFocus',
      'onmessage',
      ...DATA_KEYS,
    ]);
    expect(Object.isFrozen(g.ow)).toBe(true);
    expect(Object.isFrozen(g.ow['settings'])).toBe(true);
    expect(() => Reflect.set(g.win, '__overwolf__', 1)).not.toThrow();
    expect(Reflect.get(g.win, '__overwolf__')).toBe(g.ow);
    expect(Reflect.set(g.ow, 'testAd', false)).toBe(false);
    expect(g.ow['testAd']).toBe(true);
    expect(g.ow['muid']).toBe('STAND-IN-MUID');
    expect(g.ow['consent']).toBe('');
    expect(Object.getOwnPropertyDescriptor(g.win, '__overwolf__')?.enumerable).toBe(true);
  });

  it('gives every function length 0 and freezes it', () => {
    const g = guest();
    for (const key of Object.keys(g.ow)) {
      const value = g.ow[key];
      if (typeof value === 'function') {
        expect(value.length, key).toBe(0);
        expect(Object.isFrozen(value), key).toBe(true);
      }
    }
    expect(typeof Reflect.get(g.win, 'gc')).toBe('function');
  });

  it('is inert off www.overwolf.com, without a config, and on a second run', () => {
    for (const url of [
      'https://content.overwolf.com/x',
      'https://evil.example/',
      'http://www.overwolf.com/',
    ]) {
      const g = guest(url);
      expect(g.ow, url).toBeUndefined();
      expect(g.sent).toEqual([]);
    }
    expect(guest(ADVIEW_URL, null).ow).toBeUndefined();
    const g = guest();
    expect(installAdviewHost(g.win, config({ muid: 'other' }))).toBe(false);
    expect(g.ow['muid']).toBe('STAND-IN-MUID');
  });

  it('does not run in a sub-frame', () => {
    const g = guest();
    const frame = g.win.document.createElement('iframe');
    g.win.document.body.appendChild(frame);
    const child = frame.contentWindow;
    if (child !== null) {
      expect(installAdviewHost(child, config())).toBe(false);
    }
  });

  it('reports ready with the page facts', () => {
    const g = guest();
    const ready = g.sent.find((s) => s.args['name'] === '__host:ready');
    expect(ready?.command).toBe(ADVIEW_COMMAND);
    expect(ready?.args).toEqual({
      slotId: 'owad-bw-1-1',
      name: '__host:ready',
      data: { href: ADVIEW_URL, testAd: true, visibilityState: 'visible', pageUrl: '' },
    });
  });
});

describe('functions (D.3, D.4)', () => {
  it('forwards events with the slot id; message and messageerror are dropped', () => {
    const g = guest();
    g.sent.length = 0;
    call(g.ow, 'triggerEvent', 'impression', { foo: 1, el: document.body });
    call(g.ow, 'triggerEvent', 'multi', 1, 2);
    call(g.ow, 'triggerEvent', 'message', {});
    call(g.ow, 'triggerEvent', 'messageerror', {});
    call(g.ow, 'setMute', false);
    call(g.ow, 'applySetting', { enableHashes: true });
    call(g.ow, 'crash');
    call(g.ow, 'reload');
    expect(names(g)).toEqual([
      'impression',
      'multi',
      '__host:setMute',
      '__host:applySetting',
      '__host:crash',
      '__host:reload',
    ]);
    expect(g.sent[0]?.args['data']).toEqual({ foo: 1 });
    expect(g.sent[1]?.args['data']).toEqual([1, 2]);
    expect(g.sent[2]?.args['data']).toEqual({ muted: false });
    expect(g.sent.every((s) => s.args['slotId'] === 'owad-bw-1-1')).toBe(true);
  });

  it('caps data at 16 KiB and flattens events', () => {
    const g = guest();
    g.sent.length = 0;
    call(g.ow, 'triggerEvent', 'big', { s: 'x'.repeat(20_000) });
    call(g.ow, 'triggerEvent', 'evt', new Event('click'));
    const cyclic: Record<string, unknown> = { a: 1 };
    cyclic['self'] = cyclic;
    call(g.ow, 'triggerEvent', 'cycle', cyclic);
    expect(g.sent[0]?.args['data']).toEqual({ truncated: true, bytes: 20_008 });
    expect(g.sent[1]?.args['data']).toEqual({ type: 'click' });
    expect(g.sent[2]?.args['data']).toEqual({ a: 1 });
  });

  it('returns copies of systemInfo and customTracking', () => {
    const g = guest();
    const info = call(g.ow, 'getSystemInformation') as Record<string, unknown>;
    expect(info['cpu']).toBe('CPU');
    info['cpu'] = 'changed';
    expect((call(g.ow, 'getSystemInformation') as Record<string, unknown>)['cpu']).toBe('CPU');
    expect(call(g.ow, 'getCustomTracking')).toEqual({ a: 1 });
  });
});

describe('host to guest (D.5)', () => {
  it('passes messages to handlers as fresh copies and tracks customTracking', () => {
    const g = guest();
    const seen: unknown[] = [];
    call(g.ow, 'onmessage', (m: unknown) => {
      seen.push(m);
      throw new Error('handler errors are swallowed');
    });
    call(g.ow, 'onmessage', 'not a function');
    expect(g.host.deliver({ type: 'consent', data: 'CQ' })).toBe(true);
    expect(g.host.deliver({ type: 'window-hidden' })).toBe(true);
    expect(g.host.deliver({ type: 'customTracking', data: { b: 2 } })).toBe(true);
    expect(g.host.deliver({ nope: 1 })).toBe(false);
    expect(g.host.deliver('{"type":"x"}')).toBe(false);
    expect(seen).toEqual([
      { type: 'consent', data: 'CQ' },
      { type: 'window-hidden' },
      { type: 'customTracking', data: { b: 2 } },
    ]);
    expect(call(g.ow, 'getCustomTracking')).toEqual({ b: 2 });
    g.host.deliver({ type: 'customTracking', data: null });
    expect(call(g.ow, 'getCustomTracking')).toBeNull();
  });

  it('keeps at most 16 handlers', () => {
    const g = guest();
    let calls = 0;
    for (let i = 0; i < MAX_HANDLERS + 4; i++) call(g.ow, 'onmessage', () => (calls += 1));
    g.host.deliver({ type: 'x' });
    expect(calls).toBe(MAX_HANDLERS);
  });

  it('drives visibility and focus without reaching onmessage', () => {
    const g = guest(ADVIEW_URL, config({ visibilityState: 'hidden' }));
    const doc = g.win.document;
    const seen: unknown[] = [];
    call(g.ow, 'onmessage', (m: unknown) => seen.push(m));
    let changes = 0;
    doc.addEventListener('visibilitychange', () => (changes += 1));
    expect(doc.visibilityState).toBe('hidden');
    expect(doc.hidden).toBe(true);
    g.host.setVisibility('visible');
    g.host.setVisibility('visible');
    g.host.setVisibility('bogus');
    expect(doc.visibilityState).toBe('visible');
    expect(changes).toBe(1);
    expect(call(g.ow, 'hasWindowFocus')).toBe(false);
    g.host.setEmbedderFocus(true);
    expect(call(g.ow, 'hasWindowFocus')).toBe(true);
    expect(doc.hasFocus()).toBe(true);
    expect(seen).toEqual([]);
  });

  it('stores the next pageUrl for the following load (B.3.3)', () => {
    const g = guest(ADVIEW_URL, config({ pageUrl: 'https://example.com/a' }));
    expect(g.ow['pageUrl']).toBe('https://example.com/a');
    g.host.setNextPageUrl('https://example.com/b');
    g.host.setNextPageUrl(42);
    expect(g.win.sessionStorage.getItem(PAGE_URL_KEY)).toBe('https://example.com/b');
  });

  it('exposes the host API frozen and hidden from enumeration', () => {
    const g = guest();
    expect(Object.isFrozen(g.host)).toBe(true);
    expect(Object.keys(g.win)).not.toContain('__owTauriHost');
  });
});

describe('transport (D.1)', () => {
  it('queues until __TAURI_INTERNALS__ exists, then flushes in order', async () => {
    const g = guest(ADVIEW_URL, config(), false);
    call(g.ow, 'triggerEvent', 'early');
    const sent: string[] = [];
    Reflect.set(g.win, '__TAURI_INTERNALS__', {
      invoke: (_c: string, args: Record<string, unknown>) => {
        sent.push(String(args['name']));
      },
    });
    expect(sent).toEqual([]);
    // The retry runs every 250 ms on the guest's own timer.
    await new Promise<void>((resolve) => g.win.setTimeout(resolve, 400));
    expect(sent).toEqual(['__host:domReady', '__host:ready', 'early']);
  });

  it('keeps the transport captured at startup', () => {
    const g = guest();
    const hijack: unknown[] = [];
    Reflect.set(g.win, '__TAURI_INTERNALS__', { invoke: (...a: unknown[]) => hijack.push(a) });
    g.sent.length = 0;
    call(g.ow, 'triggerEvent', 'after');
    expect(names(g)).toEqual(['after']);
    expect(hijack).toEqual([]);
  });

  it('reports trusted gestures only', () => {
    const g = guest();
    g.sent.length = 0;
    g.win.dispatchEvent(new Event('pointerdown'));
    expect(names(g)).not.toContain('__host:gesture');
    g.win.dispatchEvent(new Event('focus'));
    expect(g.sent.at(-1)?.args).toMatchObject({ name: '__host:focus', data: { focused: true } });
  });
});
