/**
 * The per-webview runtime kernel (`docs/CONTRACT.md` section B, "One
 * runtime per webview", and [ADR 0012](../../../docs/adr/0012-js-runtime-singleton.md)).
 *
 * One kernel exists per document. It owns the host-message channel, the IPC
 * sequence counters, the listener registries (`ipcMain`, `ipcRenderer`, window
 * scopes), the state cache and the window id map. The npm entry points attach
 * to it and keep their own singletons in it, so several bundles in one
 * webview share one runtime.
 *
 * @packageDocumentation
 */
import { safeCall } from '../shared/emitter.js';
import { OwTauriError } from '../shared/errors.js';
import { decode, encode } from '../shared/otj.js';
import {
  CONTRACT_VERSION,
  PACKAGE_VERSION,
  PLUGIN,
  type HostContext,
  type HostMessage,
  type IpcResultMessage,
  type IpcSender,
  type StateMessage,
} from '../shared/protocol.js';
import { fromWireError } from '../shared/wire-error.js';
import { IpcServer, remoteError } from './ipc-main.js';
import { IpcClient, IpcRenderer } from './ipc-renderer.js';
import type { KernelServices, LogLevel } from './services.js';
import { StateCache } from './state-cache.js';
import { tauriTransport, type Transport } from './transport.js';
import { WindowIdMap } from './window-ids.js';

/** Name of the global the bootstrap installs. */
export const RUNTIME_GLOBAL = '__OW_TAURI_RUNTIME__';
/** Name of the snapshot global Rust injects. */
export const BOOTSTRAP_GLOBAL = '__OW_TAURI_BOOTSTRAP__';

/** Called for each host message of one `type`. */
export type HostMessageHandler = (message: HostMessage) => void;

/** Options for {@link Kernel}. */
export interface KernelOptions {
  /** The transport; default: `@tauri-apps/api`. */
  transport?: Transport;
  /** Makes every member throw this error (a page whose runtime has another contract). */
  mismatch?: OwTauriError;
  /** Reads the injected snapshot; default: `globalThis.__OW_TAURI_BOOTSTRAP__`. */
  readBootstrap?: () => unknown;
}

/**
 * Detects the context from a webview label: `ow-main` is the main webview,
 * `bw-<n>` a UI or overlay window, anything else has no runtime.
 *
 * @param label - the webview label
 * @returns the context
 */
export function contextOfLabel(label: string | undefined): HostContext {
  if (label === 'ow-main') return 'main';
  if (label !== undefined && /^bw-\d+$/.test(label)) return 'ui';
  return 'none';
}

interface Deferred {
  promise: Promise<void>;
  resolve: () => void;
}

