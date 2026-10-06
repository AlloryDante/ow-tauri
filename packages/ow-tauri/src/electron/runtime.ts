/**
 * The kernel the `ow-tauri/electron` modules share, and small helpers.
 *
 * @packageDocumentation
 */
import { attachRuntime } from '../bootstrap/install.js';
import type { FacadeKernel } from '../bootstrap/facade-kernel.js';
import type { Event } from './types.js';

/** The document's runtime kernel (attached at import; members check their context when used). */
export const kernel: FacadeKernel = attachRuntime();

/**
 * Creates the synthetic Electron `Event` (CONTRACT B.1.2).
 *
 * @returns a fresh event
 */
export function createEvent(): Event {
  let prevented = false;
  return {
    preventDefault: () => {
      prevented = true;
    },
    get defaultPrevented() {
      return prevented;
    },
  };
}

/**
 * Throws unless the code runs in the main webview.
 *
 * @param api - the member, for the error message
 */
export function requireMain(api: string): void {
  kernel.require('main', api);
}

/**
 * Normalises a path written for Electron (absolute under the app root, a
 * `file://` URL, Windows separators, `.` / `..` segments) to an app-asset
 * path such as `preload/preload.js`.
 *
 * @param path - the path the app passed
 * @param appPath - the virtual app root (`app.getAppPath()`)
 * @returns the asset path, without a leading slash
 */
export function toAssetPath(path: string, appPath: string | undefined): string {
  let p = path.replace(/\\/g, '/');
  if (/^file:\/\//i.test(p)) {
    p = p.replace(/^file:\/\/(localhost)?/i, '');
    try {
      p = decodeURI(p);
    } catch {
      // keep the raw text
    }
  }
  const root = (appPath ?? '').replace(/\\/g, '/').replace(/\/+$/, '');
  if (root !== '' && (p === root || p.startsWith(`${root}/`))) p = p.slice(root.length);
  const out: string[] = [];
  for (const segment of p.split('/')) {
    if (segment === '' || segment === '.') continue;
    if (segment === '..') out.pop();
    else out.push(segment);
  }
  return out.join('/');
}
