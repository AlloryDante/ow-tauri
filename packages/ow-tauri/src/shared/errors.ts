/**
 * Error types shared by every ow-tauri entry point.
 *
 * The contract for each code is in `docs/CONTRACT.md`, sections A.4 (error
 * shape), C.8 (IPC errors) and B.2 (unsupported Electron members).
 *
 * @packageDocumentation
 */

/**
 * Machine-readable reason carried by every {@link OwTauriError}.
 *
 * The same strings are the `code` of the Rust plugin's serialised errors
 * (`docs/CONTRACT.md`, section A.4), so a command rejection maps one-to-one.
 *
 * - `unsupported`: the API exists in Electron or ow-electron but ow-tauri does
 *   not implement it (see {@link OwTauriUnsupportedError}).
 * - `not-ready`: the call needs the host (or a package) to be ready and it is not.
 * - `invalid-argument`: an argument failed validation.
 * - `not-found`: the window, element, event or request it refers to does not exist.
 * - `forbidden`: the calling webview's class may not make this call, or a path
 *   is outside the allowed scope.
 * - `ipc-no-handler`: `ipcRenderer.invoke` reached a channel with no
 *   `ipcMain.handle` registration.
 * - `ipc-timeout`: an IPC request got no reply within its timeout.
 * - `ipc-serialization`: a value cannot be encoded for IPC (section C.7).
 * - `ipc-remote-error`: the `ipcMain.handle` handler threw or rejected.
 * - `ipc-overloaded`: the sender already has the maximum number of IPC
 *   requests in flight or messages queued (section C.4).
 * - `io`: a file-system or window-system operation failed.
 * - `network`: an HTTP request failed.
 * - `backend`: the plugin or a package runtime reported another failure.
 */
export type OwTauriErrorCode =
  | 'unsupported'
  | 'not-ready'
  | 'invalid-argument'
  | 'not-found'
  | 'forbidden'
  | 'ipc-no-handler'
  | 'ipc-timeout'
  | 'ipc-serialization'
  | 'ipc-remote-error'
  | 'ipc-overloaded'
  | 'io'
  | 'network'
  | 'backend';

/**
 * Registry symbol under which every ow-tauri error records the brands of its
 * class chain.
 *
 * `Symbol.for` makes the key identical across bundles, so an error created by
 * one copy of the runtime is recognised by `instanceof` in another copy (for
 * example the injected bootstrap and the app's own bundle in the same
 * webview; `docs/CONTRACT.md`, section B.3).
 */
const BRANDS = Symbol.for('ow-tauri.error.brands');

/** Collects the own `brand` of every class from `ctor` up to (excluding) `Error`. */
function brandsOf(ctor: unknown): readonly string[] {
  const brands: string[] = [];
  for (
    let current: unknown = ctor;
    typeof current === 'function' && current !== Error;
    current = Object.getPrototypeOf(current)
  ) {
    const brand: unknown = Object.getOwnPropertyDescriptor(current, 'brand')?.value;
    if (typeof brand === 'string') brands.push(brand);
  }
  return Object.freeze(brands);
}

/**
 * Base class of every error ow-tauri throws or rejects with.
 *
 * `instanceof` works across separate copies of the runtime in one webview:
 * each instance carries a `Symbol.for('ow-tauri.error.brands')` brand list and
 * the class checks that brand when the prototype chain does not match.
 *
 * @example
 * ```ts
 * try {
 *   await ipcRenderer.invoke('missing-channel');
 * } catch (error) {
 *   if (error instanceof OwTauriError && error.code === 'ipc-no-handler') {
 *     // fall back
 *   }
 * }
 * ```
 */
export class OwTauriError extends Error {
  /**
   * Brand that identifies this class across runtime copies.
   *
   * @internal
   */
  static readonly brand: string = 'OwTauriError';

  /**
   * Brand-aware `instanceof`: true for instances of this class, including
   * instances created by another copy of the ow-tauri runtime.
   *
   * @param value - the value on the left of `instanceof`
   * @returns whether `value` is an instance of this class
   */
  static override [Symbol.hasInstance](value: unknown): boolean {
    if (typeof value !== 'object' || value === null) return false;
    if (Function.prototype[Symbol.hasInstance].call(this, value)) return true;
    const brands: unknown = (value as Record<symbol, unknown>)[BRANDS];
    return Array.isArray(brands) && brands.includes(this.brand);
  }

  /** Machine-readable reason; stable across releases. */
  readonly code: OwTauriErrorCode;

  /**
   * Code-specific details, for example `{ channel }`, `{ reason }`, `{ raw }`
   * (the original text of a rejection Tauri produced before the command ran)
   * or `{ name, message }` (a remote handler's error). `undefined` when there
   * are none.
   */
  readonly data: unknown;

  /**
   * @param code - the machine-readable reason
   * @param message - a human-readable description
   * @param options - standard `ErrorOptions` (e.g. `{ cause }`) plus optional `data`
   */
  constructor(code: OwTauriErrorCode, message: string, options?: OwTauriErrorOptions) {
    super(message, options?.cause === undefined ? undefined : { cause: options.cause });
    this.name = 'OwTauriError';
    this.code = code;
    this.data = options?.data;
    Object.defineProperty(this, BRANDS, { value: brandsOf(new.target), enumerable: false });
  }
}

/**
 * Options accepted by the {@link OwTauriError} constructor.
 */
export interface OwTauriErrorOptions {
  /** The underlying error, as in the standard `ErrorOptions`. */
  cause?: unknown;
  /** Code-specific details; becomes {@link OwTauriError.data}. */
  data?: unknown;
}

/**
 * Thrown synchronously (or used to reject, for async members) when code calls
 * an Electron or ow-electron member that ow-tauri does not implement.
 *
 * Every unsupported member is listed in `docs/CONTRACT.md` section B.2 with
 * the same `api` string this error carries.
 *
 * @example
 * ```ts
 * const err = new OwTauriUnsupportedError('BrowserWindow#setVibrancy', 'no WebView2 equivalent');
 * err.code; // 'unsupported'
 * err.message; // 'ow-tauri does not support BrowserWindow#setVibrancy: no WebView2 equivalent'
 * ```
 */
export class OwTauriUnsupportedError extends OwTauriError {
  /**
   * Brand that identifies this class across runtime copies.
   *
   * @internal
   */
  static override readonly brand: string = 'OwTauriUnsupportedError';

  /** The member that was called, e.g. `BrowserWindow#setVibrancy` or `app.dock`. */
  readonly api: string;

  /**
   * @param api - the member that was called, in `Class#method` or `module.member` form
   * @param reason - why it is unsupported, and the alternative if there is one
   * @param options - optional `cause` and `data`
   */
  constructor(api: string, reason: string, options?: OwTauriErrorOptions) {
    super('unsupported', `ow-tauri does not support ${api}: ${reason}`, options);
    this.name = 'OwTauriUnsupportedError';
    this.api = api;
  }
}
