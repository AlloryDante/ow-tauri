import { Channel } from '@tauri-apps/api/core';
import { StrictMode, act, createElement, useEffect, useRef } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { OverwolfError } from '../errors.js';
import { mockOverwolf, settle, type MockOverwolf } from '../testing/index.js';
import { AdviewRuntime } from './element.js';
import { EventGate, isAdviewEventMessage, tauriServices, wireArgs } from './transport.js';

describe('isAdviewEventMessage', () => {
  it('accepts a name with an optional host or guest source', () => {
    expect(isAdviewEventMessage({ name: 'impression' })).toBe(true);
    expect(isAdviewEventMessage({ name: 'impression', data: 1, source: 'guest' })).toBe(true);
    expect(isAdviewEventMessage({ name: 'destroyed', source: 'host' })).toBe(true);
    expect(isAdviewEventMessage({ name: 'x', source: 'other' })).toBe(false);
    expect(isAdviewEventMessage({ name: 1 })).toBe(false);
    expect(isAdviewEventMessage(null)).toBe(false);
    expect(isAdviewEventMessage('impression')).toBe(false);
  });
});

describe('EventGate', () => {
  it('buffers until open, then dispatches in arrival order and at once', () => {
    const seen: string[] = [];
    const gate = new EventGate((m) => seen.push(m.name));
    gate.push({ name: 'a' });
    gate.push({ name: 'b' });
    expect(seen).toEqual([]);
    gate.open();
    expect(seen).toEqual(['a', 'b']);
    gate.push({ name: 'c' });
    expect(seen).toEqual(['a', 'b', 'c']);
    gate.open();
    expect(seen).toEqual(['a', 'b', 'c']);
  });

  it('drops the buffer and every later message once closed', () => {
    const seen: string[] = [];
    const gate = new EventGate((m) => seen.push(m.name));
    gate.push({ name: 'a' });
    gate.close();
    expect(gate.closed).toBe(true);
    gate.open();
    gate.push({ name: 'b' });
    expect(seen).toEqual([]);
  });

  it('closes after a dispatched destroyed, also in the middle of the buffer', () => {
    const seen: string[] = [];
    const gate = new EventGate((m) => seen.push(m.name));
    gate.push({ name: 'a' });
    gate.push({ name: 'destroyed', source: 'host' });
    gate.push({ name: 'late' });
    gate.open();
    gate.push({ name: 'later' });
    expect(seen).toEqual(['a', 'destroyed']);
    expect(gate.closed).toBe(true);
  });

  it('ignores values without the message shape', () => {
    const deliver = vi.fn();
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => undefined);
    const gate = new EventGate(deliver);
    gate.open();
    gate.push({ nope: true });
    gate.push(undefined);
    expect(deliver).not.toHaveBeenCalled();
    expect(debug).toHaveBeenCalled();
  });
});

describe('wireArgs', () => {
  it('wraps mount and update in a request, mount with its channel', () => {
    const channel = { id: 7 } as unknown as Channel;
    expect(wireArgs('adview_mount', { elementId: 'e1' }, channel)).toEqual({
      request: { elementId: 'e1' },
      onEvent: channel,
    });
    expect(wireArgs('adview_update', { elementId: 'e1', visible: false })).toEqual({
      request: { elementId: 'e1', visible: false },
    });
    expect(wireArgs('adview_unmount', { elementId: 'e1' })).toEqual({ elementId: 'e1' });
    expect(wireArgs('adview_command', { elementId: 'e1', command: 'reload', args: [] })).toEqual({
      elementId: 'e1',
      command: 'reload',
      args: [],
    });
  });
});

/** A layout box and visibility for every element (happy-dom has no layout). */
function giveEveryElementABox(): void {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(() => ({
    x: 0,
    y: 0,
    width: 300,
    height: 250,
    left: 0,
    top: 0,
    right: 300,
    bottom: 250,
    toJSON: () => ({}),
  }));
  Object.defineProperty(HTMLElement.prototype, 'checkVisibility', {
    configurable: true,
    writable: true,
    value: () => true,
  });
}

function createAd(): HTMLElement {
  const el = document.createElement('owadview');
  el.setAttribute('cid', 'main');
  el.setAttribute('slotsize', '300x250');
  return el;
}

