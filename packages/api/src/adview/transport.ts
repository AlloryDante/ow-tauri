/**
 * How the `<owadview>` runtime talks to the plugin: the `adview_*` commands
 * over `invoke`, and one Tauri `Channel` per mount that carries the element's
 * events (guest page events and host lifecycle events).
 *
 * @packageDocumentation
 */
import { Channel, invoke } from '@tauri-apps/api/core';

import { toOverwolfError } from '../errors.js';
import { PLUGIN, log, warnOnce } from '../internal.js';
import type { AdviewCommandName, AdviewServices } from './element.js';

/**
 * One message of a mount's event channel: an ad page event
 * (`source: 'guest'`) or a host lifecycle event (`source: 'host'`), e.g.
 * `{ name: 'display_ad_loaded', data: {...}, source: 'guest' }`.
 */
export interface AdviewEventMessage {
  /** The DOM event name dispatched on the element. */
  name: string;
  /** The payload, copied onto the event as own properties. */
  data?: unknown;
  /** Who produced it. */
  source?: 'host' | 'guest';
}

/**
 * Whether a channel value has the {@link AdviewEventMessage} shape.
 *
 * @param value - a channel message
 * @returns whether it can be dispatched
 */
export function isAdviewEventMessage(value: unknown): value is AdviewEventMessage {
  if (typeof value !== 'object' || value === null) return false;
  const { name, source } = value as Record<string, unknown>;
  return (
    typeof name === 'string' && (source === undefined || source === 'host' || source === 'guest')
  );
}

/**
 * The ordering rules of one mount's events:
 *
 * - before {@link EventGate.open} (the mount command has not resolved yet)
 *   messages are buffered;
 * - {@link EventGate.open} dispatches the buffer in arrival order, then every
 *   later message at once;
 * - after {@link EventGate.close} (unmount, remount, failed mount) and after a
 *   dispatched `destroyed`, messages are dropped.
 */
export class EventGate {
  readonly #deliver: (message: AdviewEventMessage) => void;
  #state: 'pending' | 'open' | 'closed' = 'pending';
  #queue: AdviewEventMessage[] = [];

  /**
   * @param deliver - dispatches one message on the element
   */
  constructor(deliver: (message: AdviewEventMessage) => void) {
    this.#deliver = deliver;
  }

  /** Whether the gate drops every message. */
  get closed(): boolean {
    return this.#state === 'closed';
  }

  /**
   * Accepts one channel message.
   *
   * @param message - the message (anything without the message shape is ignored)
   */
  push(message: unknown): void {
    if (!isAdviewEventMessage(message)) {
      log('debug', 'ignored an <owadview> channel message without a name');
      return;
    }
    if (this.#state === 'closed') {
      log('debug', `dropped <owadview> event '${message.name}' of a closed mount`);
      return;
    }
    if (this.#state === 'pending') {
      this.#queue.push(message);
      return;
    }
    this.#dispatch(message);
  }

  /** The mount resolved: dispatches the buffered messages and every later one. */
  open(): void {
    if (this.#state !== 'pending') return;
    this.#state = 'open';
    const queued = this.#queue;
    this.#queue = [];
    for (const message of queued) {
      // A dispatched `destroyed` closes the gate in the middle of the buffer.
      if (this.closed) break;
      this.#dispatch(message);
    }
  }

  /** Drops the buffer and every later message. */
  close(): void {
    this.#state = 'closed';
    this.#queue = [];
  }

  #dispatch(message: AdviewEventMessage): void {
    this.#deliver(message);
    if (message.name === 'destroyed') this.close();
  }
}

/**
 * The wire form of each command: `adview_mount` and `adview_update` take
 * `{ request }` (mount also `onEvent`, the channel), the other two their
 * arguments as they are.
 *
 * @param name - the command
 * @param args - the runtime's arguments
 * @param onEvent - the channel of a mount
 * @returns the `invoke` arguments
 */
export function wireArgs(
  name: AdviewCommandName,
  args: Record<string, unknown>,
  onEvent?: Channel,
): Record<string, unknown> {
  if (name === 'adview_mount') return { request: args, onEvent };
  if (name === 'adview_update') return { request: args };
  return args;
}

/**
 * The runtime services over Tauri's IPC.
 *
 * @returns the services
 */
export function tauriServices(): AdviewServices {
  return {
    command: async (name, args, onEvent) => {
      let channel: Channel | undefined;
      if (name === 'adview_mount') {
        channel = new Channel();
        channel.onmessage = (message) => {
          if (onEvent && isAdviewEventMessage(message)) onEvent(message);
        };
      }
      try {
        return await invoke(`plugin:${PLUGIN}|${name}`, wireArgs(name, args, channel));
      } catch (error) {
        throw toOverwolfError(error, name);
      }
    },
    log,
    warnOnce,
  };
}
