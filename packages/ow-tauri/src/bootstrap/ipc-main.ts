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
 * Every outbound sequence number is accounted for: a reply or emit the plugin
 * (or Tauri) rejects is reported with `ipc_emit_skip { target, seq }`, and a
 * reply that cannot be delivered is replaced by a small error reply, so the
 * renderer's `invoke` always settles.
 *
 * @packageDocumentation
 */
import { EventEmitter, emitFromHost, type EventName, type Listener } from '../shared/emitter.js';
import { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
import { decodeArgs, encode, encodedSize, type OtjValue } from '../shared/otj.js';
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

/**
 * First argument of `ipcMain.handle` handlers (Electron's `IpcMainInvokeEvent`).
 * It has no `reply`, `returnValue` or `ports`: the handler's return value is
 * the reply.
 */
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
  /**
   * Unsupported (`sendSync`): reading returns `undefined`, assigning throws.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.3): use `ipcRenderer.invoke` and `ipcMain.handle`.
   */
  returnValue: unknown;
  /**
   * Unsupported (MessagePorts): reads as `undefined`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.3).
   */
  readonly ports: undefined;
}

/**
 * Electron's main-process line for an invoke that failed (no handler, or the
 * handler threw or rejected), printed before the renderer gets the
 * rejection: `Error occurred in handler for '<channel>':` and the error.
 *
 * @param channel - the channel
 * @param error - what the handler threw, or the no-handler error
 */
function handlerFailed(channel: string, error: unknown): void {
  console.error(`Error occurred in handler for '${channel}':`, error);
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
   * The app-visible id for a plugin id, without allocating.
   *
   * @param hostId - the plugin's id
   * @returns the app-visible id, or `undefined` when the id is not bound
   */
  peekHost(hostId: number): number | undefined;
  /**
   * The plugin's id for an app-visible id.
   *
   * @param id - the app-visible id
   * @returns the plugin's id, or `undefined` while the window is being created
   */
  toHost(id: number): number | undefined;
  /**
   * Whether the plugin id belonged to a window that was closed.
   *
   * @param hostId - the plugin's id
   * @returns `true` for a forgotten window
   */
  isForgotten(hostId: number): boolean;
  /**
   * Whether a `window_create` is outstanding (its plugin id is not known yet).
   *
   * @returns `true` while any create is pending
   */
  hasPending(): boolean;
  /**
   * Subscribes to binds and forgets.
   *
   * @param listener - called after every change
   * @returns a function that unsubscribes
   */
  onChange(listener: () => void): () => void;
}

/** IPC from a window whose plugin id is not bound yet. */
interface HeldIpc {
  hostId: number;
  run: (windowId: number) => void;
}

/** An `ipc_reply` payload without its sequence number. */
interface Reply {
  id: number;
  ok: boolean;
  value?: OtjValue;
  error?: OverwolfErrorWire;
}

/** Most IPC messages held while windows are being created. */
const MAX_HELD = 4096;

/** The main-webview half of the IPC protocol. */
export class IpcServer {
  /** The global `ipcMain`. */
  readonly ipcMain: IpcMain;
  /** Builds the `sender` of events; replaced by the `ow-tauri/electron` window registry. */
  senderResolver: SenderResolver = (windowId) => this.#minimalSender(windowId);
  readonly #scoped = new Map<number, IpcMain>();
  readonly #seq = new Map<number, number>();
  #held: HeldIpc[] = [];
  readonly #limit: () => number;

  /**
   * @param services - kernel services
   * @param ids - the window id map
   * @param limit - encoded size cap for `ipc_emit` and replies, or a function
   *   read on every check (the kernel reads `HostSnapshot.ipcLimits`)
   */
  constructor(
    private readonly services: KernelServices,
    private readonly ids: WindowIds,
    limit: number | (() => number) = DEFAULT_MAX_MESSAGE_BYTES,
  ) {
    this.#limit = typeof limit === 'number' ? () => limit : limit;
    this.ipcMain = new IpcMain((api) => {
      services.require('main', api);
    });
    ids.onChange(() => {
      this.#releaseHeld();
    });
  }

