/**
 * The performance (interstitial) ad of the ads tester: a `<owadview
 * performance>` appended to `document.body`, as in the upstream sample. The
 * runtime lays it over the whole window; it is removed again when the ad
 * ends (dismissed, no fill, error or shutdown).
 *
 * @packageDocumentation
 */
import { PERFORMANCE_END_EVENTS, PERFORMANCE_EVENTS } from './formats';

/**
 * Appends a performance `<owadview>` to `doc.body` unless one is already
 * there.
 *
 * @param onEvent - called with each event of the element
 * @param doc - the document (tests pass their own)
 * @returns the new element, or `null` when one is already showing
 */
export function showPerformanceAd(
  onEvent: (name: string, event: Event) => void,
  doc: Document = document,
): HTMLElement | null {
  if (doc.querySelector('owadview[performance]')) return null;
  const el = doc.createElement('owadview');
  el.setAttribute('performance', '');
  for (const name of PERFORMANCE_EVENTS) {
    el.addEventListener(name, (event) => {
      onEvent(name, event);
      if (PERFORMANCE_END_EVENTS.includes(name)) el.remove();
    });
  }
  doc.body.append(el);
  return el;
}
