/**
 * The main-webview side of the IPC protocol (`docs/CONTRACT.md` sections
 * C.2, C.3, C.5) and the `ipcMain` object.
 *
 * Incoming `ipc` host messages are dispatched first to the sender window's
 * `webContents.ipc` scope, then to `ipcMain`. Replies (`ipc_reply`) and
 * `webContents.send` messages (`ipc_emit`) to one target share a per-target
 * sequence number, assigned when the message is sent, so a reply is delivered
 * after every message its handler sent to that window before returning.
 *
 * @packageDocumentation
 */
import { EventEmitter, emitFromHost, type EventName, type Listener } from '../shared/emitter.js';
import { OwTauriUnsupportedError } from '../shared/errors.js';
import { decodeArgs, encode, type OtjValue } from '../shared/otj.js';
import type { IpcSender } from '../shared/protocol.js';
import { describeThrown, type OverwolfErrorWire } from '../shared/wire-error.js';
import { checkChannel, DEFAULT_MAX_MESSAGE_BYTES, encodeMessageArgs } from './ipc-renderer.js';
import type { KernelServices } from './services.js';

/** A handler registered with `ipcMain.handle`. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any -- typed boundary: handler arguments are the renderer's arguments
export type IpcHandler = (event: IpcMainInvokeEvent, ...args: any[]) => unknown;

/** The `senderFrame` of an IPC event: only `url` is provided (CONTRACT B.2.3). */
export interface IpcSenderFrame {
  /** The sending document's URL. */
  url: string;
}

/** Fields shared by both main-side IPC events. */
export interface IpcMainEventBase {
  /** Always `'frame'`. */
  type: 'frame';
  /** The sender's `WebContents` facade. */
  sender: unknown;
  /** Always 0 (main frame). */
  frameId: number;
  /** Always 0. */
  processId: number;
  /** The sending frame (`url` only). */
  senderFrame: IpcSenderFrame;
  /** Electron `Event.preventDefault`; no effect on IPC events. */
  preventDefault(): void;
  /** Whether `preventDefault` was called. */
  readonly defaultPrevented: boolean;
}

/** First argument of `ipcMain.handle` handlers (Electron's `IpcMainInvokeEvent`). */
export type IpcMainInvokeEvent = IpcMainEventBase;

/** First argument of `ipcMain.on` listeners (Electron's `IpcMainEvent`). */
export interface IpcMainEvent extends IpcMainEventBase {
  /**
   * Sends a message back to the sender window (`webContents.send` to it).
   *
   * @param channel - the channel
   * @param args - the arguments
   */
  reply(channel: string, ...args: unknown[]): void;
  /** Unsupported (`sendSync`): reading returns `undefined`, assigning throws. */
  returnValue: unknown;
  /** Unsupported (MessagePorts): reads as `undefined`. */
  readonly ports: undefined;
}

/**
 * Electron's `ipcMain` and `webContents.ipc` (CONTRACT B.2.3): a Node-style
 * emitter of `(event, ...args)` for `ipcRenderer.send`, plus
 * `handle` / `handleOnce` / `removeHandler` for `ipcRenderer.invoke`.
 */
export class IpcMain extends EventEmitter {
  readonly #handlers = new Map<string, IpcHandler>();
  readonly #require: (api: string) => void;
  readonly #name: string;

  /**
   * @param require - throws unless the runtime runs in the main webview
   * @param name - `ipcMain` or `webContents.ipc`, for error messages
   */
  constructor(require: (api: string) => void, name = 'ipcMain') {
    super();
    this.#require = require;
    this.#name = name;
  }

  /**
   * Registers the handler for `ipcRenderer.invoke(channel)`.
   *
   * @param channel - the channel
   * @param handler - called with `(event, ...args)`; its (awaited) return value is the reply
   * @throws Error `Attempted to register a second handler for '<channel>'`, as Electron
   */
  handle(channel: string, handler: IpcHandler): void {
    this.#require(`${this.#name}.handle`);
    checkChannel(channel, `${this.#name}.handle`);
    if (typeof handler !== 'function')
      throw new TypeError(`${this.#name}.handle: handler must be a function`);
    if (this.#handlers.has(channel)) {
      throw new Error(`Attempted to register a second handler for '${channel}'`);
    }
    this.#handlers.set(channel, handler);
  }