  /** Encoded size cap for `ipc_emit` and replies (`ipc.maxMessageBytes`). */
  get maxMessageBytes(): number {
    return this.#limit();
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
    this.#held = [];
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
  onInvoke(id: number, channel: string, args: unknown, sender: IpcSender): Promise<void> {
    // A held or dropped request resolves at once; a held one replies later.
    let done = Promise.resolve();
    this.#route(sender.windowId, `invoke '${channel}'`, (windowId) => {
      done = this.#invoke(windowId, id, channel, args, sender);
    });
    return done;
  }

  async #invoke(
    windowId: number,
    id: number,
    channel: string,
    args: unknown,
    sender: IpcSender,
  ): Promise<void> {
    const target = sender.windowId;
    const handler =
      this.#scoped.get(windowId)?.handlerFor(channel) ?? this.ipcMain.handlerFor(channel);
    if (!handler) {
      const message = `No handler registered for '${channel}'`;
      handlerFailed(channel, new Error(message));
      this.#reply(target, {
        id,
        ok: false,
        error: { code: 'ipc-no-handler', message, data: { channel } },
      });
      return;
    }
    let result: unknown;
    try {
      const decoded = decodeArgs(args);
      result = await handler(this.#invokeEvent(windowId, sender), ...decoded);
    } catch (error) {
      handlerFailed(channel, error);
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
    const size = value === undefined ? 0 : encodedSize(value);
    if (size > this.maxMessageBytes) {
      this.#reply(target, {
        id,
        ok: false,
        error: {
          code: 'ipc-serialization',
          message: `the handler's return value cannot be sent: it is ${String(size)} bytes, more than the ${String(this.maxMessageBytes)}-byte limit (ipc.maxMessageBytes)`,
          data: { bytes: size, limit: this.maxMessageBytes },
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
    this.#route(sender.windowId, `send '${channel}'`, (windowId) => {
      this.#send(windowId, channel, args, sender);
    });
  }

  #send(windowId: number, channel: string, args: unknown, sender: IpcSender): void {
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
        this.#skip(target, seq);
      });
  }

  #reply(target: number, reply: Reply): void {
    const seq = this.#nextSeq(target);
    this.services.command('ipc_reply', { ...reply, seq }).catch((error: unknown) => {
      const message = (error as Error).message;
      this.services.log('warn', `ipc_reply for request ${String(reply.id)} failed: ${message}`);
      if (!reply.ok) {
        this.#skip(target, seq);
        return;
      }
      // Never leave the renderer's invoke pending: answer with a small error
      // under the same sequence number.
      const code =
        error instanceof OwTauriError && error.code === 'ipc-serialization'
          ? 'ipc-serialization'
          : 'backend';
      const fallback: Reply = {
        id: reply.id,
        ok: false,
        error: { code, message: `the reply could not be delivered: ${message}` },
      };
      this.services.command('ipc_reply', { ...fallback, seq }).catch((second: unknown) => {
        this.services.log(
          'error',
          `ipc_reply for request ${String(reply.id)} failed twice: ${(second as Error).message}`,
        );
        this.#skip(target, seq);
      });
    });
  }

  /**
   * Reports an outbound sequence number the plugin never accepted, so later
   * messages to `target` are not held back (CONTRACT C.5).
   */
  #skip(target: number, seq: number): void {
    this.services.command('ipc_emit_skip', { target, seq }).catch((error: unknown) => {
      this.services.log(
        'debug',
        `ipc_emit_skip ${String(seq)} for window ${String(target)} failed (the 1 s gap timeout covers it): ${(error as Error).message}`,
      );
    });
  }

  /**
   * Runs `run` with the app-visible id of the sending window. IPC from a window
   * whose plugin id is not bound yet is held while a `window_create` is
   * pending (its response may not have arrived); IPC from a closed window is
   * dropped.
   */
  #route(hostId: number, what: string, run: (windowId: number) => void): void {
    const known = this.ids.peekHost(hostId);
    if (known !== undefined && !this.#held.some((h) => h.hostId === hostId)) {
      run(known);
      return;
    }
    if (known === undefined && this.ids.isForgotten(hostId)) {
      this.services.log('debug', `dropped ipc ${what} from closed window ${String(hostId)}`);
      return;
    }
    if (known === undefined && !this.ids.hasPending()) {
      run(this.ids.fromHost(hostId));
      return;
    }
    if (this.#held.length >= MAX_HELD) {
      this.services.log('warn', `dropped ipc ${what}: too many messages from unknown windows`);
      return;
    }
    this.#held.push({ hostId, run });
  }

  #releaseHeld(): void {
    if (this.#held.length === 0) return;
    const held = this.#held;
    this.#held = [];
    for (const message of held) this.#route(message.hostId, 'message', message.run);
  }

  #nextSeq(target: number): number {
    const next = (this.#seq.get(target) ?? 0) + 1;
    this.#seq.set(target, next);
    return next;
  }

  #invokeEvent(windowId: number, sender: IpcSender): IpcMainInvokeEvent {
    let prevented = false;
    return {
      type: 'frame',
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
    };
  }

  #event(windowId: number, sender: IpcSender): IpcMainEvent {
    const services = this.services;
    const event = this.#invokeEvent(windowId, sender);
    Object.defineProperties(event, {
      reply: {
        value: (channel: string, ...args: unknown[]) => {
          this.emit(windowId, channel, args);
        },
        enumerable: true,
      },
      returnValue: {
        get(): unknown {
          services.warnOnce(
            'IpcMainEvent.returnValue',
            'IpcMainEvent.returnValue is unsupported (ipcRenderer.sendSync); it reads as undefined',
          );
          return undefined;
        },
        set(_value: unknown) {
          throw new OwTauriUnsupportedError(
            'IpcMainEvent.returnValue',
            'ipcRenderer.sendSync has no equivalent; use ipcRenderer.invoke and ipcMain.handle',
          );
        },
        enumerable: true,
      },
      ports: {
        get(): undefined {
          services.warnOnce(
            'IpcMainEvent.ports',
            'IpcMainEvent.ports is unsupported (MessagePorts); it reads as undefined',
          );
          return undefined;
        },
        enumerable: true,
      },
    });
    return event as IpcMainEvent;
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
 * `data.text` is the thrown value's `toString()`, the exact text Electron
 * puts after `Error invoking remote method '<channel>': `.
 *
 * @param error - the thrown value
 * @returns `{ code: 'ipc-remote-error', message, data: { name, message, text } }`
 */
export function remoteError(error: unknown): OverwolfErrorWire {
  const { name, message } = describeThrown(error);
  return {
    code: 'ipc-remote-error',
    message,
    data: { name, message, text: electronErrorText(error) },
  };
}

/**
 * What Electron reports for a value thrown by an `ipcMain.handle` handler:
 * `String(error)`, which is `Error.prototype.toString()` for errors
 * (`"<name>: <message>"`, or just the name when the message is empty).
 *
 * @param error - the thrown value
 * @returns the text
 */
export function electronErrorText(error: unknown): string {
  try {
    return String(error);
  } catch {
    return 'Error';
  }
}
