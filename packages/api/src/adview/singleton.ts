/**
 * The page-wide `<owadview>` runtime registration. Two bundled copies of
 * this package in one page (an app bundle and a library bundle, say) must
 * never run two runtimes, or every element would mount twice; the first copy
 * registers its runtime under a `Symbol.for` key and every later copy uses
 * that registration.
 *
 * @packageDocumentation
 */
import { warnOnce } from '../internal.js';

/** The registry key on the global object. */
export const RUNTIME_KEY = Symbol.for('tauri-plugin-overwolf.adview.runtime');

/** The registered runtime, as every copy of the package sees it. */
export interface AdviewApi {
  /** The package version of the copy that registered the runtime. */
  readonly version: string;
  /**
   * Registers an `<owadview>` element the runtime has not seen (one created
   * in a way the runtime cannot observe) and mounts it when it is ready.
   *
   * @param el - the element
   */
  upgrade(el: Element): void;
  /**
   * The `<owadview>` elements the runtime tracks in this document.
   *
   * @returns the elements, in discovery order
   */
  elements(): HTMLElement[];
}

function isAdviewApi(value: unknown): value is AdviewApi {
  if (typeof value !== 'object' || value === null) return false;
  const api = value as Record<string, unknown>;
  return (
    typeof api['version'] === 'string' &&
    typeof api['upgrade'] === 'function' &&
    typeof api['elements'] === 'function'
  );
}

/**
 * The registered runtime, or `undefined`.
 *
 * @param scope - the global object (tests pass their own)
 * @returns the registration
 */
export function registeredRuntime(scope: object = globalThis): AdviewApi | undefined {
  const value: unknown = Reflect.get(scope, RUNTIME_KEY);
  return isAdviewApi(value) ? value : undefined;
}

/**
 * Returns the registered runtime, or creates and registers one.
 *
 * @param version - the version of this copy of the package
 * @param create - creates and starts the runtime (called at most once per page)
 * @param scope - the global object (tests pass their own)
 * @returns the page's runtime
 */
export function installRuntime(
  version: string,
  create: () => AdviewApi,
  scope: object = globalThis,
): AdviewApi {
  const existing = registeredRuntime(scope);
  if (existing) {
    if (existing.version.split('.')[0] !== version.split('.')[0]) {
      warnOnce(
        'adview:two-majors',
        `two copies of tauri-plugin-overwolf-api (${existing.version} and ${version}) are bundled in this page; the <owadview> runtime of ${existing.version} serves both`,
      );
    }
    return existing;
  }
  const api = create();
  Object.defineProperty(scope, RUNTIME_KEY, {
    value: api,
    configurable: true,
    enumerable: false,
    writable: false,
  });
  return api;
}
