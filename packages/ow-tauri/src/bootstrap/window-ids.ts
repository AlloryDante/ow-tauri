/**
 * App-visible window ids.
 *
 * Electron's `BrowserWindow.id` is available synchronously after
 * `new BrowserWindow()`, but the plugin allocates its window id in the
 * asynchronous `window_create` command. The runtime therefore hands out its
 * own integer ids at construction time and maps them to the plugin's ids
 * (`hostId`) once known. Every id that crosses the wire (`ipc_emit` targets,
 * `IpcSender.windowId`, `window` messages, `parentId`) is translated here, so
 * app code only ever sees the app-visible ids.
 *
 * @packageDocumentation
 */
import type { WindowIds } from './ipc-main.js';

/** Plugin ids of closed windows remembered, so late messages are not re-bound. */
const MAX_FORGOTTEN = 1024;

/** Bidirectional map between app-visible and plugin window ids. */
export class WindowIdMap implements WindowIds {
  #next = 1;
  readonly #toHost = new Map<number, number>();
  readonly #fromHost = new Map<number, number>();
  /** Reserved ids whose `window_create` has not settled. */
  readonly #pending = new Set<number>();
  /** Plugin ids of windows that were forgotten (closed). */
  readonly #forgotten = new Set<number>();
  readonly #listeners = new Set<() => void>();

  /**
   * Allocates an app-visible id whose plugin id is not known yet.
   *
   * @returns the new id
   */
  reserve(): number {
    const id = this.#next++;
    this.#pending.add(id);
    return id;
  }

  /**
   * Records the plugin id of an app-visible id.
   *
   * @param id - the app-visible id
   * @param hostId - the plugin id
   */
  bind(id: number, hostId: number): void {
    this.#toHost.set(id, hostId);
    this.#fromHost.set(hostId, id);
    this.#forgotten.delete(hostId);
    this.#pending.delete(id);
    this.#changed();
  }

  /**
   * Whether the plugin id is known.
   *
   * @param hostId - the plugin id
   * @returns `true` once bound
   */
  knowsHost(hostId: number): boolean {
    return this.#fromHost.has(hostId);
  }

  /** {@inheritDoc WindowIds.peekHost} */
  peekHost(hostId: number): number | undefined {
    return this.#fromHost.get(hostId);
  }

  /** {@inheritDoc WindowIds.isForgotten} */
  isForgotten(hostId: number): boolean {
    return this.#forgotten.has(hostId);
  }

  /** {@inheritDoc WindowIds.hasPending} */
  hasPending(): boolean {
    return this.#pending.size > 0;
  }

  /** {@inheritDoc WindowIds.fromHost} */
  fromHost(hostId: number): number {
    const known = this.#fromHost.get(hostId);
    if (known !== undefined) return known;
    const id = this.#next++;
    this.bind(id, hostId);
    return id;
  }

  /** {@inheritDoc WindowIds.toHost} */
  toHost(id: number): number | undefined {
    return this.#toHost.get(id);
  }

  /**
   * Forgets a window (closed, or its creation failed). Later messages from its
   * plugin id are recognised as stale instead of being bound to a new id.
   *
   * @param id - the app-visible id
   */
  forget(id: number): void {
    const hostId = this.#toHost.get(id);
    this.#toHost.delete(id);
    this.#pending.delete(id);
    if (hostId !== undefined) {
      this.#fromHost.delete(hostId);
      this.#forgotten.add(hostId);
      if (this.#forgotten.size > MAX_FORGOTTEN) {
        const oldest = this.#forgotten.values().next();
        if (oldest.done !== true) this.#forgotten.delete(oldest.value);
      }
    }
    this.#changed();
  }

  /** {@inheritDoc WindowIds.onChange} */
  onChange(listener: () => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  /** Forgets everything and restarts numbering at 1. */
  reset(): void {
    this.#next = 1;
    this.#toHost.clear();
    this.#fromHost.clear();
    this.#pending.clear();
    this.#forgotten.clear();
  }

  #changed(): void {
    for (const listener of [...this.#listeners]) listener();
  }
}
