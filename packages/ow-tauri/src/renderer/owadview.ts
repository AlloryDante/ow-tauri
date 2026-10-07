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
 * The element lifecycle follows ow-electron [OBS]: an element the app
 * removes after attach gets a plain `destroyed` event, then its guest
 * closes, and it never attaches again (the app creates a new element); a
 * performance element leaves the document in the task after its `shutdown`
 * event; a second performance element while one is up is removed at once.
 *
 * Before it is attached the element is a plain `HTMLElement`; at attach the
 * runtime defines the element members ow-electron has after attach: own
 * attribute-backed properties (`customTracking`, `pageUrl`, ...), the
 * methods `setPageUrl`, `sendCommand`, `setAudioMuted` and `reload` on the
 * element's prototype, and an open shadow root with a `<style>` and an
 * `<iframe>` placeholder [OBS].
 *
 * @packageDocumentation
 */
import type { FacadeOwadview, HostMessageHandler } from '../bootstrap/facade-kernel.js';
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

/** Attributes whose change anywhere in the document triggers a geometry pass while guests are mounted. */
const LAYOUT_ATTRIBUTES = ['style', 'class', 'hidden', 'width', 'height', 'open'] as const;

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

/** An `adview_update` not sent yet, bound to the mount it was made for. */
interface PendingUpdate {
  readonly gen: number;
  readonly id: string;
  readonly patch: Record<string, unknown>;
}

