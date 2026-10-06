/**
 * The renderer side of the IPC protocol (`docs/CONTRACT.md` sections C.2,
 * C.3, C.4, C.5 and C.8) and the `ipcRenderer` object built on it.
 *
 * - Every `invoke` / `send` gets the next sequence number of the document's
 *   epoch at call time, so the router can restore call order.
 * - Calls made before `ipc_subscribe` has returned the epoch wait in a
 *   bounded startup queue and are issued in order once it arrives.
 * - `ipc_invoke` only acknowledges with a request id; the result arrives as
 *   an `ipc-result` host message on the same ordered channel as
 *   `webContents.send` messages. A result that overtakes its acknowledgement
 *   is held until the acknowledgement arrives.
 * - Every rejected call is reported with `ipc_skip`, whether Tauri rejected
 *   it before the command ran or the plugin refused it, so the router never
 *   waits for a sequence number that will not arrive (a number the router
 *   already consumed is ignored by `ipc_skip`). Only a stale epoch is not
 *   reported: the router ignores skips for old epochs.
 *
 * @packageDocumentation
 */
import { EventEmitter, emitFromHost, type EventName, type Listener } from '../shared/emitter.js';
import { OwTauriError } from '../shared/errors.js';
import { decode, decodeArgs, encodeArgs, encodedSize, type OtjValue } from '../shared/otj.js';
import type { IpcResultMessage } from '../shared/protocol.js';
import { defineUnsupported } from '../shared/unsupported.js';
import { isPreCommandRejection, type OverwolfErrorWire } from '../shared/wire-error.js';
import type { KernelServices } from './services.js';
import type { UnsupportedMethod } from '../shared/unsupported.js';

/** Default `ipc.maxMessageBytes` (CONTRACT A.1). */
export const DEFAULT_MAX_MESSAGE_BYTES = 8 * 1024 * 1024;
/** Default `ipc.startupQueueMax` (CONTRACT A.1), also used for the renderer's own queue. */
export const DEFAULT_STARTUP_QUEUE_MAX = 1024;
/** Longest channel name, in UTF-16 code units (CONTRACT C.2). */
export const MAX_CHANNEL_LENGTH = 256;
/** Results held while their acknowledgement is outstanding. */
const MAX_EARLY_RESULTS = 1024;

/** Limits the client enforces before a message leaves the webview. */
export interface IpcClientLimits {
  /** Encoded size cap per message, bytes. */
  maxMessageBytes: number;
  /** Calls buffered before the epoch is known. */
  startupQueueMax: number;
}

interface Pending {
  channel: string;
  resolve: (value: unknown) => void;
  reject: (error: unknown) => void;
}

interface Queued {
  /** Sends the call in `epoch`. */
  issue: (epoch: string) => void;
  /** Fails the call without sending it. */
  fail: (error: OwTauriError) => void;
}

/**
 * Validates an IPC channel name (CONTRACT C.2).
 *
 * @param channel - the channel
 * @param api - the calling member, for the error message
 * @throws OwTauriError `invalid-argument` for a non-string, empty or too long channel
 */
export function checkChannel(channel: unknown, api: string): asserts channel is string {
  if (typeof channel !== 'string' || channel.length === 0 || channel.length > MAX_CHANNEL_LENGTH) {
    throw new OwTauriError(
      'invalid-argument',
      `${api}: the channel must be a non-empty string of at most ${String(MAX_CHANNEL_LENGTH)} characters`,
      { data: { channel } },
    );
  }
}

/**
 * Encodes IPC arguments and enforces the size cap.
 *
 * @param args - the arguments
 * @param maxBytes - the cap
 * @returns the encoded arguments
 * @throws OwTauriError `ipc-serialization`
 */
export function encodeMessageArgs(args: readonly unknown[], maxBytes: number): OtjValue[] {
  const encoded = encodeArgs(args);
  const size = encodedSize(encoded);
  if (size > maxBytes) {
    throw new OwTauriError(
      'ipc-serialization',
      `the encoded message is ${String(size)} bytes, more than the ${String(maxBytes)}-byte limit (ipc.maxMessageBytes)`,
      { data: { bytes: size, limit: maxBytes } },
    );
  }
  return encoded;
}

