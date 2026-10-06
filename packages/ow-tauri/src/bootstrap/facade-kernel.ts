/**
 * The documented surface the npm facades (`ow-tauri/main`, `/electron`,
 * `/renderer`, `/testing`) may use on the injected runtime kernel
 * ([ADR 0012](../../../docs/adr/0012-js-runtime-singleton.md)).
 *
 * The bootstrap that creates the kernel is embedded in the plugin crate; the
 * facades are bundled from npm. The two are released separately, so this
 * interface is a versioned API: a facade attaches to an installed runtime only
 * when the runtime reports the same `contract` and `api`
 * ({@link RUNTIME_API_VERSION}). Facade code is typed against this interface
 * only, never against the `Kernel` class, so the compiler rejects any use of a
 * kernel member that is not part of it.
 *
 * Rules for changing it: adding an optional member is compatible; removing or
 * changing a member, or changing its behaviour, increments
 * {@link RUNTIME_API_VERSION}.
 *
 * @packageDocumentation
 */
import type { IpcMain, SenderResolver } from './ipc-main.js';
import type { IpcRenderer } from './ipc-renderer.js';
import type { KernelServices } from './services.js';
import type { StateListener } from './state-cache.js';
import type { OtjValue } from '../shared/otj.js';
import type { HostContext, HostMessage } from '../shared/protocol.js';
import { RUNTIME_API_VERSION } from '../shared/protocol.js';

export { RUNTIME_API_VERSION };

/** Longest time `main_ready` waits for tasks passed to `FacadeKernel.deferMainReady`. */
export const MAIN_READY_HOLD_MS = 2000;

/** Called for each host message of one `type`. */
export type HostMessageHandler = (message: HostMessage) => void;

/** The state cache, as facades read it (CONTRACT B.1.6). */
export interface FacadeStateCache {
  /**
   * The value at a dotted path, e.g. `paths.userData`.
   *
   * @param path - the path
   * @returns the value, or `undefined`
   */
  get(path: string): unknown;
  /**
   * Subscribes to changes.
   *
   * @param listener - called with `(path, value, previous)`
   * @returns a function that unsubscribes
   */
  onChange(listener: StateListener): () => void;
  /**
   * Writes a value at a dotted path: the synchronous writes of B.1.6 item 5.
   * Optional (added without a facade API change); facades check for it.
   *
   * @param path - the path, e.g. `flags.adsFpdDisabled`
   * @param value - the new value
   */
  set?(path: string, value: unknown): void;
}

/** App-visible window ids and their plugin ids (see `WindowIdMap`). */
export interface FacadeWindowIds {
  /**
   * Allocates an app-visible id for a window that is being created.
   *
   * @returns the id
   */
  reserve(): number;
  /**
   * Records the plugin id of a reserved id.
   *
   * @param id - the app-visible id
   * @param hostId - the plugin id
   */
  bind(id: number, hostId: number): void;
  /**
   * Whether a plugin id is bound.
   *
   * @param hostId - the plugin id
   * @returns `true` once bound
   */
  knowsHost(hostId: number): boolean;
  /**
   * The app-visible id of a plugin id, allocating one for an unknown id.
   *
   * @param hostId - the plugin id
   * @returns the app-visible id
   */
  fromHost(hostId: number): number;
  /**
   * The plugin id of an app-visible id.
   *
   * @param id - the app-visible id
   * @returns the plugin id, or `undefined` while the window is being created
   */
  toHost(id: number): number | undefined;
  /**
   * Forgets a window (closed, or its creation failed).
   *
   * @param id - the app-visible id
   */
  forget(id: number): void;
}

