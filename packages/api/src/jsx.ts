/**
 * `tauri-plugin-overwolf-api/jsx`: types only. Declares `<owadview>` as a
 * JSX intrinsic element of React (`React.JSX`, which React 18's global `JSX`
 * namespace extends, so both 18 and 19 see it).
 *
 * Import it once for its types (`import type {} from 'tauri-plugin-overwolf-api/jsx'`).
 * Attach event listeners with a `ref` and `addEventListener`: the element's
 * events are plain DOM events with underscore names, which React's `on*`
 * props do not map.
 *
 * Preact and Solid declare the element with the exported
 * {@link OwAdViewAttributes}:
 *
 * ```ts
 * import type { OwAdViewAttributes } from 'tauri-plugin-overwolf-api/jsx';
 * declare module 'preact' {
 *   namespace JSX {
 *     interface IntrinsicElements {
 *       owadview: OwAdViewAttributes & preact.JSX.HTMLAttributes<HTMLElement>;
 *     }
 *   }
 * }
 * ```
 *
 * @example
 * ```tsx
 * import 'tauri-plugin-overwolf-api/adview';
 * import type {} from 'tauri-plugin-overwolf-api/jsx';
 *
 * export function Banner() {
 *   return (
 *     <div style={{ width: 400, height: 300 }}>
 *       <owadview cid="main-mrec" slotsize="400x300" />
 *     </div>
 *   );
 * }
 * ```
 *
 * @packageDocumentation
 */
import type { DetailedHTMLProps, HTMLAttributes } from 'react';

import type { OwAdViewElement } from './adview/index.js';

/** The `<owadview>` attributes (`docs/api/owadview.md`). */
export interface OwAdViewAttributes {
  /** Container id, at most 20 characters. */
  cid?: string;
  /** Requested inventory, `"WxH"`. */
  slotsize?: string;
  /** Style tokens, e.g. `"high-impact-ad;"`. */
  adstyle?: string;
  /** Custom tracking, a JSON object as text. */
  customtracking?: string;
  /** Present for a performance ad. */
  performance?: boolean | '';
  /** Ad unit override. */
  unit?: string;
  /** The ad page's `pageUrl`. */
  pageurl?: string;
}

/** React props of `<owadview>`. */
export type OwAdViewProps = DetailedHTMLProps<HTMLAttributes<OwAdViewElement>, OwAdViewElement> &
  OwAdViewAttributes;

declare module 'react' {
  // eslint-disable-next-line @typescript-eslint/no-namespace -- React declares JSX as a namespace
  namespace JSX {
    interface IntrinsicElements {
      /** Overwolf's ad element (`tauri-plugin-overwolf-api/adview`). */
      owadview: OwAdViewProps;
    }
  }
}
