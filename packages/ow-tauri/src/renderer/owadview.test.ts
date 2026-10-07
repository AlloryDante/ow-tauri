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
  PERFORMANCE_OVERLAY_STYLE,
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
  setPageUrl?: (...args: unknown[]) => void;
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
    // As on ow-electron's element, the methods are inherited, not own
    // (regression: they were own properties).
    const proto = Object.getPrototypeOf(el) as object;
    for (const method of ['setPageUrl', 'sendCommand', 'setAudioMuted', 'reload']) {
      expect(typeof (el as unknown as Record<string, unknown>)[method]).toBe('function');
      expect(Object.hasOwn(el, method)).toBe(false);
      expect(Object.hasOwn(proto, method)).toBe(true);
    }
    expect(el).toBeInstanceOf(HTMLElement);
    const second = createAd({ cid: 'second' });
    document.body.append(second);
    await tick();
    expect(Object.getPrototypeOf(second)).toBe(proto);
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

  it('logs once per page when the engine refuses a shadow root', async () => {
    await startRuntime();
    const refuse = () => {
      throw new DOMException('not supported', 'NotSupportedError');
    };
    const el = createAd();
    el.attachShadow = refuse;
    document.body.append(el);
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(1);
    // Every later ad is refused too: no new attempt, no new line.
    const second = createAd();
    let attempts = 0;
    second.attachShadow = () => {
      attempts += 1;
      return refuse();
    };
    document.body.append(second);
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(2);
    expect(attempts).toBe(0);
    expect(
      services.logs.filter(
        (l) => l === 'debug: <owadview> shadow root is not available on this engine',
      ),
    ).toHaveLength(1);
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
        name: 'adview_command',
        args: {
          elementId: runtime.elementId(el),
          command: 'setPageUrl',
          args: ['https://example.com/next'],
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

  it('never merges a hide and a show into one update within a frame (AF-16)', async () => {
    await startRuntime();
    const el = createAd({ adstyle: 'rewarded-ad;' }, { width: 400, height: 300 });
    document.body.append(el);
    await tick();
    const id = runtime.elementId(el);
    // Two passes before the queued update runs: hidden, then visible again.
    el.shown = false;
    runtime.flush();
    el.box.x = 40;
    el.shown = true;
    runtime.flush();
    await tick();
    expect(callsOf('adview_update')).toEqual([
      { elementId: id, visible: false },
      {
        elementId: id,
        visible: true,
        rect: { x: 40, y: 20, width: 400, height: 300, devicePixelRatio: 1 },
      },
    ]);
  });

  it('queues a hide and a show behind an in-flight command as two updates (AF-16)', async () => {
    let release!: () => void;
    services.impls['adview_mount'] = () =>
      new Promise<void>((resolve) => {
        release = resolve;
      });
    await startRuntime();
    const el = createAd({ adstyle: 'rewarded-ad;' }, { width: 400, height: 300 });
    document.body.append(el);
    await tick();
    const id = runtime.elementId(el);
    // The app hides the reward slot and shows it again while the mount is in flight.
    el.shown = false;
    runtime.flush();
    await tick();
    el.box.y = 60;
    runtime.flush();
    el.shown = true;
    runtime.flush();
    await tick();
    el.box.y = 80;
    runtime.flush();
    el.shown = false;
    runtime.flush();
    el.shown = false;
    runtime.flush();
    await tick();
    expect(callsOf('adview_update')).toEqual([]);
    release();
    await tick();
    expect(callsOf('adview_update')).toEqual([
      {
        elementId: id,
        visible: false,
        rect: { x: 10, y: 60, width: 400, height: 300, devicePixelRatio: 1 },
      },
      {
        elementId: id,
        visible: true,
        rect: { x: 10, y: 80, width: 400, height: 300, devicePixelRatio: 1 },
      },
      { elementId: id, visible: false },
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
    // Removed after attach: dead, as on ow-electron [OBS].
    document.body.append(wrapper);
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(2);
    expect(runtime.elements()).toEqual([]);
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

/** The events the official sample's performance ad listens to (`performance-ad.tsx`). */
const SAMPLE_PERFORMANCE_EVENTS = [
  'complete',
  'impression',
  'shutdown',
  'performance_ad_no_fill',
  'performance_ad_dismiss',
  'performance_ad_loaded',
  'performance_ad_clicked',
  'performance_ad_video_complete',
  'performance_ad_video_skipped',
];

/**
 * The official sample's `performanceAd()` (`performance-ad.tsx`), with the
 * events it receives recorded instead of logged.
 */
function samplePerformanceAd(seen: string[]): AdElement {
  const performanceAdview = document.createElement('owadview');
  performanceAdview.setAttribute('performance', '');
  for (const event of [...SAMPLE_PERFORMANCE_EVENTS, 'destroyed'])
    performanceAdview.addEventListener(event, () => seen.push(event));
  document.body.appendChild(performanceAdview);
  return layout(performanceAdview, { width: 0, height: 0 });
}

describe('performance ads (B.3.4)', () => {
  it('mounts one performance element over the whole viewport', async () => {
    await startRuntime();
    const first = createAd({ performance: '' }, { width: 0, height: 0 });
    document.body.append(first);
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
  });

  it('removes a second performance element in the same tick, silently [OBS]', async () => {
    await startRuntime();
    const seen: string[] = [];
    const first = samplePerformanceAd(seen);
    await tick();
    // The sample's button pressed again while the first ad is up.
    const second = samplePerformanceAd(seen);
    expect(second.isConnected).toBe(true);
    // Mutation observers run at the end of this task, before any timer.
    await Promise.resolve();
    await Promise.resolve();
    expect(second.isConnected).toBe(false);
    expect(first.isConnected).toBe(true);
    await tick();
    expect(names()).toEqual(['adview_mount']);
    expect(seen).toEqual([]);
    expect(services.warnings).toEqual([]);
    expect(services.logs.join('\n')).toContain(
      `debug: <owadview> ${String(runtime.elementId(second))}: a window shows one performance ad at a time; this one is removed`,
    );
  });

  it('removes the element in a task after shutdown, with no destroyed [OBS]', async () => {
    await startRuntime();
    const seen: string[] = [];
    const el = samplePerformanceAd(seen);
    await tick();
    let connectedInListener: boolean | undefined;
    el.addEventListener('shutdown', () => (connectedInListener = el.isConnected));
    vi.useFakeTimers();
    try {
      services.emit({
        elementId: runtime.elementId(el),
        name: 'shutdown',
        data: {},
        source: 'guest',
      });
      expect(connectedInListener).toBe(true);
      expect(el.isConnected).toBe(true);
      await vi.advanceTimersByTimeAsync(0);
      expect(el.isConnected).toBe(false);
    } finally {
      vi.useRealTimers();
    }
    await tick();
    expect(seen).toEqual(['shutdown']);
    expect(names()).toEqual(['adview_mount', 'adview_unmount']);
    // Dead: re-inserting it, or changing it, attaches nothing.
    document.body.append(el);
    el.setAttribute('cid', 'again');
    await tick();
    runtime.flush();
    expect(callsOf('adview_mount')).toHaveLength(1);
    expect(services.logs.join('\n')).toContain('does not attach again');
  });

  it('leaves an element the app removed before the shutdown task alone', async () => {
    await startRuntime();
    const el = samplePerformanceAd([]);
    await tick();
    el.addEventListener('shutdown', () => {
      el.remove();
    });
    services.emit({ elementId: runtime.elementId(el), name: 'shutdown', source: 'guest' });
    await tick();
    expect(el.isConnected).toBe(false);
    expect(names()).toEqual(['adview_mount', 'adview_unmount']);
  });

  it('dispatches destroyed, then unmounts, when the app removes it [OBS]', async () => {
    await startRuntime();
    const seen: string[] = [];
    const el = samplePerformanceAd(seen);
    await tick();
    let callsAtDestroyed: string[] | undefined;
    let destroyed: Event | undefined;
    el.addEventListener('destroyed', (event) => {
      destroyed = event;
      callsAtDestroyed = names();
    });
    document.body.removeChild(el);
    await tick();
    expect(seen).toEqual(['destroyed']);
    expect(callsAtDestroyed).toEqual(['adview_mount']);
    expect(names()).toEqual(['adview_mount', 'adview_unmount']);
    expect(destroyed?.constructor).toBe(Event);
    expect(destroyed?.bubbles).toBe(false);
    const base = new Set(Object.keys(new Event('x')));
    expect(Object.keys(destroyed ?? {}).filter((key) => !base.has(key))).toEqual([]);
    // A new element works as before.
    samplePerformanceAd(seen);
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(2);
  });
});

describe('performance DOM shape (B.3.4) [OBS]', () => {
  it('gives a performance element no shadow root, an inline style and one overlay div', async () => {
    await startRuntime();
    const el = samplePerformanceAd([]);
    expect(el.getAttribute('style')).toBeNull();
    await tick();
    expect(el.shadowRoot).toBeNull();
    expect(el.getAttribute('style')).toBe('pointer-events: none;');
    expect(el.children).toHaveLength(1);
    const div = el.firstElementChild as HTMLElement;
    expect(div.localName).toBe('div');
    expect([...div.attributes].map((a) => [a.name, a.value])).toEqual([
      ['style', PERFORMANCE_OVERLAY_STYLE],
    ]);
    expect(PERFORMANCE_OVERLAY_STYLE).toBe(
      'position: fixed; top: 0px; left: 0px; width: 100vw; height: 100vh; background: transparent; z-index: 999999;',
    );
    // A remount (an attribute change) keeps exactly one div.
    el.setAttribute('unit', 'u1');
    await tick();
    expect(names()).toEqual(['adview_mount', 'adview_unmount', 'adview_mount']);
    expect(el.children).toHaveLength(1);
    expect(el.shadowRoot).toBeNull();
  });

  it('is a 0x0 box with a flex overlay; other elements keep the block default', async () => {
    await startRuntime();
    const el = samplePerformanceAd([]);
    const standard = document.createElement('owadview');
    document.body.append(standard);
    await tick();
    const style = getComputedStyle(el);
    expect([style.display, style.width, style.height]).toEqual(['block', '0px', '0px']);
    const overlay = el.firstElementChild;
    expect(overlay).not.toBeNull();
    if (overlay) expect(getComputedStyle(overlay).display).toBe('flex');
    expect(getComputedStyle(standard).width).not.toBe('0px');
    expect(DEFAULT_STYLE).toContain(':where(owadview[performance]) { width: 0; height: 0; }');
  });

  it('switches to pointer-events auto at the first display_ad_loaded, before dispatch', async () => {
    await startRuntime();
    const el = samplePerformanceAd([]);
    await tick();
    const id = runtime.elementId(el);
    const atDispatch: (string | null)[] = [];
    el.addEventListener('display_ad_loaded', () => atDispatch.push(el.getAttribute('style')));
    services.emit({ elementId: id, name: 'impression', source: 'guest' });
    expect(el.getAttribute('style')).toBe('pointer-events: none;');
    services.emit({ elementId: id, name: 'display_ad_loaded', data: {}, source: 'guest' });
    services.emit({ elementId: id, name: 'display_ad_loaded', data: {}, source: 'guest' });
    expect(atDispatch).toEqual(['pointer-events: auto;', 'pointer-events: auto;']);
    // The div's own style is unchanged; it inherits the element's value.
    expect((el.firstElementChild as HTMLElement).getAttribute('style')).toBe(
      PERFORMANCE_OVERLAY_STYLE,
    );
  });

  it('leaves the style of a standard element alone', async () => {
    await startRuntime();
    const el = createAd();
    document.body.append(el);
    await tick();
    services.emit({ elementId: runtime.elementId(el), name: 'display_ad_loaded' });
    expect(el.getAttribute('style')).toBeNull();
    expect(el.children).toHaveLength(0);
    expect(el.shadowRoot).not.toBeNull();
  });

  it('drops the overlay when the element stops being a performance ad', async () => {
    await startRuntime();
    const el = samplePerformanceAd([]);
    el.box = { x: 0, y: 0, width: 300, height: 250 };
    await tick();
    el.removeAttribute('performance');
    await tick();
    expect(el.children).toHaveLength(0);
    expect(el.style.getPropertyValue('pointer-events')).toBe('');
    expect(el.shadowRoot).not.toBeNull();
  });
});

describe('element lifecycle of the official sample (B.3.4) [OBS]', () => {
  /** The sample's `startAd()` (`ad.tsx`) for one container, recording events. */
  function sampleStartAd(
    container: HTMLElement,
    id: string,
    adSize: [number, number],
    seen: string[],
    enableHighImpact = false,
  ): AdElement {
    const tempAdView = document.createElement('owadview');
    const customTrackingJsonStr = JSON.stringify({ testQAKey: 'testQAValue' });
    tempAdView.setAttribute('id', `${id}-adview`);
    tempAdView.setAttribute('cid', 'mainAd');
    tempAdView.setAttribute('slotsize', `${String(adSize[0])}x${String(adSize[1])}`);
    tempAdView.setAttribute('customTracking', customTrackingJsonStr);
    if (enableHighImpact) tempAdView.setAttribute('adstyle', 'high-impact-ad;');
    for (const name of ['display_ad_loaded', 'destroyed', 'shutdown'])
      tempAdView.addEventListener(name, () => seen.push(`${id}:${name}`));
    const ad = layout(tempAdView, { width: adSize[0], height: adSize[1] });
    container.appendChild(tempAdView);
    return ad;
  }

  it('stopAd() (removeChild) dispatches destroyed and unmounts a standard slot', async () => {
    await startRuntime();
    const seen: string[] = [];
    const container = document.createElement('div');
    container.id = 'owadview-container';
    document.body.append(container);
    const ad = sampleStartAd(container, 'owadview-container', [400, 300], seen);
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(1);
    container.removeChild(ad);
    await tick();
    expect(seen).toEqual(['owadview-container:destroyed']);
    expect(names()).toEqual(['adview_mount', 'adview_unmount']);
    // startAd() again: the sample makes a new element, which attaches.
    sampleStartAd(container, 'owadview-container', [400, 300], seen);
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(2);
  });

  it('a container removed and re-appended (documented high-impact handler) stays dead', async () => {
    await startRuntime();
    const seen: string[] = [];
    const zone = document.createElement('div');
    const tower = document.createElement('div');
    const small = document.createElement('div');
    zone.append(tower, small);
    document.body.append(zone);
    const hi = sampleStartAd(tower, 'owadview-container', [400, 600], seen, true);
    const smallAd = sampleStartAd(small, 'owadview-container2', [400, 60], seen);
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(2);
    // high-impact-ad-loaded: remove the small container; -removed: append it again.
    services.emit({ elementId: runtime.elementId(hi), name: 'high-impact-ad-loaded' });
    zone.removeChild(small);
    await tick();
    services.emit({ elementId: runtime.elementId(hi), name: 'high-impact-ad-removed' });
    zone.appendChild(small);
    await tick();
    runtime.flush();
    await tick();
    expect(seen).toEqual(['owadview-container2:destroyed']);
    expect(callsOf('adview_mount')).toHaveLength(2);
    expect(callsOf('adview_unmount')).toEqual([{ elementId: runtime.elementId(smallAd) }]);
    expect(runtime.elements()).toEqual([hi]);
    expect(services.logs.join('\n')).toContain(
      `debug: <owadview> ${String(runtime.elementId(smallAd))} was attached and removed before`,
    );
  });

  it('an element never attached may move freely', async () => {
    await startRuntime();
    const el = createAd({}, { width: 0 });
    document.body.append(el);
    await tick();
    el.remove();
    await tick();
    el.box.width = 300;
    document.body.append(el);
    await tick();
    expect(callsOf('adview_mount')).toHaveLength(1);
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

  it('spreads a string payload per character, as Object.assign does [OBS]', async () => {
    await startRuntime();
    const el = createAd({ performance: '' });
    document.body.append(el);
    await tick();
    const seen: Event[] = [];
    el.addEventListener('performance_ad_error', (event) => seen.push(event));
    const message = 'RangeError: too small';
    services.emit({
      elementId: runtime.elementId(el),
      name: 'performance_ad_error',
      data: message,
      source: 'guest',
    });
    expect(seen).toHaveLength(1);
    const event = seen[0] as Event & Record<string, unknown>;
    const base = new Set(Object.keys(new Event('x')));
    const own = Object.keys(event).filter((key) => !base.has(key));
    expect(own).toEqual(Array.from({ length: message.length }, (_v, i) => String(i)));
    expect(own.map((key) => event[key]).join('')).toBe(message);
    expect(event.type).toBe('performance_ad_error');
    expect(Object.hasOwn(event, 'length')).toBe(false);
  });

  it('createAdviewEvent copies payloads with Object.assign semantics and keeps Event members', () => {
    // Own names every Event of this engine has (happy-dom defines a few).
    const base = new Set(Object.keys(new Event('x')));
    const own = (data: unknown): string[] =>
      Object.keys(createAdviewEvent('x', data)).filter((key) => !base.has(key));
    expect(createAdviewEvent('x', { type: 'y', n: 1 }).type).toBe('x');
    expect(Object.getOwnPropertyDescriptor(createAdviewEvent('x', { n: 1 }), 'n')).toEqual({
      value: 1,
      writable: true,
      enumerable: true,
      configurable: true,
    });
    // Arrays by index, strings per character, primitives and null add nothing.
    const array = createAdviewEvent('x', ['a', { b: 2 }]) as Event & Record<string, unknown>;
    expect(own(['a', { b: 2 }])).toEqual(['0', '1']);
    expect(array['1']).toEqual({ b: 2 });
    expect(own('ab')).toEqual(['0', '1']);
    expect(own('')).toEqual([]);
    for (const value of [null, undefined, 0, 7, true, false]) expect(own(value)).toEqual([]);
    expect(own(new Date())).toEqual([]);
    expect(own({ 2: 'b', a: 1, 1: 'a' })).toEqual(['1', '2', 'a']);
    // Same result as Object.assign onto a plain object, minus the Event's own names.
    const payload = { size: '300x250', cpm: 0, nested: { a: 1 }, timeStamp: 5 };
    const expected: Record<string, unknown> = Object.assign({}, payload);
    delete expected['timeStamp'];
    const event = createAdviewEvent('x', payload) as Event & Record<string, unknown>;
    expect(Object.fromEntries(own(payload).map((key) => [key, event[key]]))).toEqual(expected);
    expect(event.timeStamp).not.toBe(5);
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

describe('sendCommand and setPageUrl reach the guest unchanged (B.3.3, AF-8)', () => {
  async function mounted(): Promise<AdElement> {
    await startRuntime();
    const el = createAd();
    document.body.append(el);
    await tick();
    services.calls.length = 0;
    return el;
  }

  it('sendCommand passes JSON arguments through as given, in order', async () => {
    const el = await mounted();
    const id = runtime.elementId(el);
    const nested = { a: [1, 'two', { three: true }], b: null, c: -0.5, d: '' };
    el.sendCommand?.('userPlay');
    el.sendCommand?.();
    el.sendCommand?.('play', 1, true, null, ['x', 2], nested, 'é😀');
    await tick();
    expect(callsOf('adview_command')).toEqual([
      { elementId: id, command: 'sendCommand', args: ['userPlay'] },
      { elementId: id, command: 'sendCommand', args: [] },
      {
        elementId: id,
        command: 'sendCommand',
        args: ['play', 1, true, null, ['x', 2], nested, 'é😀'],
      },
    ]);
    // The arguments are copies: a later change by the app does not leak in.
    expect(callsOf('adview_command')[2]?.['args']).not.toBe(nested);
  });

  it('setPageUrl sends [url] and sets the attribute, without a second update', async () => {
    const el = await mounted();
    const id = runtime.elementId(el);
    el.setPageUrl?.('https://example.com/a');
    await tick();
    el.setPageUrl?.('https://example.com/a');
    await tick();
    el.setPageUrl?.(undefined);
    await tick();
    expect(el.getAttribute('pageurl')).toBe('');
    expect(services.calls).toEqual([
      {
        name: 'adview_command',
        args: { elementId: id, command: 'setPageUrl', args: ['https://example.com/a'] },
      },
      {
        name: 'adview_command',
        args: { elementId: id, command: 'setPageUrl', args: ['https://example.com/a'] },
      },
      { name: 'adview_command', args: { elementId: id, command: 'setPageUrl', args: [null] } },
    ]);
  });

  it('setPageUrl with a non-string keeps the value and stores its string form', async () => {
    const el = await mounted();
    const id = runtime.elementId(el);
    el.setPageUrl?.(42, 'ignored');
    await tick();
    expect(el.getAttribute('pageurl')).toBe('42');
    expect(services.calls).toEqual([
      { name: 'adview_command', args: { elementId: id, command: 'setPageUrl', args: [42] } },
      { name: 'adview_update', args: { elementId: id, attributes: { pageurl: '42' } } },
    ]);
  });

  it('setPageUrl on a detached element only sets the attribute', async () => {
    const el = await mounted();
    el.remove();
    await tick();
    services.calls.length = 0;
    el.setPageUrl?.('https://example.com/late');
    await tick();
    expect(el.getAttribute('pageurl')).toBe('https://example.com/late');
    expect(services.calls).toEqual([]);
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