/**
 * Builds the error a renderer sees for a failed invoke (CONTRACT C.2 step 6, C.8).
 *
 * @param channel - the invoked channel
 * @param wire - the error from the `ipc-result` message
 * @returns the error, with Electron's `Error invoking remote method` prefix
 */
export function remoteInvokeError(channel: string, wire: OverwolfErrorWire): OwTauriError {
  const prefix = `Error invoking remote method '${channel}': `;
  const options = wire.data === undefined ? undefined : { data: wire.data };
  switch (wire.code) {
    case 'ipc-no-handler':
      return new OwTauriError('ipc-no-handler', `${prefix}Error: ${wire.message}`, options);
    case 'ipc-remote-error': {
      const data = (typeof wire.data === 'object' && wire.data !== null ? wire.data : {}) as Record<
        string,
        unknown
      >;
      const name = typeof data['name'] === 'string' ? data['name'] : 'Error';
      const message = typeof data['message'] === 'string' ? data['message'] : wire.message;
      // `text` is the thrown value's toString(), Electron's exact wording.
      const text = typeof data['text'] === 'string' ? data['text'] : `${name}: ${message}`;
      return new OwTauriError('ipc-remote-error', `${prefix}${text}`, {
        data: { ...data, name, message },
      });
    }
    default:
      return new OwTauriError(wire.code, `${prefix}${wire.message}`, options);
  }
}

/**
 * The renderer half of the IPC protocol: sequence numbers, the startup queue,
 * pending invokes and their results.
 */
export class IpcClient {
  #epoch: string | null = null;
  #seq = 0;
  #queue: Queued[] = [];
  #pending = new Map<number, Pending>();
  #early = new Map<number, IpcResultMessage>();
  #failure: OwTauriError | null = null;

  /**
   * @param services - kernel services
   * @param limits - client-side limits
   */
  constructor(
    private readonly services: KernelServices,
    readonly limits: IpcClientLimits = {
      maxMessageBytes: DEFAULT_MAX_MESSAGE_BYTES,
      startupQueueMax: DEFAULT_STARTUP_QUEUE_MAX,
    },
  ) {}

  /** The current epoch, or `null` before `ipc_subscribe` returned. */
  get epoch(): string | null {
    return this.#epoch;
  }

  /** Number of invokes waiting for a result. */
  get pendingCount(): number {
    return this.#pending.size;
  }

