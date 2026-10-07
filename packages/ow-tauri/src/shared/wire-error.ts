/**
 * Conversion between the plugin's serialised errors and {@link OwTauriError}.
 *
 * Contract: `docs/CONTRACT.md` sections A.4 (wire shape and the wrapping of
 * rejections Tauri produces before a command runs) and C.8 (IPC errors).
 *
 * @packageDocumentation
 */
import { OwTauriError, OwTauriUnsupportedError, type OwTauriErrorCode } from './errors.js';

/**
 * The JSON shape of every error the plugin returns (`OverwolfErrorWire`,
 * CONTRACT A.4). `IpcErrorWire` in section C is the same shape.
 */
export interface OverwolfErrorWire {
  /** Machine-readable reason. */
  code: OwTauriErrorCode;
  /** English, single sentence. */
  message: string;
  /** Code-specific details. */
  data?: unknown;
}

const CODES: ReadonlySet<string> = new Set<OwTauriErrorCode>([
  'unsupported',
  'not-ready',
  'invalid-argument',
  'not-found',
  'forbidden',
  'ipc-no-handler',
  'ipc-timeout',
  'ipc-serialization',
  'ipc-remote-error',
  'ipc-overloaded',
  'io',
  'network',
  'backend',
]);

/**
 * Whether `value` has the {@link OverwolfErrorWire} shape.
 *
 * @param value - any rejection value
 * @returns `true` for `{ code, message }` objects with a known code
 */
export function isErrorWire(value: unknown): value is OverwolfErrorWire {
  if (typeof value !== 'object' || value === null) return false;
  const { code, message } = value as Record<string, unknown>;
  return typeof code === 'string' && CODES.has(code) && typeof message === 'string';
}

/** Matches the text of a Tauri ACL denial ("... not allowed ..."). */
const DENIAL = /not allowed|denied|forbidden|permission|capabilit/i;

/**
 * Converts any rejection value from a plugin command into an
 * {@link OwTauriError}.
 *
 * - An {@link OverwolfErrorWire} keeps its `code`, `message` and `data`;
 *   `unsupported` becomes an {@link OwTauriUnsupportedError} for `command`.
 * - Anything else was produced by Tauri before the command ran (CONTRACT A.4):
 *   an ACL denial becomes `forbidden`, everything else `invalid-argument`, with
 *   the original text in `data.raw`.
 *
 * @param raw - the rejection value
 * @param command - the command name, used for `OwTauriUnsupportedError.api`
 * @returns the mapped error (an existing `OwTauriError` is returned as is)
 */
export function fromWireError(raw: unknown, command: string): OwTauriError {
  if (raw instanceof OwTauriError) return raw;
  if (isErrorWire(raw)) {
    const options = raw.data === undefined ? undefined : { data: raw.data };
    if (raw.code === 'unsupported')
      return new OwTauriUnsupportedError(command, raw.message, options);
    return new OwTauriError(raw.code, raw.message, options);
  }
  const text = rawText(raw);
  const code: OwTauriErrorCode = DENIAL.test(text) ? 'forbidden' : 'invalid-argument';
  return new OwTauriError(code, `${command} was rejected by Tauri: ${text}`, {
    data: { raw: text },
    cause: raw,
  });
}

/**
 * Whether a mapped error came from Tauri rejecting the call before the
 * command ran (so the plugin never saw it; CONTRACT A.4, C.3 "Gaps").
 *
 * @param error - an error produced by {@link fromWireError}
 * @returns `true` when `data.raw` is present
 */
export function isPreCommandRejection(error: OwTauriError): boolean {
  return typeof error.data === 'object' && error.data !== null && 'raw' in error.data;
}

/**
 * Name and message of any thrown value, the way Electron reports a remote
 * handler error (`<name>: <message>`).
 *
 * @param error - the thrown value
 * @returns `{ name, message }`; non-errors use `Error` and their string form
 */
export function describeThrown(error: unknown): { name: string; message: string } {
  if (error instanceof Error) return { name: error.name, message: error.message };
  if (typeof error === 'object' && error !== null) {
    const { name, message } = error as Record<string, unknown>;
    if (typeof message === 'string') {
      return { name: typeof name === 'string' ? name : 'Error', message };
    }
  }
  return { name: 'Error', message: rawText(error) };
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

/**
 * `error.stack` in V8's shape: a `<name>: <message>` line, then the frames.
 * JavaScriptCore (WKWebView) writes only the frames, so text that Electron
 * code builds from `error.stack` (electron-updater's `Error: <stack>` log
 * line and `error` event message) would lose the message; this adds the
 * line when the first line is a frame.
 *
 * @param error - the error
 * @returns the stack with its header line, or `String(error)` without a stack
 */
export function v8Stack(error: Error): string {
  const stack = error.stack;
  if (typeof stack !== 'string' || stack === '') return String(error);
  const first = stack.split('\n', 1)[0] ?? '';
  const isFrame =
    /^\s+at /.test(first) ||
    /@.*:\d+:\d+$/.test(first) ||
    /^[^\s:]*@(\S*|\[native code\])$/.test(first);
  return isFrame ? `${String(error)}\n${stack}` : stack;
}
