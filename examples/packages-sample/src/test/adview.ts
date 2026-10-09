/**
 * Test helper: gives happy-dom the layout the `<owadview>` runtime needs to
 * mount an element (a non-empty box that is visible).
 *
 * @packageDocumentation
 */

/** Stubs `getBoundingClientRect` and `checkVisibility` of every element. */
export function layoutForAds(): void {
  Object.defineProperty(HTMLElement.prototype, 'getBoundingClientRect', {
    configurable: true,
    writable: true,
    value: () => ({
      x: 0,
      y: 0,
      width: 300,
      height: 250,
      left: 0,
      top: 0,
      right: 300,
      bottom: 250,
      toJSON: () => ({}),
    }),
  });
  Object.defineProperty(HTMLElement.prototype, 'checkVisibility', {
    configurable: true,
    writable: true,
    value: () => true,
  });
}