  /**
   * Sends an invoke request.
   *
   * @param channel - the channel
   * @param args - the arguments
   * @returns the decoded handler result
   * @throws OwTauriError `invalid-argument` (channel) or `ipc-serialization`, synchronously
   */
  invoke(channel: string, args: readonly unknown[]): Promise<unknown> {
    checkChannel(channel, 'ipcRenderer.invoke');
    const encoded = encodeMessageArgs(args, this.limits.maxMessageBytes);
    const seq = ++this.#seq;
    return new Promise<unknown>((resolve, reject) => {
      const issue = (epoch: string): void => {
        this.services
          .command('ipc_invoke', { channel, args: encoded, epoch, seq })
          .then((response) => {
            const id = (response as { id?: unknown } | null)?.id;
            if (typeof id !== 'number') {
              reject(
                new OwTauriError(
                  'backend',
                  `Error invoking remote method '${channel}': malformed ipc_invoke response`,
                ),
              );
              return;
            }
            this.#pending.set(id, { channel, resolve, reject });
            const early = this.#early.get(id);
            if (early) {
              this.#early.delete(id);
              this.onResult(early);
            }
          })
          .catch((error: unknown) => {
            const mapped = this.#rejected(error as OwTauriError, epoch, seq);
            reject(
              new OwTauriError(
                mapped.code,
                `Error invoking remote method '${channel}': ${mapped.message}`,
                { data: mapped.data, cause: mapped },
              ),
            );
          });
      };
      this.#dispatch(channel, {
        issue,
        fail: (error) => {
          reject(error);
        },
      });
    });
  }

  /**
   * Sends a one-way message. Failures after the call returned are logged.
   *
   * @param channel - the channel
   * @param args - the arguments
   * @throws OwTauriError `invalid-argument` (channel) or `ipc-serialization`, synchronously
   */
  send(channel: string, args: readonly unknown[]): void {
    checkChannel(channel, 'ipcRenderer.send');
    const encoded = encodeMessageArgs(args, this.limits.maxMessageBytes);
    const seq = ++this.#seq;
    const issue = (epoch: string): void => {
      this.services
        .command('ipc_send', { channel, args: encoded, epoch, seq })
        .catch((error: unknown) => {
          const mapped = this.#rejected(error as OwTauriError, epoch, seq);
          this.services.log('warn', `ipcRenderer.send('${channel}') failed: ${mapped.message}`);
        });
    };
    this.#dispatch(channel, {
      issue,
      fail: (error) => {
        this.services.log('warn', `ipcRenderer.send('${channel}') dropped: ${error.message}`);
      },
    });
  }

  /**
   * Handles an `ipc-result` host message.
   *
   * @param message - the message
   */
  onResult(message: IpcResultMessage): void {
    const pending = this.#pending.get(message.id);
    if (!pending) {
      // The acknowledgement has not arrived yet: hold the result.
      if (this.#early.size >= MAX_EARLY_RESULTS) {
        const oldest = this.#early.keys().next();
        if (oldest.done !== true) this.#early.delete(oldest.value);
      }
      this.#early.set(message.id, message);
      return;
    }
    this.#pending.delete(message.id);
    if (message.ok) {
      let value: unknown;
      try {
        value = decode(message.value);
      } catch (error) {
        pending.reject(error);
        return;
      }
      pending.resolve(value);
    } else {
      pending.reject(
        remoteInvokeError(
          pending.channel,
          message.error ?? {
            code: 'backend',
            message: 'the host reported a failure without details',
          },
        ),
      );
    }
  }

  /**
   * Starts a new epoch: issues every queued call in order.
   *
   * @param epoch - the epoch `ipc_subscribe` returned
   */
  open(epoch: string): void {
    this.#epoch = epoch;
    this.#failure = null;
    const queue = this.#queue;
    this.#queue = [];
    for (const call of queue) call.issue(epoch);
  }

  /**
   * Fails the queued calls (the subscription failed). Later calls fail the
   * same way until {@link IpcClient.open} or {@link IpcClient.reset}.
   *
   * @param error - the reason
   */
  fail(error: OwTauriError): void {
    this.#failure = error;
    const queue = this.#queue;
    this.#queue = [];
    for (const call of queue) call.fail(error);
  }

  /**
   * Forgets the epoch and rejects every pending and queued call with
   * `not-ready` (document reset in tests, or a fresh subscription).
   *
   * @param reason - message for the rejections
   */
  reset(reason = 'the ow-tauri runtime was reset'): void {
    const error = new OwTauriError('not-ready', reason);
    for (const pending of this.#pending.values()) pending.reject(error);
    this.#pending.clear();
    this.#early.clear();
    this.fail(error);
    this.#failure = null;
    this.#epoch = null;
    this.#seq = 0;
  }

  #dispatch(channel: string, call: Queued): void {
    if (this.#failure) {
      call.fail(this.#failure);
      return;
    }
    if (this.#epoch !== null) {
      call.issue(this.#epoch);
      return;
    }
    if (this.#queue.length >= this.limits.startupQueueMax) {
      call.fail(
        new OwTauriError(
          'not-ready',
          `'${channel}': the IPC startup queue is full (${String(this.limits.startupQueueMax)} calls before the host subscription completed)`,
        ),
      );
      return;
    }
    this.#queue.push(call);
  }

  /**
   * Reports a rejected call's sequence number with `ipc_skip` (CONTRACT C.3
   * "Gaps"). A `not-ready` from the plugin means a stale epoch, which the
   * router no longer orders, so it is not reported.
   */
  #rejected(error: OwTauriError, epoch: string, seq: number): OwTauriError {
    if (isPreCommandRejection(error) || error.code !== 'not-ready') {
      this.services.command('ipc_skip', { epoch, seq }).catch(() => {
        // the 1 s gap timeout in the router covers a failed skip
      });
    }
    return error;
  }
}

