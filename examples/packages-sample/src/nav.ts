/**
 * The pages of the side navigation and the URL hash that selects one
 * (`#ads`, `#settings`, …), so a page survives a reload and the lab driver
 * can open one.
 *
 * @packageDocumentation
 */

/** A page of the sample. */
export interface PageInfo {
  /** The hash without `#`. */
  readonly id: PageId;
  /** The navigation label and page title. */
  readonly title: string;
  /** The line under the page title. */
  readonly description: string;
}

/** The page ids. */
export type PageId = 'logger' | 'ads' | 'settings' | 'updater' | 'packages';

/** The start page. */
const LOGGER: PageInfo = {
  id: 'logger',
  title: 'Logger',
  description: 'App info, every API call and result, and every ad event',
};

/** Every page, in navigation order; the first is the start page. */
export const PAGES: readonly PageInfo[] = [
  LOGGER,
  {
    id: 'ads',
    title: 'Ads Tester',
    description: 'Preview ad layouts, a performance ad and the video slot, and track their events',
  },
  {
    id: 'settings',
    title: 'CMP & Settings',
    description: 'Consent, e-mail hashes, machine ids and the analytics and ads switches',
  },
  {
    id: 'updater',
    title: 'Updater',
    description: "Check Overwolf's update feed and follow the download",
  },
  {
    id: 'packages',
    title: 'Packages',
    description: 'GEP, overlay, recorder and utility on Tauri',
  },
];

/**
 * The page a URL hash selects; an unknown or empty hash selects the first.
 *
 * @param hash - `location.hash`, with or without `#`
 * @returns the page
 */
export function pageOf(hash: string): PageInfo {
  const id = hash.replace(/^#\/?/, '');
  return PAGES.find((p) => p.id === id) ?? LOGGER;
}
