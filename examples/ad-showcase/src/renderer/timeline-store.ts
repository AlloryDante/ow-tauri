/**
 * The event timeline's data: every `<owadview>` event and every control
 * action, in arrival order, with counts per event name. Pure (no DOM), so it
 * is unit-tested in `timeline-store.test.ts`; `timeline-view.ts` renders it.
 *
 * @packageDocumentation
 */
import type { Family } from './events.js';

/** One timeline row. */
export interface TimelineEntry {
  /** Arrival order, from 1. */
  seq: number;
  /** Milliseconds since the window loaded. */
  t: number;
  /** The slot's `cid` (`app` for rows that are not about one element). */
  cid: string;
  /** The event name, or `control:<action>` for an action the presenter took. */
  name: string;
  /** Colour family. */
  family: Family;
  /** Own properties of the event, or the action's details. */
  payload: Record<string, unknown>;
  /** Milliseconds since the element was created, when the row is about one. */
  sinceMount: number | null;
  /**
   * The page visit the row belongs to (see {@link TimelineStore.beginVisit}):
   * an element's rows belong to the visit that created the element, even
   * when they arrive after the page was left; `app` rows to the visit during
   * which they arrived.
   */
  visit: number;
}

/** The fields of a new row; the store adds `seq` and `visit`. */
export type NewEntry = Omit<TimelineEntry, 'seq' | 'visit'>;

/** Which rows are shown. */
export interface TimelineFilter {
  /** A page visit, or `null` for every page (the rail's scope). */
  visit: number | null;
  /** A `cid`, or `null` for every element. */
  cid: string | null;
  /** A family, or `null` for every family. */
  family: Family | null;
}

/** What an export file holds besides the rows. */
export interface ExportMeta {
  /** `ow-electron` or `ow-tauri`. */
  host: string;
  /** Host version. */
  hostVersion: string;
  /** `test` or `live`. */
  mode: string;
  /** The uid, masked. */
  uidMasked: string;
  /** `process.platform` style platform. */
  platform: string;
}

/** The timeline store. */
export class TimelineStore {
  readonly #entries: TimelineEntry[] = [];
  readonly #counts = new Map<string, number>();
  readonly #listeners = new Set<(entry: TimelineEntry) => void>();
  readonly #visitListeners = new Set<(visit: number) => void>();
  /** The visit of the element that last used each `cid`. */
  readonly #cidVisit = new Map<string, number>();
  #seq = 0;
  #visit = 0;