  /**
   * Registers a handler that is removed after its first invocation.
   *
   * @param channel - the channel
   * @param handler - called with `(event, ...args)`
   */
  handleOnce(channel: string, handler: IpcHandler): void {
    const once: IpcHandler = (event, ...args: unknown[]) => {
      if (this.#handlers.get(channel) === once) this.#handlers.delete(channel);
      return handler(event, ...args);
    };
    this.handle(channel, once);
  }

  /**
   * Removes the handler of `channel`, if any.
   *
   * @param channel - the channel
   */
  removeHandler(channel: string): void {
    this.#handlers.delete(channel);
  }

  /**
   * The handler of `channel`.
   *
   * @param channel - the channel
   * @returns the handler, or `undefined`
   * @internal
   */
  handlerFor(channel: string): IpcHandler | undefined {
    return this.#handlers.get(channel);
  }

  /**
   * Drops every handler and listener.
   *
   * @internal
   */
  clear(): void {
    this.#handlers.clear();
    this.removeAllListeners();
  }

  /**
   * Adds a listener for `ipcRenderer.send(channel)`.
   *
   * @param channel - the channel
   * @param listener - called with `(event, ...args)`
   * @returns this object
   */
  override on(channel: EventName, listener: Listener): this {
    this.#require(`${this.#name}.on`);
    return super.on(channel, listener);
  }

  /**
   * Adds a one-time listener.
   *
   * @param channel - the channel
   * @param listener - called with `(event, ...args)`
   * @returns this object
   */
  override once(channel: EventName, listener: Listener): this {
    this.#require(`${this.#name}.once`);
    return super.once(channel, listener);
  }

  /**
   * Alias of {@link IpcMain.on}.
   *
   * @param channel - the channel
   * @param listener - called with `(event, ...args)`
   * @returns this object
   */
  override addListener(channel: EventName, listener: Listener): this {
    this.#require(`${this.#name}.addListener`);
    return super.addListener(channel, listener);
  }
}

/**
 * Resolves the `sender` of an IPC event: the `WebContents` facade of the
 * window with this (app-visible) id.
 */
export type SenderResolver = (windowId: number, sender: IpcSender) => unknown;

/**
 * Maps between the integer window ids app code sees and the plugin's ids.
 * See `WindowIdMap` in the kernel.
 */
export interface WindowIds {
  /**
   * The app-visible id for a plugin id (allocating one for an unknown id).
   *
   * @param hostId - the plugin's id
   * @returns the app-visible id
   */
  fromHost(hostId: number): number;
  /**
   * The plugin's id for an app-visible id.
   *
   * @param id - the app-visible id
   * @returns the plugin's id, or `undefined` while the window is being created
   */
  toHost(id: number): number | undefined;
}

/** The main-webview half of the IPC protocol. */
export class IpcServer {
  /** The global `ipcMain`. */
  readonly ipcMain: IpcMain;
  /** Builds the `sender` of events; replaced by the `ow-tauri/electron` window registry. */
  senderResolver: SenderResolver = (windowId) => this.#minimalSender(windowId);
  readonly #scoped = new Map<number, IpcMain>();
  readonly #seq = new Map<number, number>();

  /**
   * @param services - kernel services
   * @param ids - the window id map
   * @param maxMessageBytes - encoded size cap for `ipc_emit` and replies
   */
  constructor(
    private readonly services: KernelServices,
    private readonly ids: WindowIds,
    readonly maxMessageBytes = DEFAULT_MAX_MESSAGE_BYTES,
  ) {
    this.ipcMain = new IpcMain((api) => {
      services.require('main', api);
    });
  }

  /**
   * The `webContents.ipc` scope of a window, created on first use.
   *
   * @param windowId - the app-visible window id
   * @returns the scope
   */
  scoped(windowId: number): IpcMain {
    let scope = this.#scoped.get(windowId);
    if (!scope) {
      scope = new IpcMain((api) => {
        this.services.require('main', api);
      }, 'webContents.ipc');
      this.#scoped.set(windowId, scope);
    }
    return scope;
  }

  /**
   * Forgets a window's scope (the window closed).
   *
   * @param windowId - the app-visible window id
   */
  dropScope(windowId: number): void {
    this.#scoped.get(windowId)?.clear();
    this.#scoped.delete(windowId);
  }

