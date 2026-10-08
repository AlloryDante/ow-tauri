/**
 * `tauri-plugin-overwolf-api/adview`: the `<owadview>` element.
 *
 * Importing this module installs the `<owadview>` runtime in the page (once
 * per page, however many copies of the package are bundled). Elements then
 * behave as in ow-electron: the same attributes, properties, methods and DOM
 * events (`docs/api/owadview.md`). Each element shows a native ad webview
 * placed over its box. Needs `overwolf:default` for the page's webview.
 *
 * The runtime stays inert outside a Tauri webview and in the plugin's own
 * webviews (`owad-*`, `ow-cmp*`). Elements inside shadow roots or iframes
 * are not supported.
 *
 * @example
 * ```ts
 * import 'tauri-plugin-overwolf-api/adview';
 *
 * const ad = document.createElement('owadview');
 * ad.setAttribute('cid', 'main-mrec');
 * ad.setAttribute('slotsize', '400x300');
 * ad.addEventListener('display_ad_loaded', () => console.log('ad loaded'));
 * document.querySelector('.ad-container')?.append(ad);
 * ```
 *
 * @packageDocumentation
 */
import {
  RUNTIME_VERSION,
  currentWebviewLabel,
  inTauri,
  isReservedLabel,
  log,
} from '../internal.js';
import { AdviewRuntime } from './element.js';
import { installRuntime, type AdviewApi } from './singleton.js';
import { tauriServices } from './transport.js';

export type { AdviewApi } from './singleton.js';
export type { AdviewAttributes, AdviewGeometry, AdviewRect } from './attributes.js';
export type { AdviewEventMessage } from './transport.js';

/**
 * An `<owadview>` element after the runtime attached it (ow-electron's
 * element members, B.3.3). Before attach it is a plain `HTMLElement`.
 */
export interface OwAdViewElement extends HTMLElement {
  /** Container id (`cid` attribute). */
  cid: string;
  /** Requested inventory, `"WxH"` (`slotsize` attribute). */
  slotsize: string;
  /** The ad page's `pageUrl` (`pageurl` attribute). */
  pageUrl: string;
  /** Whether it is a performance ad (`performance` attribute). */
  performance: boolean;
  /** Ad unit override (`unit` attribute). */
  unit: string;
  /** Style tokens (`adstyle` attribute). */
  adstyle: string;
  /** Custom tracking JSON (`customtracking` attribute). */
  customTracking: string;
  /**
   * Sets `pageurl` and tells the ad page.
   *
   * @param url - the page URL
   */
  setPageUrl(url: string): void;
  /**
   * Sends a command to the ad page.
   *
   * @param args - the command arguments (JSON values)
   */
  sendCommand(...args: unknown[]): void;
  /**
   * Mutes or unmutes the ad.
   *
   * @param muted - whether to mute
   */
  setAudioMuted(muted: boolean): void;
  /** Reloads the ad page. */
  reload(): void;
}

declare global {
  interface HTMLElementTagNameMap {
    /** Overwolf's ad element (`tauri-plugin-overwolf-api/adview`). */
    owadview: OwAdViewElement;
  }
}

const INERT: AdviewApi = Object.freeze({
  version: RUNTIME_VERSION,
  upgrade: () => undefined,
  elements: () => [],
});

function start(): AdviewApi {
  if (typeof document === 'undefined') return INERT;
  if (!inTauri()) {
    log('warn', '<owadview> stays inert outside a Tauri webview');
    return INERT;
  }
  const label = currentWebviewLabel();
  if (label !== undefined && isReservedLabel(label)) return INERT;
  return installRuntime(RUNTIME_VERSION, () => {
    const runtime = new AdviewRuntime(tauriServices());
    runtime.start();
    return Object.freeze({
      version: RUNTIME_VERSION,
      upgrade: (el: Element) => {
        runtime.upgrade(el);
      },
      elements: () => runtime.elements(),
    });
  });
}

/**
 * The page's `<owadview>` runtime. The import installs it, so apps rarely
 * need this object; it registers elements created in ways the runtime cannot
 * observe, and lists the tracked elements (tests).
 *
 * @example
 * ```ts
 * import { adview } from 'tauri-plugin-overwolf-api/adview';
 *
 * adview.elements(); // the tracked <owadview> elements
 * ```
 */
export const adview: AdviewApi = start();