/** Per-element state. */
interface Entry {
  readonly el: HTMLElement;
  /** Stable key of the element in the runtime (its first element id). */
  readonly key: string;
  /** The element id on the wire; a remount gets a fresh one, so late events of the old guest are dropped. */
  id: string;
  /** Whether {@link Entry.id} was already used for a mount. */
  used: boolean;
  /**
   * The element was attached and then left the document: it never attaches
   * again, as on ow-electron [OBS]; the app creates a new element (B.3.4).
   */
  dead: boolean;
  /** The runtime removes (or removed) the element itself after `shutdown`: no `destroyed` follows. */
  hostRemoved: boolean;
  /** Mount generation: incremented by every mount and unmount. */
  gen: number;
  tracked: boolean;
  mounted: boolean;
  closed: boolean;
  membersDefined: boolean;
  attributes: AdviewAttributes | undefined;
  rect: AdviewRect | undefined;
  visible: boolean | undefined;
  ratio: number | undefined;
  chain: Promise<void>;
  pendingUpdate: PendingUpdate | undefined;
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
export class AdviewRuntime implements FacadeOwadview {
  readonly #services: AdviewServices;
  readonly #env: AdviewEnvironment;
  readonly #entries = new WeakMap<Element, Entry>();
  readonly #tracked = new Map<string, Entry>();
  /** Mounted elements by their current wire id (events of other ids are dropped). */
  readonly #live = new Map<string, Entry>();
  #nextId = 0;
  #sheet: CSSStyleSheet | undefined;
  #layout: MutationObserver | undefined;
  #layoutObserved = false;
  /** The engine refused a shadow root once; it refuses every later one too. */
  #noShadow = false;
  #started = false;
  #scheduled = false;
  #poll: ReturnType<typeof setInterval> | undefined;
  #mutations: MutationObserver | undefined;
  #resize: ResizeObserver | undefined;
  #intersection: IntersectionObserver | undefined;
  #cleanups: (() => void)[] = [];
  /** The methods' prototype, by the element prototype it extends. */
  readonly #prototypes = new Map<object, object>();

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
    // A CSS transition or animation moves content without a mutation.
    doc.addEventListener('transitionend', onChange, { capture: true, passive: true });
    doc.addEventListener('animationend', onChange, { capture: true, passive: true });
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
      () => {
        doc.removeEventListener('transitionend', onChange, { capture: true });
        doc.removeEventListener('animationend', onChange, { capture: true });
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
    this.#layout?.disconnect();
    this.#layoutObserved = false;
    this.#resize?.disconnect();
    this.#intersection?.disconnect();
    for (const cleanup of this.#cleanups.splice(0)) cleanup();
    if (this.#poll !== undefined) clearInterval(this.#poll);
    this.#poll = undefined;
    for (const entry of this.#tracked.values()) entry.tracked = false;
    this.#tracked.clear();
    this.#live.clear();
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
   * `undefined` for an element the runtime has not seen. A remount gives the
   * element a fresh id; this returns the current (or last) one.
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
        this.#sheet = sheet;
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

  /**
   * Adopts the default style again when the page replaced
   * `document.adoptedStyleSheets` (instead of appending to it), which drops
   * it and leaves unstyled elements at 0x0.
   */
  #ensureStyle(): void {
    const sheet = this.#sheet;
    const doc = this.#env.document;
    if (!sheet || !Array.isArray(doc.adoptedStyleSheets) || doc.adoptedStyleSheets.includes(sheet))
      return;
    doc.adoptedStyleSheets = [...doc.adoptedStyleSheets, sheet];
    this.#services.warnOnce(
      'owadview:style',
      'document.adoptedStyleSheets was replaced, which removed the default <owadview> style; it was added again',
    );
    this.schedule();
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
      const id = `e${String(++this.#nextId)}`;
      entry = {
        el,
        key: id,
        id,
        used: false,
        dead: false,
        hostRemoved: false,
        gen: 0,
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
    if (entry.dead) {
      this.#services.log(
        'debug',
        `<owadview> ${entry.id} was attached and removed before; it does not attach again (create a new element)`,
      );
      return;
    }
    if (entry.el.getRootNode() !== this.#env.document) {
      this.#services.warnOnce(
        'owadview:shadow',
        '<owadview> inside a shadow root stays inert; ads mount only in the light DOM of the top document',
      );
      return;
    }
    entry.tracked = true;
    this.#tracked.set(entry.key, entry);
    this.#resize?.observe(entry.el);
    this.#intersection?.observe(entry.el);
  }

  #untrack(entry: Entry): void {
    if (!entry.tracked) return;
    entry.tracked = false;
    this.#tracked.delete(entry.key);
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
        if (entry && !entry.dead) {
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
    // Content inserted or removed elsewhere can move a mounted element.
    if (this.#live.size > 0) this.schedule();
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
    if (!this.#started || entry.dead) return;
    if (!el.isConnected) {
      if (entry.used) this.#detach(entry);
      return;
    }
    if (!entry.tracked) {
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
    if (!attributes.performance && (rect.width <= 0 || rect.height <= 0)) {
      this.#ensureStyle();
      return;
    }
    if (attributes.performance && this.#otherPerformance(entry)) {
      // ow-electron removes it in the same tick, without a guest, an event or a warning [OBS].
      this.#services.log(
        'debug',
        `<owadview> ${entry.id}: a window shows one performance ad at a time; this one is removed`,
      );
      el.remove();
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
    if (entry.used) entry.id = `e${String(++this.#nextId)}`;
    entry.used = true;
    const gen = ++entry.gen;
    const id = entry.id;
    entry.mounted = true;
    entry.attributes = current;
    entry.rect = this.#rect(entry.el, current.performance);
    entry.visible = visible;
    entry.pendingUpdate = undefined;
    this.#live.set(id, entry);
    const request = { elementId: id, attributes: current, rect: entry.rect, visible };
    this.#enqueue(entry, async () => {
      try {
        await this.#services.command('adview_mount', request);
      } catch (error) {
        this.#services.log('warn', `<owadview> ${id} did not mount: ${(error as Error).message}`);
        // Only when no unmount or remount happened meanwhile.
        if (entry.gen === gen) {
          this.#forget(entry);
          entry.closed = true;
          this.#updatePoll();
        }
      }
    });
    this.#updatePoll();
  }

  /**
   * The element left the document after it was attached: it is dead from
   * now on (B.3.4). A mounted element the app removed gets a plain
   * `destroyed` event first, then its guest closes, as on ow-electron
   * [OBS]; one the runtime removed after `shutdown` was unmounted already.
   */
  #detach(entry: Entry): void {
    entry.dead = true;
    if (!entry.mounted) return;
    if (!entry.hostRemoved) {
      const EventCtor = (this.#env.window as Window & { Event?: typeof Event }).Event ?? Event;
      entry.el.dispatchEvent(createAdviewEvent('destroyed', undefined, EventCtor));
    }
    this.#unmount(entry);
  }

  #unmount(entry: Entry): void {
    const id = entry.id;
    this.#forget(entry);
    this.#enqueue(entry, async () => {
      try {
        await this.#services.command('adview_unmount', { elementId: id });
      } catch (error) {
        this.#services.log('debug', `adview_unmount ${id} failed: ${(error as Error).message}`);
      }
    });
    this.#updatePoll();
  }

  /** Marks an element unmounted and drops what belonged to its mount. */
  #forget(entry: Entry): void {
    entry.gen++;
    entry.mounted = false;
    entry.attributes = undefined;
    entry.rect = undefined;
    entry.visible = undefined;
    entry.pendingUpdate = undefined;
    this.#live.delete(entry.id);
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

