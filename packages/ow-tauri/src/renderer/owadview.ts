/**
 * The `<owadview>` runtime of UI windows (`docs/CONTRACT.md` B.3,
 * [ADR 0003](../../../docs/adr/0003-owadview-native-child-webviews.md)).
 *
 * `owadview` has no hyphen, so it cannot be a custom element. The runtime
 * finds the elements itself (a wrapped `document.createElement` and a
 * `MutationObserver`), mounts one native guest webview per element through
 * the plugin (`adview_mount`), keeps its geometry and visibility current
 * (`adview_update`), closes it when the element leaves the document
 * (`adview_unmount`), and re-dispatches the guest's events on the element
 * exactly as ow-electron does (B.3.5).
 *
 * Before it is attached the element is a plain `HTMLElement`; at attach the
 * runtime defines the element members ow-electron has after attach
 * (`customTracking`, `pageUrl`, `setPageUrl`, `sendCommand`,
 * `setAudioMuted`, `reload`) and an open shadow root with a `<style>` and an
 * `<iframe>` placeholder [OBS].
 *
 * @packageDocumentation
 */
import type { HostMessageHandler } from '../bootstrap/facade-kernel.js';
import type { LogLevel } from '../bootstrap/services.js';
import type { HostContext } from '../shared/protocol.js';
import {
  ADVIEW_ATTRIBUTES,
  needsRemount,
  readAttributes,
  sameTracking,
  type AdviewAttributes,
  type AdviewRect,
} from './owadview-attributes.js';
import {
  CLICK_DEDUPE_MS,
  CLICK_NAMES,
  SPELLING_TWINS,
  createAdviewEvent,
} from './owadview-events.js';

/** The local name of the element. */
export const TAG = 'owadview';

/** The default style: zero specificity, so any app rule wins (B.3.1 item 1). */
export const DEFAULT_STYLE = ':where(owadview) { display: block; width: 100%; height: 100%; }';

/** Style of the shadow root's placeholder iframe (B.3.4, "Shadow root"). */
export const SHADOW_STYLE =
  ':host { position: relative; } iframe { position: absolute; inset: 0; width: 100%; height: 100%; border: 0; background: transparent; pointer-events: none; }';

/** Intersection ratio from which an element counts as visible (B.3.4) [DEC]. */
export const VISIBLE_RATIO = 0.5;

/** How often `checkVisibility()` is polled while guests are mounted (B.3.4). */
export const VISIBILITY_POLL_MS = 500;

/** The kernel services the runtime uses (a subset of `FacadeKernel`). */
export interface AdviewServices {
  /** Where the runtime runs; only `'ui'` mounts guests. */
  readonly context: HostContext;
  /**
   * Invokes `plugin:overwolf|<name>`.
   *
   * @param name - the command
   * @param args - its arguments
   * @returns the response
   */
  command(name: string, args?: Record<string, unknown>): Promise<unknown>;
  /**
   * Logs a message.
   *
   * @param level - the level
   * @param message - the message
   */
  log(level: LogLevel, message: string): void;
  /**
   * Logs a warning once per key.
   *
   * @param key - deduplication key
   * @param message - the message
   */
  warnOnce(key: string, message: string): void;
  /**
   * Subscribes to host messages of one type.
   *
   * @param type - the message type
   * @param handler - the handler
   * @returns a function that unsubscribes
   */
  on(type: string, handler: HostMessageHandler): () => void;
}

/** Platform hooks, replaceable in tests. */
export interface AdviewEnvironment {
  /** The document to watch. */
  readonly document: Document;
  /** Its window. */
  readonly window: Window;
  /** `ResizeObserver`, when the engine has it. */
  readonly ResizeObserver?: typeof ResizeObserver | undefined;
  /** `IntersectionObserver`, when the engine has it. */
  readonly IntersectionObserver?: typeof IntersectionObserver | undefined;
  /** `MutationObserver`. */
  readonly MutationObserver: typeof MutationObserver;
  /**
   * Runs `callback` before the next paint, or soon when the document does not paint.
   *
   * @param callback - the callback
   */
  frame(callback: () => void): void;
  /**
   * Milliseconds since an arbitrary origin.
   *
   * @returns the time
   */
  now(): number;
}

/**
 * The environment of the current document.
 *
 * @returns the environment
 */
