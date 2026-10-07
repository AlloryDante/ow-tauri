/**
 * Ad slots: a card with a header row (size, `cid`, `adstyle`, status chip)
 * above a container of exactly the slot size that holds one `<owadview>`.
 * The container shows a hatch and "no fill yet" behind the transparent ad,
 * so an unfilled slot visibly shows the app's own fallback. Nothing of ours
 * is drawn over the ad box.
 *
 * Every event the element dispatches goes to the timeline; elements are
 * created fresh for every page visit and never re-appended once removed
 * (ow-electron does not attach an element again).
 *
 * @packageDocumentation
 */
import { h } from './dom.js';
import { EVENT_NAMES, familyOf, payloadOf, type SlotKind } from './events.js';
import type { TimelineStore } from './timeline-store.js';

/** What a page needs to create slots and log actions. */
export interface AdContext {
  /** The timeline. */
  store: TimelineStore;
  /** Milliseconds since the window loaded. */
  now(): number;
  /** `test` or `live`. */
  mode: 'test' | 'live';
  /** Logs a presenter action as a `control:<action>` row. */
  control(action: string, cid?: string, details?: Record<string, unknown>): void;
}

/** Slot status shown in the chip. */
export type SlotStatus =
  | 'mounting'
  | 'waiting for view'
  | 'loaded'
  | 'ready'
  | 'playing'
  | 'no fill'
  | 'hidden'
  | 'removed'
  | 'unavailable';

/** Options of {@link createSlot}. */
export interface SlotOptions {
  /** Container width and height in CSS pixels. */
  size: readonly [number, number];
  /** The `cid` (at most 20 characters, unique in the window). */
  cid: string;
  /** What kind of slot this is (colours its video events). */
  kind?: SlotKind;
  /** The `adstyle` attribute. */
  adstyle?: string;
  /** Set `slotsize="WxH"` (default `true`, as the official sample does). */
  slotsize?: boolean;
  /** Seconds in view without an ad before the chip says "no fill" (default 30). */
  noFillAfterS?: number;
}

/** A created slot. */
export interface Slot {
  /** The card (header plus container). */
  card: HTMLElement;
  /** The container of exactly the slot size. */
  box: HTMLElement;
  /** The `<owadview>` element. */
  el: HTMLElement;
  /** The `cid`. */
  cid: string;
  /** The current status. */
  readonly status: SlotStatus;
  /** Sets the status chip. */
  setStatus(status: SlotStatus): void;
  /**
   * Hides the card with `display: none` (chip "hidden") or shows it again
   * (the chip goes back to the status it had, or to the newest one an event
   * set while hidden).
   */
  setHidden(hidden: boolean): void;
  /**
   * Listens to one event of the element (after the timeline row is added).
   *
   * @returns an unsubscribe function
   */
  on(name: string, listener: (event: Event) => void): () => void;
  /** Stops observers and timers (the element goes with the page). */
  dispose(): void;
}

const STATUS_TONE: Record<SlotStatus, string> = {
  mounting: 'muted',
  'waiting for view': 'info',
  loaded: 'ok',
  ready: 'ok',
  playing: 'ok',
  'no fill': 'warn',
  hidden: 'muted',
  removed: 'muted',
  unavailable: 'err',
};

/**
 * Creates a slot card with a fresh `<owadview>` inside. Append `card` to the
 * page; the element mounts when its container is in the document.
 *
 * @param ctx - the ad context
 * @param options - size, `cid`, kind and attributes
 * @returns the slot
 */
