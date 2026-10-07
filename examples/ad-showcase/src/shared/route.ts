/**
 * The window's route: which page is open and its one argument (the size
 * group on page 1, the layout on page 2). It lives in the URL hash
 * (`#layouts/tower`) and in the `--showcase-page=<route>` switch, so a
 * restart in TEST or LIVE comes back on the same page without first mounting
 * page 1. Pure functions, shared by the main process and the window;
 * unit-tested in `route.test.ts`.
 *
 * @packageDocumentation
 */

/** The switch that names the page to open at start (`--showcase-page=<route>`). */
export const PAGE_SWITCH = 'showcase-page';

/** A parsed route. */
export interface Route {
  /** The page id, for example `layouts`. */
  page: string;
  /** The page's argument, or `null`. */
  arg: string | null;
}

/** Page ids and arguments: lower-case letters, digits, `-`, `_` and `x`. */
const PART = /^[a-z0-9][a-z0-9_-]{0,39}$/;

/**
 * Parses `page` or `page/arg`, with or without a leading `#`.
 *
 * @param text - the hash or switch value
 * @returns the route, or `null` when `text` is empty or malformed
 */
export function parseRoute(text: string): Route | null {
  const raw = text.replace(/^#/, '').trim();
  if (raw === '') return null;
  const [page = '', arg, ...rest] = raw.split('/');
  if (rest.length > 0 || !PART.test(page)) return null;
  if (arg === undefined) return { page, arg: null };
  return PART.test(arg) ? { page, arg } : null;
}

/**
 * Formats a route as `page` or `page/arg`.
 *
 * @param route - the route
 * @returns the text (no leading `#`)
 */
export function formatRoute(route: Route): string {
  return route.arg === null ? route.page : `${route.page}/${route.arg}`;
}

/**
 * The arguments without any `--showcase-page` switch, plus one for `route`
 * when it is given.
 *
 * @param args - command-line arguments
 * @param route - the route to open, or `null`
 * @returns the new arguments
 */
export function withPageSwitch(args: readonly string[], route: string | null): string[] {
  const prefix = `--${PAGE_SWITCH}`;
  const rest = args.filter((a) => a !== prefix && !a.startsWith(`${prefix}=`));
  const parsed = route === null ? null : parseRoute(route);
  return parsed ? [...rest, `${prefix}=${formatRoute(parsed)}`] : rest;
}