export function browserEnvironment(): AdviewEnvironment {
  const win = globalThis as unknown as Window & typeof globalThis;
  return {
    document: win.document,
    window: win,
    ResizeObserver: typeof win.ResizeObserver === 'function' ? win.ResizeObserver : undefined,
    IntersectionObserver:
      typeof win.IntersectionObserver === 'function' ? win.IntersectionObserver : undefined,
    MutationObserver: win.MutationObserver,
    frame: (callback) => {
      // A hidden document never paints: the timeout keeps updates flowing.
      let done = false;
      const run = (): void => {
        if (done) return;
        done = true;
        callback();
      };
      if (typeof win.requestAnimationFrame === 'function') win.requestAnimationFrame(run);
      setTimeout(run, 100);
    },
    now: () => (typeof performance === 'object' ? performance.now() : Date.now()),
  };
}

/** Per-element state. */
interface Entry {
  readonly el: HTMLElement;
  readonly id: string;
  tracked: boolean;
  mounted: boolean;
  closed: boolean;
  membersDefined: boolean;
  attributes: AdviewAttributes | undefined;
  rect: AdviewRect | undefined;
  visible: boolean | undefined;
  ratio: number | undefined;
  chain: Promise<void>;
  pendingUpdate: Record<string, unknown> | undefined;
  lastGuestClick: number;
}

/** Fields of an `adview-event` host message (A.3). */
interface AdviewEventMessage {
  elementId?: unknown;
  name?: unknown;
  data?: unknown;
  source?: unknown;
}

function sameRect(a: AdviewRect | undefined, b: AdviewRect): boolean {
  return (
    a?.x === b.x &&
    a.y === b.y &&
    a.width === b.width &&
    a.height === b.height &&
    a.devicePixelRatio === b.devicePixelRatio
  );
}

/** The object in `start`'s prototype chain (itself included) that owns `name`. */
function ownerOf(start: object | null, name: string): object | undefined {
  for (let p = start; p !== null; p = Object.getPrototypeOf(p) as object | null)
    if (Object.hasOwn(p, name)) return p;
  return undefined;
}

function isAdview(node: Node): node is HTMLElement {
  return node.nodeType === 1 && (node as Element).localName.toLowerCase() === TAG;
}

/**
 * The per-document `<owadview>` runtime. The bootstrap starts one in every
 * UI window; `ow-tauri/renderer` exposes it as `owadview`.
 */
export class AdviewRuntime {
  readonly #services: AdviewServices;
  readonly #env: AdviewEnvironment;
  readonly #entries = new WeakMap<Element, Entry>();
  readonly #tracked = new Map<string, Entry>();
  #nextId = 0;
  #started = false;
  #scheduled = false;
  #poll: ReturnType<typeof setInterval> | undefined;
  #mutations: MutationObserver | undefined;
  #resize: ResizeObserver | undefined;
  #intersection: IntersectionObserver | undefined;
  #cleanups: (() => void)[] = [];

  /**
   * @param services - kernel services
   * @param env - platform hooks (default: the current document)
   */
  constructor(services: AdviewServices, env: AdviewEnvironment = browserEnvironment()) {
    this.#services = services;
    this.#env = env;
  }

  /** Whether {@link AdviewRuntime.start} ran (and {@link AdviewRuntime.stop} did not). */
  get started(): boolean {
    return this.#started;
  }

