/**
 * The synchronous state cache of the main webview (`docs/CONTRACT.md`
 * sections B.1.6 and C.6).
 *
 * Built from the snapshot Rust injects before any page script
 * (`window.__OW_TAURI_BOOTSTRAP__`) and kept current by numbered `state`
 * patches. Synchronous members (`app.getPath()`, `screen.getAllDisplays()`,
 * `uid`, ...) read it.
 *
 * @packageDocumentation
 */
import type { StateMessage } from '../shared/protocol.js';

/** Called after a patch (or a resync) changed the value at `path`. */
export type StateListener = (path: string, value: unknown, previous: unknown) => void;

/** Outcome of {@link StateCache.apply}. */
export type ApplyResult = 'applied' | 'ignored' | 'gap';

/**
 * Snapshot plus patches, with the sequence rules of CONTRACT B.1.6:
 * patches with `seq <= cache.seq` are ignored, `seq === cache.seq + 1` is
 * applied, and anything later is a gap the owner resolves with a
 * `bootstrap` call ({@link StateCache.load}).
 */
export class StateCache {
  #seq = 0;
  #data: Record<string, unknown> = {};
  readonly #listeners = new Set<StateListener>();

  /** The last applied state sequence number. */
  get seq(): number {
    return this.#seq;
  }

  /**
   * Replaces the whole cache with a snapshot and notifies listeners for every
   * top-level key.
   *
   * @param snapshot - a `HostSnapshot`; a non-object empties the cache
   */
  load(snapshot: unknown): void {
    const previous = this.#data;
    this.#data = isRecord(snapshot) ? structuredCloneSafe(snapshot) : {};
    this.#seq = typeof this.#data['seq'] === 'number' ? this.#data['seq'] : 0;
    const keys = new Set([...Object.keys(previous), ...Object.keys(this.#data)]);
    for (const key of keys) {
      if (key !== 'seq' && previous[key] !== this.#data[key])
        this.#notify(key, this.#data[key], previous[key]);
    }
  }

  /**
   * Applies a `state` message if it is the next in sequence.
   *
   * @param message - the message
   * @returns `'applied'`, `'ignored'` (old or duplicate) or `'gap'` (a
   *   sequence number is missing; nothing was applied)
   */
  apply(message: Pick<StateMessage, 'seq' | 'patches'>): ApplyResult {
    if (message.seq <= this.#seq) return 'ignored';
    if (message.seq > this.#seq + 1) return 'gap';
    for (const patch of message.patches) this.set(patch.path, patch.value);
    this.#seq = message.seq;
    this.#data['seq'] = message.seq;
    return 'applied';
  }

  /**
   * Sets the sequence number without changing data, so the next patch
   * applies (used when a resync could not close a gap).
   *
   * @param seq - the new sequence number
   */
  forceSeq(seq: number): void {
    this.#seq = seq;
    this.#data['seq'] = seq;
  }

  /**
   * Reads the value at a dot path, e.g. `identity.uid` or `displays`.
   *
   * @param path - the dot path; `''` returns the whole snapshot
   * @returns the value, or `undefined`
   */
  get(path: string): unknown {
    if (path === '') return this.#data;
    let current: unknown = this.#data;
    for (const key of path.split('.')) {
      if (!isRecord(current) && !Array.isArray(current)) return undefined;
      current = Object.hasOwn(current, key) ? (current as Record<string, unknown>)[key] : undefined;
    }
    return current;
  }

  /**
   * Sets the value at a dot path, creating intermediate objects. Used for
   * patches and for the synchronous writes of B.1.6 item 5.
   *
   * @param path - the dot path
   * @param value - the new value
   */
  set(path: string, value: unknown): void {
    const keys = path.split('.').filter((k) => k !== '');
    const last = keys.pop();
    if (last === undefined || [...keys, last].some(isUnsafeKey)) return;
    let current: Record<string, unknown> = this.#data;
    for (const key of keys) {
      const next = current[key];
      if (!isRecord(next)) {
        const created: Record<string, unknown> = {};
        current[key] = created;
        current = created;
      } else current = next;
    }
    const previous = current[last];
    current[last] = value;
    this.#notify(path, value, previous);
  }

  /**
   * Subscribes to changes.
   *
   * @param listener - called with the changed path, its new and previous value
   * @returns a function that unsubscribes
   */
  onChange(listener: StateListener): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  #notify(path: string, value: unknown, previous: unknown): void {
    for (const listener of [...this.#listeners]) {
      try {
        listener(path, value, previous);
      } catch (error) {
        console.error('[ow-tauri] state listener failed', error);
      }
    }
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isUnsafeKey(key: string): boolean {
  return key === '__proto__' || key === 'constructor' || key === 'prototype';
}

function structuredCloneSafe(value: Record<string, unknown>): Record<string, unknown> {
  try {
    return structuredClone<Record<string, unknown>>(value);
  } catch {
    return { ...value };
  }
}
