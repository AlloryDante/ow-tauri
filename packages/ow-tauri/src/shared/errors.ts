/**
 * Error types shared by every ow-tauri entry point.
 *
 * The contract for each code is in `docs/CONTRACT.md`, section C.5 (IPC
 * errors) and section B.2 (unsupported Electron members).
 *
 * @packageDocumentation
 */

/**
 * Machine-readable reason carried by every {@link OwTauriError}.
 *
 * - `unsupported`: the API exists in Electron or ow-electron but ow-tauri does
 *   not implement it (see {@link OwTauriUnsupportedError}).
 * - `ipc-no-handler`: `ipcRenderer.invoke` reached a channel with no
 *   `ipcMain.handle` registration.
 * - `ipc-timeout`: an IPC request got no reply within its timeout.
 * - `ipc-serialization`: an argument or result cannot cross IPC as JSON.
 * - `not-ready`: the call needs the host to be ready and it is not yet.
 * - `invalid-argument`: an argument failed validation.
 * - `backend`: the Rust plugin or a package runtime reported a failure.
 */
export type OwTauriErrorCode =
  | 'unsupported'
  | 'ipc-no-handler'
  | 'ipc-timeout'
  | 'ipc-serialization'
  | 'not-ready'
  | 'invalid-argument'
  | 'backend';

/**
 * Base class of every error ow-tauri throws or rejects with.
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
  /** Machine-readable reason; stable across releases. */
  readonly code: OwTauriErrorCode;

  /**
   * @param code - the machine-readable reason
   * @param message - a human-readable description
   * @param options - standard `ErrorOptions`, e.g. `{ cause }`
   */
  constructor(code: OwTauriErrorCode, message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = 'OwTauriError';
    this.code = code;
  }
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
  /** The member that was called, e.g. `BrowserWindow#setVibrancy` or `app.dock`. */
  readonly api: string;

  /**
   * @param api - the member that was called, in `Class#method` or `module.member` form
   * @param reason - why it is unsupported, and the alternative if there is one
   */
  constructor(api: string, reason: string) {
    super('unsupported', `ow-tauri does not support ${api}: ${reason}`);
    this.name = 'OwTauriUnsupportedError';
    this.api = api;
  }
}
