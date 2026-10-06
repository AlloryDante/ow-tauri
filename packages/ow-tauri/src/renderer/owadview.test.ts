import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { HostMessageHandler } from '../bootstrap/facade-kernel.js';
import { OwTauriError } from '../shared/errors.js';
import { mockHost, setHostContext, settle, type MockHost } from '../testing/index.js';
import { owadview } from './index.js';
import { parseCustomTracking } from './owadview-attributes.js';
import { createAdviewEvent } from './owadview-events.js';
import {
  AdviewRuntime,
  DEFAULT_STYLE,
  SHADOW_STYLE,
  VISIBILITY_POLL_MS,
  adviewRuntimeOf,
  browserEnvironment,
  type AdviewEnvironment,
  type AdviewKernel,
  type AdviewServices,
} from './owadview.js';

type Command = (args: Record<string, unknown>) => unknown;

interface FakeServices extends AdviewServices {
  calls: { name: string; args: Record<string, unknown> }[];
  impls: Record<string, Command>;
  logs: string[];
  warnings: string[];
  emit(message: Record<string, unknown>): void;
}

function fakeServices(context: AdviewServices['context'] = 'ui'): FakeServices {
  const handlers = new Map<string, Set<HostMessageHandler>>();
  const services: FakeServices = {
    context,
    calls: [],
    impls: {},
    logs: [],
    warnings: [],
    command: async (name, args = {}) => {
      services.calls.push({ name, args });
      const impl = services.impls[name];
      return await Promise.resolve(impl ? impl(args) : null);
    },
    log: (level, message) => {
      services.logs.push(`${level}: ${message}`);
    },
    warnOnce: (key, message) => {
      services.warnings.push(`${key}: ${message}`);
    },
    on: (type, handler) => {
      const set = handlers.get(type) ?? new Set<HostMessageHandler>();
      handlers.set(type, set);
      set.add(handler);
      return () => set.delete(handler);
    },
    emit: (message) => {
      for (const handler of handlers.get('adview-event') ?? [])
        handler({ type: 'adview-event', ...message });
    },
  };
  return services;
}

class FakeResizeObserver {
  static instances: FakeResizeObserver[] = [];
  readonly observed = new Set<Element>();
  constructor(readonly callback: () => void) {
    FakeResizeObserver.instances.push(this);
  }
  observe(el: Element): void {
    this.observed.add(el);
  }
  unobserve(el: Element): void {
    this.observed.delete(el);
  }
  disconnect(): void {
    this.observed.clear();
  }
  fire(): void {
    this.callback();
  }
}

class FakeIntersectionObserver {
  static instances: FakeIntersectionObserver[] = [];
  readonly observed = new Set<Element>();
  constructor(
    readonly callback: (records: Partial<IntersectionObserverEntry>[]) => void,
    readonly options: IntersectionObserverInit,
  ) {
    FakeIntersectionObserver.instances.push(this);
  }
  observe(el: Element): void {
    this.observed.add(el);
  }
  unobserve(el: Element): void {
    this.observed.delete(el);
  }
  disconnect(): void {
    this.observed.clear();
  }
  fire(target: Element, ratio: number): void {
    this.callback([{ target, isIntersecting: ratio > 0, intersectionRatio: ratio }]);
  }
}

let clock = 0;

function environment(overrides: Partial<AdviewEnvironment> = {}): AdviewEnvironment {
  return {
    document,
    window,
    ResizeObserver: FakeResizeObserver as unknown as typeof ResizeObserver,
    IntersectionObserver: FakeIntersectionObserver as unknown as typeof IntersectionObserver,
    MutationObserver,
    frame: (callback) => {
      callback();
    },
    now: () => clock,
    ...overrides,
  };
}

interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

type AdElement = HTMLElement & {
  box: Box;
  shown: boolean;
  customTracking?: unknown;
  pageUrl?: unknown;
  setPageUrl?: (url: unknown) => void;
  sendCommand?: (...args: unknown[]) => void;
  setAudioMuted?: (muted: unknown) => void;
  reload?: () => void;
};