  /**
   * Starts watching the document: default style, `createElement` wrapper,
   * observers, host events. Idempotent; does nothing outside UI windows.
   */
  start(): void {
    if (this.#started || this.#services.context !== 'ui') return;
    this.#started = true;
    const { document: doc, window: win } = this.#env;
    this.#installStyle();
    this.#wrapCreateElement();
    this.#mutations = new this.#env.MutationObserver((records) => {
      this.#onMutations(records);
    });
    this.#mutations.observe(doc, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: [...ADVIEW_ATTRIBUTES],
    });
    if (this.#env.ResizeObserver) {
      this.#resize = new this.#env.ResizeObserver(() => {
        this.schedule();
      });
    }
    if (this.#env.IntersectionObserver) {
      this.#intersection = new this.#env.IntersectionObserver(
        (records) => {
          for (const record of records) {
            const entry = this.#entries.get(record.target);
            if (entry) entry.ratio = record.isIntersecting ? record.intersectionRatio : 0;
          }
          this.schedule();
        },
        { threshold: [0, VISIBLE_RATIO, 1] },
      );
    }
    const onChange = (): void => {
      this.schedule();
    };
    win.addEventListener('resize', onChange);
    doc.addEventListener('scroll', onChange, { capture: true, passive: true });
    doc.addEventListener('visibilitychange', onChange);
    this.#cleanups.push(
      () => {
        win.removeEventListener('resize', onChange);
      },
      () => {
        doc.removeEventListener('scroll', onChange, { capture: true });
      },
      () => {
        doc.removeEventListener('visibilitychange', onChange);
      },
      this.#services.on('adview-event', (message) => {
        this.#onHostEvent(message as AdviewEventMessage);
      }),
    );
    this.#scan(doc);
  }

  /** Stops watching and forgets every element (tests). Mounted guests are not closed. */
  stop(): void {
    if (!this.#started) return;
    this.#started = false;
    this.#mutations?.disconnect();
    this.#resize?.disconnect();
    this.#intersection?.disconnect();
    for (const cleanup of this.#cleanups.splice(0)) cleanup();
    if (this.#poll !== undefined) clearInterval(this.#poll);
    this.#poll = undefined;
    for (const entry of this.#tracked.values()) entry.tracked = false;
    this.#tracked.clear();
    this.#scheduled = false;
  }

  /**
   * Registers an element that the runtime did not see being created or
   * inserted, and evaluates it at once (B.3.1 item 3).
   *
   * @param el - an `<owadview>` element
   */
  upgrade(el: Element): void {
    if (!isAdview(el)) return;
    const entry = this.#register(el);
    if (el.isConnected) this.#track(entry);
    this.#evaluate(entry);
  }

  /**
   * The elements the runtime currently tracks (connected `<owadview>`
   * elements of the document), in discovery order.
   *
   * @returns the elements
   */
  elements(): HTMLElement[] {
    return [...this.#tracked.values()].map((entry) => entry.el);
  }

  /**
   * The runtime-assigned id of an element (`"e1"`, `"e2"`, ...), or
   * `undefined` for an element the runtime has not seen.
   *
   * @param el - the element
   * @returns the id
   */
  elementId(el: Element): string | undefined {
    return this.#entries.get(el)?.id;
  }

  /** Requests a geometry and visibility pass, coalesced per animation frame. */
  schedule(): void {
    if (this.#scheduled || !this.#started) return;
    this.#scheduled = true;
    this.#env.frame(() => {
      this.#scheduled = false;
      this.flush();
    });
  }

  /** Runs a geometry and visibility pass now. */
  flush(): void {
    if (!this.#started) return;
    for (const entry of [...this.#tracked.values()]) this.#evaluate(entry);
  }

  #installStyle(): void {
    const doc = this.#env.document;
    const Sheet = (this.#env.window as Window & { CSSStyleSheet?: typeof CSSStyleSheet })
      .CSSStyleSheet;
    try {
      if (Sheet && Array.isArray(doc.adoptedStyleSheets)) {
        const sheet = new Sheet();
        sheet.replaceSync(DEFAULT_STYLE);
        doc.adoptedStyleSheets = [...doc.adoptedStyleSheets, sheet];
        return;
      }
    } catch {
      // fall back to a <style> element
    }
    const style = doc.createElement('style');
    style.textContent = DEFAULT_STYLE;
    const insert = (): void => {
      (doc.querySelector('head') ?? doc.documentElement).prepend(style);
    };
    if ((doc.documentElement as Element | null) !== null) insert();
    else doc.addEventListener('DOMContentLoaded', insert, { once: true });
  }

  #wrapCreateElement(): void {
    const proto = (this.#env.window as Window & { Document?: typeof Document }).Document?.prototype;
    if (!proto) return;
    const seen = (el: Element, owner: Document): Element => {
      if (isAdview(el)) {
        this.#register(el);
        if (owner !== this.#env.document)
          this.#services.warnOnce(
            'owadview:foreign-document',
            '<owadview> created in another document (an iframe) stays inert; ads mount only in the top document',
          );
      }
      return el;
    };
    for (const name of ['createElement', 'createElementNS'] as const) {
      // `Document.prototype`, and the prototype that gives this document the
      // method when that is another one (an `HTMLDocument` override, or a
      // base class shared by several windows in some engines).
      const targets = new Set<object>([proto]);
      const nearest = ownerOf(Object.getPrototypeOf(this.#env.document) as object | null, name);
      if (nearest) targets.add(nearest);
      for (const target of targets) this.#wrapMethod(target, name, seen);
    }
  }

  #wrapMethod(
    target: object,
    name: 'createElement' | 'createElementNS',
    seen: (el: Element, owner: Document) => Element,
  ): void {
    const own = Object.getOwnPropertyDescriptor(target, name);
    const owner = own ? target : ownerOf(target, name);
    const descriptor = own ?? (owner ? Object.getOwnPropertyDescriptor(owner, name) : undefined);
    const original: unknown = descriptor?.value;
    if (!descriptor || typeof original !== 'function') return;
    const wrapped = function (this: Document, ...args: unknown[]): Element {
      return seen(Reflect.apply(original, this, args) as Element, this);
    };
    Object.defineProperty(target, name, { ...descriptor, value: wrapped });
    this.#cleanups.push(() => {
      if (Object.getOwnPropertyDescriptor(target, name)?.value !== wrapped) return;
      if (own) Object.defineProperty(target, name, own);
      else Reflect.deleteProperty(target, name);
    });
  }

  #register(el: HTMLElement): Entry {
    let entry = this.#entries.get(el);
    if (!entry) {
      entry = {
        el,
        id: `e${String(++this.#nextId)}`,
        tracked: false,
        mounted: false,
        closed: false,
        membersDefined: false,
        attributes: undefined,
        rect: undefined,
        visible: undefined,
        ratio: undefined,
        chain: Promise.resolve(),
        pendingUpdate: undefined,
        lastGuestClick: Number.NEGATIVE_INFINITY,
      };
      this.#entries.set(el, entry);
    }
    return entry;
  }

  #track(entry: Entry): void {
    if (entry.tracked || !this.#started) return;
    if (entry.el.getRootNode() !== this.#env.document) {
      this.#services.warnOnce(
        'owadview:shadow',
        '<owadview> inside a shadow root stays inert; ads mount only in the light DOM of the top document',
      );
      return;
    }
    entry.tracked = true;
    this.#tracked.set(entry.id, entry);
    this.#resize?.observe(entry.el);
    this.#intersection?.observe(entry.el);
  }

  #untrack(entry: Entry): void {
    if (!entry.tracked) return;
    entry.tracked = false;
    this.#tracked.delete(entry.id);
    this.#resize?.unobserve(entry.el);
    this.#intersection?.unobserve(entry.el);
    entry.ratio = undefined;
  }

  #scan(root: Element | Document): void {
    if (isAdview(root)) this.upgrade(root);
    for (const el of root.querySelectorAll(TAG)) this.upgrade(el);
  }

  #onMutations(records: readonly MutationRecord[]): void {
    const touched = new Set<Entry>();
    let removed = false;
    for (const record of records) {
      if (record.type === 'attributes') {
        const entry = isAdview(record.target) ? this.#register(record.target) : undefined;
        if (entry) {
          // An attribute change reopens an element closed after `shutdown` or a failed mount.
          if (!entry.mounted) entry.closed = false;
          touched.add(entry);
        }
        continue;
      }
      for (const node of record.addedNodes) {
        if (node.nodeType === 1) this.#scan(node as Element);
      }
      if (record.removedNodes.length > 0) removed = true;
    }
    if (removed) {
      for (const entry of [...this.#tracked.values()])
        if (!entry.el.isConnected) touched.add(entry);
    }
    for (const entry of touched) {
      if (!entry.el.isConnected) {
        this.#untrack(entry);
        entry.closed = false;
      } else this.#track(entry);
      this.#evaluate(entry);
    }
  }

  /** Mounts, remounts, updates or unmounts one element as its state requires (B.3.4). */
  #evaluate(entry: Entry): void {
    const { el } = entry;
    if (!this.#started) return;
    if (!entry.tracked || !el.isConnected) {
      if (entry.mounted) this.#unmount(entry);
      return;
    }
    const attributes = readAttributes(el);
    if (entry.mounted && entry.attributes) {
      if (needsRemount(entry.attributes, attributes)) {
        this.#unmount(entry);
        entry.closed = false;
      } else {
        const patch: Record<string, unknown> = {};
        if (!sameTracking(entry.attributes.customTracking, attributes.customTracking))
          patch['customTracking'] = attributes.customTracking;
        if (entry.attributes.pageurl !== attributes.pageurl) patch['pageurl'] = attributes.pageurl;
        if (Object.keys(patch).length > 0) {
          entry.attributes = attributes;
          this.#update(entry, { attributes: patch });
        }
        this.#updateGeometry(entry, attributes.performance);
        return;
      }
    }
    if (entry.closed) return;
    const rect = this.#rect(el, attributes.performance);
    if (!attributes.performance && (rect.width <= 0 || rect.height <= 0)) return;
    if (attributes.performance && this.#otherPerformance(entry)) {
      this.#services.warnOnce(
        `owadview:performance:${entry.id}`,
        'a window shows at most one performance <owadview>; this one is ignored',
      );
      return;
    }
    this.#mount(entry);
  }

  #otherPerformance(entry: Entry): boolean {
    for (const other of this.#tracked.values())
      if (other !== entry && other.mounted && other.attributes?.performance === true) return true;
    return false;
  }

  #mount(entry: Entry): void {
    this.#defineMembers(entry);
    // Read again: defining the members moves values set on the plain element into attributes.
    const current = readAttributes(entry.el);
    const visible = this.#visible(entry, current.performance);
    entry.mounted = true;
    entry.attributes = current;
    entry.rect = this.#rect(entry.el, current.performance);
    entry.visible = visible;
    entry.pendingUpdate = undefined;
    const request = {
      elementId: entry.id,
      attributes: current,
      rect: entry.rect,
      visible,
    };
    this.#enqueue(entry, async () => {
      try {
        await this.#services.command('adview_mount', request);
      } catch (error) {
        this.#services.log(
          'warn',
          `<owadview> ${entry.id} did not mount: ${(error as Error).message}`,
        );
        if (entry.attributes === current) {
          entry.mounted = false;
          entry.closed = true;
        }
      }
    });
    this.#updatePoll();
  }

  #unmount(entry: Entry): void {
    entry.mounted = false;
    entry.attributes = undefined;
    entry.rect = undefined;
    entry.visible = undefined;
    entry.pendingUpdate = undefined;
    this.#enqueue(entry, async () => {
      try {
        await this.#services.command('adview_unmount', { elementId: entry.id });
      } catch (error) {
        this.#services.log(
          'debug',
          `adview_unmount ${entry.id} failed: ${(error as Error).message}`,
        );
      }
    });
    this.#updatePoll();
  }

  #updateGeometry(entry: Entry, performance: boolean): void {
    const patch: Record<string, unknown> = {};
    const rect = this.#rect(entry.el, performance);
    if (!sameRect(entry.rect, rect)) {
      entry.rect = rect;
      patch['rect'] = rect;
    }
    const visible = this.#visible(entry, performance);
    if (visible !== entry.visible) {
      entry.visible = visible;
      patch['visible'] = visible;
    }
    if (Object.keys(patch).length > 0) this.#update(entry, patch);
  }

  /** Queues an `adview_update`, merged with one not sent yet. */
  #update(entry: Entry, patch: Record<string, unknown>): void {
    if (entry.pendingUpdate) {
      const { attributes, ...rest } = patch;
      Object.assign(entry.pendingUpdate, rest);
      if (attributes !== undefined)
        entry.pendingUpdate['attributes'] = {
          ...(entry.pendingUpdate['attributes'] ?? {}),
          ...attributes,
        };
      return;
    }
    entry.pendingUpdate = { ...patch };
    this.#enqueue(entry, async () => {
      const update = entry.pendingUpdate;
      entry.pendingUpdate = undefined;
      if (!update || !entry.mounted) return;
      try {
        await this.#services.command('adview_update', { elementId: entry.id, ...update });
      } catch (error) {
        this.#services.log(
          'debug',
          `adview_update ${entry.id} failed: ${(error as Error).message}`,
        );
      }
    });
  }

  /** Runs element commands one after another, so the plugin sees mount, update and unmount in order. */
  #enqueue(entry: Entry, task: () => Promise<void>): void {
    entry.chain = entry.chain.then(task, task);
  }

  #rect(el: HTMLElement, performance: boolean): AdviewRect {
    const win = this.#env.window;
    const devicePixelRatio = win.devicePixelRatio || 1;
    if (performance) {
      return { x: 0, y: 0, width: win.innerWidth, height: win.innerHeight, devicePixelRatio };
    }
    const box = el.getBoundingClientRect();
    return { x: box.x, y: box.y, width: box.width, height: box.height, devicePixelRatio };
  }

  /**
   * Visibility as B.3.4 defines it: the document is visible and, for an
   * element that is not a performance ad, at least half of it intersects
   * the viewport and `checkVisibility()` holds. Whether the embedder window
   * is shown and not minimized is applied by the plugin, which knows the
   * window state.
   */
  #visible(entry: Entry, performance: boolean): boolean {
    const doc = this.#env.document;
    if (doc.visibilityState !== 'visible') return false;
    if (performance) return true;
    const el = entry.el;
    const ratio = entry.ratio ?? this.#computedRatio(el);
    if (ratio < VISIBLE_RATIO) return false;
    const check = (el as HTMLElement & { checkVisibility?: (o?: object) => boolean })
      .checkVisibility;
    if (typeof check === 'function')
      return check.call(el, { opacityProperty: true, visibilityProperty: true });
    if (el.getClientRects().length === 0) return false;
    const style = this.#env.window.getComputedStyle(el);
    return style.visibility !== 'hidden' && style.opacity !== '0';
  }

  /** Viewport intersection ratio from the element box (before the `IntersectionObserver` reports). */
  #computedRatio(el: HTMLElement): number {
    const box = el.getBoundingClientRect();
    const area = box.width * box.height;
    if (area <= 0) return 0;
    const win = this.#env.window;
    const width = Math.min(box.right, win.innerWidth) - Math.max(box.left, 0);
    const height = Math.min(box.bottom, win.innerHeight) - Math.max(box.top, 0);
    return width > 0 && height > 0 ? (width * height) / area : 0;
  }

  #updatePoll(): void {
    const any = [...this.#tracked.values()].some((entry) => entry.mounted);
    if (any && this.#poll === undefined) {
      this.#poll = setInterval(() => {
        this.schedule();
      }, VISIBILITY_POLL_MS);
    } else if (!any && this.#poll !== undefined) {
      clearInterval(this.#poll);
      this.#poll = undefined;
    }
  }

  /**
   * Defines the members ow-electron's element has after attach (B.3.3), once
   * per element, and the open shadow root (B.3.4). A value an app assigned to
   * `customTracking` or `pageUrl` on the plain element is moved into the
   * attribute first, so it is not lost.
   */
  #defineMembers(entry: Entry): void {
    if (entry.membersDefined) return;
    entry.membersDefined = true;
    const el = entry.el;
    const own = (key: string): unknown => {
      const descriptor = Object.getOwnPropertyDescriptor(el, key);
      if (!descriptor || !('value' in descriptor)) return undefined;
      Reflect.deleteProperty(el, key);
      return descriptor.value;
    };
    const tracking = own('customTracking');
    if (tracking !== undefined && tracking !== null)
      el.setAttribute('customtracking', domString(tracking));
    const pageUrl = own('pageUrl');
    if (pageUrl !== undefined && pageUrl !== null) el.setAttribute('pageurl', domString(pageUrl));
    const command = (name: string, args: unknown[]): void => {
      if (!entry.mounted) {
        this.#services.log('debug', `<owadview> ${entry.id}: ${name}() before attach is ignored`);
        return;
      }
      this.#enqueue(entry, async () => {
        try {
          await this.#services.command('adview_command', {
            elementId: entry.id,
            command: name,
            args,
          });
        } catch (error) {
          this.#services.log('debug', `adview_command ${name} failed: ${(error as Error).message}`);
        }
      });
    };
    const method = (fn: (...args: unknown[]) => void): PropertyDescriptor => ({
      value: fn,
      writable: true,
      configurable: true,
      enumerable: false,
    });
    Object.defineProperties(el, {
      customTracking: {
        get: () => el.getAttribute('customtracking') ?? '',
        set: (value: unknown) => {
          if (value === undefined || value === null) el.removeAttribute('customtracking');
          else el.setAttribute('customtracking', domString(value));
        },
        enumerable: true,
        configurable: true,
      },
      pageUrl: {
        get: () => el.getAttribute('pageurl') ?? '',
        set: (value: unknown) => {
          el.setAttribute('pageurl', value === undefined || value === null ? '' : domString(value));
        },
        enumerable: true,
        configurable: true,
      },
      setPageUrl: method((url: unknown) => {
        el.setAttribute('pageurl', url === undefined || url === null ? '' : domString(url));
      }),
      sendCommand: method((...args: unknown[]) => {
        command('sendCommand', jsonArgs(args));
      }),
      setAudioMuted: method((muted: unknown) => {
        command('setAudioMuted', [muted === true]);
      }),
      reload: method(() => {
        command('reload', []);
      }),
    });
    this.#attachShadow(entry);
  }

  #attachShadow(entry: Entry): void {
    const el = entry.el;
    if (el.shadowRoot) return;
    let root: ShadowRoot;
    try {
      root = el.attachShadow({ mode: 'open' });
    } catch {
      // Engines allow shadow roots only on valid custom element names and a
      // fixed list of HTML elements; `owadview` is neither.
      this.#services.log('debug', '<owadview> shadow root is not available on this engine');
      return;
    }
    const doc = this.#env.document;
    const style = doc.createElement('style');
    style.textContent = SHADOW_STYLE;
    const frame = doc.createElement('iframe');
    frame.setAttribute('src', 'about:blank');
    frame.setAttribute('tabindex', '-1');
    frame.setAttribute('aria-hidden', 'true');
    root.append(style, frame);
  }

  #onHostEvent(message: AdviewEventMessage): void {
    const { elementId, name, data, source } = message;
    if (typeof elementId !== 'string' || typeof name !== 'string' || name === '') return;
    if (name.startsWith('__host:')) return;
    const entry = this.#tracked.get(elementId);
    if (!entry) {
      this.#services.log('debug', `adview-event '${name}' for unknown element ${elementId}`);
      return;
    }
    const now = this.#env.now();
    if (source === 'host' && name === 'ad-clicked' && now - entry.lastGuestClick < CLICK_DEDUPE_MS)
      return;
    if (source !== 'host' && CLICK_NAMES.has(name)) entry.lastGuestClick = now;
    const EventCtor = (this.#env.window as Window & { Event?: typeof Event }).Event ?? Event;
    entry.el.dispatchEvent(createAdviewEvent(name, data, EventCtor));
    const twin = SPELLING_TWINS[name];
    if (twin !== undefined) entry.el.dispatchEvent(createAdviewEvent(twin, data, EventCtor));
    if (name === 'shutdown' && entry.attributes?.performance === true && entry.mounted) {
      this.#unmount(entry);
      entry.closed = true;
    }
  }
}