describe('the element runtime over the Channel transport', () => {
  let overwolf: MockOverwolf;
  let runtime: AdviewRuntime;

  beforeEach(() => {
    giveEveryElementABox();
    document.body.innerHTML = '';
    overwolf = mockOverwolf();
    runtime = new AdviewRuntime(tauriServices());
    runtime.start();
  });

  afterEach(() => {
    runtime.stop();
    document.body.innerHTML = '';
    overwolf.restore();
  });

  it('mounts with a channel and dispatches the events it carries', async () => {
    const el = createAd();
    document.body.append(el);
    await settle();
    const [mount] = overwolf.mounts();
    expect(mount?.request).toMatchObject({
      elementId: runtime.elementId(el),
      attributes: { cid: 'main', slotsize: '300x250' },
      rect: { x: 0, y: 0, width: 300, height: 250 },
      visible: true,
      runtimeVersion: '0.1.0',
    });
    expect(overwolf.callsOf('adview_mount')[0]?.['onEvent']).toBeInstanceOf(Channel);
    const seen: Event[] = [];
    el.addEventListener('display_ad_loaded', (event) => seen.push(event));
    overwolf.emit(mount?.elementId ?? '', 'display_ad_loaded', { adUnit: 'x' });
    await settle();
    expect(seen).toHaveLength(1);
    expect((seen[0] as Event & { adUnit?: string }).adUnit).toBe('x');
  });

  it('buffers events that arrive before the mount resolves', async () => {
    let release!: () => void;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    overwolf.setCommand('adview_mount', async (args, original) => {
      const result = original(args);
      await held;
      return result;
    });
    const el = createAd();
    const seen: string[] = [];
    for (const name of ['display_ad_loaded', 'impression'])
      el.addEventListener(name, () => seen.push(name));
    document.body.append(el);
    await settle();
    const id = overwolf.mounts()[0]?.elementId ?? '';
    overwolf.emit(id, 'display_ad_loaded', {});
    overwolf.emit(id, 'impression', {});
    await settle();
    expect(seen).toEqual([]);
    release();
    await settle();
    expect(seen).toEqual(['display_ad_loaded', 'impression']);
  });

  it('drops events after destroyed', async () => {
    const el = createAd();
    const seen: string[] = [];
    for (const name of ['destroyed', 'impression'])
      el.addEventListener(name, () => seen.push(name));
    document.body.append(el);
    await settle();
    const id = overwolf.mounts()[0]?.elementId ?? '';
    overwolf.emit(id, 'destroyed', undefined, 'host');
    overwolf.emit(id, 'impression', {});
    await settle();
    expect(seen).toEqual(['destroyed']);
  });

  it('maps a rejected mount to an OverwolfError and closes the element', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    overwolf.setCommand('adview_mount', () => {
      throw { code: 'forbidden', message: 'ads are not allowed in this window' };
    });
    const services = tauriServices();
    await expect(services.command('adview_mount', { elementId: 'e9' })).rejects.toMatchObject({
      code: 'forbidden',
    });
    await expect(services.command('adview_unmount', { elementId: 'e9' })).resolves.toBeNull();
    const el = createAd();
    document.body.append(el);
    await settle();
    expect(overwolf.mounts()).toEqual([]);
    expect(warn.mock.calls.flat().join('\n')).toContain('did not mount');
    await expect(services.command('adview_mount', { elementId: 'e10' })).rejects.toBeInstanceOf(
      OverwolfError,
    );
  });
});

describe('React StrictMode', () => {
  let overwolf: MockOverwolf;
  let runtime: AdviewRuntime;
  let root: Root;
  let container: HTMLElement;

  beforeEach(() => {
    Reflect.set(globalThis, 'IS_REACT_ACT_ENVIRONMENT', true);
    giveEveryElementABox();
    document.body.innerHTML = '';
    container = document.createElement('div');
    document.body.append(container);
    overwolf = mockOverwolf();
    runtime = new AdviewRuntime(tauriServices());
    runtime.start();
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    runtime.stop();
    document.body.innerHTML = '';
    overwolf.restore();
    Reflect.deleteProperty(globalThis, 'IS_REACT_ACT_ENVIRONMENT');
  });

  it('keeps one live guest for a declarative <owadview>', async () => {
    await act(async () => {
      root.render(
        createElement(
          StrictMode,
          null,
          createElement('owadview', { cid: 'main', slotsize: '300x250' }),
        ),
      );
      await settle();
    });
    await settle();
    expect(overwolf.mounts()).toHaveLength(1);
    expect(overwolf.callsOf('adview_mount')).toHaveLength(1);
    expect(runtime.elements()).toHaveLength(1);
  });

  it('keeps one live guest when an effect creates the element (mount, unmount, mount)', async () => {
    let created = 0;
    function Banner(): ReturnType<typeof createElement> {
      const host = useRef<HTMLDivElement>(null);
      useEffect(() => {
        created++;
        const ad = createAd();
        host.current?.append(ad);
        return () => {
          ad.remove();
        };
      }, []);
      return createElement('div', { ref: host });
    }
    await act(async () => {
      root.render(createElement(StrictMode, null, createElement(Banner)));
      await settle();
    });
    await settle();
    // StrictMode ran the effect twice: two elements were created, one survives.
    expect(created).toBe(2);
    expect(overwolf.mounts()).toHaveLength(1);
    expect(container.querySelectorAll('owadview')).toHaveLength(1);
    const live = overwolf.mounts()[0];
    expect(live?.elementId).toBe(runtime.elementId(container.querySelector('owadview') as Element));
  });

  it('unmounts the guest when the component unmounts', async () => {
    await act(async () => {
      root.render(
        createElement(
          StrictMode,
          null,
          createElement('owadview', { cid: 'main', slotsize: '300x250' }),
        ),
      );
      await settle();
    });
    await act(async () => {
      root.render(createElement(StrictMode, null));
      await settle();
    });
    await settle();
    expect(overwolf.mounts()).toEqual([]);
    expect(overwolf.callsOf('adview_unmount')).toHaveLength(1);
  });
});
