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

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return false;
  const proto: unknown = Object.getPrototypeOf(value);
  return proto === Object.prototype || proto === null;
}

/**
 * Builds the event ow-electron dispatches:
 * `new Event(name, { bubbles: false, cancelable: false })` with the fields of
 * `data` copied as own properties (`detail` stays absent, so it reads as the
 * engine's default). Fields are copied only when `data` is a plain object;
 * names the event already has (`type`, `target`, `isTrusted`, ...) are
 * skipped.
 *
 * @param name - the event type
 * @param data - the payload
 * @param EventCtor - the realm's `Event` constructor
 * @returns the event
 */
export function createAdviewEvent(
  name: string,
  data: unknown,
  EventCtor: typeof Event = Event,
): Event {
  const event = new EventCtor(name, { bubbles: false, cancelable: false });
  if (isPlainObject(data)) {
    for (const key of Object.keys(data)) {
      if (key in event) continue;
      Object.defineProperty(event, key, {
        value: data[key],
        writable: true,
        enumerable: true,
        configurable: true,
      });
    }
  }
  return event;
}