/** `sendCommand` arguments as JSON values (anything else is dropped). */
function jsonArgs(args: unknown[]): unknown[] {
  try {
    const encoded = JSON.stringify(args, (_key, value: unknown) =>
      typeof value === 'bigint' ? value.toString() : value,
    );
    return JSON.parse(encoded) as unknown[];
  } catch {
    return [];
  }
}

/** Kernel members {@link adviewRuntimeOf} needs. */
export interface AdviewKernel extends AdviewServices {
  /**
   * The webview's singleton under `key`.
   *
   * @param key - the key
   * @param factory - creates the value
   * @returns the singleton
   */
  singleton<T>(key: string, factory: () => T): T;
  /**
   * Registers a hook that a runtime reset runs.
   *
   * @param hook - the hook
   * @returns a function that unregisters it
   */
  onReset(hook: () => void): () => void;
}

/**
 * The document's `<owadview>` runtime (one per webview, shared by every copy
 * of the package), started when the document is a UI window.
 *
 * @param kernel - the runtime kernel
 * @returns the runtime
 */
export function adviewRuntimeOf(kernel: AdviewKernel): AdviewRuntime {
  const runtime = kernel.singleton('renderer.owadview', () => {
    const created = new AdviewRuntime(kernel);
    kernel.onReset(() => {
      created.stop();
    });
    return created;
  });
  runtime.start();
  return runtime;
}

/**
 * Converts a value the way a DOM attribute setter does (`ToString`).
 *
 * @param value - any value
 * @returns its string form
 */
function domString(value: unknown): string {
  if (typeof value === 'string') return value;
  return String(value);
}
