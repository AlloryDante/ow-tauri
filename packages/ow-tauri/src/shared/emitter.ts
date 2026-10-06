/**
 * A dependency-free `EventEmitter` with Node.js semantics.
 *
 * ow-electron's package manager, its packages, Electron's `app`,
 * `BrowserWindow`, `webContents`, `ipcMain` and `ipcRenderer` are all Node
 * `EventEmitter`s. Webviews have no Node, so ow-tauri ships this
 * re-implementation (`docs/CONTRACT.md` section B.1.2): the same members,
 * listener order, `once` wrappers, `newListener` / `removeListener` events,
 * `'error'` handling with `errorMonitor`, and the max-listeners warning.
 *
 * @packageDocumentation
 */

/** Any listener function. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any -- typed boundary: listener signatures are per event, as in Node's typings
export type Listener = (...args: any[]) => unknown;

/** An event name, as in Node: a string or a symbol. */
export type EventName = string | symbol;

/**
 * Symbol for listeners that observe `'error'` events without handling them;
 * they run before the regular `'error'` listeners, as Node's
 * `events.errorMonitor` does.
 */
export const errorMonitor: unique symbol = Symbol('events.errorMonitor');

/** A `once` wrapper, as returned by `rawListeners()`. */
interface OnceWrapper {
  (...args: unknown[]): unknown;
  /** The original listener. */
  listener: Listener;
}

let defaultMaxListeners = 10;

/**
 * Node.js-compatible event emitter.
 *
 * @example
 * ```ts
 * const emitter = new EventEmitter();
 * emitter.once('ready', (version: string) => console.log(version));
 * emitter.emit('ready', '1.0.0'); // true
 * emitter.emit('ready', '1.0.0'); // false: the once listener was removed
 * ```
 */
export class EventEmitter {
  /** Node's `EventEmitter.errorMonitor`. */
  static readonly errorMonitor: typeof errorMonitor = errorMonitor;

  /**
   * The default for `getMaxListeners` of new emitters
   * (Node's `EventEmitter.defaultMaxListeners`), 10 unless changed.
   */
  static get defaultMaxListeners(): number {
    return defaultMaxListeners;
  }

  /**
   * Sets the default listener limit of new emitters.
   *
   * @param n - a non-negative integer
   */
  static set defaultMaxListeners(n: number) {
    if (!Number.isInteger(n) || n < 0)
      throw new RangeError(`defaultMaxListeners must be a non-negative integer, got ${String(n)}`);
    defaultMaxListeners = n;
  }

  #events = new Map<EventName, Listener[]>();
  #max: number | undefined;
  #warned = new Set<EventName>();

  /**
   * Adds `listener` to the end of the listeners for `event`.
   *
   * @param event - the event name
   * @param listener - the function to call
   * @returns this emitter
   */
  on(event: EventName, listener: Listener): this {
    return this.#add(event, listener, false);
  }

  /**
   * Alias of `on`.
   *
   * @param event - the event name
   * @param listener - the function to call
   * @returns this emitter
   */
  addListener(event: EventName, listener: Listener): this {
    return this.#add(event, listener, false);
  }

  /**
   * Adds `listener` to the beginning of the listeners for `event`.
   *
   * @param event - the event name
   * @param listener - the function to call
   * @returns this emitter
   */
  prependListener(event: EventName, listener: Listener): this {
    return this.#add(event, listener, true);
  }

  /**
   * Adds a one-time listener: it is removed before it is called.
   *
   * @param event - the event name
   * @param listener - the function to call once
   * @returns this emitter
   */
  once(event: EventName, listener: Listener): this {
    return this.#add(event, this.#onceWrap(event, listener), false);
  }

  /**
   * Adds a one-time listener to the beginning of the listeners for `event`.
   *
   * @param event - the event name
   * @param listener - the function to call once
   * @returns this emitter
   */
  prependOnceListener(event: EventName, listener: Listener): this {
    return this.#add(event, this.#onceWrap(event, listener), true);
  }

