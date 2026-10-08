/**
 * `<owadview>` attributes (`docs/CONTRACT.md` B.3.2) and the wire shapes of
 * the `adview_*` commands.
 *
 * @packageDocumentation
 */

/**
 * The attribute names the runtime reads and observes. HTML lower-cases
 * attribute names, so `customTracking` is stored as `customtracking`.
 */
export const ADVIEW_ATTRIBUTES = [
  'cid',
  'slotsize',
  'adstyle',
  'customtracking',
  'performance',
  'unit',
  'pageurl',
] as const;

/** Attributes whose change after mount remounts the guest (B.3.2). */
export const REMOUNT_ATTRIBUTES = ['cid', 'slotsize', 'adstyle', 'performance', 'unit'] as const;

/** Longest container id Overwolf accepts [DOC]. */
export const MAX_CID_LENGTH = 20;

/** The element attributes as `adview_mount` sends them. */
export interface AdviewAttributes {
  /** Container id, trimmed, at most 20 characters. */
  cid: string;
  /** Requested inventory, `"WxH"`. */
  slotsize: string;
  /** Style tokens, e.g. `"high-impact-ad;"`, or `""`. */
  adstyle: string;
  /** The parsed `customtracking` JSON object, or `null`. */
  customTracking: unknown;
  /** Whether the element is a performance ad. */
  performance: boolean;
  /** Ad unit override, or `null`. */
  unit: string | null;
  /** The guest's `__overwolf__.pageUrl`, `""` when absent. */
  pageurl: string;
}

/** An element rectangle in CSS pixels relative to the embedder viewport. */
export interface AdviewRect {
  /** Left edge. */
  x: number;
  /** Top edge. */
  y: number;
  /** Width. */
  width: number;
  /** Height. */
  height: number;
}

/**
 * What the plugin needs to place a guest: the element rectangle in CSS
 * pixels, and the page's `devicePixelRatio` and `innerWidth` (the plugin
 * derives the page zoom from them, per OS).
 */
export interface AdviewGeometry {
  /** The element rectangle. */
  rect: AdviewRect;
  /** `window.devicePixelRatio` of the embedder page. */
  devicePixelRatio: number;
  /** `window.innerWidth` of the embedder page, in CSS pixels. */
  innerWidth: number;
}

/**
 * Parses a `customTracking` value: a JSON object (or array) is kept, anything
 * else, including invalid JSON, clears it silently [DOC].
 *
 * @param text - the attribute value, or `null` when absent
 * @returns the parsed object, or `null`
 */
export function parseCustomTracking(text: string | null): unknown {
  if (text === null || text.trim() === '') return null;
  try {
    const value: unknown = JSON.parse(text);
    return typeof value === 'object' && value !== null ? value : null;
  } catch {
    return null;
  }
}

/**
 * Reads the attributes of an element (B.3.2).
 *
 * @param el - the `<owadview>` element
 * @returns the wire attributes
 */
export function readAttributes(el: Element): AdviewAttributes {
  return {
    cid: (el.getAttribute('cid') ?? '').trim().slice(0, MAX_CID_LENGTH),
    slotsize: (el.getAttribute('slotsize') ?? '').trim(),
    adstyle: el.getAttribute('adstyle') ?? '',
    customTracking: parseCustomTracking(el.getAttribute('customtracking')),
    performance: el.hasAttribute('performance'),
    unit: el.getAttribute('unit'),
    pageurl: el.getAttribute('pageurl') ?? '',
  };
}

/**
 * Whether two attribute sets differ in a field that remounts the guest.
 *
 * @param a - the mounted attributes
 * @param b - the current attributes
 * @returns `true` when a remount is needed
 */
export function needsRemount(a: AdviewAttributes, b: AdviewAttributes): boolean {
  return REMOUNT_ATTRIBUTES.some((key) => a[key] !== b[key]);
}

/**
 * Whether two `customTracking` values are equal (as JSON).
 *
 * @param a - one value
 * @param b - the other value
 * @returns `true` when they encode the same
 */
export function sameTracking(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
