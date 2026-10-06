/**
 * Package errors (`docs/CONTRACT.md` section B.1.5): the `RecorderError`
 * class and the rebuild of a package runtime's `$kind`-tagged error value.
 *
 * @packageDocumentation
 */

/** Wire shape of `internalError` and of plain package errors. */
interface ErrorLike {
  name?: unknown;
  message?: unknown;
}

/**
 * The recorder package's error (upstream
 * `class RecorderError extends Error`). `instanceof RecorderError` and
 * `instanceof Error` both hold.
 *
 * @example
 * ```ts
 * import { RecorderError } from 'ow-tauri/main';
 *
 * try {
 *   await recorder.startRecording(options);
 * } catch (error) {
 *   if (error instanceof RecorderError) console.warn(error.codeStr, error.code);
 * }
 * ```
 */
export class RecorderError extends Error {
  /** Numeric error code reported by the recorder. */
  readonly code: number;
  /** Symbolic error code reported by the recorder. */
  readonly codeStr: string;
  /** The underlying error, when the recorder reported one. */
  readonly internalError?: Error;

  /**
   * @param message - the message
   * @param code - numeric code
   * @param codeStr - symbolic code
   * @param internalError - the underlying error
   */
  constructor(message: string, code: number, codeStr: string, internalError?: Error) {
    super(message);
    this.name = 'RecorderError';
    this.code = code;
    this.codeStr = codeStr;
    if (internalError !== undefined) this.internalError = internalError;
  }
}

function plainError(value: ErrorLike): Error {
  const message = value.message;
  const error = new Error(
    typeof message === 'string'
      ? message
      : message === undefined || message === null
        ? ''
        : JSON.stringify(message),
  );
  if (typeof value.name === 'string' && value.name !== '') error.name = value.name;
  return error;
}

/**
 * Rebuilds the value a package call rejects with from the `data.error` the
 * runtime reported (B.1.5): a {@link RecorderError}, a frozen
 * `UtilityApiError` object, or an `Error` with the reported name.
 *
 * @param raw - the reported error value
 * @returns the rejection value
 * @internal
 */
export function rebuildPackageError(raw: unknown): unknown {
  const value = (
    typeof raw === 'object' && raw !== null ? raw : { message: String(raw) }
  ) as Record<string, unknown>;
  switch (value['$kind']) {
    case 'RecorderError': {
      const inner = value['internalError'];
      return new RecorderError(
        typeof value['message'] === 'string' ? value['message'] : '',
        typeof value['code'] === 'number' ? value['code'] : 0,
        typeof value['codeStr'] === 'string' ? value['codeStr'] : '',
        typeof inner === 'object' && inner !== null ? plainError(inner) : undefined,
      );
    }
    case 'UtilityApiError': {
      const out: { message: string; exitCode?: number } = {
        message: typeof value['message'] === 'string' ? value['message'] : '',
      };
      if (typeof value['exitCode'] === 'number') out.exitCode = value['exitCode'];
      return Object.freeze(out);
    }
    default:
      return plainError(value);
  }
}
