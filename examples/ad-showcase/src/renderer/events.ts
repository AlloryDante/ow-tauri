/**
 * The `<owadview>` events the showcase listens to, their colour family and
 * how a payload is read. Pure functions; unit-tested in `events.test.ts`.
 *
 * Events are plain, non-bubbling `Event`s whose payload fields are own
 * properties (the same on both hosts), so each slot listens to every name
 * here on its element. The list is the documented and observed names
 * (docs/CONTRACT.md B.3.5) plus the host lifecycle
 * events.
 *
 * @packageDocumentation
 */

/** Event names each slot listens to. */
export const EVENT_NAMES: readonly string[] = [
  // Display and video (standard slots).
  'display_ad_loaded',
  'player_loaded',
  'play',
  'impression',
  'complete',
  // Reward (adstyle "rewarded-ad;").
  'video_ad_ready',
  'video_no_ready_ads',
  'video_no_impression_timeout',
  'video_ad_skipped',
  // High impact.
  'high-impact-ad-loaded',
  'high-impact-ad-removed',
  // Interstitial (performance).
  'performance_ad_loaded',
  'performance_ad_error',
  'performance_ad_dismiss',
  'performance_ad_clicked',
  'performance_ad_video_complete',
  'performance_ad_video_skipped',
  'performance_ad_no_fill',
  'shutdown',
  // House ads, in both spellings.
  'house_ad_action',
  'house-ad-action',
  // Clicks, in both spellings.
  'ad-clicked',
  'ad_clicked',
  // Host lifecycle.
  'did-attach',
  'dom-ready',
  'did-finish-load',
  'did-fail-load',
  'render-process-gone',
  'destroyed',
];

/** Colour families of the timeline. */
export type Family =
  | 'display'
  | 'video'
  | 'performance'
  | 'reward'
  | 'high-impact'
  | 'house'
  | 'lifecycle'
  | 'error'
  | 'control';

/** Every family, in filter order. */
export const FAMILIES: readonly Family[] = [
  'display',
  'video',
  'reward',
  'high-impact',
  'performance',
  'house',
  'lifecycle',
  'error',
  'control',
];

/** What kind of slot raised an event; reward slots colour their video events as reward. */
export type SlotKind = 'standard' | 'reward' | 'high-impact' | 'performance' | 'house';

const VIDEO = new Set(['player_loaded', 'play', 'impression', 'complete']);
const ERRORS = new Set(['did-fail-load', 'render-process-gone', 'performance_ad_error']);

/**
 * The family an event belongs to.
 *
 * @param name - the event name
 * @param kind - the kind of slot that raised it
 * @returns the family
 */
export function familyOf(name: string, kind: SlotKind = 'standard'): Family {
  if (ERRORS.has(name)) return 'error';
  if (name.startsWith('control:')) return 'control';
  if (name.startsWith('video_')) return 'reward';
  if (VIDEO.has(name)) return kind === 'reward' ? 'reward' : 'video';
  if (name === 'display_ad_loaded') return 'display';
  if (name.startsWith('high-impact-')) return 'high-impact';
  if (name.startsWith('performance_') || name === 'shutdown') return 'performance';
  if (name === 'house_ad_action' || name === 'house-ad-action') return 'house';
  return 'lifecycle';
}

/**
 * The payload of an `<owadview>` event: its own properties, except
 * `isTrusted` (an own property of every `Event` in both engines). Values that
 * cannot be serialised become strings.
 *
 * @param event - the dispatched event
 * @returns a JSON-safe copy of the own properties
 */
export function payloadOf(event: Event): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  const source = event as unknown as Record<string, unknown>;
  for (const key of Object.getOwnPropertyNames(event)) {
    if (key === 'isTrusted') continue;
    out[key] = jsonSafe(source[key]);
  }
  return out;
}

function jsonSafe(value: unknown): unknown {
  if (value === undefined) return null;
  try {
    return JSON.parse(JSON.stringify(value)) as unknown;
  } catch {
    // A cycle or a BigInt: keep a readable form.
    return typeof value === 'bigint' ? value.toString() : Object.prototype.toString.call(value);
  }
}
