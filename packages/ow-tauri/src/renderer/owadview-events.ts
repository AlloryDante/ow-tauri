/**
 * DOM events of `<owadview>` (`docs/CONTRACT.md` B.3.5): ow-electron
 * dispatches each ad page and guest lifecycle event on the element as a plain,
 * non-bubbling `Event` whose payload fields are own properties [OBS].
 *
 * @packageDocumentation
 */

/**
 * Event names Overwolf documents in two spellings: each is also dispatched in
 * the other spelling, the received spelling first (B.3.5).
 */
export const SPELLING_TWINS: Readonly<Record<string, string>> = Object.freeze({
  ad_clicked: 'ad-clicked',
  'ad-clicked': 'ad_clicked',
  house_ad_action: 'house-ad-action',
  'house-ad-action': 'house_ad_action',
});

/** Names of guest-originated click events, for the host `ad-clicked` de-duplication. */
export const CLICK_NAMES: ReadonlySet<string> = new Set(['ad_clicked', 'ad-clicked']);

/** How long a guest click suppresses the host's own `ad-clicked` (B.3.5). */
export const CLICK_DEDUPE_MS = 1000;

/**
 * Builds the event ow-electron dispatches:
 * `new Event(name, { bubbles: false, cancelable: false })` with the payload
 * copied as own properties the way `Object.assign(event, data)` copies it
 * [OBS]: the own enumerable properties of an object (an array by index), a
 * string spread into one property per character (`0`, `1`, ...; seen with
 * `performance_ad_error`), and nothing for `null`, `undefined`, numbers and
 * booleans. `detail` stays absent, so it reads as the engine's default.
 * Names the event already has (`type`, `target`, `isTrusted`, ...) are
 * skipped, where `Object.assign` would throw on the read-only ones.
 *
 * @param name - the event type
 * @param data - the payload
 * @param EventCtor - the realm's `Event` constructor
 * @returns the event
 *
 * @example
 * ```ts
 * const event = createAdviewEvent('performance_ad_error', 'Error');
 * event[0]; // 'E'
 * ```
 */
export function createAdviewEvent(
  name: string,
  data: unknown,
  EventCtor: typeof Event = Event,
): Event {
  const event = new EventCtor(name, { bubbles: false, cancelable: false });
  if (data === null || (typeof data !== 'object' && typeof data !== 'string')) return event;
  // `Object(data)` is what `Object.assign` reads: a string becomes a String object.
  const source = Object(data) as Record<string, unknown>;
  for (const key of Object.keys(source)) {
    if (key in event) continue;
    Object.defineProperty(event, key, {
      value: source[key],
      writable: true,
      enumerable: true,
      configurable: true,
    });
  }
  return event;
}
