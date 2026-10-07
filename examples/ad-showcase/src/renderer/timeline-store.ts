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
}

/** The fields of a new row; the store adds `seq`. */
export type NewEntry = Omit<TimelineEntry, 'seq'>;

/** Which rows are shown. */
export interface TimelineFilter {
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
  #seq = 0;

  /**
   * Appends a row and notifies listeners.
   *
   * @param entry - the row without its sequence number
   * @returns the stored row
   */
  add(entry: NewEntry): TimelineEntry {
    this.#seq += 1;
    const stored: TimelineEntry = { seq: this.#seq, ...entry };
    this.#entries.push(stored);
    this.#counts.set(entry.name, (this.#counts.get(entry.name) ?? 0) + 1);
    for (const listener of this.#listeners) listener(stored);
    return stored;
  }

  /** Every row, oldest first. */
  get entries(): readonly TimelineEntry[] {
    return this.#entries;
  }

  /** Counts per event name, most frequent first, then by name. */
  counts(): [string, number][] {
    return [...this.#counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  }

  /** How many rows of `name` arrived. */
  count(name: string): number {
    return this.#counts.get(name) ?? 0;
  }

  /** Every `cid` seen, in first-seen order. */
  cids(): string[] {
    return [...new Set(this.#entries.map((e) => e.cid))];
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
    (filter.cid === null || entry.cid === filter.cid) &&
    (filter.family === null || entry.family === filter.family)
  );
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