/** The first argument of every `ipcRenderer` listener (Electron's `IpcRendererEvent`). */
export interface IpcRendererEvent {
  /** The `ipcRenderer` object. */
  sender: IpcRenderer;
  /** Always 0: messages come from the main webview. */
  senderId: number;
  /** Always empty: MessagePorts are unsupported. */
  ports: never[];
}

/**
 * Electron's `ipcRenderer` (CONTRACT B.2.3), for preload and renderer code in
 * UI windows. A Node-style emitter of `(event, ...args)` per channel, plus
 * `invoke` and `send`.
 *
 * Unsupported: `sendSync`, `sendTo`, `sendToHost`, `postMessage` (they throw
 * `OwTauriUnsupportedError`).
 */
export class IpcRenderer extends EventEmitter {
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.3).
   */
  declare readonly sendSync: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.3).
   */
  declare readonly sendTo: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.3).
   */
  declare readonly sendToHost: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.3).
   */
  declare readonly postMessage: UnsupportedMethod;
  readonly #services: KernelServices;
  readonly #client: () => IpcClient;

  /**
   * @param services - kernel services
   * @param client - the IPC client of the current document
   * @internal
   */
  constructor(services: KernelServices, client: () => IpcClient) {
    super();
    this.#services = services;
    this.#client = client;
  }

  /**
   * Sends `args` to `channel` and resolves with the main webview handler's
   * return value (`ipcMain.handle`).
   *
   * @param channel - the channel
   * @param args - the arguments (OTJ-encodable)
   * @returns the handler's result
   * @throws OwTauriError `ipc-serialization` or `invalid-argument` synchronously;
   *   rejects with the C.8 errors
   */
  invoke(channel: string, ...args: unknown[]): Promise<unknown> {
    this.#services.require('ui', 'ipcRenderer.invoke');
    return this.#client().invoke(channel, args);
  }

  /**
   * Sends a one-way message to `ipcMain.on` listeners in the main webview.
   *
   * @param channel - the channel
   * @param args - the arguments (OTJ-encodable)
   * @throws OwTauriError `ipc-serialization` or `invalid-argument` synchronously
   */
  send(channel: string, ...args: unknown[]): void {
    this.#services.require('ui', 'ipcRenderer.send');
    this.#client().send(channel, args);
  }

  /**
   * Adds a listener for messages on `channel` (`webContents.send`).
   *
   * @param channel - the channel
   * @param listener - called with `(event, ...args)`
   * @returns this object
   */
  override on(channel: EventName, listener: Listener): this {
    this.#services.require('ui', 'ipcRenderer.on');
    return super.on(channel, listener);
  }

  /**
   * Adds a one-time listener for messages on `channel`.
   *
   * @param channel - the channel
   * @param listener - called with `(event, ...args)`
   * @returns this object
   */
  override once(channel: EventName, listener: Listener): this {
    this.#services.require('ui', 'ipcRenderer.once');
    return super.once(channel, listener);
  }

  /**
   * Alias of {@link IpcRenderer.on}.
   *
   * @param channel - the channel
   * @param listener - called with `(event, ...args)`
   * @returns this object
   */
  override addListener(channel: EventName, listener: Listener): this {
    this.#services.require('ui', 'ipcRenderer.addListener');
    return super.addListener(channel, listener);
  }

  /**
   * Dispatches a host `ipc { kind: 'message' }` to the listeners.
   *
   * @param channel - the channel
   * @param args - the encoded arguments
   * @internal
   */
  deliver(channel: string, args: unknown): void {
    let decoded: unknown[];
    try {
      decoded = decodeArgs(args);
    } catch (error) {
      this.#services.log('warn', `dropped a message on '${channel}': ${String(error)}`);
      return;
    }
    const event: IpcRendererEvent = { sender: this, senderId: 0, ports: [] };
    emitFromHost(this, channel, event, ...decoded);
  }
}

defineUnsupported(IpcRenderer.prototype, 'ipcRenderer.', [
  'sendSync',
  'sendTo',
  'sendToHost',
  'postMessage',
]);