  /**
   * Removes the most recently added instance of `listener` (or of a `once`
   * wrapper around it) from `event`, then emits `'removeListener'`.
   *
   * @param event - the event name
   * @param listener - the function to remove
   * @returns this emitter
   */
  removeListener(event: EventName, listener: Listener): this {
    checkListener(listener);
    const list = this.#events.get(event);
    if (!list) return this;
    for (let i = list.length - 1; i >= 0; i--) {
      const entry = list[i];
      if (entry === listener || (entry as Partial<OnceWrapper>).listener === listener) {
        list.splice(i, 1);
        if (list.length === 0) this.#events.delete(event);
        if (this.#events.has('removeListener')) {
          this.emit('removeListener', event, (entry as Partial<OnceWrapper>).listener ?? entry);
        }
        break;
      }
    }
    return this;
  }

  /**
   * Alias of `removeListener`.
   *
   * @param event - the event name
   * @param listener - the function to remove
   * @returns this emitter
   */
  off(event: EventName, listener: Listener): this {
    return this.removeListener(event, listener);
  }

  /**
   * Removes all listeners, or those of `event`. Emits `'removeListener'` for
   * each one (last first), as Node does.
   *
   * @param event - the event name; omit to remove every listener
   * @returns this emitter
   */
  removeAllListeners(event?: EventName): this {
    const hasRemoveListener = this.#events.has('removeListener');
    if (event === undefined) {
      if (!hasRemoveListener) {
        this.#events.clear();
        return this;
      }
      for (const name of [...this.#events.keys()]) {
        if (name !== 'removeListener') this.removeAllListeners(name);
      }
      this.removeAllListeners('removeListener');
      return this;
    }
    const list = this.#events.get(event);
    if (!list) return this;
    if (!hasRemoveListener) {
      this.#events.delete(event);
      return this;
    }
    for (let i = list.length - 1; i >= 0; i--) {
      const entry = list[i];
      if (entry) this.removeListener(event, entry);
    }
    return this;
  }

  /**
   * Calls every listener of `event` synchronously, in registration order,
   * with `args`.
   *
   * An `'error'` event without listeners throws its first argument (or an
   * `Error` describing it), after `errorMonitor` listeners ran.
   *
   * @param event - the event name
   * @param args - the arguments passed to each listener
   * @returns whether the event had listeners
   */
  emit(event: EventName, ...args: unknown[]): boolean {
    if (event === 'error') {
      const monitors = this.#events.get(errorMonitor);
      if (monitors) for (const fn of [...monitors]) fn.apply(this, args);
      if (!this.#events.has('error')) throw unhandledError(args[0]);
    }
    const list = this.#events.get(event);
    if (!list) return false;
    for (const fn of [...list]) fn.apply(this, args);
    return true;
  }

  /**
   * A copy of the listeners of `event`, with `once` wrappers unwrapped.
   *
   * @param event - the event name
   * @returns the listener functions
   */
  listeners(event: EventName): Listener[] {
    return (this.#events.get(event) ?? []).map((fn) => (fn as Partial<OnceWrapper>).listener ?? fn);
  }

  /**
   * A copy of the listeners of `event`, including `once` wrappers.
   *
   * @param event - the event name
   * @returns the listener functions as registered
   */
  rawListeners(event: EventName): Listener[] {
    return [...(this.#events.get(event) ?? [])];
  }

  /**
   * The number of listeners of `event`, or of `listener` only.
   *
   * @param event - the event name
   * @param listener - when given, count only this function (and its `once` wrappers)
   * @returns the listener count
   */
  listenerCount(event: EventName, listener?: Listener): number {
    const list = this.#events.get(event) ?? [];
    if (listener === undefined) return list.length;
    return list.filter(
      (fn) => fn === listener || (fn as Partial<OnceWrapper>).listener === listener,
    ).length;
  }

  /**
   * The names of events that have listeners, in insertion order.
   *
   * @returns the event names
   */
  eventNames(): EventName[] {
    return [...this.#events.keys()];
  }

  /**
   * Sets the listener count per event above which a warning is printed
   * (0 = unlimited).
   *
   * @param n - the limit
   * @returns this emitter
   */
  setMaxListeners(n: number): this {
    if (!Number.isInteger(n) || n < 0)
      throw new RangeError(`n must be a non-negative integer, got ${String(n)}`);
    this.#max = n;
    return this;
  }

  /**
   * The current max-listeners limit.
   *
   * @returns the limit (default `EventEmitter.defaultMaxListeners`)
   */
  getMaxListeners(): number {
    return this.#max ?? defaultMaxListeners;
  }

  #add(event: EventName, listener: Listener, prepend: boolean): this {
    checkListener(listener);
    if (this.#events.has('newListener')) {
      this.emit('newListener', event, (listener as Partial<OnceWrapper>).listener ?? listener);
    }
    let list = this.#events.get(event);
    if (!list) {
      list = [];
      this.#events.set(event, list);
    }
    if (prepend) list.unshift(listener);
    else list.push(listener);
    const max = this.getMaxListeners();
    if (max > 0 && list.length > max && !this.#warned.has(event)) {
      this.#warned.add(event);
      console.warn(
        `MaxListenersExceededWarning: Possible EventEmitter memory leak detected. ${String(list.length)} ${String(event)} listeners added. Use emitter.setMaxListeners() to increase limit`,
      );
    }
    return this;
  }

  #onceWrap(event: EventName, listener: Listener): OnceWrapper {
    checkListener(listener);
    let fired = false;
    const wrapper = ((...args: unknown[]): unknown => {
      if (fired) return undefined;
      fired = true;
      this.removeListener(event, wrapper);
      return listener.apply(this, args);
    }) as OnceWrapper;
    wrapper.listener = listener;
    return wrapper;
  }
}

function checkListener(listener: unknown): asserts listener is Listener {
  if (typeof listener !== 'function') {
    throw new TypeError(
      `The "listener" argument must be of type function. Received ${typeof listener}`,
    );
  }
}

function unhandledError(value: unknown): Error {
  if (value instanceof Error) return value;
  let text: string;
  try {
    const json: string | undefined = typeof value === 'string' ? undefined : JSON.stringify(value);
    text = typeof value === 'string' ? `'${value}'` : (json ?? String(value));
  } catch {
    text = String(value);
  }
  const error = new Error(`Unhandled error. (${text})`);
  Object.defineProperty(error, 'context', { value, enumerable: false });
  return error;
}

/**
 * Emits an event that originates from the host (a host message), not from
 * app code: an `'error'` without listeners is logged at warn level instead of
 * thrown, because there is no caller to throw to (CONTRACT B.1.2). Listener
 * exceptions are reported with `reportError` and do not stop the dispatch of
 * later host messages.
 *
 * @param emitter - the emitter
 * @param event - the event name
 * @param args - the listener arguments
 * @returns whether the event had listeners
 */
export function emitFromHost(emitter: EventEmitter, event: EventName, ...args: unknown[]): boolean {
  if (event === 'error' && emitter.listenerCount('error') === 0) {
    for (const fn of emitter.listeners(errorMonitor)) safeCall(() => fn.apply(emitter, args));
    console.warn('[ow-tauri] unhandled error event from the host:', ...args);
    return false;
  }
  let had = false;
  safeCall(() => {
    had = emitter.emit(event, ...args);
  });
  return had;
}

/**
 * Runs `fn`, reporting (not throwing) any exception the way an uncaught
 * listener error would be reported in a browser.
 *
 * @param fn - the function to run
 */
export function safeCall(fn: () => unknown): void {
  try {
    fn();
  } catch (error) {
    reportUncaught(error);
  }
}

/**
 * Reports an exception as uncaught without interrupting the caller.
 *
 * @param error - the exception
 */
export function reportUncaught(error: unknown): void {
  const report = (globalThis as { reportError?: (e: unknown) => void }).reportError;
  if (typeof report === 'function') report(error);
  else console.error(error);
}
