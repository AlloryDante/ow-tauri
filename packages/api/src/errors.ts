/**
 * The error every function of this package rejects with, and the mapping from
 * what the plugin (or Tauri) sends back.
 *
 * The plugin serialises every command error as
 * `{ code, message, data? }` (Rust `tauri_plugin_overwolf::Error`); the
 * `code` strings below are its wire codes. A rejection that has no such
 * shape was produced by Tauri before the command ran (an ACL denial, a
 * missing command, an argument that did not deserialise).
 *
 * @packageDocumentation
 */

/**
 * Machine-readable reason carried by every {@link OverwolfError}; the same
 * strings are the Rust plugin's wire codes.
 *
 * - `unsupported`: not available on this platform or build (ads on Linux, the
 *   updater off Windows, a switch the configuration does not enable).
 * - `invalid-argument`: an argument failed validation.
 * - `not-found`: the window, element or update it refers to does not exist.
 * - `forbidden`: the calling webview may not make this call (a missing
 *   permission, a reserved label, a page that is not a local app page).
 * - `io`: a file-system or window-system operation failed.
 * - `network`: an HTTP request failed.
 * - `verification`: an update failed its signature or hash check.
 * - `backend`: another failure inside the plugin.
 * - `config`: the plugin configuration is invalid, or the JS runtime and the
 *   crate have different major versions.
 * - `tauri`: Tauri rejected the call before the plugin ran it (other than a
 *   permission denial, which is `forbidden`).
 */
export type OverwolfErrorCode =
  | 'unsupported'
  | 'invalid-argument'
  | 'not-found'
  | 'forbidden'
  | 'io'
  | 'network'
  | 'verification'
  | 'backend'
  | 'config'
  | 'tauri';

/** The JSON shape of every error the plugin returns. */
export interface OverwolfErrorWire {
  /** Machine-readable reason. */
  code: OverwolfErrorCode;
  /** English, one sentence. */
  message: string;
  /** Code-specific details, e.g. `{ status }` for `network`. */
  data?: unknown;
}

/** Every wire code, for {@link isOverwolfErrorWire}. */
export const ERROR_CODES: readonly OverwolfErrorCode[] = Object.freeze([
  'unsupported',
  'invalid-argument',
  'not-found',
  'forbidden',
  'io',
  'network',
  'verification',
  'backend',
  'config',
  'tauri',
]);

const CODES: ReadonlySet<string> = new Set<string>(ERROR_CODES);

/**
 * Registry key under which an {@link OverwolfError} records its brand, so
 * `instanceof` also holds for an error created by another bundled copy of
 * this package in the same page.
 */
const BRAND = Symbol.for('tauri-plugin-overwolf.error');

/** Options of the {@link OverwolfError} constructor. */
export interface OverwolfErrorOptions {
  /** The underlying error, as in the standard `ErrorOptions`. */
  cause?: unknown;
  /** Code-specific details; becomes {@link OverwolfError.data}. */
  data?: unknown;
}

/**
 * The error every function of `tauri-plugin-overwolf-api` rejects with.
 *
 * `instanceof` works across two bundled copies of the package in one page.
 *
 * @example
 * ```ts
 * import { OverwolfError, getMachineIds } from 'tauri-plugin-overwolf-api';
 *
 * try {
 *   await getMachineIds();
 * } catch (error) {
 *   if (error instanceof OverwolfError && error.code === 'forbidden') {
 *     // the capability does not grant overwolf:machine-id
 *   }
 * }
 * ```
 */
export class OverwolfError extends Error {
  /**
   * Brand-aware `instanceof`: true for instances of this class, including
   * instances created by another copy of the package.
   *
   * @param value - the value on the left of `instanceof`
   * @returns whether `value` is an {@link OverwolfError}
   */
  static override [Symbol.hasInstance](value: unknown): boolean {
    if (typeof value !== 'object' || value === null) return false;
    if (Function.prototype[Symbol.hasInstance].call(this, value)) return true;
    return (value as Record<symbol, unknown>)[BRAND] === true;
  }

  /** Machine-readable reason; stable across releases. */
  readonly code: OverwolfErrorCode;

  /** Code-specific details, or `undefined` when there are none. */
  readonly data: unknown;

  /**
   * @param code - the machine-readable reason
   * @param message - a human-readable description
   * @param options - standard `ErrorOptions` (`cause`) plus optional `data`
   */
  constructor(code: OverwolfErrorCode, message: string, options?: OverwolfErrorOptions) {
    super(message, options?.cause === undefined ? undefined : { cause: options.cause });
    this.name = 'OverwolfError';
    this.code = code;
    this.data = options?.data;
    Object.defineProperty(this, BRAND, { value: true, enumerable: false });
  }
}

/**
 * Whether `value` has the {@link OverwolfErrorWire} shape.
 *
 * @param value - any rejection value
 * @returns `true` for `{ code, message }` objects with a known code
 */
export function isOverwolfErrorWire(value: unknown): value is OverwolfErrorWire {
  if (typeof value !== 'object' || value === null) return false;
  const { code, message } = value as Record<string, unknown>;
  return typeof code === 'string' && CODES.has(code) && typeof message === 'string';
}

/** Matches the text of a Tauri ACL denial ("... not allowed ..."). */
const DENIAL = /not allowed|denied|forbidden|permission|capabilit/i;

/**
 * Converts any rejection of a plugin command into an {@link OverwolfError}.
 *
 * - An {@link OverwolfErrorWire} keeps its `code`, `message` and `data`.
 * - Anything else came from Tauri before the command ran: an ACL denial
 *   becomes `forbidden`, everything else `tauri`; the original text is in
 *   `data.raw`.
 *
 * @param raw - the rejection value
 * @param command - the command name, for the message of a Tauri rejection
 * @returns the mapped error (an existing {@link OverwolfError} is returned as is)
 */
export function toOverwolfError(raw: unknown, command: string): OverwolfError {
  if (raw instanceof OverwolfError) return raw;
  if (isOverwolfErrorWire(raw)) {
    return new OverwolfError(
      raw.code,
      raw.message,
      raw.data === undefined ? undefined : { data: raw.data },
    );
  }
  const text = rawText(raw);
  return new OverwolfError(
    DENIAL.test(text) ? 'forbidden' : 'tauri',
    `${command} was rejected by Tauri: ${text}`,
    { data: { raw: text }, cause: raw },
  );
}

function rawText(value: unknown): string {
  if (typeof value === 'string') return value;
  if (value instanceof Error) return value.message;
  try {
    const json: unknown = JSON.stringify(value);
    return typeof json === 'string' ? json : String(value);
  } catch {
    return String(value);
  }
}