/** The main-side IPC server, as facades use it (CONTRACT C.2, C.3, C.5). */
export interface FacadeIpcServer {
  /** The global `ipcMain`. */
  readonly ipcMain: IpcMain;
  /** Builds the `sender` of IPC events; the window registry replaces it. */
  senderResolver: SenderResolver;
  /** Encoded size cap for `webContents.send` and replies. */
  readonly maxMessageBytes: number;
  /**
   * The `webContents.ipc` scope of a window, created on first use.
   *
   * @param windowId - the app-visible window id
   * @returns the scope
   */
  scoped(windowId: number): IpcMain;
  /**
   * Forgets a window's scope.
   *
   * @param windowId - the app-visible window id
   */
  dropScope(windowId: number): void;
  /**
   * `webContents.send` (C.5).
   *
   * @param windowId - the app-visible target window id
   * @param channel - the channel
   * @param args - the arguments
   */
  emit(windowId: number, channel: string, args: readonly unknown[]): void;
  /**
   * `webContents.send` with arguments encoded earlier.
   *
   * @param windowId - the app-visible target window id
   * @param channel - the channel (already checked)
   * @param encoded - OTJ-encoded arguments
   */
  emitEncoded(windowId: number, channel: string, encoded: OtjValue[]): void;
}

/**
 * The kernel members the npm facades may use. Everything else on the
 * runtime is private to the bootstrap.
 */
export interface FacadeKernel extends KernelServices {
  /** Package version of the code that created the kernel. */
  readonly version: string;
  /** Contract version of the code that created the kernel. */
  readonly contract: number;
  /** Facade API version of the code that created the kernel. */
  readonly api: number;
  /** Whether `main_ready` has been acknowledged (main webview). */
  readonly isReady: boolean;
  /** The state cache. */
  readonly state: FacadeStateCache;
  /** App-visible window ids. */
  readonly windowIds: FacadeWindowIds;
  /** The main-side IPC server. */
  readonly server: FacadeIpcServer;
  /** The document's `ipcRenderer`. */
  readonly ipcRenderer: IpcRenderer;
  /**
   * The singleton stored under `key`, created with `factory` the first time;
   * shared by every copy of the package in the webview.
   *
   * @param key - a namespaced key, e.g. `electron.app`
   * @param factory - creates the value
   * @returns the singleton
   */
  singleton<T>(key: string, factory: () => T): T;
  /**
   * Registers a hook that a runtime reset (tests) runs.
   *
   * @param hook - clears state kept outside the kernel
   * @returns a function that unregisters the hook
   */
  onReset(hook: () => void): () => void;
  /**
   * Subscribes to host messages of one `type` (`window`, `lifecycle`, ...).
   * Several subscribers may claim a lifecycle or close request; the kernel
   * answers it with its default only when nobody subscribed.
   *
   * @param type - the message type
   * @param handler - called synchronously, in channel order
   * @returns a function that unsubscribes
   */
  on(type: string, handler: HostMessageHandler): () => void;
  /**
   * Resolves once `ipc_main_ready` and `main_ready` were sent (main webview).
   *
   * @returns the readiness promise
   */
  whenHostReady(): Promise<void>;
  /**
   * Holds `main_ready` until `task` settles (at most {@link MAIN_READY_HOLD_MS}
   * in total), so startup calls such as `disableAnonymousAnalytics()` reach
   * the plugin before the launch sequence starts (CONTRACT A.2.2, E.3).
   * Ignored once `main_ready` was sent. Optional: added in the same facade
   * API version; facades check for it before use.
   *
   * @param task - a command the plugin must see before `main_ready`
   */
  deferMainReady?(task: Promise<unknown>): void;
  /**
   * Records the browser switches for the next launch (CONTRACT A.1.1):
   * sends `app_record_browser_args { args }` with the session's full set and
   * repeats the set as `main_ready { pendingBrowserArgs }`. Optional, like
   * {@link FacadeKernel.deferMainReady}.
   *
   * @param args - every switch recorded so far, e.g. `['--disable-gpu']`
   */
  recordBrowserArgs?(args: readonly string[]): void;
  /**
   * Starts the runtime (subscribes the host channel); idempotent.
   *
   * @returns the epoch, or `null` when the document has no IPC
   */
  start(): Promise<string | null>;
  /**
   * Testing: overrides context detection.
   *
   * @param context - the context, or `null` to detect it again
   */
  setContextOverride(context: HostContext | null): void;
  /** Testing: restores a fresh document state and reloads the snapshot. */
  reset(): void;
}