  /**
   * Starts a new page visit: rows of elements first seen from now on, and
   * `app` rows from now on, belong to it. Call it before the page mounts.
   *
   * @returns the new visit number
   */
  beginVisit(): number {
    this.#visit += 1;
    for (const listener of this.#visitListeners) listener(this.#visit);
    return this.#visit;
  }

  /** The current page visit (0 before the first page). */
  get visit(): number {
    return this.#visit;
  }

  /**
   * Binds `cid` to the current visit: call it when an element with that
   * `cid` is created, so that a page visited again, whose elements reuse
   * their `cid`s, shows the new elements' rows on the new visit. Without it
   * a `cid` belongs to the visit it was first seen in.
   *
   * @param cid - the element's row label
   */
  bindElement(cid: string): void {
    if (cid !== 'app') this.#cidVisit.set(cid, this.#visit);
  }

  /**
   * Subscribes to page visits.
   *
   * @param listener - called with each new visit number
   * @returns an unsubscribe function
   */
  onVisit(listener: (visit: number) => void): () => void {
    this.#visitListeners.add(listener);
    return () => this.#visitListeners.delete(listener);
  }

  /**
   * Appends a row and notifies listeners.
   *
   * @param entry - the row without its sequence number and visit
   * @returns the stored row
   */
  add(entry: NewEntry): TimelineEntry {
    this.#seq += 1;
    let visit = this.#visit;
    if (entry.cid !== 'app') {
      const first = this.#cidVisit.get(entry.cid);
      if (first === undefined) this.#cidVisit.set(entry.cid, visit);
      else visit = first;
    }
    const stored: TimelineEntry = { seq: this.#seq, ...entry, visit };
    this.#entries.push(stored);
    this.#counts.set(entry.name, (this.#counts.get(entry.name) ?? 0) + 1);
    for (const listener of this.#listeners) listener(stored);
    return stored;
  }

  /** Every row, oldest first. */
  get entries(): readonly TimelineEntry[] {
    return this.#entries;
  }

  /** Counts per event name over every row, most frequent first, then by name. */
  counts(): [string, number][] {
    return sortCounts(this.#counts);
  }

  /**
   * Counts per event name of the rows in one visit (or every row for
   * `null`), most frequent first, then by name.
   *
   * @param visit - a page visit, or `null` for every page
   * @returns the counts
   */
  countsIn(visit: number | null): [string, number][] {
    if (visit === null) return this.counts();
    const counts = new Map<string, number>();
    for (const e of this.#entries) {
      if (e.visit === visit) counts.set(e.name, (counts.get(e.name) ?? 0) + 1);
    }
    return sortCounts(counts);
  }

  /** How many rows of `name` arrived. */
  count(name: string): number {
    return this.#counts.get(name) ?? 0;
  }

  /**
   * Every `cid` seen, in first-seen order; with `visit`, only those of rows
   * in that visit.
   *
   * @param visit - a page visit, or `null` (default) for every page
   * @returns the `cid`s
   */
  cids(visit: number | null = null): string[] {
    return [
      ...new Set(
        this.#entries.filter((e) => visit === null || e.visit === visit).map((e) => e.cid),
      ),
    ];
  }

  /**
   * Subscribes to new rows.
   *
   * @param listener - called with each new row
   * @returns an unsubscribe function
   */
  subscribe(listener: (entry: TimelineEntry) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  /**
   * The JSON document `Export JSON` writes.
   *
   * @param meta - host, mode and masked identity
   * @param exportedAt - the export time
   * @returns the export object
   */
  toExport(meta: ExportMeta, exportedAt: Date): Record<string, unknown> {
    return {
      kind: 'ow-tauri-ad-showcase-timeline',
      version: 1,
      exportedAt: exportedAt.toISOString(),
      ...meta,
      counts: Object.fromEntries(this.counts()),
      entries: this.#entries,
    };
  }
}

/**
 * Whether a row passes a filter.
 *
 * @param entry - the row
 * @param filter - the filter
 * @returns `true` when the row is shown
 */
export function matches(entry: TimelineEntry, filter: TimelineFilter): boolean {
  return (
    (filter.visit === null || entry.visit === filter.visit) &&
    (filter.cid === null || entry.cid === filter.cid) &&
    (filter.family === null || entry.family === filter.family)
  );
}

/** Counts sorted most frequent first, then by name. */
function sortCounts(counts: ReadonlyMap<string, number>): [string, number][] {
  return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
}

/**
 * Formats milliseconds as `mm:ss.mmm`.
 *
 * @param ms - milliseconds
 * @returns the clock text
 */
export function clock(ms: number): string {
  const total = Math.max(0, Math.round(ms));
  const m = Math.floor(total / 60000);
  const s = Math.floor((total % 60000) / 1000);
  const rest = total % 1000;
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}.${String(rest).padStart(3, '0')}`;
}

/**
 * A payload whose own keys are `0`, `1`, ... (a string the ad page sent,
 * spread one property per character) read back as that string.
 *
 * @param payload - the payload
 * @returns the string, or `null` when the payload is not a spread string
 */
export function spreadString(payload: Record<string, unknown>): string | null {
  const keys = Object.keys(payload);
  if (keys.length === 0) return null;
  let out = '';
  for (let i = 0; i < keys.length; i += 1) {
    const v = payload[String(i)];
    if (typeof v !== 'string' || v.length !== 1) return null;
    out += v;
  }
  return out;
}
