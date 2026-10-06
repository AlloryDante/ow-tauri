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

/** Bidirectional map between app-visible and plugin window ids. */
export class WindowIdMap implements WindowIds {
  #next = 1;
  readonly #toHost = new Map<number, number>();
  readonly #fromHost = new Map<number, number>();

  /**
   * Allocates an app-visible id whose plugin id is not known yet.
   *
   * @returns the new id
   */
  reserve(): number {
    return this.#next++;
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

  /** {@inheritDoc WindowIds.fromHost} */
  fromHost(hostId: number): number {
    const known = this.#fromHost.get(hostId);
    if (known !== undefined) return known;
    const id = this.reserve();
    this.bind(id, hostId);
    return id;
  }

  /** {@inheritDoc WindowIds.toHost} */
  toHost(id: number): number | undefined {
    return this.#toHost.get(id);
  }

  /**
   * Forgets a window.
   *
   * @param id - the app-visible id
   */
  forget(id: number): void {
    const hostId = this.#toHost.get(id);
    this.#toHost.delete(id);
    if (hostId !== undefined) this.#fromHost.delete(hostId);
  }

  /** Forgets everything and restarts numbering at 1. */
  reset(): void {
    this.#next = 1;
    this.#toHost.clear();
    this.#fromHost.clear();
  }
}