/** Gives an element a layout box and a `checkVisibility()` answer the test controls. */
function layout(el: HTMLElement, box: Partial<Box> = {}): AdElement {
  const ad = el as AdElement;
  ad.box = { x: 10, y: 20, width: 300, height: 250, ...box };
  ad.shown = true;
  ad.getBoundingClientRect = () => {
    const { x, y, width, height } = ad.box;
    return {
      x,
      y,
      width,
      height,
      left: x,
      top: y,
      right: x + width,
      bottom: y + height,
      toJSON: () => ({}),
    };
  };
  (ad as { checkVisibility: () => boolean }).checkVisibility = () => ad.shown;
  return ad;
}

function createAd(attributes: Record<string, string> = {}, box?: Partial<Box>): AdElement {
  const el = document.createElement('owadview');
  for (const [name, value] of Object.entries({ cid: 'main', slotsize: '300x250', ...attributes }))
    el.setAttribute(name, value);
  return layout(el, box);
}

async function tick(): Promise<void> {
  for (let i = 0; i < 4; i++) await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

let services: FakeServices;
let runtime: AdviewRuntime;

function names(): string[] {
  return services.calls.map((c) => c.name);
}

function callsOf(name: string): Record<string, unknown>[] {
  return services.calls.filter((c) => c.name === name).map((c) => c.args);
}

async function startRuntime(overrides: Partial<AdviewEnvironment> = {}): Promise<void> {
  runtime = new AdviewRuntime(services, environment(overrides));
  runtime.start();
  await tick();
}

beforeEach(() => {
  clock = 0;
  FakeResizeObserver.instances = [];
  FakeIntersectionObserver.instances = [];
  services = fakeServices();
  document.body.innerHTML = '';
  document.adoptedStyleSheets = [];
});

afterEach(() => {
  runtime.stop();
  document.body.innerHTML = '';
});

describe('discovery and attach (B.3.1)', () => {
  it('adds the default style and starts only in UI windows', async () => {
    services = fakeServices('main');
    await startRuntime();
    expect(runtime.started).toBe(false);
    services = fakeServices();
    await startRuntime();
    runtime.start();
    expect(runtime.started).toBe(true);
    const rules = document.adoptedStyleSheets.flatMap((s) => [...s.cssRules].map((r) => r.cssText));
    expect(rules.join(' ')).toContain('owadview');
    expect(DEFAULT_STYLE).toContain(':where(owadview)');
  });

  it('falls back to a <style> element without constructable style sheets', async () => {
    const win = Object.create(window, { CSSStyleSheet: { value: undefined } }) as Window;
    await startRuntime({ window: win });
    const style = document.head.querySelector('style');
    expect(style?.textContent).toBe(DEFAULT_STYLE);
    style?.remove();
  });

  it('keeps the element plain until it is attached [OBS]', async () => {
    await startRuntime();
    const el = createAd();
    expect(runtime.elementId(el)).toMatch(/^e\d+$/);
    expect(el).toBeInstanceOf(HTMLElement);
    expect(el.shadowRoot).toBeNull();
    expect('sendCommand' in el).toBe(false);
    expect(runtime.elements()).toEqual([]);
    await tick();
    expect(services.calls).toEqual([]);
  });

  it('mounts an attached element with its attributes, rect and visibility (B.3.2, B.3.4)', async () => {
    await startRuntime();
    const el = createAd({
      cid: '  a-very-long-container-id-xyz ',
      adstyle: 'dark',
      customtracking: '{"campaign":"x"}',
      unit: 'video',
      pageurl: 'https://example.com/p',
    });
    document.body.append(el);
    await tick();
    expect(callsOf('adview_mount')).toEqual([
      {
        elementId: runtime.elementId(el),
        attributes: {
          cid: 'a-very-long-containe',
          slotsize: '300x250',
          adstyle: 'dark',
          customTracking: { campaign: 'x' },
          performance: false,
          unit: 'video',
          pageurl: 'https://example.com/p',
        },
        rect: { x: 10, y: 20, width: 300, height: 250, devicePixelRatio: 1 },
        visible: true,
      },
    ]);
    expect(runtime.elements()).toEqual([el]);
  });

  it('defines the element members and an open shadow root at attach [OBS]', async () => {
    await startRuntime();
    const el = createAd({ customtracking: '{"a":1}' });
    document.body.append(el);
    await tick();
    const root = el.shadowRoot;
    expect(root?.mode).toBe('open');
    expect([...(root?.children ?? [])].map((c) => c.localName)).toEqual(['style', 'iframe']);
    expect(root?.querySelector('style')?.textContent).toBe(SHADOW_STYLE);
    expect(root?.querySelector('iframe')?.getAttribute('src')).toBe('about:blank');
    expect(el.customTracking).toBe('{"a":1}');
    expect(el.pageUrl).toBe('');
    const enumerable = Object.keys(el);
    expect(enumerable).toContain('customTracking');
    expect(enumerable).toContain('pageUrl');
    expect(enumerable).not.toContain('sendCommand');
    for (const method of ['setPageUrl', 'sendCommand', 'setAudioMuted', 'reload'])
      expect(typeof (el as unknown as Record<string, unknown>)[method]).toBe('function');
    el.customTracking = null;
    expect(el.hasAttribute('customtracking')).toBe(false);
    el.customTracking = '{"b":2}';
    el.pageUrl = 'https://example.com/a';
    expect(el.getAttribute('pageurl')).toBe('https://example.com/a');
    el.pageUrl = null;
    expect(el.getAttribute('pageurl')).toBe('');
    el.setPageUrl?.(undefined);
    expect(el.getAttribute('pageurl')).toBe('');
  });

  it('defines the attribute-backed own properties in ow-electron order [OBS]', async () => {
    await startRuntime();
    const el = createAd({ adstyle: 'high-impact-ad;', unit: 'u1' });
    (el as unknown as Record<string, unknown>)['slotsize'] = '400x600';
    (el as unknown as Record<string, unknown>)['performance'] = null;
    document.body.append(el);
    await tick();
    const own = Object.keys(el).filter((key) => !['box', 'shown'].includes(key));
    expect(own.filter((key) => !key.startsWith('getBoundingClientRect'))).toEqual(
      expect.arrayContaining(['cid', 'slotsize', 'pageUrl', 'performance', 'unit', 'adstyle']),
    );
    const order = [
      'cid',
      'slotsize',
      'pageUrl',
      'performance',
      'unit',
      'adstyle',
      'customTracking',
    ];
    expect(Object.keys(el).filter((key) => order.includes(key))).toEqual(order);
    const view = el as unknown as Record<string, unknown>;
    expect(callsOf('adview_mount')[0]).toMatchObject({ attributes: { slotsize: '400x600' } });
    expect(view['cid']).toBe('main');
    expect(view['slotsize']).toBe('400x600');
    expect(view['unit']).toBe('u1');
    expect(view['adstyle']).toBe('high-impact-ad;');
    expect(view['performance']).toBe(false);
    view['performance'] = 1;
    expect(el.getAttribute('performance')).toBe('');
    expect(view['performance']).toBe(true);
    view['performance'] = false;
    expect(el.hasAttribute('performance')).toBe(false);
    view['unit'] = null;
    expect(el.hasAttribute('unit')).toBe(false);
    expect(view['unit']).toBe('');
    view['cid'] = 7;
    expect(el.getAttribute('cid')).toBe('7');
  });

  it('adopts the default style again when the page replaces adoptedStyleSheets', async () => {
    await startRuntime();
    const sheets = [...document.adoptedStyleSheets];
    expect(sheets).toHaveLength(1);
    document.adoptedStyleSheets = [];
    const el = createAd({}, { width: 0, height: 0 });
    document.body.append(el);
    await tick();
    expect(document.adoptedStyleSheets).toEqual(sheets);
    expect(services.warnings.join('\n')).toContain('owadview:style');
    // Adopted: an empty box does not adopt it twice.
    runtime.flush();
    expect(document.adoptedStyleSheets).toHaveLength(1);
  });

  it('keeps values set on the plain element by moving them into attributes', async () => {
    await startRuntime();
    const el = createAd();
    el.customTracking = '{"from":"property"}';
    el.pageUrl = 42;
    document.body.append(el);
    await tick();
    expect(callsOf('adview_mount')[0]).toMatchObject({
      attributes: { customTracking: { from: 'property' }, pageurl: '42' },
    });
    expect(Object.getOwnPropertyDescriptor(el, 'pageUrl')?.get).toBeTypeOf('function');
  });

  it('finds elements already in the document, from HTML, and through upgrade()', async () => {
    document.body.innerHTML = '<owadview cid="early" slotsize="300x250"></owadview>';
    const early = layout(document.body.firstElementChild as HTMLElement);
    await startRuntime();
    expect(runtime.elementId(early)).toBeDefined();
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(1);
    const holder = document.createElement('div');
    holder.innerHTML = '<owadview cid="parsed"></owadview>';
    layout(holder.firstElementChild as HTMLElement);
    document.body.append(holder);
    await tick();
    expect(callsOf('adview_mount').map((c) => (c['attributes'] as { cid: string }).cid)).toEqual([
      'early',
      'parsed',
    ]);
    runtime.upgrade(document.createElement('div'));
    const hidden = document.createElement('section');
    hidden.innerHTML = '<owadview cid="unseen"></owadview>';
    const unseen = layout(hidden.firstElementChild as HTMLElement);
    runtime.upgrade(unseen);
    expect(runtime.elements()).not.toContain(unseen);
  });

  it('waits for a box before mounting an element that is not a performance ad', async () => {
    await startRuntime();
    const el = createAd({}, { width: 0 });
    document.body.append(el);
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(0);
    el.box.width = 300;
    FakeResizeObserver.instances[0]?.fire();
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(1);
  });

  it('leaves elements in shadow roots and other documents inert, with one warning', async () => {
    await startRuntime();
    const host = document.createElement('div');
    document.body.append(host);
    const shadow = host.attachShadow({ mode: 'open' });
    const inner = createAd();
    shadow.append(inner);
    runtime.upgrade(inner);
    expect(services.warnings.join('\n')).toContain('owadview:shadow');
    const other = document.implementation.createHTMLDocument('x');
    other.createElement('owadview');
    expect(services.warnings.join('\n')).toContain('owadview:foreign-document');
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(0);
  });

  it('logs when the engine refuses a shadow root', async () => {
    await startRuntime();
    const el = createAd();
    el.attachShadow = () => {
      throw new DOMException('not supported', 'NotSupportedError');
    };
    document.body.append(el);
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(1);
    expect(services.logs).toContain(
      'debug: <owadview> shadow root is not available on this engine',
    );
  });
});

describe('lifecycle (B.3.2, B.3.4)', () => {
  it('remounts when a remount attribute changes and updates otherwise', async () => {
    await startRuntime();
    const el = createAd();
    document.body.append(el);
    await tick();
    el.setAttribute('cid', 'other');
    await tick();
    expect(names()).toEqual(['adview_mount', 'adview_unmount', 'adview_mount']);
    services.calls.length = 0;
    el.setAttribute('customtracking', '{"c":3}');
    await tick();
    el.setPageUrl?.('https://example.com/next');
    await tick();
    el.setAttribute('customtracking', '{"c":3}');
    await tick();
    expect(services.calls).toEqual([
      {
        name: 'adview_update',
        args: { elementId: runtime.elementId(el), attributes: { customTracking: { c: 3 } } },
      },
      {
        name: 'adview_update',
        args: {
          elementId: runtime.elementId(el),
          attributes: { pageurl: 'https://example.com/next' },
        },
      },
    ]);
  });

  it('merges updates queued behind a pending mount', async () => {
    let release!: () => void;
    services.impls['adview_mount'] = () =>
      new Promise<void>((resolve) => {
        release = resolve;
      });
    await startRuntime();
    const el = createAd();
    document.body.append(el);
    await tick();
    el.setAttribute('customtracking', '{"a":1}');
    await tick();
    el.setAttribute('pageurl', 'https://example.com/');
    el.box.x = 50;
    await tick();
    runtime.flush();
    release();
    await tick();
    expect(callsOf('adview_update')).toEqual([
      {
        elementId: runtime.elementId(el),
        attributes: { customTracking: { a: 1 }, pageurl: 'https://example.com/' },
        rect: { x: 50, y: 20, width: 300, height: 250, devicePixelRatio: 1 },
      },
    ]);
  });

  it('never sends an update made for one mount to another (remount race)', async () => {
    let release!: () => void;
    let first = true;
    services.impls['adview_mount'] = () => {
      if (!first) return null;
      first = false;
      return new Promise<void>((resolve) => {
        release = resolve;
      });
    };
    await startRuntime();
    const el = createAd();
    document.body.append(el);
    await tick();
    const firstId = runtime.elementId(el);
    el.box.x = 50;
    runtime.flush(); // update A, for mount 1
    el.setAttribute('cid', 'other');
    await tick(); // unmount + mount 2 at x = 50
    const secondId = runtime.elementId(el);
    expect(secondId).not.toBe(firstId);
    el.box.x = 99;
    runtime.flush(); // update B, for mount 2
    release();
    await tick();
    expect(
      services.calls.map((c) => [
        c.name,
        c.args['elementId'],
        (c.args['rect'] as Box | undefined)?.x,
      ]),
    ).toEqual([
      ['adview_mount', firstId, 10],
      ['adview_unmount', firstId, undefined],
      ['adview_mount', secondId, 50],
      ['adview_update', secondId, 99],
    ]);
  });

  it('drops late events of a replaced guest', async () => {
    await startRuntime();
    const el = createAd();
    document.body.append(el);
    await tick();
    const oldId = runtime.elementId(el);
    el.setAttribute('cid', 'other');
    await tick();
    const listener = vi.fn();
    el.addEventListener('display_ad_loaded', listener);
    services.emit({ elementId: oldId, name: 'display_ad_loaded', source: 'guest' });
    expect(listener).not.toHaveBeenCalled();
    services.emit({ elementId: runtime.elementId(el), name: 'display_ad_loaded', source: 'guest' });
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it('closes an element whose mount fails after an update attribute changed meanwhile', async () => {
    let reject!: (error: Error) => void;
    services.impls['adview_mount'] = () =>
      new Promise<void>((_resolve, fail) => {
        reject = fail;
      });
    await startRuntime();
    const el = createAd();
    document.body.append(el);
    await tick();
    el.setAttribute('customtracking', '{"a":1}');
    await tick();
    vi.useFakeTimers();
    try {
      reject(new Error('no guest'));
      await vi.advanceTimersByTimeAsync(0);
      el.box.x = 70;
      await vi.advanceTimersByTimeAsync(VISIBILITY_POLL_MS * 3);
      runtime.flush();
      await vi.advanceTimersByTimeAsync(0);
    } finally {
      vi.useRealTimers();
    }
    expect(names()).toEqual(['adview_mount']);
    el.setAttribute('cid', 'retry');
    await tick();
    expect(names()).toEqual(['adview_mount', 'adview_mount']);
  });

  it('runs a geometry pass when content elsewhere moves a mounted element', async () => {
    const frames: (() => void)[] = [];
    await startRuntime({
      frame: (callback) => {
        frames.push(callback);
      },
    });
    const el = createAd();
    document.body.append(el);
    await tick();
    for (const frame of frames.splice(0)) frame();
    runtime.flush();
    await tick();
    services.calls.length = 0;
    frames.length = 0;
    // A sibling above grows: no resize of the element, no scroll.
    const banner = document.createElement('div');
    document.body.prepend(banner);
    el.box.y = 120;
    await tick();
    expect(frames.length).toBeGreaterThan(0);
    for (const frame of frames.splice(0)) frame();
    await tick();
    expect(callsOf('adview_update')).toEqual([
      {
        elementId: runtime.elementId(el),
        rect: { x: 10, y: 120, width: 300, height: 250, devicePixelRatio: 1 },
      },
    ]);
    services.calls.length = 0;
    banner.className = 'tall';
    el.box.y = 200;
    await tick();
    for (const frame of frames.splice(0)) frame();
    await tick();
    expect(callsOf('adview_update')).toHaveLength(1);
    // Nothing mounted: the layout observer is off.
    el.remove();
    await tick();
    for (const frame of frames.splice(0)) frame();
    await tick();
    frames.length = 0;
    banner.className = 'short';
    await tick();
    expect(frames).toHaveLength(0);
  });

  it('unmounts when the element or an ancestor leaves the document', async () => {
    await startRuntime();
    const wrapper = document.createElement('div');
    const a = createAd({ cid: 'a' });
    const b = createAd({ cid: 'b' });
    wrapper.append(a);
    document.body.append(wrapper, b);
    await tick();
    expect(runtime.elements()).toEqual([a, b]);
    b.remove();
    await tick();
    wrapper.remove();
    await tick();
    expect(callsOf('adview_unmount')).toEqual([
      { elementId: runtime.elementId(b) },
      { elementId: runtime.elementId(a) },
    ]);
    expect(runtime.elements()).toEqual([]);
    a.sendCommand?.('late');
    await tick();
    expect(callsOf('adview_command')).toHaveLength(0);
    expect(services.logs.join('\n')).toContain('sendCommand() before attach is ignored');
    document.body.append(wrapper);
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(3);
  });

  it('reports geometry and visibility changes', async () => {
    await startRuntime();
    const el = createAd();
    document.body.append(el);
    await tick();
    const io = FakeIntersectionObserver.instances[0];
    expect(io?.options.threshold).toEqual([0, 0.5, 1]);
    expect(io?.observed.has(el)).toBe(true);
    io?.fire(el, 0.3);
    io?.fire(el, 0.3);
    el.box = { x: 0, y: 0, width: 320, height: 50 };
    FakeResizeObserver.instances[0]?.fire();
    await tick();
    io?.fire(el, 1);
    await tick();
    el.shown = false;
    window.dispatchEvent(new Event('resize'));
    await tick();
    el.shown = true;
    document.dispatchEvent(new Event('scroll'));
    await tick();
    Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true });
    document.dispatchEvent(new Event('visibilitychange'));
    await tick();
    Reflect.deleteProperty(document, 'visibilityState');
    const id = runtime.elementId(el);
    expect(callsOf('adview_update')).toEqual([
      {
        elementId: id,
        rect: { x: 0, y: 0, width: 320, height: 50, devicePixelRatio: 1 },
        visible: false,
      },
      { elementId: id, visible: true },
      { elementId: id, visible: false },
      { elementId: id, visible: true },
      { elementId: id, visible: false },
    ]);
  });

  it('computes visibility itself without observers or checkVisibility()', async () => {
    await startRuntime({ ResizeObserver: undefined, IntersectionObserver: undefined });
    const el = createAd({}, { x: 900, y: 0, width: 300, height: 100 });
    (el as { checkVisibility?: unknown }).checkVisibility = undefined;
    let rects = 1;
    el.getClientRects = () => ({ length: rects }) as DOMRectList;
    document.body.append(el);
    await tick();
    expect(callsOf('adview_mount')[0]?.['visible']).toBe(false);
    el.box.x = 0;
    runtime.flush();
    await tick();
    el.style.opacity = '0';
    runtime.flush();
    await tick();
    el.style.opacity = '';
    runtime.flush();
    await tick();
    rects = 0;
    runtime.flush();
    await tick();
    el.box = { x: 0, y: 0, width: 0, height: 0 };
    runtime.flush();
    await tick();
    expect(callsOf('adview_update').map((u) => u['visible'])).toEqual([
      true,
      false,
      true,
      false,
      undefined,
    ]);
  });

  it('polls visibility while a guest is mounted', async () => {
    const setSpy = vi.spyOn(globalThis, 'setInterval');
    const clearSpy = vi.spyOn(globalThis, 'clearInterval');
    await startRuntime();
    const el = createAd();
    document.body.append(el);
    await tick();
    const poll = setSpy.mock.calls.find((c) => c[1] === VISIBILITY_POLL_MS);
    expect(poll).toBeDefined();
    el.shown = false;
    (poll?.[0] as () => void)();
    await tick();
    expect(callsOf('adview_update')).toEqual([
      { elementId: runtime.elementId(el), visible: false },
    ]);
    el.remove();
    await tick();
    expect(clearSpy).toHaveBeenCalled();
  });

  it('closes an element whose mount failed until its attributes change', async () => {
    services.impls['adview_mount'] = () => {
      throw new Error('no guest');
    };
    await startRuntime();
    const el = createAd();
    document.body.append(el);
    await tick();
    expect(services.logs.join('\n')).toContain('did not mount: no guest');
    runtime.flush();
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(1);
    services.impls['adview_mount'] = () => null;
    el.setAttribute('slotsize', '320x50');
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(2);
  });

  it('logs failed updates, unmounts and commands', async () => {
    const fail = (): never => {
      throw new Error('gone');
    };
    services.impls['adview_update'] = fail;
    services.impls['adview_unmount'] = fail;
    services.impls['adview_command'] = fail;
    await startRuntime();
    const el = createAd();
    document.body.append(el);
    await tick();
    el.setAttribute('pageurl', 'https://example.com/');
    el.reload?.();
    await tick();
    el.remove();
    await tick();
    const logs = services.logs.join('\n');
    expect(logs).toContain('adview_update e');
    expect(logs).toContain('adview_command reload failed: gone');
    expect(logs).toContain('adview_unmount e');
  });
});

describe('performance ads (B.3.4)', () => {
  it('mounts one performance element over the whole viewport', async () => {
    await startRuntime();
    const first = createAd({ performance: '' }, { width: 0, height: 0 });
    const second = createAd({ performance: '', cid: 'second' });
    document.body.append(first, second);
    await tick();
    expect(callsOf('adview_mount')).toEqual([
      expect.objectContaining({
        elementId: runtime.elementId(first),
        rect: {
          x: 0,
          y: 0,
          width: window.innerWidth,
          height: window.innerHeight,
          devicePixelRatio: 1,
        },
        visible: true,
      }),
    ]);
    expect(services.warnings.join('\n')).toContain(
      `owadview:performance:${String(runtime.elementId(second))}`,
    );
  });

  it('closes the guest on shutdown and reopens on an attribute change', async () => {
    await startRuntime();
    const el = createAd({ performance: '' });
    document.body.append(el);
    await tick();
    services.emit({ elementId: runtime.elementId(el), name: 'shutdown', source: 'guest' });
    await tick();
    expect(names()).toEqual(['adview_mount', 'adview_unmount']);
    runtime.flush();
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(1);
    el.setAttribute('cid', 'again');
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(2);
  });
});

describe('events (B.3.5)', () => {
  async function mounted(): Promise<AdElement> {
    await startRuntime();
    const el = createAd();
    document.body.append(el);
    await tick();
    return el;
  }

  it('dispatches plain, non-bubbling events with the data as own properties [OBS]', async () => {
    const el = await mounted();
    const seen: Event[] = [];
    const parent = vi.fn();
    document.body.addEventListener('display_ad_loaded', parent);
    el.addEventListener('display_ad_loaded', (event) => seen.push(event));
    const id = runtime.elementId(el);
    services.emit({
      elementId: id,
      name: 'display_ad_loaded',
      data: { size: '300x250', type: 'x' },
    });
    services.emit({ elementId: id, name: 'display_ad_loaded', data: null });
    expect(seen).toHaveLength(2);
    const [event] = seen;
    expect(event?.constructor).toBe(Event);
    expect(event?.type).toBe('display_ad_loaded');
    expect(event?.bubbles).toBe(false);
    expect(event?.cancelable).toBe(false);
    expect(event?.isTrusted).toBeFalsy();
    expect(Object.getOwnPropertyDescriptor(event, 'size')?.value).toBe('300x250');
    expect(parent).not.toHaveBeenCalled();
  });

  it('dispatches both spellings of clicks and house-ad actions', async () => {
    const el = await mounted();
    const order: string[] = [];
    for (const name of ['ad_clicked', 'ad-clicked', 'house_ad_action', 'house-ad-action'])
      el.addEventListener(name, () => order.push(name));
    const id = runtime.elementId(el);
    services.emit({ elementId: id, name: 'house-ad-action', data: { action: 'open' } });
    services.emit({ elementId: id, name: 'ad_clicked', source: 'guest' });
    clock = 500;
    services.emit({ elementId: id, name: 'ad-clicked', source: 'host' });
    clock = 2000;
    services.emit({ elementId: id, name: 'ad-clicked', source: 'host' });
    expect(order).toEqual([
      'house-ad-action',
      'house_ad_action',
      'ad_clicked',
      'ad-clicked',
      'ad-clicked',
      'ad_clicked',
    ]);
  });

  it('ignores internal, malformed and unknown-element messages', async () => {
    const el = await mounted();
    const listener = vi.fn();
    el.addEventListener('__host:navigate', listener);
    services.emit({ elementId: runtime.elementId(el), name: '__host:navigate' });
    services.emit({ elementId: 1, name: 'x' });
    services.emit({ elementId: runtime.elementId(el), name: '' });
    services.emit({ elementId: 'e999', name: 'impression' });
    expect(listener).not.toHaveBeenCalled();
    expect(services.logs.join('\n')).toContain(
      "adview-event 'impression' for unknown or replaced element e999",
    );
  });

  it('createAdviewEvent keeps Event members and ignores non-plain data', () => {
    expect(createAdviewEvent('x', { type: 'y', n: 1 }).type).toBe('x');
    expect(Object.keys(createAdviewEvent('x', [1, 2]))).not.toContain('0');
    expect(Object.keys(createAdviewEvent('x', new Date()))).toEqual(Object.keys(new Event('x')));
    expect(createAdviewEvent('x', Object.create(null) as object)).toBeInstanceOf(Event);
    expect(parseCustomTracking('[1]')).toEqual([1]);
    expect(parseCustomTracking('"text"')).toBeNull();
    expect(parseCustomTracking('{')).toBeNull();
  });
});

describe('element methods (B.3.3)', () => {
  it('send adview_command with JSON arguments after attach', async () => {
    await startRuntime();
    const el = createAd();
    document.body.append(el);
    await tick();
    el.sendCommand?.('play', { at: 1n, skip: undefined }, () => undefined);
    const circular: Record<string, unknown> = {};
    circular['self'] = circular;
    el.sendCommand?.('loop', circular);
    el.setAudioMuted?.('yes');
    el.setAudioMuted?.(true);
    el.reload?.();
    await tick();
    const id = runtime.elementId(el);
    expect(callsOf('adview_command')).toEqual([
      { elementId: id, command: 'sendCommand', args: ['play', { at: '1' }, null] },
      { elementId: id, command: 'sendCommand', args: [] },
      { elementId: id, command: 'setAudioMuted', args: [false] },
      { elementId: id, command: 'setAudioMuted', args: [true] },
      { elementId: id, command: 'reload', args: [] },
    ]);
  });
});

describe('stop and the runtime singleton', () => {
  it('stop() restores createElement and forgets the elements', async () => {
    await startRuntime();
    expect(Object.hasOwn(window.Document.prototype, 'createElement')).toBe(true);
    const el = createAd();
    document.body.append(el);
    await tick();
    runtime.stop();
    runtime.stop();
    expect(Object.hasOwn(window.Document.prototype, 'createElement')).toBe(false);
    expect(runtime.elements()).toEqual([]);
    expect(runtime.elementId(document.createElement('owadview'))).toBeUndefined();
    runtime.schedule();
    runtime.flush();
    await tick();
    expect(callsOf('adview_unmount')).toHaveLength(0);
  });

  it('restores an own createElement it replaced', async () => {
    const proto = window.Document.prototype as unknown as Record<string, unknown>;
    const own = function (this: Document, name: string): Element {
      return Document.prototype.createElementNS.call(this, 'http://www.w3.org/1999/xhtml', name);
    };
    Object.defineProperty(proto, 'createElement', {
      value: own,
      configurable: true,
      writable: true,
    });
    await startRuntime();
    expect(proto['createElement']).not.toBe(own);
    runtime.stop();
    expect(proto['createElement']).toBe(own);
    Reflect.deleteProperty(proto, 'createElement');
  });

  it('browserEnvironment() runs frames once and tells the time', async () => {
    const env = browserEnvironment();
    expect(env.document).toBe(document);
    const callback = vi.fn();
    env.frame(callback);
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(callback).toHaveBeenCalledTimes(1);
    expect(env.now()).toBeGreaterThan(0);
  });

  it('adviewRuntimeOf() registers one runtime per kernel and stops it on reset', async () => {
    const hooks = new Set<() => void>();
    const kernel: AdviewKernel = {
      ...fakeServices(),
      onReset: (hook) => {
        hooks.add(hook);
        return () => hooks.delete(hook);
      },
    };
    runtime = adviewRuntimeOf(kernel) as AdviewRuntime;
    expect(kernel.owadview).toBe(runtime);
    expect(adviewRuntimeOf(kernel)).toBe(runtime);
    expect(runtime.started).toBe(true);
    for (const hook of [...hooks]) hook();
    expect(hooks.size).toBe(0);
    expect(runtime.started).toBe(false);
    expect(kernel.owadview).toBeUndefined();
    await tick();
  });

  it('adviewRuntimeOf() uses the runtime another copy registered', () => {
    const registered = { upgrade: vi.fn(), elements: () => [] };
    const kernel: AdviewKernel = {
      ...fakeServices(),
      owadview: registered,
      onReset: () => () => undefined,
    };
    expect(adviewRuntimeOf(kernel)).toBe(registered);
  });
});

describe('ow-tauri/renderer owadview', () => {
  let host: MockHost | undefined;

  afterEach(() => {
    setHostContext(null);
    host?.dispose();
    host = undefined;
  });

  it('is available in UI windows only', async () => {
    host = mockHost({ label: 'ow-main' });
    runtime = new AdviewRuntime(fakeServices('none'), environment());
    expect(() => owadview.elements()).toThrow(OwTauriError);
    host.dispose();
    host = mockHost({ label: 'bw-1' });
    await settle();
    const el = createAd();
    document.body.append(el);
    owadview.upgrade(el);
    expect(owadview.elements()).toEqual([el]);
    await settle();
    expect(host.callsOf('adview_mount')).toHaveLength(1);
  });
});