export function createSlot(ctx: AdContext, options: SlotOptions): Slot {
  const [w, hgt] = options.size;
  const kind = options.kind ?? 'standard';
  const sizeText = `${String(w)}x${String(hgt)}`;
  const chip = h('span', { class: 'chip', attrs: { role: 'status' } });
  const head = h(
    'div',
    { class: 'slot-head' },
    h('span', { class: 'mono slot-size', text: sizeText }),
    h('span', { class: 'mono muted slot-cid', text: options.cid, attrs: { title: options.cid } }),
    options.adstyle ? h('span', { class: 'mono tag', text: options.adstyle }) : null,
    chip,
  );
  const box = h(
    'div',
    {
      class: 'slot-box',
      style: `width:${String(w)}px;height:${String(hgt)}px`,
      data: { cid: options.cid },
    },
    h('span', { class: 'slot-fallback', text: 'no fill yet', attrs: { 'aria-hidden': 'true' } }),
  );
  const card = h(
    'section',
    {
      class: 'slot',
      attrs: { 'aria-label': `${sizeText} ad slot ${options.cid}` },
    },
    head,
    box,
  );
  card.style.width = `${String(w)}px`;

  const el = document.createElement('owadview');
  el.setAttribute('cid', options.cid);
  if (options.slotsize !== false) el.setAttribute('slotsize', sizeText);
  if (options.adstyle) el.setAttribute('adstyle', options.adstyle);
  const created = ctx.now();

  let status: SlotStatus = 'mounting';
  let shownStatus: SlotStatus = 'mounting';
  let hiddenNow = false;
  let filled = false;
  let inView = false;
  let noFillTimer: ReturnType<typeof setTimeout> | undefined;
  const setStatus = (next: SlotStatus): void => {
    if (next !== 'hidden') shownStatus = next;
    // While hidden the chip keeps saying so; events still update the status
    // the chip returns to.
    if (hiddenNow && next !== 'hidden') return;
    status = next;
    chip.textContent = next;
    chip.className = `chip tone-${STATUS_TONE[next]}`;
    card.dataset['status'] = next;
  };
  setStatus('mounting');

  const armNoFill = (): void => {
    clearTimeout(noFillTimer);
    if (filled || !inView) return;
    noFillTimer = setTimeout(
      () => {
        if (!filled && (status === 'mounting' || status === 'waiting for view'))
          setStatus('no fill');
      },
      (options.noFillAfterS ?? 30) * 1000,
    );
  };

  const listeners = new Map<string, Set<(event: Event) => void>>();
  listenAll(ctx, el, options.cid, kind, created, (name, event) => {
    switch (name) {
      case 'did-attach':
        if (status === 'mounting') setStatus('waiting for view');
        break;
      case 'display_ad_loaded':
      case 'player_loaded':
      case 'complete':
        filled = true;
        setStatus('loaded');
        break;
      case 'video_ad_ready':
        filled = true;
        setStatus('ready');
        break;
      case 'play':
        filled = true;
        setStatus('playing');
        break;
      case 'destroyed':
        setStatus('removed');
        break;
    }
    for (const listener of listeners.get(name) ?? []) listener(event);
  });

  const io = new IntersectionObserver(
    (records) => {
      for (const record of records) inView = record.intersectionRatio >= 0.5;
      armNoFill();
    },
    { threshold: [0, 0.5, 1] },
  );
  io.observe(box);

  box.append(el);
  return {
    card,
    box,
    el,
    cid: options.cid,
    get status() {
      return status;
    },
    setStatus,
    setHidden(hidden) {
      if (hidden === hiddenNow) return;
      card.style.display = hidden ? 'none' : '';
      if (hidden) {
        setStatus('hidden');
        hiddenNow = true;
      } else {
        hiddenNow = false;
        setStatus(shownStatus);
      }
    },
    on(name, listener) {
      let set = listeners.get(name);
      if (!set) {
        set = new Set();
        listeners.set(name, set);
      }
      set.add(listener);
      return () => set.delete(listener);
    },
    dispose() {
      io.disconnect();
      clearTimeout(noFillTimer);
    },
  };
}

/**
 * Watches the document for an element leaving it (performance ads are removed
 * by the host after `shutdown`, and a second one at once) and logs a
 * `dom:removed` row once. Call it right after appending the element: an
 * element the host removes before this observer runs is still reported.
 *
 * @param ctx - the ad context
 * @param el - the element
 * @param cid - its `cid`
 * @param created - `ctx.now()` when it was created
 * @param onRemoved - called after the row is added
 * @returns a function that stops watching
 */
export function watchRemoval(
  ctx: AdContext,
  el: Element,
  cid: string,
  created: number,
  onRemoved?: () => void,
): () => void {
  const observer = new MutationObserver(() => {
    if (el.isConnected) return;
    observer.disconnect();
    ctx.store.add({
      t: ctx.now(),
      cid,
      name: 'dom:removed',
      family: 'lifecycle',
      payload: {},
      sinceMount: ctx.now() - created,
    });
    onRemoved?.();
  });
  observer.observe(document.body, { childList: true, subtree: true });
  return () => {
    observer.disconnect();
  };
}

/**
 * Adds a timeline listener for every name in {@link EVENT_NAMES} to an
 * `<owadview>` element. Each event becomes one row (own properties as the
 * payload), then `onEvent` runs.
 *
 * @param ctx - the ad context
 * @param el - the element
 * @param cid - the label of its rows
 * @param kind - the slot kind (colours video events of reward slots)
 * @param created - `ctx.now()` when the element was created
 * @param onEvent - called after the row is added
 */
export function listenAll(
  ctx: AdContext,
  el: Element,
  cid: string,
  kind: SlotKind,
  created: number,
  onEvent?: (name: string, event: Event) => void,
): void {
  for (const name of EVENT_NAMES) {
    el.addEventListener(name, (event) => {
      ctx.store.add({
        t: ctx.now(),
        cid,
        name,
        family: familyOf(name, kind),
        payload: payloadOf(event),
        sinceMount: ctx.now() - created,
      });
      onEvent?.(name, event);
    });
  }
}