  /**
   * Queues an `adview_update` for the current mount, merged with one not
   * sent yet. An update made for a mount that was unmounted or replaced
   * before it ran is dropped: the new mount carries the geometry of its time.
   */
  #update(entry: Entry, patch: Record<string, unknown>): void {
    const pending = entry.pendingUpdate;
    if (pending?.gen === entry.gen) {
      const { attributes, ...rest } = patch;
      Object.assign(pending.patch, rest);
      if (attributes !== undefined)
        pending.patch['attributes'] = {
          ...(pending.patch['attributes'] ?? {}),
          ...attributes,
        };
      return;
    }
    const next: PendingUpdate = { gen: entry.gen, id: entry.id, patch: { ...patch } };
    entry.pendingUpdate = next;
    this.#enqueue(entry, async () => {
      if (entry.pendingUpdate === next) entry.pendingUpdate = undefined;
      if (next.gen !== entry.gen || !entry.mounted) return;
      try {
        await this.#services.command('adview_update', { elementId: next.id, ...next.patch });
      } catch (error) {
        this.#services.log('debug', `adview_update ${next.id} failed: ${(error as Error).message}`);
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
    const target = el as HTMLElement & {
      checkVisibility?: (this: Element, options?: object) => boolean;
    };
    if (typeof target.checkVisibility === 'function')
      return target.checkVisibility({ opacityProperty: true, visibilityProperty: true });
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

  /**
   * Runs the visibility poll and the layout observer while any guest is
   * mounted. The layout observer catches changes that move an element
   * without resizing it (a sibling grows, a class or style changes).
   */
  #updatePoll(): void {
    const any = this.#started && this.#live.size > 0;
    if (any && this.#poll === undefined) {
      this.#poll = setInterval(() => {
        this.schedule();
      }, VISIBILITY_POLL_MS);
    } else if (!any && this.#poll !== undefined) {
      clearInterval(this.#poll);
      this.#poll = undefined;
    }
    if (any && !this.#layoutObserved) {
      this.#layout ??= new this.#env.MutationObserver(() => {
        this.schedule();
      });
      this.#layout.observe(this.#env.document, {
        childList: true,
        subtree: true,
        characterData: true,
        attributes: true,
        attributeFilter: [...LAYOUT_ATTRIBUTES],
      });
      this.#layoutObserved = true;
    } else if (!any && this.#layoutObserved) {
      this.#layout?.disconnect();
      this.#layoutObserved = false;
    }
  }

  /**
   * Defines the members ow-electron's element has after attach (B.3.3), once
   * per element, and the open shadow root (B.3.4). The element's own
   * properties are `cid`, `slotsize`, `pageUrl`, `performance`, `unit`,
   * `adstyle` and `customTracking`, in ow-electron's order [OBS], each backed
   * by its attribute (`performance` is a boolean) [DEC]. A value an app
   * assigned to one of them on the plain element is moved into the attribute
   * first, so it is not lost. The methods live on a prototype inserted
   * between the element and its own prototype, as on ow-electron's element
   * [OBS], so they are not own properties.
   */
  #defineMembers(entry: Entry): void {
    if (entry.membersDefined) return;
    entry.membersDefined = true;
    const el = entry.el;
    for (const [property, attribute] of REFLECTED) {
      const descriptor = Object.getOwnPropertyDescriptor(el, property);
      if (!descriptor || !('value' in descriptor)) continue;
      Reflect.deleteProperty(el, property);
      const value: unknown = descriptor.value;
      if (value !== undefined && value !== null) setReflected(el, attribute, value);
    }
    const accessor = (attribute: string): PropertyDescriptor => ({
      get: () =>
        attribute === 'performance'
          ? el.hasAttribute(attribute)
          : (el.getAttribute(attribute) ?? ''),
      set: (value: unknown) => {
        setReflected(el, attribute, value);
      },
      enumerable: true,
      configurable: true,
    });
    const members: PropertyDescriptorMap = {};
    for (const [property, attribute] of REFLECTED) members[property] = accessor(attribute);
    Object.defineProperties(el, members);
    const base = Object.getPrototypeOf(el) as object | null;
    if (base !== null && ![...this.#prototypes.values()].includes(base)) {
      Object.setPrototypeOf(el, this.#methodsPrototype(base));
    }
    this.#attachShadow(entry);
  }

  /**
   * The prototype that carries the element methods (B.3.3) on top of
   * `base`, one per base prototype.
   */
  #methodsPrototype(base: object): object {
    const known = this.#prototypes.get(base);
    if (known) return known;
    const entries = this.#entries;
    const command = (entry: Entry, name: string, args: unknown[]): void => {
      this.#command(entry, name, args);
    };
    const method = (fn: (entry: Entry, args: unknown[]) => void): PropertyDescriptor => ({
      value: function (this: unknown, ...args: unknown[]): void {
        const entry =
          typeof this === 'object' && this !== null ? entries.get(this as Element) : undefined;
        if (entry) fn(entry, args);
      },
      writable: true,
      configurable: true,
      enumerable: false,
    });
    const proto = Object.create(base, {
      setPageUrl: method((entry, [url]) => {
        entry.el.setAttribute('pageurl', url === undefined || url === null ? '' : domString(url));
      }),
      sendCommand: method((entry, args) => {
        command(entry, 'sendCommand', jsonArgs(args));
      }),
      setAudioMuted: method((entry, [muted]) => {
        command(entry, 'setAudioMuted', [muted === true]);
      }),
      reload: method((entry) => {
        command(entry, 'reload', []);
      }),
    }) as object;
    this.#prototypes.set(base, proto);
    return proto;
  }

  /** Runs an element command on the guest of `entry` (B.3.3). */
  #command(entry: Entry, name: string, args: unknown[]): void {
    if (!entry.mounted) {
      this.#services.log('debug', `<owadview> ${entry.id}: ${name}() before attach is ignored`);
      return;
    }
    const { gen, id } = entry;
    this.#enqueue(entry, async () => {
      if (entry.gen !== gen) return;
      try {
        await this.#services.command('adview_command', { elementId: id, command: name, args });
      } catch (error) {
        this.#services.log('debug', `adview_command ${name} failed: ${(error as Error).message}`);
      }
    });
  }

  #attachShadow(entry: Entry): void {
    const el = entry.el;
    if (el.shadowRoot || this.#noShadow) return;
    let root: ShadowRoot;
    try {
      root = el.attachShadow({ mode: 'open' });
    } catch {
      // Engines allow shadow roots only on valid custom element names and a
      // fixed list of HTML elements; `owadview` is neither. Said once per
      // page: ow-electron says nothing, and every later ad would repeat it.
      this.#noShadow = true;
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
    const entry = this.#live.get(elementId);
    if (!entry) {
      this.#services.log(
        'debug',
        `adview-event '${name}' for unknown or replaced element ${elementId}`,
      );
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
      // The guest closes now; the element leaves the document in a later
      // task, after the listeners of `shutdown` ran (B.3.4) [OBS].
      this.#unmount(entry);
      entry.closed = true;
      entry.hostRemoved = true;
      const { el } = entry;
      setTimeout(() => {
        if (this.#started && el.isConnected) el.remove();
      }, 0);
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
  /** The registered `<owadview>` runtime (`FacadeKernel.owadview`). */
  owadview?: FacadeOwadview | undefined;
  /**
   * Registers a hook that a runtime reset runs.
   *
   * @param hook - the hook
   * @returns a function that unregisters it
   */
  onReset(hook: () => void): () => void;
}

/**
 * The document's `<owadview>` runtime (one per webview), started when the
 * document is a UI window. The first copy of the package that asks (normally
 * the injected bootstrap) creates it and registers it as
 * `FacadeKernel.owadview`; every other copy uses that registration, which
 * is part of the versioned facade API (ADR 0012), never the other copy's
 * class.
 *
 * @param kernel - the runtime kernel
 * @returns the runtime
 */
export function adviewRuntimeOf(kernel: AdviewKernel): FacadeOwadview {
  const registered = kernel.owadview;
  if (registered) return registered;
  const runtime = new AdviewRuntime(kernel);
  kernel.owadview = runtime;
  const off = kernel.onReset(() => {
    off();
    runtime.stop();
    if (kernel.owadview === runtime) kernel.owadview = undefined;
  });
  runtime.start();
  return runtime;
}

/**
 * The element's attribute-backed own properties after attach, in
 * ow-electron's order [OBS], with their attributes.
 */
const REFLECTED: readonly (readonly [string, string])[] = [
  ['cid', 'cid'],
  ['slotsize', 'slotsize'],
  ['pageUrl', 'pageurl'],
  ['performance', 'performance'],
  ['unit', 'unit'],
  ['adstyle', 'adstyle'],
  ['customTracking', 'customtracking'],
];

/**
 * Writes an attribute-backed property: `performance` is a boolean
 * attribute set when the value is truthy; `pageUrl` stores `""` for `null` or `undefined`; the others
 * remove the attribute for `null` or `undefined` [DEC].
 *
 * @param el - the element
 * @param attribute - the attribute name
 * @param value - the assigned value
 */
function setReflected(el: Element, attribute: string, value: unknown): void {
  if (attribute === 'performance') {
    if (value) el.setAttribute(attribute, '');
    else el.removeAttribute(attribute);
  } else if (value === undefined || value === null) {
    if (attribute === 'pageurl') el.setAttribute(attribute, '');
    else el.removeAttribute(attribute);
  } else el.setAttribute(attribute, domString(value));
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