  /**
   * Forgets every scope, handler, listener and sequence counter.
   */
  reset(): void {
    for (const scope of this.#scoped.values()) scope.clear();
    this.#scoped.clear();
    this.#seq.clear();
    this.ipcMain.clear();
  }

  /**
   * Handles `ipc { kind: 'invoke' }` (CONTRACT C.2 steps 4 and 5).
   *
   * @param id - the request id
   * @param channel - the channel
   * @param args - encoded arguments
   * @param sender - the sender Rust stamped
   * @returns resolves when the reply was handed to the plugin
   */
  async onInvoke(id: number, channel: string, args: unknown, sender: IpcSender): Promise<void> {
    const windowId = this.ids.fromHost(sender.windowId);
    const target = sender.windowId;
    const handler =
      this.#scoped.get(windowId)?.handlerFor(channel) ?? this.ipcMain.handlerFor(channel);
    if (!handler) {
      this.#reply(target, {
        id,
        ok: false,
        error: {
          code: 'ipc-no-handler',
          message: `No handler registered for '${channel}'`,
          data: { channel },
        },
      });
      return;
    }
    let result: unknown;
    try {
      const decoded = decodeArgs(args);
      result = await handler(this.#event(windowId, sender), ...decoded);
    } catch (error) {
      this.#reply(target, { id, ok: false, error: remoteError(error) });
      return;
    }
    let value: OtjValue | undefined;
    try {
      value = result === undefined ? undefined : encode(result, { root: 'result' });
    } catch (error) {
      const { message } = describeThrown(error);
      this.#reply(target, {
        id,
        ok: false,
        error: {
          code: 'ipc-serialization',
          message: `the handler's return value cannot be sent: ${message}`,
        },
      });
      return;
    }
    this.#reply(target, value === undefined ? { id, ok: true } : { id, ok: true, value });
  }

  /**
   * Handles `ipc { kind: 'send' }` (CONTRACT C.3): window scope first, then
   * `ipcMain`. Listener exceptions are reported and do not stop dispatch of
   * later messages.
   *
   * @param channel - the channel
   * @param args - encoded arguments
   * @param sender - the sender Rust stamped
   */
  onSend(channel: string, args: unknown, sender: IpcSender): void {
    const windowId = this.ids.fromHost(sender.windowId);
    let decoded: unknown[];
    try {
      decoded = decodeArgs(args);
    } catch (error) {
      this.services.log('warn', `dropped a message on '${channel}': ${String(error)}`);
      return;
    }
    const event = this.#event(windowId, sender);
    const scope = this.#scoped.get(windowId);
    if (scope) emitFromHost(scope, channel, event, ...decoded);
    emitFromHost(this.ipcMain, channel, event, ...decoded);
  }

  /**
   * `webContents.send` / `event.reply` (CONTRACT C.5).
   *
   * @param windowId - the app-visible target window id
   * @param channel - the channel
   * @param args - the arguments
   * @throws OwTauriError `invalid-argument` (channel) or `ipc-serialization`, synchronously
   */
  emit(windowId: number, channel: string, args: readonly unknown[]): void {
    checkChannel(channel, 'webContents.send');
    this.emitEncoded(windowId, channel, encodeMessageArgs(args, this.maxMessageBytes));
  }

  /**
   * {@link IpcServer.emit} with arguments encoded at call time, for sends
   * that wait for the target window to exist.
   *
   * @param windowId - app-visible window id
   * @param channel - the channel (already checked)
   * @param encoded - OTJ-encoded arguments
   */
  emitEncoded(windowId: number, channel: string, encoded: OtjValue[]): void {
    const target = this.ids.toHost(windowId);
    if (target === undefined) {
      this.services.log(
        'warn',
        `webContents.send('${channel}') to window ${String(windowId)} dropped: the window does not exist yet`,
      );
      return;
    }
    const seq = this.#nextSeq(target);
    this.services
      .command('ipc_emit', { target, channel, args: encoded, seq })
      .catch((error: unknown) => {
        this.services.log(
          'warn',
          `webContents.send('${channel}') failed: ${(error as Error).message}`,
        );
      });
  }

  #reply(
    target: number,
    reply: { id: number; ok: boolean; value?: OtjValue; error?: unknown },
  ): void {
    const seq = this.#nextSeq(target);
    this.services.command('ipc_reply', { ...reply, seq }).catch((error: unknown) => {
      this.services.log(
        'warn',
        `ipc_reply for request ${String(reply.id)} failed: ${(error as Error).message}`,
      );
    });
  }

  #nextSeq(target: number): number {
    const next = (this.#seq.get(target) ?? 0) + 1;
    this.#seq.set(target, next);
    return next;
  }

  #event(windowId: number, sender: IpcSender): IpcMainEvent {
    const services = this.services;
    let prevented = false;
    const event = {
      type: 'frame' as const,
      sender: this.senderResolver(windowId, sender),
      frameId: 0,
      processId: 0,
      senderFrame: Object.freeze({ url: sender.url }),
      preventDefault: () => {
        prevented = true;
      },
      get defaultPrevented() {
        return prevented;
      },
      reply: (channel: string, ...args: unknown[]) => {
        this.emit(windowId, channel, args);
      },
      get returnValue(): unknown {
        services.warnOnce(
          'IpcMainEvent.returnValue',
          'IpcMainEvent.returnValue is unsupported (ipcRenderer.sendSync); it reads as undefined',
        );
        return undefined;
      },
      set returnValue(_value: unknown) {
        throw new OwTauriUnsupportedError(
          'IpcMainEvent.returnValue',
          'ipcRenderer.sendSync has no equivalent; use ipcRenderer.invoke and ipcMain.handle',
        );
      },
      get ports(): undefined {
        services.warnOnce(
          'IpcMainEvent.ports',
          'IpcMainEvent.ports is unsupported (MessagePorts); it reads as undefined',
        );
        return undefined;
      },
    };
    return event;
  }

  #minimalSender(windowId: number): unknown {
    return Object.freeze({
      id: windowId,
      send: (channel: string, ...args: unknown[]) => {
        this.emit(windowId, channel, args);
      },
    });
  }
}

/**
 * The wire error for a handler that threw or rejected (CONTRACT C.2 step 5).
 *
 * @param error - the thrown value
 * @returns `{ code: 'ipc-remote-error', message, data: { name, message } }`
 */
export function remoteError(error: unknown): OverwolfErrorWire {
  const { name, message } = describeThrown(error);
  return { code: 'ipc-remote-error', message, data: { name, message } };
}
