/**
 * What the ads tester shows: the upstream sample's ad layouts (two slots
 * around the app's main area), the events each `<owadview>` can dispatch,
 * and the performance ad's events.
 *
 * @packageDocumentation
 */

/** A slot size in CSS pixels, `[width, height]`. */
export type SlotSize = readonly [number, number];

/** A layout of the ads tester. */
export interface AdLayout {
  /** The select value. */
  readonly id: string;
  /** The select label. */
  readonly label: string;
  /** The first slot (left, or top of the ad zone). */
  readonly ad1: SlotSize;
  /** The second slot (right, or bottom of the ad zone). */
  readonly ad2: SlotSize;
  /** Both slots in one ad zone on the right, the 400x600 one with `adstyle="high-impact-ad;"`. */
  readonly highImpact?: true;
}

/** The video-capable size: at most one per page with live ads (Overwolf ad policy). */
export const VIDEO_SIZE: SlotSize = [400, 300];

/** The upstream sample's layouts, in its order. */
export const LAYOUTS: readonly AdLayout[] = [
  {
    id: 'tower-plus-high-impact',
    label: 'Tower Plus + High Impact',
    ad1: [400, 60],
    ad2: [400, 600],
    highImpact: true,
  },
  { id: 'tall-duo-right', label: 'Tall Duo (right)', ad1: [160, 600], ad2: [400, 600] },
  { id: 'tall-duo-left', label: 'Tall Duo (left)', ad1: [400, 600], ad2: [160, 600] },
  { id: 'combo-classic-right', label: 'Combo Classic (right)', ad1: [300, 250], ad2: [400, 600] },
  { id: 'combo-classic-left', label: 'Combo Classic (left)', ad1: [400, 600], ad2: [300, 250] },
  { id: 'studio-tower-right', label: 'Studio Tower (right)', ad1: [160, 600], ad2: [300, 250] },
  { id: 'studio-tower-left', label: 'Studio Tower (left)', ad1: [300, 250], ad2: [160, 600] },
  { id: 'tower-right', label: 'Tower (right)', ad1: [728, 90], ad2: [400, 600] },
  { id: 'tower-left', label: 'Tower (left)', ad1: [400, 600], ad2: [728, 90] },
  { id: 'tower-plus-right', label: 'Tower Plus (right)', ad1: [400, 60], ad2: [400, 600] },
  { id: 'tower-plus-left', label: 'Tower Plus (left)', ad1: [400, 600], ad2: [400, 60] },
  { id: 'studio-right', label: 'Studio, video (right)', ad1: [728, 90], ad2: [400, 300] },
  { id: 'studio-left', label: 'Studio, video (left)', ad1: [400, 300], ad2: [728, 90] },
  { id: 'studio-plus-right', label: 'Studio Plus, video (right)', ad1: [400, 60], ad2: [400, 300] },
  { id: 'studio-plus-left', label: 'Studio Plus, video (left)', ad1: [400, 300], ad2: [400, 60] },
];

/** The layout the page opens with (the upstream default). */
export const DEFAULT_LAYOUT = 'tall-duo-right';

/**
 * The layout with id `id`, else the default one.
 *
 * @param id - a layout id
 * @returns the layout
 */
export function layoutOf(id: string): AdLayout {
  const found = LAYOUTS.find((l) => l.id === id) ?? LAYOUTS.find((l) => l.id === DEFAULT_LAYOUT);
  if (!found) throw new Error('the default layout is missing');
  return found;
}

/**
 * `"WxH"`, the `slotsize` attribute.
 *
 * @param size - the slot size
 * @returns the text
 */
export function sizeText([w, h]: SlotSize): string {
  return `${String(w)}x${String(h)}`;
}

/**
 * Whether a slot of `size` can play video ads.
 *
 * @param size - the slot size
 * @returns whether it is the video size
 */
export function isVideoSize(size: SlotSize): boolean {
  return size[0] === VIDEO_SIZE[0] && size[1] === VIDEO_SIZE[1];
}

/** Every event a display or video `<owadview>` dispatches (`docs/api/owadview.md`). */
export const SLOT_EVENTS: readonly string[] = [
  'display_ad_loaded',
  'player_loaded',
  'play',
  'impression',
  'complete',
  'high-impact-ad-loaded',
  'high-impact-ad-removed',
  'house_ad_action',
  'ad-clicked',
  'did-attach',
  'did-fail-load',
  'render-process-gone',
  'destroyed',
];

/** Every event of the performance (interstitial) `<owadview>`. */
export const PERFORMANCE_EVENTS: readonly string[] = [
  'performance_ad_loaded',
  'performance_ad_error',
  'performance_ad_no_fill',
  'performance_ad_dismiss',
  'performance_ad_clicked',
  'performance_ad_video_complete',
  'performance_ad_video_skipped',
  'impression',
  'complete',
  'shutdown',
];

/** Performance events after which the element is removed again. */
export const PERFORMANCE_END_EVENTS: readonly string[] = [
  'performance_ad_error',
  'performance_ad_no_fill',
  'performance_ad_dismiss',
  'shutdown',
];
