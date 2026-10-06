/**
 * Guest-to-host transport shared by the injected guest shims
 * (`docs/CONTRACT.md` D.1, D.4): the plugin command through
 * `window.__TAURI_INTERNALS__.invoke`, captured once so page scripts cannot
 * redirect it later, with a bounded retry queue.
 *
 * @packageDocumentation
 */

/** Messages kept while the transport is not available yet (D.1). */
export const MAX_QUEUED = 200;

/** Retry interval of the queue, in milliseconds (D.1). */
export const RETRY_MS = 250;

/** Largest JSON encoding of `data` sent as is (D.4, A.2.6). */
export const MAX_DATA_BYTES = 16 * 1024;

/** `invoke(cmd, args)` as captured from `__TAURI_INTERNALS__`. */
export type Invoke = (command: string, args: Record<string, unknown>) => unknown;

/**
 * Captures `window.__TAURI_INTERNALS__.invoke`, bound to its owner, or
 * returns `undefined` when it is not installed (yet).
 *
 * @param win - the guest window
 * @returns the captured invoke function
 */
export function captureInvoke(win: Window): Invoke | undefined {
  try {
    const internals: unknown = Reflect.get(win, '__TAURI_INTERNALS__');
    if (typeof internals !== 'object' || internals === null) return undefined;
    const invoke: unknown = Reflect.get(internals, 'invoke');
    if (typeof invoke !== 'function') return undefined;
    const apply = Reflect.apply;
    return (command, args): unknown => apply(invoke, internals, [command, args]) as unknown;
  } catch {
    return undefined;
  }
}

/**
 * Reduces `value` to plain JSON data (D.4): functions, symbols, DOM nodes
 * and `Window` objects are dropped, cycles are cut, `Event` objects become
 * `{ type }`, big integers become strings, and a value whose JSON encoding
 * exceeds {@link MAX_DATA_BYTES} is replaced by `{ truncated: true, bytes }`.
 *
 * @param value - any value a page passed
 * @returns JSON data, or `null` when nothing is left
 */
export function sanitize(value: unknown): unknown {
  if (value === undefined) return null;
  const seen = new WeakSet();
  // `JSON.stringify` returns `undefined` for a function or symbol at the top.
  let json: unknown;
  try {
    json = JSON.stringify(value, (_key, v: unknown): unknown => {
      if (typeof v === 'function' || typeof v === 'symbol') return undefined;
      if (typeof v === 'bigint') return v.toString();
      if (typeof v === 'object' && v !== null) {
        if (typeof Node !== 'undefined' && v instanceof Node) return undefined;
        if (typeof Window !== 'undefined' && v instanceof Window) return undefined;
        if (seen.has(v)) return undefined;
        seen.add(v);
        if (typeof Event !== 'undefined' && v instanceof Event) return { type: v.type };
      }
      return v;
    });
  } catch {
    return null;
  }
  if (typeof json !== 'string') return null;
  const bytes = new TextEncoder().encode(json).length;
  if (bytes > MAX_DATA_BYTES) return { truncated: true, bytes };
  return JSON.parse(json) as unknown;
}

/**
 * A fresh JSON copy of `value` (`undefined` stays `undefined`).
 *
 * @param value - JSON data
 * @returns a deep copy, or `null` when it cannot be copied
 */
export function copy(value: unknown): unknown {
  if (value === undefined) return undefined;
  try {
    return JSON.parse(JSON.stringify(value)) as unknown;
  } catch {
    return null;
  }
}

/**
 * Freezes `value` and everything reachable from it.
 *
 * @param value - the value to freeze
 * @returns `value`
 */
export function deepFreeze<T>(value: T): T {
  if ((typeof value === 'object' && value !== null) || typeof value === 'function') {
    if (!Object.isFrozen(value)) {
      Object.freeze(value);
      for (const key of Object.keys(value)) deepFreeze(Reflect.get(value, key));
    }
  }
  return value;
}

/** Sends plugin commands from a guest, queueing until the transport exists. */
export class Outbox {
  readonly #win: Window;
  readonly #command: string;
  readonly #setTimeout: (handler: () => void, ms: number) => unknown;
  #invoke: Invoke | undefined;
  readonly #queue: Record<string, unknown>[] = [];
  #timer = false;

  /**
   * @param win - the guest window; its transport and timer are captured now
   * @param command - the plugin command, e.g. `plugin:overwolf|adview_event`
   */
  constructor(win: Window, command: string) {
    this.#win = win;
    this.#command = command;
    const timer = win.setTimeout.bind(win);
    this.#setTimeout = (handler, ms) => timer(handler, ms);
    this.#invoke = captureInvoke(win);
  }

  /** Messages waiting for the transport. */
  get queued(): number {
    return this.#queue.length;
  }

  /**
   * Sends `args` now, or queues it (at most {@link MAX_QUEUED}; later ones
   * are dropped) until the transport is available.
   *
   * @param args - the command arguments
   */
  post(args: Record<string, unknown>): void {
    this.#invoke ??= captureInvoke(this.#win);
    if (this.#invoke !== undefined && this.#queue.length === 0) {
      this.#send(this.#invoke, args);
      return;
    }
    if (this.#queue.length < MAX_QUEUED) this.#queue.push(args);
    this.#schedule();
  }

  #send(invoke: Invoke, args: Record<string, unknown>): void {
    try {
      const result = invoke(this.#command, args);
      if (result instanceof Promise) result.catch(() => undefined);
    } catch {
      // The host refused it (limits, validation); nothing to retry.
    }
  }

  #schedule(): void {
    if (this.#timer) return;
    this.#timer = true;
    this.#setTimeout(() => {
      this.#timer = false;
      this.#flush();
    }, RETRY_MS);
  }

  #flush(): void {
    this.#invoke ??= captureInvoke(this.#win);
    const invoke = this.#invoke;
    if (invoke === undefined) {
      if (this.#queue.length > 0) this.#schedule();
      return;
    }
    for (let next = this.#queue.shift(); next !== undefined; next = this.#queue.shift()) {
      this.#send(invoke, next);
    }
  }
}