function deferred(): Deferred {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

/** The per-document runtime. */
export class Kernel implements KernelServices {
  /** Version of the code that created this kernel. */
  readonly version = PACKAGE_VERSION;
  /** Contract version of the code that created this kernel. */
  readonly contract = CONTRACT_VERSION;
  /** The state cache (main webview). */
  readonly state = new StateCache();
  /** App-visible window ids. */
  readonly windowIds = new WindowIdMap();
  /** The main-side IPC server (`ipcMain`, window scopes). */
  readonly server: IpcServer;
  /** The renderer-side IPC client. */
  readonly client: IpcClient;
  /** The `ipcRenderer` object. */
  readonly ipcRenderer: IpcRenderer;

  readonly #transport: Transport;
  readonly #mismatch: OwTauriError | undefined;
  readonly #readBootstrap: () => unknown;
  #contextOverride: HostContext | null = null;
  #generation = 0;
  #subscription: Promise<string> | null = null;
  #inbox: HostMessage[] = [];
  #blocked = false;
  #draining = false;
  #ready = deferred();
  #isReady = false;
  readonly #handlers = new Map<string, Set<HostMessageHandler>>();
  readonly #singletons = new Map<string, unknown>();
  readonly #resetHooks = new Set<() => void>();
  readonly #warned = new Set<string>();
  readonly #evalBegun = new Set<number>();

  /**
   * @param options - transport and test hooks
   */
  constructor(options: KernelOptions = {}) {
    this.#transport = options.transport ?? tauriTransport;
    this.#mismatch = options.mismatch;
    this.#readBootstrap =
      options.readBootstrap ?? (() => (globalThis as Record<string, unknown>)[BOOTSTRAP_GLOBAL]);
    this.server = new IpcServer(this, this.windowIds);
    this.client = new IpcClient(this);
    this.ipcRenderer = new IpcRenderer(this, () => this.client);
    this.#loadSnapshot();
  }

  /** Where this runtime runs: `'main'`, `'ui'` or `'none'`. */
  get context(): HostContext {
    return this.#contextOverride ?? contextOfLabel(this.#transport.label());
  }

  /** Whether `main_ready` has been acknowledged (main webview). */
  get isReady(): boolean {
    return this.#isReady;
  }

  /**
   * Overrides context detection (tests, through `ow-tauri/testing`).
   *
   * @param context - the context, or `null` to detect it from the label again
   */
  setContextOverride(context: HostContext | null): void {
    this.#contextOverride = context;
  }

  /** {@inheritDoc KernelServices.require} */
  require(context: 'main' | 'ui', api: string): void {
    if (this.#mismatch) throw this.#mismatch;
    const actual = this.context;
    if (actual !== context) {
      const where =
        context === 'main'
          ? 'the main webview (ow-main)'
          : 'a UI window (preload or renderer code)';
      throw new OwTauriError(
        'forbidden',
        `${api} is only available in ${where}; this runtime runs in context '${actual}'`,
        {
          data: { api, context: actual },
        },
      );
    }
  }

  /** {@inheritDoc KernelServices.command} */
  command(name: string, args?: Record<string, unknown>): Promise<unknown> {
    return this.raw(`plugin:${PLUGIN}|${name}`, args);
  }

  /** {@inheritDoc KernelServices.raw} */
  async raw(command: string, args?: Record<string, unknown>): Promise<unknown> {
    try {
      return await this.#transport.invoke(command, args);
    } catch (error) {
      throw fromWireError(error, command);
    }
  }

  /** {@inheritDoc KernelServices.log} */
  log(level: LogLevel, message: string): void {
    const sink =
      level === 'debug'
        ? console.debug
        : level === 'info'
          ? console.info
          : level === 'warn'
            ? console.warn
            : console.error;
    sink(`[ow-tauri] ${message}`);
    if (this.context === 'main' && this.#transport.available()) {
      this.command('log', { level, message }).catch(() => {
        // the console already has it
      });
    }
  }

  /** {@inheritDoc KernelServices.warnOnce} */
  warnOnce(key: string, message: string): void {
    if (this.#warned.has(key)) return;
    this.#warned.add(key);
    this.log('warn', message);
  }

  /**
   * Returns the singleton stored under `key`, creating it with `factory` the
   * first time. Several copies of the npm package in one webview share it.
   *
   * @param key - a namespaced key, e.g. `electron.app`
   * @param factory - creates the value
   * @returns the singleton
   */
  singleton<T>(key: string, factory: () => T): T {
    if (this.#singletons.has(key)) return this.#singletons.get(key) as T;
    const value = factory();
    this.#singletons.set(key, value);
    return value;
  }

  /**
   * Registers a hook that {@link Kernel.reset} runs.
   *
   * @param hook - clears state kept outside the kernel
   * @returns a function that unregisters the hook
   */
  onReset(hook: () => void): () => void {
    this.#resetHooks.add(hook);
    return () => this.#resetHooks.delete(hook);
  }

  /**
   * Subscribes to host messages of one `type` (`window`, `lifecycle`,
   * `global-shortcut`, `packages`, ...). `ipc`, `ipc-result` and `state` are
   * handled by the kernel itself.
   *
   * @param type - the message type
   * @param handler - called synchronously, in channel order
   * @returns a function that unsubscribes
   */
  on(type: string, handler: HostMessageHandler): () => void {
    let set = this.#handlers.get(type);
    if (!set) {
      set = new Set();
      this.#handlers.set(type, set);
    }
    set.add(handler);
    return () => set.delete(handler);
  }

  /**
   * Starts the runtime: subscribes the host-message channel (CONTRACT A.2.1
   * `ipc_subscribe`) when the document runs in a context with IPC.
   * Idempotent until the next {@link Kernel.reset}.
   *
   * @returns the epoch, or `null` when the document has no IPC
   */
  start(): Promise<string | null> {
    if (this.#subscription) return this.#subscription;
    if (this.#mismatch || this.context === 'none' || !this.#transport.available())
      return Promise.resolve(null);
    const generation = this.#generation;
    const onMessage = this.#transport.channel((batch) => {
      if (generation === this.#generation) this.deliver(batch);
    });
    const subscription = this.command('ipc_subscribe', { onMessage }).then((response) => {
      const epoch = (response as { epoch?: unknown } | null)?.epoch;
      if (typeof epoch !== 'string')
        throw new OwTauriError('backend', 'ipc_subscribe returned no epoch');
      return epoch;
    });
    this.#subscription = subscription;
    subscription.then(
      (epoch) => {
        if (generation !== this.#generation) return;
        this.client.open(epoch);
        if (this.context === 'main') this.#scheduleMainReady(generation);
      },
      (error: unknown) => {
        if (generation !== this.#generation) return;
        const mapped =
          error instanceof OwTauriError ? error : fromWireError(error, 'ipc_subscribe');
        this.client.fail(mapped);
        this.log('error', `ipc_subscribe failed: ${mapped.message}`);
      },
    );
    return subscription;
  }

  /**
   * Resolves once the main webview's `ipc_main_ready` and `main_ready` were
   * acknowledged (`whenHostReady()` of `ow-tauri/main`).
   *
   * @returns the readiness promise
   */
  whenHostReady(): Promise<void> {
    return this.#ready.promise;
  }

  /**
   * Feeds one channel payload (`HostMessage[]`) into the runtime. Messages are
   * processed strictly in order; a state-sequence gap pauses processing until
   * the cache is resynchronised (CONTRACT B.1.6 item 3), so a patch is always
   * applied before the event it caused.
   *
   * @param batch - the payload
   */
  deliver(batch: unknown): void {
    const messages = Array.isArray(batch) ? batch : [batch];
    for (const message of messages) {
      if (
        typeof message === 'object' &&
        message !== null &&
        typeof (message as { type?: unknown }).type === 'string'
      ) {
        this.#inbox.push(message as HostMessage);
      }
    }
    this.#drain();
  }

  /**
   * Evaluates the expression form of `webContents.executeJavaScript` code
   * (CONTRACT A.2.3 `window_eval`) and reports the result with `eval_result`.
   *
   * @param id - the evaluation id Rust assigned
   * @param fn - returns the expression's value
   */
  evalBegin(id: number, fn: () => unknown): void {
    this.#evalBegun.add(id);
    if (this.#evalBegun.size > 1024) {
      const oldest = this.#evalBegun.values().next();
      if (oldest.done !== true) this.#evalBegun.delete(oldest.value);
    }
    this.#runEval(id, fn, true);
  }

  /**
   * Runs the statement form when the expression form did not parse; does
   * nothing when {@link Kernel.evalBegin} already ran for `id`.
   *
   * @param id - the evaluation id
   * @param fn - runs the statements
   */
  evalFallback(id: number, fn: () => unknown): void {
    if (this.#evalBegun.has(id)) {
      this.#evalBegun.delete(id);
      return;
    }
    this.#runEval(id, fn, false);
  }

  /**
   * Restores a fresh document state: rejects pending IPC, clears every
   * registry and singleton state (through reset hooks), reloads the snapshot
   * and subscribes again. Used by `ow-tauri/testing`.
   */
  reset(): void {
    this.#generation++;
    this.client.reset();
    this.server.reset();
    this.ipcRenderer.removeAllListeners();
    this.windowIds.reset();
    this.#subscription = null;
    this.#inbox = [];
    this.#blocked = false;
    this.#draining = false;
    this.#isReady = false;
    this.#ready = deferred();
    this.#evalBegun.clear();
    this.#warned.clear();
    for (const hook of [...this.#resetHooks]) safeCall(hook);
    this.#loadSnapshot();
  }

  #loadSnapshot(): void {
    const snapshot = this.#readBootstrap();
    this.state.load(typeof snapshot === 'object' && snapshot !== null ? snapshot : {});
  }

  #drain(): void {
    if (this.#draining) return;
    this.#draining = true;
    try {
      while (!this.#blocked && this.#inbox.length > 0) {
        const message = this.#inbox.shift();
        if (message === undefined) break;
        safeCall(() => {
          this.#dispatch(message);
        });
      }
    } finally {
      this.#draining = false;
    }
  }

  #dispatch(message: HostMessage): void {
    switch (message.type) {
      case 'ipc':
        this.#dispatchIpc(message);
        return;
      case 'ipc-result':
        this.client.onResult(message as IpcResultMessage);
        return;
      case 'state':
        this.#applyState(message as StateMessage);
        return;
      default:
        break;
    }
    const handlers = this.#handlers.get(message.type);
    if (!handlers || handlers.size === 0) {
      this.log('debug', `no handler for host message '${message.type}'`);
      return;
    }
    for (const handler of [...handlers])
      safeCall(() => {
        handler(message);
      });
  }

  #dispatchIpc(message: Record<string, unknown>): void {
    const channel = String(message['channel']);
    switch (message['kind']) {
      case 'invoke':
        void this.server.onInvoke(
          Number(message['id']),
          channel,
          message['args'],
          message['sender'] as IpcSender,
        );
        return;
      case 'send':
        this.server.onSend(channel, message['args'], message['sender'] as IpcSender);
        return;
      case 'message':
        this.ipcRenderer.deliver(channel, message['args']);
        return;
      default:
        this.log('debug', `unknown ipc message kind '${String(message['kind'])}'`);
    }
  }

  #applyState(message: StateMessage): void {
    if (!Array.isArray(message.patches) || typeof message.seq !== 'number') return;
    if (this.state.apply(message) !== 'gap') return;
    // A sequence number is missing: put the message back, pause and resync.
    this.#inbox.unshift(message);
    this.#blocked = true;
    const generation = this.#generation;
    this.command('bootstrap')
      .then((snapshot) => {
        if (generation !== this.#generation) return;
        this.state.load(snapshot);
        if (this.state.seq < message.seq - 1) {
          this.log(
            'warn',
            `state resync returned seq ${String(this.state.seq)}, before ${String(message.seq)}; continuing from there`,
          );
          this.state.forceSeq(message.seq - 1);
        }
      })
      .catch((error: unknown) => {
        if (generation !== this.#generation) return;
        this.log(
          'error',
          `state resync failed: ${(error as Error).message}; continuing with the cached state`,
        );
        this.state.forceSeq(message.seq - 1);
      })
      .finally(() => {
        if (generation !== this.#generation) return;
        this.#blocked = false;
        this.#drain();
      });
  }

  #scheduleMainReady(generation: number): void {
    const run = (): void => {
      setTimeout(() => {
        if (generation !== this.#generation) return;
        void this.#announceMainReady(generation);
      }, 0);
    };
    if (typeof document === 'undefined' || document.readyState !== 'loading') run();
    else document.addEventListener('DOMContentLoaded', run, { once: true });
  }

  async #announceMainReady(generation: number): Promise<void> {
    try {
      await this.command('ipc_main_ready');
      await this.command('main_ready');
    } catch (error) {
      this.log('error', `main webview readiness was not acknowledged: ${(error as Error).message}`);
    }
    if (generation !== this.#generation) return;
    this.#isReady = true;
    this.#ready.resolve();
  }

  #runEval(id: number, fn: () => unknown, wantValue: boolean): void {
    const report = (result: { ok: boolean; value?: unknown; error?: unknown }): void => {
      this.command('eval_result', { id, ...result }).catch((error: unknown) => {
        this.log('warn', `eval_result ${String(id)} failed: ${(error as Error).message}`);
      });
    };
    const settle = (value: unknown): void => {
      if (!wantValue || value === undefined) {
        report({ ok: true });
        return;
      }
      try {
        report({ ok: true, value: encode(value, { root: 'result' }) });
      } catch (error) {
        report({
          ok: false,
          error: { code: 'ipc-serialization', message: (error as Error).message },
        });
      }
    };
    let value: unknown;
    try {
      value = fn();
    } catch (error) {
      report({ ok: false, error: remoteError(error) });
      return;
    }
    if (
      typeof value === 'object' &&
      value !== null &&
      typeof (value as { then?: unknown }).then === 'function'
    ) {
      (value as PromiseLike<unknown>).then(settle, (error: unknown) => {
        report({ ok: false, error: remoteError(error) });
      });
    } else settle(value);
  }
}

/**
 * Decodes an OTJ value received from the host (re-exported for facades that
 * receive values outside IPC messages, e.g. `window_eval` results).
 *
 * @param value - the OTJ value
 * @returns the decoded value
 */
export function decodeHostValue(value: unknown): unknown {
  return decode(value);
}
