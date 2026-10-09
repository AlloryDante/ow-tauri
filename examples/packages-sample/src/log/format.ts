/**
 * Text helpers of the log view: time stamps, search, and JSON-safe copies of
 * event payloads.
 *
 * @packageDocumentation
 */
import type { LogEntry } from './store';

/**
 * `HH:MM:SS.mmm` in local time.
 *
 * @param at - `Date.now()` milliseconds
 * @returns the time stamp
 */
export function formatTime(at: number): string {
  const d = new Date(at);
  const two = (n: number): string => String(n).padStart(2, '0');
  return `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}.${String(d.getMilliseconds()).padStart(3, '0')}`;
}

/**
 * A value as one line of text, for search: strings as they are, everything
 * else as JSON (or `String()` when it cannot be serialised).
 *
 * @param value - any value
 * @returns its text
 */
export function valueText(value: unknown): string {
  if (typeof value === 'string') return value;
  if (value === undefined) return 'undefined';
  try {
    // `undefined` for a function or a symbol.
    const json = JSON.stringify(value) as string | undefined;
    return json ?? typeof value;
  } catch {
    return Object.prototype.toString.call(value);
  }
}

/**
 * Whether `entry` matches a search: its message or any of its values
 * contains `query`, ignoring case. An empty query matches everything.
 *
 * @param entry - the log entry
 * @param query - the search text
 * @returns whether it matches
 */
export function matches(entry: LogEntry, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  if (entry.message.toLowerCase().includes(needle)) return true;
  return entry.args.some((arg) => valueText(arg).toLowerCase().includes(needle));
}

/**
 * A JSON-safe copy of `value`: `undefined` becomes `null`, a cycle or a
 * `BigInt` becomes a readable string.
 *
 * @param value - any value
 * @returns the copy
 */
export function jsonSafe(value: unknown): unknown {
  if (value === undefined) return null;
  if (typeof value === 'bigint') return value.toString();
  try {
    return JSON.parse(JSON.stringify(value)) as unknown;
  } catch {
    return Object.prototype.toString.call(value);
  }
}

/**
 * The payload of an `<owadview>` event: the event's own properties (the
 * runtime copies the ad page's data onto the event), without those every
 * `Event` has of its own (`isTrusted` in browsers).
 *
 * @param event - the dispatched event
 * @returns a JSON-safe copy of its own properties
 */
export function payloadOf(event: Event): Record<string, unknown> {
  const own = new Set(Object.getOwnPropertyNames(new Event('x')));
  const out: Record<string, unknown> = {};
  for (const key of Object.getOwnPropertyNames(event)) {
    if (own.has(key)) continue;
    out[key] = jsonSafe(Reflect.get(event, key));
  }
  return out;
}
